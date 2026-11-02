//! Metrics collector for PN Gateway

use super::config::{MetricsConfig, MetricsExporter};
use anyhow::Result;
use serde_json::Value;
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::debug;

/// Metrics collector for PN Gateway
pub struct MetricsCollector {
    config: MetricsConfig,
    counters: Arc<RwLock<HashMap<String, AtomicU64>>>,
    gauges: Arc<RwLock<HashMap<String, Arc<RwLock<f64>>>>>,
    histograms: Arc<RwLock<HashMap<String, Vec<f64>>>>,
    exporters: Vec<MetricsExporter>,
    running: Arc<tokio::sync::Mutex<bool>>,
}

impl MetricsCollector {
    pub fn new(config: MetricsConfig) -> Self {
        let exporters = config.exporters.clone();

        Self {
            config,
            counters: Arc::new(RwLock::new(HashMap::new())),
            gauges: Arc::new(RwLock::new(HashMap::new())),
            histograms: Arc::new(RwLock::new(HashMap::new())),
            exporters,
            running: Arc::new(tokio::sync::Mutex::new(false)),
        }
    }

    pub async fn start(&mut self) -> Result<()> {
        if !self.config.enabled {
            return Ok(());
        }

        *self.running.lock().await = true;

        let interval = self.config.interval;
        let counters = self.counters.clone();
        let gauges = self.gauges.clone();
        let histograms = self.histograms.clone();
        let exporters = self.exporters.clone();
        let running = self.running.clone();

        tokio::spawn(async move {
            let mut interval_timer = tokio::time::interval(interval);

            loop {
                interval_timer.tick().await;

                if !*running.lock().await {
                    break;
                }

                // Collect and export metrics
                let metrics = Self::collect_all(&counters, &gauges, &histograms).await;

                for exporter in &exporters {
                    if let Err(e) = Self::export(&exporter, &metrics).await {
                        tracing::warn!("Failed to export metrics: {}", e);
                    }
                }

                // Cleanup old histogram data
                Self::cleanup_histograms(&histograms).await;
            }
        });

        Ok(())
    }

    pub async fn stop(&mut self) -> Result<()> {
        *self.running.lock().await = false;
        Ok(())
    }

    pub fn increment_counter(&self, name: &str, value: u64) {
        let counters = self.counters.clone();
        let name = name.to_string();
        tokio::spawn(async move {
            let mut counters = counters.write().await;
            counters
                .entry(name)
                .or_insert_with(|| AtomicU64::new(0))
                .fetch_add(value, Ordering::Relaxed);
        });
    }

    pub fn set_gauge(&self, name: &str, value: f64) {
        let gauges = self.gauges.clone();
        let name = name.to_string();
        tokio::spawn(async move {
            let mut gauges = gauges.write().await;
            let gauge = gauges
                .entry(name)
                .or_insert_with(|| Arc::new(RwLock::new(0.0)));
            *gauge.write().await = value;
        });
    }

    pub fn record_histogram(&self, name: &str, value: f64) {
        let histograms = self.histograms.clone();
        let name = name.to_string();
        tokio::spawn(async move {
            let mut histograms = histograms.write().await;
            let hist = histograms.entry(name).or_insert_with(Vec::new);
            hist.push(value);
            // Keep only last 1000 values
            if hist.len() > 1000 {
                hist.drain(0..hist.len() - 1000);
            }
        });
    }

    pub async fn get_metrics(&self) -> serde_json::Value {
        let counters = self.counters.read().await;
        let gauges = self.gauges.read().await;
        let histograms = self.histograms.read().await;

        let mut result = serde_json::Map::new();

        // Counters
        let mut counters_map = serde_json::Map::new();
        for (name, counter) in counters.iter() {
            counters_map.insert(
                name.clone(),
                serde_json::Value::Number(serde_json::Number::from(
                    counter.load(Ordering::Relaxed),
                )),
            );
        }
        result.insert("counters".to_string(), Value::Object(counters_map));

        // Gauges
        let mut gauges_map = serde_json::Map::new();
        for (name, gauge) in gauges.iter() {
            let value = *gauge.read().await;
            gauges_map.insert(
                name.clone(),
                Value::Number(
                    serde_json::Number::from_f64(value).unwrap_or(serde_json::Number::from(0)),
                ),
            );
        }
        result.insert("gauges".to_string(), Value::Object(gauges_map));

        // Histograms (summary stats)
        let mut histograms_map = serde_json::Map::new();
        for (name, hist) in histograms.iter() {
            if !hist.is_empty() {
                let sum: f64 = hist.iter().sum();
                let count = hist.len() as f64;
                let mean = sum / count;
                let min = hist.iter().fold(f64::INFINITY, |a, &b| a.min(b));
                let max = hist.iter().fold(f64::NEG_INFINITY, |a, &b| a.max(b));

                let mut stats = serde_json::Map::new();
                stats.insert(
                    "count".to_string(),
                    Value::Number(serde_json::Number::from(hist.len())),
                );
                stats.insert(
                    "sum".to_string(),
                    Value::Number(serde_json::Number::from_f64(sum).unwrap()),
                );
                stats.insert(
                    "mean".to_string(),
                    Value::Number(serde_json::Number::from_f64(mean).unwrap()),
                );
                stats.insert(
                    "min".to_string(),
                    Value::Number(serde_json::Number::from_f64(min).unwrap()),
                );
                stats.insert(
                    "max".to_string(),
                    Value::Number(serde_json::Number::from_f64(max).unwrap()),
                );

                histograms_map.insert(name.clone(), Value::Object(stats));
            }
        }
        result.insert("histograms".to_string(), Value::Object(histograms_map));

        Value::Object(result)
    }

