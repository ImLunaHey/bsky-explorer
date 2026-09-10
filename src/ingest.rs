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
    identity_queue: crate::identity_queue::IdentityQueue,
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
        match consume(
            &url,
            &cfg,
            &tx,
            &storage,
            &analysis,
            &metrics,
            &identities,
            &identity_queue,
        )
        .await
        {
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
#[allow(clippy::too_many_arguments)]
async fn consume(
    url: &str,
    cfg: &Config,
    tx: &mpsc::Sender<crate::model::NormalizedEvent>,
    storage: &Storage,
    analysis: &Analyzer,
    metrics: &Metrics,
    identities: &IdentityService,
    identity_queue: &crate::identity_queue::IdentityQueue,
) -> Result<()> {
    let (ws, _) = connect_async(url).await.context("connect Jetstream")?;
    metrics.connected.set(1);
    let (_, mut read) = ws.split();
    let (archive_tx, mut archive_rx) = mpsc::channel::<(i64, Vec<u8>)>(20_000);
    let archive_dir = cfg.data_dir.clone();
    let segment_mib = cfg.archive_segment_mib;
    let retention_days = cfg.archive_retention_days;
    let archive_gauge = metrics.archive_bytes.clone();
    let archive_task = tokio::task::spawn_blocking(move || -> Result<()> {
        let mut archive = Archive::open(&archive_dir, segment_mib)?;
        if let Err(error) = Archive::prune(&archive_dir, retention_days) {
            tracing::warn!(%error, "archive retention pass failed");
        }
        while let Some((time_us, raw)) = archive_rx.blocking_recv() {
            if archive.append(time_us, &raw)? {
                archive_gauge.set(Archive::total_size(&archive_dir) as i64);
            }
        }
        archive.checkpoint()
    });
    let mut processed_since_yield = 0_u16;
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
        archive_tx
            .send((time_us, raw.clone()))
            .await
            .context("archive worker stopped")?;
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
        // Durable, O(1) queueing keeps profile network I/O entirely off ingest.
        for did in std::iter::once(&event.actor_did).chain(event.target_did.as_ref()) {
            if let Err(error) = identity_queue.enqueue(did) {
                tracing::debug!(%error, %did, "identity enqueue skipped");
            }
        }
        metrics.event(&event.kind);
        let lag_us = chrono::Utc::now()
            .timestamp_micros()
            .saturating_sub(event.time_us);
        if lag_us > 5 * 60 * 1_000_000 {
            // Prioritize durable catch-up after downtime. Running pairwise
            // coordination analysis over stale backlog would delay the live
            // stream further; replay can analyze it later.
            analysis.observe_without_detection(event.clone());
        } else if let Some(d) = analysis.observe(event.clone())
            && let Err(e) = storage.insert_detection(&d).await
        {
            tracing::error!(error=%e,"persist detection")
        }
        tx.send(event).await.context("storage worker stopped")?;
        processed_since_yield += 1;
        if processed_since_yield >= 128 {
            processed_since_yield = 0;
            tokio::task::yield_now().await;
        }
    }
    drop(archive_tx);
    archive_task.await.context("archive worker panicked")??;
    Ok(())
}
