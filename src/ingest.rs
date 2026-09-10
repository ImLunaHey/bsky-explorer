use crate::{
    analysis::Analyzer,
    archive::Archive,
    config::Config,
    identity::{IdentityService, parse_identity_event},
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
    identities: IdentityService,
) -> Result<()> {
    let (tx, mut rx) = mpsc::channel(20_000);
    let st = storage.clone();
    let interval = cfg.flush_interval();
    let batch_size = cfg.batch_size;
    tokio::spawn(async move {
        let mut batch = Vec::with_capacity(batch_size);
        let mut flush_tick = tokio::time::interval(interval);
        flush_tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            let flush_due = tokio::select! {
                item = rx.recv() => match item {
                    Some(e) => { batch.push(e); false },
                    None => {
                        persist_batch(&st, &mut batch).await;
                        break;
                    }
                },
                _ = flush_tick.tick() => true,
            };
            if batch.len() >= batch_size || (flush_due && !batch.is_empty()) {
                // Retrying this same bounded batch applies backpressure during an outage
                // instead of accumulating an unbounded in-process backlog.
                persist_batch(&st, &mut batch).await;
            }
        }
    });
    loop {
        let mut url = cfg.jetstream_url.clone();
        if let Some(cursor) = Archive::cursor(&cfg.data_dir) {
            url.push_str(if url.contains('?') { "&" } else { "?" });
            url.push_str(&format!("cursor={cursor}"));
        }
        match consume(&url, &cfg, &tx, &storage, &analysis, &metrics, &identities).await {
            Ok(()) => tracing::warn!("Jetstream closed"),
            Err(e) => tracing::error!(error=%e,"Jetstream disconnected"),
        };
        metrics.connected.set(0);
        sleep(std::time::Duration::from_secs(2)).await;
    }
}

async fn persist_batch(storage: &Storage, batch: &mut Vec<crate::model::NormalizedEvent>) {
    while !batch.is_empty() {
        match storage.insert_events(batch).await {
            Ok(()) => batch.clear(),
            Err(error) => {
                tracing::error!(%error, events = batch.len(), "ClickHouse insert failed; retrying");
                sleep(std::time::Duration::from_secs(2)).await;
            }
        }
    }
}
async fn consume(
    url: &str,
    cfg: &Config,
    tx: &mpsc::Sender<crate::model::NormalizedEvent>,
    storage: &Storage,
    analysis: &Analyzer,
    metrics: &Metrics,
    identities: &IdentityService,
) -> Result<()> {
    let (ws, _) = connect_async(url).await.context("connect Jetstream")?;
    metrics.connected.set(1);
    let (_, mut read) = ws.split();
    let mut archive = Archive::open(&cfg.data_dir, cfg.archive_segment_mib)?;
    if let Err(error) = Archive::prune(&cfg.data_dir, cfg.archive_retention_days) {
        tracing::warn!(%error, "archive retention pass failed");
    }
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
        match parse_identity_event(&raw) {
            Ok(Some(identity)) => {
                if let Err(error) = identities.observe(identity).await {
                    tracing::error!(%error, "persist Jetstream identity update");
                }
                continue;
            }
            Ok(None) => {}
            Err(error) => {
                tracing::warn!(%error, "bad Jetstream identity event");
                continue;
            }
        }
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
