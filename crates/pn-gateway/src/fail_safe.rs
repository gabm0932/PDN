//! Fail-safe manager for PN Gateway

use super::config::{CircuitBreakerState, FailSafeAction, FailSafeConfig};
use anyhow::Result;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::Mutex;

/// Fail-safe manager for handling failures gracefully
pub struct FailSafeManager {
    config: FailSafeConfig,
    circuit_breakers: Arc<Mutex<HashMap<String, CircuitBreaker>>>,
    retry_counts: Arc<Mutex<HashMap<String, u32>>>,
}

impl FailSafeManager {
    pub fn new(config: FailSafeConfig) -> Self {
        Self {
            config,
            circuit_breakers: Arc::new(Mutex::new(HashMap::new())),
            retry_counts: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub fn config(&self) -> &FailSafeConfig {
        &self.config
    }

    pub async fn execute_with_failsafe<F, Fut, T>(&self, mut operation: F) -> Result<T>
    where
        F: FnMut() -> Fut,
        Fut: std::future::Future<Output = Result<T>>,
        T: Send,
    {
        if !self.config.enabled {
            return operation().await;
        }

        let mut last_error = None;

        for attempt in 0..=self.config.max_retries {
            match operation().await {
                Ok(result) => {
                    // Success - reset retry count
                    self.reset_retry_count("default").await;
                    return Ok(result);
                }
                Err(e) => {
                    last_error = Some(e);

                    // Check circuit breaker
                    if self.is_circuit_open("default").await {
                        return Err(anyhow::anyhow!("Circuit breaker is open"));
                    }

                    // Increment retry count
                    self.increment_retry_count("default").await;

                    // Check if we should retry
                    if attempt < self.config.max_retries {
                        // Wait before retry
                        tokio::time::sleep(self.config.retry_delay).await;
                        continue;
                    }
                }
            }
        }

        // All retries exhausted - execute fallback actions
        self.execute_fallback_actions().await;

        Err(last_error.unwrap_or_else(|| anyhow::anyhow!("Operation failed after retries")))
    }

    pub async fn is_circuit_open(&self, key: &str) -> bool {
        let breakers = self.circuit_breakers.lock().await;
        if let Some(breaker) = breakers.get(key) {
            breaker.is_open()
        } else {
            false
        }
    }

    pub async fn increment_retry_count(&self, key: &str) {
        let mut counts = self.retry_counts.lock().await;
        *counts.entry(key.to_string()).or_insert(0) += 1;
    }

    pub async fn reset_retry_count(&self, key: &str) {
        let mut counts = self.retry_counts.lock().await;
        counts.remove(key);
    }

    pub async fn execute_fallback_actions(&self) {
        for action in &self.config.fallback_actions {
            match action {
                FailSafeAction::Retry => {
                    // Already retried
                }
                FailSafeAction::FallbackToHost => {
                    // Execute on host instead
                    tracing::warn!("Falling back to host execution");
                }
                FailSafeAction::RestartContainer => {
                    // Restart the container
                    tracing::warn!("Restarting container as fallback");
                }
                FailSafeAction::RestartGateway => {
                    tracing::warn!("Restarting gateway as fallback");
                }
                FailSafeAction::GracefulShutdown => {
                    tracing::warn!("Initiating graceful shutdown");
                }
                FailSafeAction::AlertAdmin => {
                    tracing::error!("ALERT: Admin notification required");
                }
            }
        }
    }

    pub async fn record_failure(&self, key: &str) {
        let mut breakers = self.circuit_breakers.lock().await;
        let breaker = breakers
            .entry(key.to_string())
            .or_insert_with(CircuitBreaker::new);
        breaker.record_failure();
    }

    pub async fn record_success(&self, key: &str) {
        let mut breakers = self.circuit_breakers.lock().await;
        if let Some(breaker) = breakers.get_mut(key) {
            breaker.record_success();
        }
    }

    pub async fn get_circuit_state(&self, key: &str) -> Option<CircuitBreakerState> {
        let breakers = self.circuit_breakers.lock().await;
        breakers.get(key).map(|b| b.state)
    }
}

#[derive(Debug, Clone, Default)]
struct CircuitBreaker {
    state: CircuitBreakerState,
    failure_count: u32,
    success_count: u32,
    last_failure: Option<Instant>,
    last_state_change: Option<Instant>,
}

impl CircuitBreaker {
    fn new() -> Self {
        Self {
            state: CircuitBreakerState::Closed,
            failure_count: 0,
            success_count: 0,
            last_failure: None,
            last_state_change: None,
        }
    }

    fn record_failure(&mut self) {
        self.failure_count += 1;
        self.success_count = 0;
        self.last_failure = Some(Instant::now());

        if self.failure_count >= 5 {
            // threshold
            self.state = CircuitBreakerState::Open;
            self.last_state_change = Some(Instant::now());
        }
    }

    fn record_success(&mut self) {
        self.success_count += 1;
        self.failure_count = 0;

        if self.state == CircuitBreakerState::HalfOpen && self.success_count >= 3 {
            self.state = CircuitBreakerState::Closed;
            self.last_state_change = Some(Instant::now());
        }
    }

    fn is_open(&self) -> bool {
        match self.state {
            CircuitBreakerState::Open => {
                // Check if timeout has passed to move to half-open
                if let Some(last_change) = self.last_state_change {
                    if Instant::now().duration_since(last_change) > Duration::from_secs(60) {
                        // Move to half-open
                        return false; // Would need mutable access to change state
                    }
                }
                true
            }
            CircuitBreakerState::HalfOpen => false,
            CircuitBreakerState::Closed => false,
        }
    }
}
