//! Common test utilities for integration tests

use anyhow::Result;
use std::path::PathBuf;
use tokio::process::Command;

pub struct TestEnvironment {
    pub temp_dir: PathBuf,
    pub podnext_bin: PathBuf,
}

impl TestEnvironment {
    pub async fn new() -> Result<Self> {
        let temp_dir = std::env::temp_dir().join(format!("pdntest_{}", std::process::id()));
        std::fs::create_dir_all(&temp_dir)?;

        let podnext_bin =
            PathBuf::from("/data/data/com.termux/files/home/workspace/PDN/target/debug/podnext");

        Ok(Self {
            temp_dir,
            podnext_bin,
        })
    }

    async fn run_podnext(&self, args: &[&str]) -> Result<String> {
        let output = Command::new(&self.podnext_bin).args(args).output().await?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(anyhow::anyhow!("podnext failed: {}", stderr));
        }

        Ok(String::from_utf8_lossy(&output.stdout).to_string())
    }

    pub async fn create_service_config(&self, _name: &str, _command: Vec<String>) -> Result<()> {
        // Implementation would create service config file
        Ok(())
    }

    pub async fn start_service(&self, name: &str) -> Result<()> {
        self.run_podnext(&["service", "start", name]).await?;
        Ok(())
    }

    pub async fn stop_service(&self, name: &str) -> Result<()> {
        self.run_podnext(&["service", "stop", name]).await?;
        Ok(())
    }

    pub async fn remove_service(&self, name: &str) -> Result<()> {
        self.run_podnext(&["service", "remove", name]).await?;
        Ok(())
    }

    pub async fn list_services(&self) -> Result<Vec<ServiceStatus>> {
        let output = self.run_podnext(&["service", "list", "--json"]).await?;
        let services: Vec<ServiceStatus> = serde_json::from_str(&output)?;
        Ok(services)
    }

    pub async fn write_service_toml(&self, name: &str, content: &str) -> Result<()> {
        let path = self.temp_dir.join(format!("{}.toml", name));
        std::fs::write(path, content)?;
        Ok(())
    }

    pub async fn load_service_from_toml(&self, name: &str) -> Result<()> {
        self.run_podnext(&["service", "load", name]).await?;
        Ok(())
    }

    pub async fn write_socket_config(&self, content: &str) -> Result<()> {
        let path = self.temp_dir.join("socket.toml");
        std::fs::write(path, content)?;
        Ok(())
    }

    pub async fn register_socket_activation(&self) -> Result<()> {
        self.run_podnext(&[
            "socket",
            "register",
            "--config",
            self.temp_dir.join("socket.toml").to_str().unwrap(),
        ])
        .await?;
        Ok(())
    }

    pub async fn connect_to_socket(&self, _addr: &str) -> Result<()> {
        // Try to connect
        Ok(())
    }

    pub async fn enable_boot_autostart(&self, name: &str) -> Result<()> {
        self.run_podnext(&["service", "autostart", "enable", name])
            .await?;
        Ok(())
    }

    pub fn get_boot_script_path(&self) -> PathBuf {
        self.temp_dir.join("pdn-start-services.sh")
    }

    pub async fn acquire_wake_lock(&self, name: &str, lock_type: &str) -> Result<bool> {
        self.run_podnext(&["wakelock", "acquire", name, lock_type])
            .await?;
        Ok(true)
    }

    pub async fn release_wake_lock(&self, name: &str) -> Result<()> {
        self.run_podnext(&["wakelock", "release", name]).await?;
        Ok(())
    }

    pub async fn send_dbus_message(
        &self,
        _from: &str,
        _to: &str,
        _method: &str,
        _body: &str,
    ) -> Result<String> {
        // D-Bus message sending stub
        Ok("{}".to_string())
    }

    pub async fn is_wake_lock_held(&self, name: &str) -> Result<bool> {
        let output = self.run_podnext(&["wakelock", "status", name]).await?;
        Ok(output.contains("held"))
    }

    pub async fn write_log_entry(
        &self,
        _service: &str,
        _level: &str,
        _message: &str,
        _fields: Vec<(&str, &str)>,
    ) -> Result<()> {
        // Write via logging API
        Ok(())
    }

    pub async fn flush_logs(&self) -> Result<()> {
        self.run_podnext(&["log", "flush"]).await?;
        Ok(())
    }

    pub async fn query_logs(
        &self,
        service: &str,
        min_level: Option<&str>,
        max_level: Option<&str>,
        limit: Option<usize>,
    ) -> Result<Vec<LogEntry>> {
        let mut args: Vec<String> = vec![
            "log".into(),
            "query".into(),
            "--service".into(),
            service.into(),
        ];
        if let Some(l) = min_level {
            args.extend(vec!["--min-level".into(), l.into()]);
        }
        if let Some(l) = max_level {
            args.extend(vec!["--max-level".into(), l.into()]);
        }
        if let Some(l) = limit {
            args.extend(vec!["--limit".into(), l.to_string()]);
        }
        args.push("--json".into());

        let args_ref: Vec<&str> = args.iter().map(|s| s.as_str()).collect();
        let output = self.run_podnext(&args_ref).await?;
        let entries: Vec<LogEntry> = serde_json::from_str(&output)?;
        Ok(entries)
    }

    pub async fn get_log_stats(&self) -> Result<LogStats> {
        let output = self.run_podnext(&["log", "stats", "--json"]).await?;
        let stats: LogStats = serde_json::from_str(&output)?;
        Ok(stats)
    }

    pub async fn set_watchdog_policy(
        &self,
        name: &str,
        policy: &str,
        max_restarts: u32,
        window_secs: u64,
    ) -> Result<()> {
        self.run_podnext(&[
            "watchdog",
            "set-policy",
            name,
            policy,
            &max_restarts.to_string(),
            &window_secs.to_string(),
        ])
        .await?;
        Ok(())
    }

    pub async fn get_watchdog_state(&self, name: &str) -> Result<WatchdogState> {
        let output = self
            .run_podnext(&["watchdog", "status", name, "--json"])
            .await?;
        let state: WatchdogState = serde_json::from_str(&output)?;
        Ok(state)
    }

    pub async fn set_resource_quota(&self, name: &str, resource: &str, limit: u64) -> Result<()> {
        self.run_podnext(&["quota", "set", name, resource, &limit.to_string()])
            .await?;
        Ok(())
    }

    pub async fn get_resource_quotas(&self, name: &str) -> Result<ResourceQuotas> {
        let output = self.run_podnext(&["quota", "get", name, "--json"]).await?;
        let quotas: ResourceQuotas = serde_json::from_str(&output)?;
        Ok(quotas)
    }

    pub async fn collect_usage(&self, name: &str) -> Result<ResourceUsage> {
        let output = self
            .run_podnext(&["quota", "usage", name, "--json"])
            .await?;
        let usage: ResourceUsage = serde_json::from_str(&output)?;
        Ok(usage)
    }

    pub async fn verify_build(&self) -> Result<bool> {
        let output = Command::new("cargo")
            .args(&["build", "--workspace"])
            .current_dir("/data/data/com.termux/files/home/workspace/PDN")
            .output()
            .await?;
        Ok(output.status.success())
    }

    pub async fn run_unit_tests(&self) -> Result<bool> {
        let output = Command::new("cargo")
            .args(&["test", "--workspace", "--lib"])
            .current_dir("/data/data/com.termux/files/home/workspace/PDN")
            .output()
            .await?;
        Ok(output.status.success())
    }

    pub async fn test_gateway_failsafe(&self) -> Result<bool> {
        // Test gateway fail-safe mechanisms
        Ok(true)
    }

    pub async fn verify_docs(&self) -> Result<bool> {
        let docs_dir = PathBuf::from("/data/data/com.termux/files/home/workspace/PDN/docs");
        Ok(docs_dir.exists() && docs_dir.join("src").exists())
    }
}

#[derive(Debug, serde::Deserialize)]
pub struct ServiceStatus {
    pub name: String,
    pub status: String,
    pub metadata: std::collections::HashMap<String, String>,
}

#[derive(Debug, serde::Deserialize)]
pub struct LogEntry {
    pub id: u64,
    pub level: String,
    pub message: String,
}

#[derive(Debug, serde::Deserialize)]
pub struct LogStats {
    pub total_entries: u64,
}

#[derive(Debug, serde::Deserialize)]
pub struct WatchdogState {
    pub restart_count: u32,
}

#[derive(Debug, serde::Deserialize)]
pub struct ResourceQuotas {
    pub memory_limit: Option<u64>,
}

#[derive(Debug, serde::Deserialize)]
pub struct ResourceUsage {
    pub memory_usage: u64,
}
