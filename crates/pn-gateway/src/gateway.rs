//! Gateway module for PN Gateway

use super::config::GatewayState;
use proot_core::ProotEngine;
use std::sync::Arc;
use uuid::Uuid;

/// PN Gateway - orchestrates PRoot, PRoot-distro, and proot-compat
pub struct PnGateway {
    config: crate::config::GatewayConfig,
    state: Arc<tokio::sync::RwLock<GatewayState>>,
    proot_engine: Option<ProotEngine>,
    distro_core: Option<proot_distro_core::ProotDistroCore>,
    fail_safe_manager: super::fail_safe::FailSafeManager,
    guardrail_manager: super::guardrails::GuardrailManager,
    health_monitor: super::health::HealthMonitor,
    metrics_collector: super::metrics::MetricsCollector,
    gateway_id: uuid::Uuid,
}

impl PnGateway {
    pub async fn new(config: crate::config::GatewayConfig) -> anyhow::Result<Self> {
        let gateway_id = Uuid::new_v4();

        // Initialize proot-compat first
        proot_compat::init_compat()?;

        let fail_safe_manager = super::fail_safe::FailSafeManager::new(config.fail_safe.clone());
        let guardrail_manager = super::guardrails::GuardrailManager::new(config.guardrails.clone());
        let health_monitor = super::health::HealthMonitor::new(config.health.clone());
        let metrics_collector = super::metrics::MetricsCollector::new(config.metrics.clone());

        let gateway = Self {
            config,
            state: Arc::new(tokio::sync::RwLock::new(GatewayState::Initializing)),
            proot_engine: None,
            distro_core: None,
            fail_safe_manager,
            guardrail_manager,
            health_monitor,
            metrics_collector,
            gateway_id,
        };

        Ok(gateway)
    }

    pub async fn initialize(&mut self) -> anyhow::Result<()> {
        *self.state.write().await = GatewayState::Initializing;

        // Initialize proot-compat compatibility layer
        proot_compat::init_compat()?;

        // Initialize PRoot core
        let proot_config = proot_core::cli::ProotConfig::default();
        let proot_engine = proot_core::ProotEngine::new(proot_config)?;

        // Initialize PRoot-distro core
        let distro_core = proot_distro_core::ProotDistroCore::new()?;

        self.proot_engine = Some(proot_engine);
        self.distro_core = Some(distro_core);

        *self.state.write().await = GatewayState::Ready;

        // Start health monitoring
        self.health_monitor.start().await?;

        // Start metrics collection
        self.metrics_collector.start().await?;

        *self.state.write().await = GatewayState::Running;

        Ok(())
    }

    pub async fn run_container(
        &mut self,
        container_name: &str,
        _args: Vec<String>,
    ) -> anyhow::Result<()> {
        // Check guardrails before running
        self.guardrail_manager.check_all()?;

        // Check health
        if !self.health_monitor.is_healthy().await {
            return Err(anyhow::anyhow!("System unhealthy, cannot run container"));
        }

        // Run with fail-safe - manually implement retry logic
        let container_name = container_name.to_string();
        let mut last_error = None;

        for attempt in 0..=self.fail_safe_manager.config().max_retries {
            let result = async {
                if let Some(distro) = self.distro_core.as_mut() {
                    let mut options = proot_distro_core::LoginOptions::default();
                    options.container_name = container_name.clone();
                    options.command = vec!["/bin/bash".to_string()];
                    distro.login(options).await?;
                }
                Ok(())
            }
            .await;

            match result {
                Ok(()) => return Ok(()),
                Err(e) => {
                    last_error = Some(e);

                    // Check circuit breaker
                    if self.fail_safe_manager.is_circuit_open("default").await {
                        return Err(anyhow::anyhow!("Circuit breaker is open"));
                    }

                    // Increment retry count
                    self.fail_safe_manager
                        .increment_retry_count("default")
                        .await;

                    // Check if we should retry
                    if attempt < self.fail_safe_manager.config().max_retries {
                        tokio::time::sleep(self.fail_safe_manager.config().retry_delay).await;
                        continue;
                    }
                }
            }
        }

        // All retries exhausted - execute fallback actions
        self.fail_safe_manager.execute_fallback_actions().await;

        Err(last_error.unwrap_or_else(|| anyhow::anyhow!("Operation failed after retries")))
    }

    pub async fn shutdown(&mut self) -> anyhow::Result<()> {
        *self.state.write().await = GatewayState::ShuttingDown;

        // Stop health monitoring
        self.health_monitor.stop().await?;

        // Stop metrics collection
        self.metrics_collector.stop().await?;

        // Shutdown engines
        if let Some(mut engine) = self.proot_engine.take() {
            engine.shutdown()?;
        }

        *self.state.write().await = GatewayState::Stopped;

        Ok(())
    }

    pub async fn get_state(&self) -> crate::config::GatewayState {
        self.state.read().await.clone()
    }

    pub async fn get_health(&self) -> super::health::HealthStatus {
        self.health_monitor.get_status().await
    }

    pub async fn get_metrics(&self) -> serde_json::Value {
        self.metrics_collector.get_metrics().await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_gateway_creation() {
        let config = crate::config::GatewayConfig::default();
        let gateway = PnGateway::new(config).await;
        assert!(gateway.is_ok());
    }
}
