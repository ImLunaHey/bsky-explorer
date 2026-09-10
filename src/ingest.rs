use crate::{
    analysis::Analyzer,
    archive::Archive,
    config::Config,
    metrics::Metrics,
    normalize::{jetstream_time_us, parse_jetstream},
    storage::Storage,
};
use anyhow::{Context, Result};
use futures_util::StreamExt;
use std::sync::Arc;
use tokio::{sync::mpsc, time::sleep};
use tokio_tungstenite::{connect_async, tungstenite::Message};

pub async fn run(
    cfg: Config,
    storage: Storage,
    analysis: Analyzer,
    metrics: Arc<Metrics>,
) -> Result<()> {
    let (tx, mut rx) = mpsc::channel(20_000);
    let st = storage.clone();
    let interval = cfg.flush_interval();
    let batch_size = cfg.batch_size;
    tokio::spawn(async move {
        let mut batch = Vec::with_capacity(batch_size);
        loop {
            tokio::select! { Some(e)=rx.recv()=>{batch.push(e);if batch.len()>=batch_size{if let Err(x)=st.insert_events(&batch).await{tracing::error!(error=%x,"ClickHouse insert failed");sleep(std::time::Duration::from_secs(2)).await;continue}batch.clear();}}, _=sleep(interval)=>{if !batch.is_empty(){if let Err(x)=st.insert_events(&batch).await{tracing::error!(error=%x,"ClickHouse insert failed")}else{batch.clear()}}} }
        }
    });
    loop {
        let mut url = cfg.jetstream_url.clone();
        if let Some(cursor) = Archive::cursor(&cfg.data_dir) {
            url.push_str(if url.contains('?') { "&" } else { "?" });
            url.push_str(&format!("cursor={cursor}"));
        }
        match consume(&url, &cfg, &tx, &storage, &analysis, &metrics).await {
            Ok(()) => tracing::warn!("Jetstream closed"),
            Err(e) => tracing::error!(error=%e,"Jetstream disconnected"),
        };
        metrics.connected.set(0);
        sleep(std::time::Duration::from_secs(2)).await;
    }
}
async fn consume(
    url: &str,
    cfg: &Config,
    tx: &mpsc::Sender<crate::model::NormalizedEvent>,
    storage: &Storage,
    analysis: &Analyzer,
    metrics: &Metrics,
) -> Result<()> {
    let (ws, _) = connect_async(url).await.context("connect Jetstream")?;
    metrics.connected.set(1);
    let (_, mut read) = ws.split();
    let mut archive = Archive::open(&cfg.data_dir, cfg.archive_segment_mib)?;
    while let Some(msg) = read.next().await {
        let msg = msg?;
        let raw = match msg {
            Message::Text(s) => s.as_bytes().to_vec(),
            Message::Binary(b) => b.to_vec(),
            _ => continue,
        };
        // Archive every valid Jetstream JSON message before interpretation. This keeps
        // unknown event kinds replayable as support is added in future versions.
        let time_us = match jetstream_time_us(&raw) {
            Ok(t) => t,
            Err(e) => {
                tracing::warn!(error=%e, "bad Jetstream envelope");
                continue;
            }
        };
        archive.append(time_us, &raw)?;
        metrics
            .archive_bytes
            .set(Archive::total_size(&cfg.data_dir) as i64);
        let event = match parse_jetstream(&raw) {
            Ok(Some(e)) => e,
            Ok(None) => continue,
            Err(e) => {
                tracing::warn!(error=%e,"bad event");
                continue;
            }
        };
        metrics.event(&event.kind);
        if let Some(d) = analysis.observe(event.clone())
            && let Err(e) = storage.insert_detection(&d).await
        {
            tracing::error!(error=%e,"persist detection")
        }
        tx.send(event).await.context("storage worker stopped")?;
    }
    Ok(())
}
