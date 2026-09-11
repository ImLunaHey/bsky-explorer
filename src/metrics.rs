use crate::model::EventKind;
use anyhow::Result;
use prometheus::{Encoder, IntCounterVec, IntGauge, Registry, TextEncoder};
use std::sync::Arc;

#[derive(Clone)]
pub struct Metrics {
    pub registry: Registry,
    events: IntCounterVec,
    pub connected: IntGauge,
    pub archive_bytes: IntGauge,
    pub clickhouse_bytes: IntGauge,
    pub hot_detail_events: IntGauge,
    pub jetstream_lag_milliseconds: IntGauge,
}
impl Metrics {
    pub fn new() -> Result<Arc<Self>> {
        let registry = Registry::new();
        let events = IntCounterVec::new(
            prometheus::Opts::new("observatory_events_total", "Normalized public events"),
            &["kind"],
        )?;
        let connected = IntGauge::new(
            "observatory_jetstream_connected",
            "Jetstream connection state",
        )?;
        let archive_bytes = IntGauge::new("observatory_archive_bytes", "Compressed archive bytes")?;
        let clickhouse_bytes =
            IntGauge::new("observatory_clickhouse_bytes", "ClickHouse bytes on disk")?;
        let hot_detail_events = IntGauge::new(
            "observatory_hot_detail_events",
            "Events retained in bounded detailed analysis memory",
        )?;
        let jetstream_lag_milliseconds = IntGauge::new(
            "observatory_jetstream_lag_milliseconds",
            "Difference between local time and the newest processed Jetstream event",
        )?;
        registry.register(Box::new(events.clone()))?;
        registry.register(Box::new(connected.clone()))?;
        registry.register(Box::new(archive_bytes.clone()))?;
        registry.register(Box::new(clickhouse_bytes.clone()))?;
        registry.register(Box::new(hot_detail_events.clone()))?;
        registry.register(Box::new(jetstream_lag_milliseconds.clone()))?;
        Ok(Arc::new(Self {
            registry,
            events,
            connected,
            archive_bytes,
            clickhouse_bytes,
            hot_detail_events,
            jetstream_lag_milliseconds,
        }))
    }
    pub fn event(&self, k: &EventKind) {
        self.events.with_label_values(&[k.as_str()]).inc()
    }
    pub fn render(&self) -> String {
        let mut out = vec![];
        TextEncoder::new()
            .encode(&self.registry.gather(), &mut out)
            .unwrap();
        String::from_utf8(out).unwrap()
    }
}