    async fn collect_all(
        counters: &Arc<RwLock<HashMap<String, AtomicU64>>>,
        gauges: &Arc<RwLock<HashMap<String, Arc<RwLock<f64>>>>>,
        histograms: &Arc<RwLock<HashMap<String, Vec<f64>>>>,
    ) -> serde_json::Value {
        let metrics = MetricsCollector {
            config: MetricsConfig::default(),
            counters: counters.clone(),
            gauges: gauges.clone(),
            histograms: histograms.clone(),
            exporters: vec![],
            running: Arc::new(tokio::sync::Mutex::new(true)),
        };

        metrics.get_metrics().await
    }

    async fn export(exporter: &MetricsExporter, _metrics: &serde_json::Value) -> Result<()> {
        match exporter {
            MetricsExporter::Prometheus => {
                // Export to Prometheus format
                debug!("Exporting to Prometheus");
            }
            MetricsExporter::JsonFile => {
                // Write to JSON file
                debug!("Exporting to JSON file");
            }
            MetricsExporter::InfluxDB => {
                // Export to InfluxDB
                debug!("Exporting to InfluxDB");
            }
            MetricsExporter::StatsD => {
                // Export to StatsD
                debug!("Exporting to StatsD");
            }
        }
        Ok(())
    }

    async fn cleanup_histograms(histograms: &Arc<RwLock<HashMap<String, Vec<f64>>>>) {
        let mut histograms = histograms.write().await;
        for (_, hist) in histograms.iter_mut() {
            if hist.len() > 1000 {
                hist.drain(0..hist.len() - 1000);
            }
        }
    }

    // Predefined metrics helpers
    pub fn increment_containers_created(&self) {
        self.increment_counter("containers.created", 1);
    }

    pub fn increment_containers_destroyed(&self) {
        self.increment_counter("containers.destroyed", 1);
    }

    pub fn increment_containers_started(&self) {
        self.increment_counter("containers.started", 1);
    }

    pub fn increment_containers_stopped(&self) {
        self.increment_counter("containers.stopped", 1);
    }

    pub fn increment_containers_failed(&self) {
        self.increment_counter("containers.failed", 1);
    }

    pub fn set_active_containers(&self, count: u64) {
        self.set_gauge("containers.active", count as f64);
    }

    pub fn record_container_startup_time(&self, duration_ms: f64) {
        self.record_histogram("container.startup_time_ms", duration_ms);
    }

    pub fn record_container_memory_usage(&self, container: &str, bytes: u64) {
        self.set_gauge(
            &format!("container.{}.memory_bytes", container),
            bytes as f64,
        );
    }

    pub fn record_container_cpu_usage(&self, container: &str, percent: f64) {
        self.set_gauge(&format!("container.{}.cpu_percent", container), percent);
    }

    pub fn record_gateway_uptime(&self, seconds: u64) {
        self.set_gauge("gateway.uptime_seconds", seconds as f64);
    }

    pub fn record_gateway_memory_usage(&self, bytes: u64) {
        self.set_gauge("gateway.memory_bytes", bytes as f64);
    }

    pub fn record_gateway_cpu_usage(&self, percent: f64) {
        self.set_gauge("gateway.cpu_percent", percent);
    }

    pub fn record_api_request(&self, endpoint: &str, status: u16, duration_ms: f64) {
        self.increment_counter(&format!("api.requests.{}.{}", endpoint, status), 1);
        self.record_histogram(&format!("api.duration.{}.ms", endpoint), duration_ms);
    }

    pub fn record_error(&self, error_type: &str) {
        self.increment_counter(&format!("errors.{}", error_type), 1);
    }
}
