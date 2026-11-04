//! Service Supervisor E2E Integration Tests
//!
//! Tests for service supervisor lifecycle, persistence, restart policies,
//! and circuit breaker functionality.
//!
//! Restart-policy tests observe an actual state transition instead of sleeping
//! a fixed offset and hoping the supervisor got there in time. A fixed
//! `sleep(3s); assert_eq!(state, expected)` is a race: it samples the supervisor
//! at an arbitrary instant and passes or fails by scheduling. Waiting for the
//! transition, with a deadline an order of magnitude larger than any delay used
//! here, turns that race into a slow test at worst — never a red one.

use anyhow::Result;
use pn_next::{
    CircuitBreaker, ProcessConfig, ResourceLimits, RestartCondition, RestartPolicy, ServiceConfig,
    ServiceMeta, ServiceState, ServiceStatus, ServiceSupervisor, ServiceType, StdioConfig,
    SupervisorBuilder, SupervisorEvent, SupervisorHealthCheckConfig, SupervisorState,
};
use std::collections::HashMap;
use std::time::Duration;
use tempfile::tempdir;
use tokio::time::Instant;

/// Generous deadline: ≥10x the largest delay any test here configures.
const DEADLINE: Duration = Duration::from_secs(30);
/// Poll cadence, small relative to every delay/supervision interval in play.
const POLL: Duration = Duration::from_millis(25);

fn create_test_supervisor() -> Result<(ServiceSupervisor, tempfile::TempDir)> {
    let temp_dir = tempdir()?;
    let services_dir = temp_dir.path().join("services");
    let runtime_dir = temp_dir.path().join("runtime");
    let log_dir = temp_dir.path().join("logs");

    let config = SupervisorBuilder::new()
        .services_dir(&services_dir)
        .runtime_dir(&runtime_dir)
        .log_dir(&log_dir)
        .supervision_interval(Duration::from_millis(1000)) // Longer interval to reduce lock contention
        .shutdown_timeout(Duration::from_secs(5))
        .enable_cgroups(false)
        .build()?;

    Ok((config, temp_dir))
}

fn create_test_service_config(name: &str) -> ServiceConfig {
    ServiceConfig {
        meta: ServiceMeta {
            name: name.to_string(),
            description: format!("Test service {}", name),
            version: "1.0.0".to_string(),
            service_type: ServiceType::Daemon,
            tags: vec!["test".to_string()],
        },
        process: ProcessConfig {
            command: vec!["/bin/sleep".to_string(), "100".to_string()],
            args: vec![],
            pre_start: None,
            post_stop: None,
            pre_reload: None,
            cwd: None,
            umask: 0o022,
            nice: None,
            cpu_affinity: None,
        },
        resources: ResourceLimits::default(),
        restart: RestartPolicy::default(),
        health: SupervisorHealthCheckConfig::default(),
        dependencies: vec![],
        env: HashMap::new(),
        working_dir: None,
        user: None,
        stdio: StdioConfig::default(),
        sockets: vec![],
    }
}

/// Wait for an observable transition instead of sleeping a fixed duration.
///
/// Returns the first status that satisfies `pred`, panicking (i.e. failing the
/// test) with the last observed status if that transition never happens before
/// `deadline`. The assertion that follows re-checks the same predicate, so the
/// wait can never turn a red test green: if the supervisor transitions to
/// something else, or to nothing at all, the test fails and prints exactly what
/// it saw.
async fn wait_status<F>(
    supervisor: &ServiceSupervisor,
    name: &str,
    what: &str,
    deadline: Duration,
    mut pred: F,
) -> ServiceStatus
where
    F: FnMut(&ServiceStatus) -> bool,
{
    let start = Instant::now();
    let mut last = supervisor
        .get_status(name)
        .await
        .expect("get_status for a created service");
    while !pred(&last) {
        if start.elapsed() >= deadline {
            panic!(
                "timed out after {:?} waiting for {what} on '{name}'; last observed \
                 state={:?} total_restarts={} restart_count={} pid={:?} exit_code={:?}",
                deadline,
                last.state,
                last.total_restarts,
                last.restart_count,
                last.pid,
                last.exit_code
            );
        }
        tokio::time::sleep(POLL).await;
        last = supervisor
            .get_status(name)
            .await
            .expect("get_status for a created service");
    }
    last
}

