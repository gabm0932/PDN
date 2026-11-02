//! Security hardening for PRoot containers (re-export from proot-compat)
//!
//! This module re-exports security hardening types from proot-compat
//! to provide a unified API for container security hardening.

pub use proot_compat::{
    AppArmorProfile, CapabilityConfig, SeccompAction, SeccompArch, SeccompArg, SeccompOp,
    SeccompProfile, SeccompSyscallRule, SecurityHardeningManager, SecurityProfileConfig,
    SecurityProfileType, SelLinuxProfile,
};

// Additional proot-distro-core specific security types
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Extended security profile for proot-distro-core containers
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DistroSecurityProfile {
    pub base: SecurityProfileConfig,
    pub network_policy: Option<NetworkSecurityPolicy>,
    pub filesystem_policy: Option<FilesystemSecurityPolicy>,
}

impl Default for DistroSecurityProfile {
    fn default() -> Self {
        Self {
            base: SecurityProfileConfig::default(),
            network_policy: Some(NetworkSecurityPolicy::default()),
            filesystem_policy: Some(FilesystemSecurityPolicy::default()),
        }
    }
}

/// Network security policy for containers
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetworkSecurityPolicy {
    pub allow_inbound: bool,
    pub allow_outbound: bool,
    pub allowed_ports: Vec<u16>,
    pub blocked_ports: Vec<u16>,
    pub allowed_ips: Vec<String>,
    pub blocked_ips: Vec<String>,
    pub dns_policy: DnsPolicy,
}

impl Default for NetworkSecurityPolicy {
    fn default() -> Self {
        Self {
            allow_inbound: false,
            allow_outbound: true,
            allowed_ports: vec![],
            blocked_ports: vec![],
            allowed_ips: vec![],
            blocked_ips: vec![],
            dns_policy: DnsPolicy::default(),
        }
    }
}

/// DNS policy
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum DnsPolicy {
    AllowAll,
    AllowList(Vec<String>),
    BlockList(Vec<String>),
    UseHost,
}

impl Default for DnsPolicy {
    fn default() -> Self {
        DnsPolicy::UseHost
    }
}

/// Filesystem security policy
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FilesystemSecurityPolicy {
    pub read_only_rootfs: bool,
    pub no_exec: Vec<String>,
    pub no_suid: Vec<String>,
    pub no_dev: Vec<String>,
    pub allowed_mounts: Vec<String>,
    pub blocked_mounts: Vec<String>,
}

impl Default for FilesystemSecurityPolicy {
    fn default() -> Self {
        Self {
            read_only_rootfs: false,
            no_exec: vec!["/tmp".to_string(), "/var/tmp".to_string()],
            no_suid: vec!["/".to_string()],
            no_dev: vec![],
            allowed_mounts: vec![],
            blocked_mounts: vec!["proc".to_string(), "sys".to_string()],
        }
    }
}

/// Distro security manager (extends base SecurityHardeningManager)
pub struct DistroSecurityManager {
    base: SecurityHardeningManager,
    profiles: HashMap<String, DistroSecurityProfile>,
}

impl DistroSecurityManager {
    pub fn new() -> anyhow::Result<Self> {
        Ok(Self {
            base: SecurityHardeningManager::new()?,
            profiles: HashMap::new(),
        })
    }

    pub fn register_profile(&mut self, name: String, config: DistroSecurityProfile) {
        self.profiles.insert(name, config);
    }

    pub fn get_profile(&self, name: &str) -> Option<&DistroSecurityProfile> {
        self.profiles.get(name)
    }

    pub async fn apply_profile(
        &self,
        profile_name: &str,
        container_rootfs: &std::path::Path,
    ) -> anyhow::Result<()> {
        let profile = self
            .profiles
            .get(profile_name)
            .ok_or_else(|| anyhow::anyhow!("Profile not found: {}", profile_name))?;

        // Apply base security profile
        self.base
            .apply_profile(&profile.base.name, container_rootfs)
            .await?;

        // Apply network policy
        if let Some(net_policy) = &profile.network_policy {
            self.apply_network_policy(net_policy).await?;
        }

        // Apply filesystem policy
        if let Some(fs_policy) = &profile.filesystem_policy {
            self.apply_filesystem_policy(fs_policy, container_rootfs)
                .await?;
        }

        Ok(())
    }

    async fn apply_network_policy(&self, _policy: &NetworkSecurityPolicy) -> anyhow::Result<()> {
        // Apply network policy (iptables, nftables, etc.)
        Ok(())
    }

    async fn apply_filesystem_policy(
        &self,
        _policy: &FilesystemSecurityPolicy,
        _rootfs: &std::path::Path,
    ) -> anyhow::Result<()> {
        // Apply filesystem policy (mount options, etc.)
        Ok(())
    }
}

// Re-export additional types

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_distro_security_profile_default() {
        let profile = DistroSecurityProfile::default();
        assert!(profile.base.enabled);
    }
}