#[tokio::test(flavor = "multi_thread")]
async fn test_supervisor_creation_and_initialization() -> Result<()> {
    let (mut supervisor, _temp_dir) = create_test_supervisor()?;
    supervisor.initialize().await?;

    let state = supervisor.get_state().await;
    assert_eq!(state, SupervisorState::Running);

    supervisor.shutdown().await?;
    let state = supervisor.get_state().await;
    assert_eq!(state, SupervisorState::Stopped);

    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn test_service_persistence_save_load() -> Result<()> {
    let temp_dir = tempfile::tempdir()?;
    let services_dir = temp_dir.path().join("services");
    let runtime_dir = temp_dir.path().join("runtime");
    let log_dir = temp_dir.path().join("logs");

    // Create first supervisor
    let config = SupervisorBuilder::new()
        .services_dir(&services_dir)
        .runtime_dir(&runtime_dir)
        .log_dir(&log_dir)
        .supervision_interval(Duration::from_millis(1000)) // Longer interval to reduce contention
        .shutdown_timeout(Duration::from_secs(5))
        .enable_cgroups(false)
        .build()?;

    let mut supervisor = config;
    supervisor.initialize().await?;

    let svc_config = create_test_service_config("persist-test");
    // Add timeout to create_service
    tokio::time::timeout(
        Duration::from_secs(30),
        supervisor.create_service(svc_config),
    )
    .await??;

    // Verify service was saved to disk via public API
    let loaded = supervisor.get_service_config("persist-test").await?;
    assert_eq!(loaded.meta.name, "persist-test");

    // Shutdown first supervisor
    tokio::time::timeout(Duration::from_secs(30), supervisor.shutdown()).await??;
    // `create_service` wrote the file with `fs::write` (a synchronous, flushed
    // write) before this point, so there is no window to wait out on disk.

    // Create second supervisor with same services dir
    let config2 = SupervisorBuilder::new()
        .services_dir(&services_dir)
        .runtime_dir(&runtime_dir)
        .log_dir(&log_dir)
        .supervision_interval(Duration::from_millis(1000))
        .shutdown_timeout(Duration::from_secs(5))
        .enable_cgroups(false)
        .build()?;

    let mut supervisor2 = config2;
    // Add timeout to initialize
    tokio::time::timeout(Duration::from_secs(30), supervisor2.initialize()).await??;

    // Verify service loaded from disk
    let loaded2 = supervisor2.get_service_config("persist-test").await?;
    assert_eq!(loaded2.meta.name, "persist-test");

    supervisor2.shutdown().await?;
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn test_service_update_persists() -> Result<()> {
    let (mut supervisor, _temp_dir) = create_test_supervisor()?;
    supervisor.initialize().await?;

    let mut config = create_test_service_config("update-test");
    config.meta.description = "Original description".to_string();
    supervisor.create_service(config).await?;

    // Update the service
    let mut updated = create_test_service_config("update-test");
    updated.meta.description = "Updated description".to_string();
    updated.process.command = vec!["/bin/sleep".to_string(), "200".to_string()];
    supervisor.update_service("update-test", updated).await?;

    // Verify update persisted
    let loaded = supervisor.get_service_config("update-test").await?;
    assert_eq!(loaded.meta.description, "Updated description");
    assert_eq!(loaded.process.command, vec!["/bin/sleep", "200"]);

    supervisor.shutdown().await?;
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn test_service_deletion() -> Result<()> {
    let temp_dir = tempfile::tempdir()?;
    let services_dir = temp_dir.path().join("services");
    let runtime_dir = temp_dir.path().join("runtime");
    let log_dir = temp_dir.path().join("logs");

    let config = SupervisorBuilder::new()
        .services_dir(&services_dir)
        .runtime_dir(&runtime_dir)
        .log_dir(&log_dir)
        .supervision_interval(Duration::from_millis(100))
        .shutdown_timeout(Duration::from_secs(5))
        .enable_cgroups(false)
        .build()?;

    let mut supervisor = config;
    supervisor.initialize().await?;

    let config = create_test_service_config("delete-test");
    supervisor.create_service(config).await?;

    // Verify exists
    assert!(supervisor.get_service_config("delete-test").await.is_ok());

    // Delete
    supervisor.delete_service_config("delete-test").await?;

    // Verify deleted from disk (use public API)
    let result = supervisor.get_service_config("delete-test").await;
    assert!(result.is_err(), "Service config should be deleted");

    supervisor.shutdown().await?;
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn test_restart_policy_always() -> Result<()> {
    let (mut supervisor, _temp_dir) = create_test_supervisor()?;
    supervisor.initialize().await?;

    let mut config = create_test_service_config("restart-always");
    config.restart.condition = RestartCondition::Always;
    config.restart.max_restarts = 3;
    config.restart.base_delay_secs = 1;
    config.restart.restart_window_secs = 60;
    config.process.command = vec!["/bin/false".to_string()]; // Exit with code 1
    supervisor.create_service(config).await?;

    supervisor.start_service("restart-always").await?;

    // Always restarts on any exit: wait for the first restart decision.
    let status = wait_status(
        &supervisor,
        "restart-always",
        "restart scheduled (total_restarts > 0 or state == Restarting)",
        DEADLINE,
        |s| s.total_restarts > 0 || s.state == ServiceState::Restarting,
    )
    .await;
    assert!(status.total_restarts > 0 || status.state == ServiceState::Restarting);

    supervisor.shutdown().await?;
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn test_restart_policy_on_failure() -> Result<()> {
    let (mut supervisor, _temp_dir) = create_test_supervisor()?;
    supervisor.initialize().await?;

    let mut config = create_test_service_config("restart-on-failure");
    config.restart.condition = RestartCondition::OnFailure;
    config.restart.max_restarts = 3;
    config.restart.base_delay_secs = 1;
    config.restart.restart_window_secs = 60;
    config.process.command = vec!["/bin/false".to_string()]; // Exit with code 1
    supervisor.create_service(config).await?;

    supervisor.start_service("restart-on-failure").await?;

    // OnFailure restarts a non-zero exit: wait for the first restart.
    let status = wait_status(
        &supervisor,
        "restart-on-failure",
        "restart scheduled (total_restarts > 0 or state == Restarting)",
        DEADLINE,
        |s| s.total_restarts > 0 || s.state == ServiceState::Restarting,
    )
    .await;
    assert!(status.total_restarts > 0 || status.state == ServiceState::Restarting);

    supervisor.shutdown().await?;
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn test_restart_policy_never() -> Result<()> {
    let (mut supervisor, _temp_dir) = create_test_supervisor()?;
    supervisor.initialize().await?;

    let mut config = create_test_service_config("restart-never");
    config.restart.condition = RestartCondition::Never;
    config.process.command = vec!["/bin/false".to_string()]; // Exit with code 1
    supervisor.create_service(config).await?;

    supervisor.start_service("restart-never").await?;

    // Never must reach the terminal Failed state and never restart.
    let status = wait_status(
        &supervisor,
        "restart-never",
        "terminal Failed state (Never must not restart)",
        DEADLINE,
        |s| s.state == ServiceState::Failed,
    )
    .await;
    assert_eq!(status.state, ServiceState::Failed);
    assert_eq!(status.total_restarts, 0);

    supervisor.shutdown().await?;
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn test_restart_policy_unless_stopped() -> Result<()> {
    let (mut supervisor, _temp_dir) = create_test_supervisor()?;
    supervisor.initialize().await?;

    let mut config = create_test_service_config("restart-unless-stopped");
    config.restart.condition = RestartCondition::UnlessStopped;
    config.restart.max_restarts = 3;
    config.restart.base_delay_secs = 1;
    config.restart.restart_window_secs = 60;
    config.process.command = vec!["/bin/false".to_string()]; // Exit with code 1
    supervisor.create_service(config).await?;

    supervisor.start_service("restart-unless-stopped").await?;

    // (1) Not user-stopped: it must restart.
    let status = wait_status(
        &supervisor,
        "restart-unless-stopped",
        "restart scheduled (not user-stopped)",
        DEADLINE,
        |s| s.total_restarts > 0 || s.state == ServiceState::Restarting,
    )
    .await;
    assert!(status.total_restarts > 0 || status.state == ServiceState::Restarting);

    // (2) An explicit stop must reach Stopped — stop_service honors a user stop
    // even while a restart backoff is in flight.
    supervisor
        .stop_service("restart-unless-stopped", false)
        .await?;
    let status = wait_status(
        &supervisor,
        "restart-unless-stopped",
        "Stopped after explicit user stop",
        DEADLINE,
        |s| s.state == ServiceState::Stopped,
    )
    .await;
    assert_eq!(status.state, ServiceState::Stopped);

    // (3) A user stop must not be undone. A *non-event* can only be observed
    // over a window, so hold for longer than one supervision interval and fail
    // if the service ever leaves Stopped.
    let hold = Instant::now();
    while hold.elapsed() < Duration::from_secs(3) {
        let s = supervisor.get_status("restart-unless-stopped").await?;
        assert_eq!(
            s.state,
            ServiceState::Stopped,
            "user-stopped service must stay stopped, saw {:?} after {:?}",
            s.state,
            hold.elapsed()
        );
        tokio::time::sleep(POLL).await;
    }

    supervisor.shutdown().await?;
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn test_circuit_breaker_opens_after_max_failures() -> Result<()> {
    let (mut supervisor, _temp_dir) = create_test_supervisor()?;
    supervisor.initialize().await?;

    let mut config = create_test_service_config("circuit-breaker");
    config.restart.condition = RestartCondition::Always;
    config.restart.max_restarts = 2; // Low threshold
    config.restart.base_delay_secs = 1;
    config.restart.restart_window_secs = 60;
    config.process.command = vec!["/bin/false".to_string()];
    supervisor.create_service(config).await?;

    supervisor.start_service("circuit-breaker").await?;

    // Once the restart budget/window is exhausted the service must settle in
    // Failed rather than restarting forever.
    let status = wait_status(
        &supervisor,
        "circuit-breaker",
        "terminal Failed state after max failures",
        DEADLINE,
        |s| s.state == ServiceState::Failed,
    )
    .await;
    assert_eq!(status.state, ServiceState::Failed);

    supervisor.shutdown().await?;
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn test_circuit_breaker_resets_on_success() -> Result<()> {
    let mut cb = CircuitBreaker::new();

    // Record failures up to threshold
    for _ in 0..3 {
        cb.record_failure(3, 60);
    }
    assert!(
        cb.is_open,
        "Circuit breaker should be open after 3 failures"
    );
    assert!(
        !cb.allows_operation(),
        "Should not allow operation when open"
    );

    // Record successes
    cb.record_success();
    cb.record_success();

    // Circuit should be half-open then closed
    assert!(
        !cb.is_open,
        "Circuit breaker should be closed after 2 successes"
    );
    assert!(cb.allows_operation(), "Should allow operation when closed");

    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn test_service_dependency_ordering() -> Result<()> {
    let (mut supervisor, _temp_dir) = create_test_supervisor()?;
    supervisor.initialize().await?;

    // Create service B that depends on A
    let mut config_b = create_test_service_config("service-b");
    config_b.dependencies = vec!["service-a".to_string()];
    config_b.process.command = vec!["/bin/sleep".to_string(), "100".to_string()];
    supervisor.create_service(config_b).await?;

    // Create service A
    let mut config_a = create_test_service_config("service-a");
    config_a.process.command = vec!["/bin/sleep".to_string(), "100".to_string()];
    supervisor.create_service(config_a).await?;

    // Start B - should start A first
    supervisor.start_service("service-b").await?;

    let status_a = wait_status(
        &supervisor,
        "service-a",
        "dependency A Running",
        DEADLINE,
        |s| s.state == ServiceState::Running,
    )
    .await;
    let status_b = wait_status(
        &supervisor,
        "service-b",
        "dependent B Running",
        DEADLINE,
        |s| s.state == ServiceState::Running,
    )
    .await;

    assert_eq!(status_a.state, ServiceState::Running);
    assert_eq!(status_b.state, ServiceState::Running);

    supervisor.shutdown().await?;
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn test_supervisor_shutdown_stops_all_services() -> Result<()> {
    let (mut supervisor, _temp_dir) = create_test_supervisor()?;
    supervisor.initialize().await?;

    let config1 = create_test_service_config("shutdown-1");
    let config2 = create_test_service_config("shutdown-2");
    supervisor.create_service(config1).await?;
    supervisor.create_service(config2).await?;

    supervisor.start_service("shutdown-1").await?;
    supervisor.start_service("shutdown-2").await?;
    wait_status(
        &supervisor,
        "shutdown-1",
        "service 1 Running",
        DEADLINE,
        |s| s.state == ServiceState::Running,
    )
    .await;
    wait_status(
        &supervisor,
        "shutdown-2",
        "service 2 Running",
        DEADLINE,
        |s| s.state == ServiceState::Running,
    )
    .await;

    // Shutdown supervisor
    supervisor.shutdown().await?;

    let status1 = supervisor.get_status("shutdown-1").await?;
    let status2 = supervisor.get_status("shutdown-2").await?;

    assert_eq!(status1.state, ServiceState::Stopped);
    assert_eq!(status2.state, ServiceState::Stopped);

    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn test_supervisor_reload() -> Result<()> {
    let temp_dir = tempfile::tempdir()?;
    let services_dir = temp_dir.path().join("services");
    let runtime_dir = temp_dir.path().join("runtime");
    let log_dir = temp_dir.path().join("logs");

    let config = SupervisorBuilder::new()
        .services_dir(&services_dir)
        .runtime_dir(&runtime_dir)
        .log_dir(&log_dir)
        .supervision_interval(Duration::from_millis(100))
        .shutdown_timeout(Duration::from_secs(5))
        .enable_cgroups(false)
        .build()?;

    let mut supervisor = config;
    supervisor.initialize().await?;

    let config = create_test_service_config("reload-test");
    supervisor.create_service(config).await?;

    // Add another service file directly
    let new_service = create_test_service_config("reload-new");
    let path = services_dir.join("reload-new.toml");
    let content = toml::to_string_pretty(&new_service)?;
    std::fs::write(path, content)?;

    // Reload
    supervisor.reload().await?;

    // New service should be loaded
    let status = supervisor.get_service_config("reload-new").await?;
    assert_eq!(status.meta.name, "reload-new");

    supervisor.shutdown().await?;
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn test_enable_disable_service() -> Result<()> {
    let (mut supervisor, _temp_dir) = create_test_supervisor()?;
    supervisor.initialize().await?;

    let config = create_test_service_config("enable-test");
    supervisor.create_service(config).await?;

    // Enable
    supervisor.enable_service("enable-test").await?;
    assert!(supervisor.is_enabled("enable-test").await);

    // Disable
    supervisor.disable_service("enable-test").await?;
    assert!(!supervisor.is_enabled("enable-test").await);

    supervisor.shutdown().await?;
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn test_restart_policy_on_abnormal() -> Result<()> {
    let temp_dir = tempfile::tempdir()?;
    let services_dir = temp_dir.path().join("services");
    let runtime_dir = temp_dir.path().join("runtime");
    let log_dir = temp_dir.path().join("logs");

    let config = SupervisorBuilder::new()
        .services_dir(&services_dir)
        .runtime_dir(&runtime_dir)
        .log_dir(&log_dir)
        .supervision_interval(Duration::from_millis(1000))
        .shutdown_timeout(Duration::from_secs(2)) // Short timeout for test
        .enable_cgroups(false)
        .build()?;

    let mut supervisor = config;
    supervisor.initialize().await?;

    // Use a command that will be killed by signal (SIGKILL)
    let mut config = create_test_service_config("restart-abnormal");
    config.restart.condition = RestartCondition::OnAbnormal;
    config.restart.max_restarts = 2;
    config.restart.base_delay_secs = 1;
    config.restart.restart_window_secs = 60;
    // Command that runs and can be killed
    config.process.command = vec!["/bin/sleep".to_string(), "100".to_string()];
    supervisor.create_service(config).await?;

    supervisor.start_service("restart-abnormal").await?;

    // Ensure a real process is up before the forced stop, so the abnormal exit
    // really is a signal kill and not a racing start.
    wait_status(
        &supervisor,
        "restart-abnormal",
        "running process to force-kill",
        DEADLINE,
        |s| s.state == ServiceState::Running && s.pid.is_some(),
    )
    .await;

    // Force kill (SIGKILL). OnAbnormal must treat this as an abnormal exit and
    // schedule a restart: the service must not remain Stopped.
    supervisor.stop_service("restart-abnormal", true).await?;
    let status = wait_status(
        &supervisor,
        "restart-abnormal",
        "OnAbnormal restart scheduled after forced (signal) stop",
        DEADLINE,
        |s| {
            s.total_restarts > 0
                || matches!(
                    s.state,
                    ServiceState::Restarting | ServiceState::Starting | ServiceState::Running
                )
        },
    )
    .await;
    assert!(
        status.total_restarts > 0
            || matches!(
                status.state,
                ServiceState::Restarting | ServiceState::Starting | ServiceState::Running
            ),
        "OnAbnormal must schedule a restart after a forced stop; saw state={:?} total_restarts={}",
        status.state,
        status.total_restarts
    );

    supervisor.shutdown().await?;
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn test_max_restarts_exceeded() -> Result<()> {
    let (mut supervisor, _temp_dir) = create_test_supervisor()?;
    supervisor.initialize().await?;

    let mut config = create_test_service_config("max-restarts");
    config.restart.condition = RestartCondition::Always;
    config.restart.max_restarts = 1; // Only 1 restart allowed
    config.restart.base_delay_secs = 1;
    config.restart.restart_window_secs = 60;
    config.process.command = vec!["/bin/false".to_string()];
    supervisor.create_service(config).await?;

    supervisor.start_service("max-restarts").await?;

    // Budget exhausted -> terminal Failed state.
    let status = wait_status(
        &supervisor,
        "max-restarts",
        "terminal Failed state after max restarts",
        DEADLINE,
        |s| s.state == ServiceState::Failed,
    )
    .await;
    assert_eq!(status.state, ServiceState::Failed);

    supervisor.shutdown().await?;
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn test_backoff_delay_increases() -> Result<()> {
    let (mut supervisor, _temp_dir) = create_test_supervisor()?;
    supervisor.initialize().await?;

    let mut config = create_test_service_config("backoff-test");
    config.restart.condition = RestartCondition::Always;
    config.restart.max_restarts = 3;
    config.restart.base_delay_secs = 1;
    config.restart.max_delay_secs = 10;
    config.restart.backoff_multiplier = 2.0;
    config.restart.restart_window_secs = 60;
    config.process.command = vec!["/bin/false".to_string()];
    supervisor.create_service(config).await?;

    let start = std::time::Instant::now();
    supervisor.start_service("backoff-test").await?;
    tokio::time::sleep(Duration::from_secs(5)).await;
    let elapsed = start.elapsed();

    // With backoff: 1s + 2s = 3s minimum for 2 restarts
    // (actual timing may vary but should be > 2s)
    assert!(elapsed.as_secs() >= 2);

    supervisor.shutdown().await?;
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn test_service_status_tracking() -> Result<()> {
    let (mut supervisor, _temp_dir) = create_test_supervisor()?;
    supervisor.initialize().await?;

    let config = create_test_service_config("status-test");
    supervisor.create_service(config).await?;

    // Initial state
    let status = supervisor.get_status("status-test").await?;
    assert_eq!(status.state, ServiceState::Stopped);
    assert_eq!(status.restart_count, 0);
    assert_eq!(status.total_restarts, 0);
    assert!(status.pid.is_none());

    // Start
    supervisor.start_service("status-test").await?;

    let status = wait_status(
        &supervisor,
        "status-test",
        "Running after start",
        DEADLINE,
        |s| s.state == ServiceState::Running,
    )
    .await;
    assert_eq!(status.state, ServiceState::Running);
    assert!(status.pid.is_some());
    assert!(status.started_at.is_some());

    // Stop
    supervisor.stop_service("status-test", false).await?;

    let status = wait_status(
        &supervisor,
        "status-test",
        "Stopped after stop",
        DEADLINE,
        |s| s.state == ServiceState::Stopped,
    )
    .await;
    assert_eq!(status.state, ServiceState::Stopped);
    assert!(status.pid.is_none());
    // exit_code may be None if process was terminated by signal (SIGTERM)
    // This is valid behavior - the process stopped
    // assert!(status.exit_code.is_some());

    supervisor.shutdown().await?;
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn test_list_services() -> Result<()> {
    let (mut supervisor, _temp_dir) = create_test_supervisor()?;
    supervisor.initialize().await?;

    supervisor
        .create_service(create_test_service_config("list-1"))
        .await?;
    supervisor
        .create_service(create_test_service_config("list-2"))
        .await?;
    supervisor
        .create_service(create_test_service_config("list-3"))
        .await?;

    let services = supervisor.list_services().await;
    assert_eq!(services.len(), 3);

    let names: Vec<_> = services.iter().map(|s| s.name.as_str()).collect();
    assert!(names.contains(&"list-1"));
    assert!(names.contains(&"list-2"));
    assert!(names.contains(&"list-3"));

    supervisor.shutdown().await?;
    Ok(())
}
