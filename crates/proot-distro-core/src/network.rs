//! Network module - Stub for future implementation

use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;

/// Network configuration
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct NetworkConfig {
    pub name: String,
    pub driver: String,
    pub subnet: Option<String>,
    pub gateway: Option<String>,
    pub dns_servers: Vec<String>,
    pub enable_ipv6: bool,
    pub mtu: Option<u16>,
    /// Containers attached to this network.
    ///
    /// `NetworkInfo` has always exposed this list, but the persisted struct had
    /// no field for it, so membership could never survive a process restart —
    /// which is why `network connect`/`disconnect` were never written.
    #[serde(default)]
    pub containers: Vec<String>,
    /// Static address per attached container, when `connect --ip` was used.
    #[serde(default)]
    pub ips: HashMap<String, String>,
}

/// Network information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetworkInfo {
    pub name: String,
    pub driver: String,
    pub subnet: Option<String>,
    pub gateway: Option<String>,
    pub dns_servers: Vec<String>,
    pub enable_ipv6: bool,
    pub mtu: Option<u16>,
    pub containers: Vec<String>,
}

/// Network manager
pub struct NetworkManager {
    networks: HashMap<String, NetworkConfig>,
    config_path: PathBuf,
}

impl NetworkManager {
    pub fn new() -> Result<Self, anyhow::Error> {
        let config_dir = dirs::config_dir()
            .unwrap_or_else(|| PathBuf::from("/tmp"))
            .join("podnext");
        std::fs::create_dir_all(&config_dir)?;
        let config_path = config_dir.join("networks.toml");

        let mut manager = Self {
            networks: HashMap::new(),
            config_path,
        };
        manager.load()?;
        Ok(manager)
    }

    fn load(&mut self) -> Result<()> {
        if self.config_path.exists() {
            let content = std::fs::read_to_string(&self.config_path)?;
            let networks: HashMap<String, NetworkConfig> = toml::from_str(&content)?;
            self.networks = networks;
        }
        Ok(())
    }

    fn save(&self) -> Result<()> {
        let content = toml::to_string_pretty(&self.networks)?;
        std::fs::write(&self.config_path, content)?;
        Ok(())
    }

    pub fn create(&mut self, config: &NetworkConfig) -> Result<()> {
        if self.networks.contains_key(&config.name) {
            anyhow::bail!("Network '{}' already exists", config.name);
        }
        self.networks.insert(config.name.clone(), config.clone());
        self.save()?;
        Ok(())
    }

    pub fn remove(&mut self, name: &str) -> Result<()> {
        if self.networks.remove(name).is_some() {
            self.save()?;
            Ok(())
        } else {
            anyhow::bail!("Network '{}' not found", name)
        }
    }

    pub fn list(&self) -> Result<Vec<NetworkInfo>> {
        let infos: Vec<NetworkInfo> = self
            .networks
            .iter()
            .map(|(_, config)| NetworkInfo {
                name: config.name.clone(),
                driver: config.driver.clone(),
                subnet: config.subnet.clone(),
                gateway: config.gateway.clone(),
                dns_servers: config.dns_servers.clone(),
                enable_ipv6: config.enable_ipv6,
                mtu: config.mtu,
                containers: config.containers.clone(),
            })
            .collect();
        Ok(infos)
    }

    pub fn inspect(&self, name: &str) -> Result<NetworkInfo> {
        if let Some(config) = self.networks.get(name) {
            Ok(NetworkInfo {
                name: config.name.clone(),
                driver: config.driver.clone(),
                subnet: config.subnet.clone(),
                gateway: config.gateway.clone(),
                dns_servers: config.dns_servers.clone(),
                enable_ipv6: config.enable_ipv6,
                mtu: config.mtu,
                containers: config.containers.clone(),
            })
        } else {
            anyhow::bail!("Network '{}' not found", name)
        }
    }

    /// Attach `container` to `network`.
    ///
    /// Membership is recorded and persisted. proot offers no network namespace,
    /// so the observable effect is DNS: the container's `/etc/resolv.conf` is
    /// rewritten to the network's `dns_servers` (when it defines any), which a
    /// guest can verify with `getent hosts`/`cat /etc/resolv.conf`.
    pub fn connect(
        &mut self,
        network: &str,
        container: &str,
        ip: Option<&str>,
        rootfs: Option<&std::path::Path>,
    ) -> Result<()> {
        let cfg = self
            .networks
            .get_mut(network)
            .ok_or_else(|| anyhow::anyhow!("Network '{}' not found", network))?;

        if cfg.containers.iter().any(|c| c == container) {
            anyhow::bail!(
                "Container '{}' is already attached to network '{}'",
                container,
                network
            );
        }

        cfg.containers.push(container.to_string());
        if let Some(addr) = ip {
            cfg.ips.insert(container.to_string(), addr.to_string());
        }
        let dns = cfg.dns_servers.clone();
        self.save()?;

        if let Some(rootfs) = rootfs {
            apply_resolv_conf(rootfs, &dns)?;
        }
        Ok(())
    }

    /// Detach `container` from `network`, restoring the default resolver.
    pub fn disconnect(
        &mut self,
        network: &str,
        container: &str,
        rootfs: Option<&std::path::Path>,
    ) -> Result<()> {
        let cfg = self
            .networks
            .get_mut(network)
            .ok_or_else(|| anyhow::anyhow!("Network '{}' not found", network))?;

        let before = cfg.containers.len();
        cfg.containers.retain(|c| c != container);
        if cfg.containers.len() == before {
            anyhow::bail!(
                "Container '{}' is not attached to network '{}'",
                container,
                network
            );
        }
        cfg.ips.remove(container);
        self.save()?;

        if let Some(rootfs) = rootfs {
            apply_resolv_conf(rootfs, &[])?;
        }
        Ok(())
    }
}

/// Write the guest's `/etc/resolv.conf`.
///
/// An empty `dns` restores the default public resolvers that `install` writes,
/// so `disconnect` leaves the container in the state it started in.
fn apply_resolv_conf(rootfs: &std::path::Path, dns: &[String]) -> Result<()> {
    const DEFAULTS: [&str; 3] = ["8.8.8.8", "8.8.4.4", "1.1.1.1"];

    let servers: Vec<String> = if dns.is_empty() {
        DEFAULTS.iter().map(|s| s.to_string()).collect()
    } else {
        dns.to_vec()
    };

    let etc = rootfs.join("etc");
    std::fs::create_dir_all(&etc)?;
    let body: String = servers
        .iter()
        .map(|s| format!("nameserver {}\n", s))
        .collect();
    std::fs::write(etc.join("resolv.conf"), body)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp_manager(tag: &str) -> NetworkManager {
        let dir = std::env::temp_dir().join(format!("pdn-net-{}-{}", tag, std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        NetworkManager {
            networks: HashMap::new(),
            config_path: dir.join("networks.toml"),
        }
    }

    fn cfg(name: &str, dns: &[&str]) -> NetworkConfig {
        NetworkConfig {
            name: name.to_string(),
            driver: "bridge".to_string(),
            dns_servers: dns.iter().map(|s| s.to_string()).collect(),
            ..Default::default()
        }
    }

    #[test]
    fn connect_records_membership_and_survives_reload() {
        let mut m = tmp_manager("connect");
        m.create(&cfg("front", &["9.9.9.9"])).unwrap();

        let rootfs = std::env::temp_dir().join(format!("pdn-net-rootfs-{}", std::process::id()));
        std::fs::create_dir_all(rootfs.join("etc")).unwrap();

        m.connect("front", "debian", Some("10.0.0.5"), Some(&rootfs))
            .unwrap();

        // The list now reports membership instead of always being empty.
        let info = m.inspect("front").unwrap();
        assert_eq!(info.containers, vec!["debian".to_string()]);

        // DNS actually applied inside the guest rootfs.
        let resolv = std::fs::read_to_string(rootfs.join("etc/resolv.conf")).unwrap();
        assert_eq!(resolv, "nameserver 9.9.9.9\n");

        // Persisted: a fresh manager reading the same file sees it.
        let mut reloaded = NetworkManager {
            networks: HashMap::new(),
            config_path: m.config_path.clone(),
        };
        reloaded.load().unwrap();
        assert_eq!(reloaded.inspect("front").unwrap().containers.len(), 1);
        assert_eq!(reloaded.inspect("front").unwrap().containers[0], "debian");

        // Double connect is refused, disconnect restores defaults.
        assert!(m.connect("front", "debian", None, None).is_err());
        m.disconnect("front", "debian", Some(&rootfs)).unwrap();
        assert!(m.inspect("front").unwrap().containers.is_empty());
        assert!(m.disconnect("front", "debian", None).is_err());
        let resolv = std::fs::read_to_string(rootfs.join("etc/resolv.conf")).unwrap();
        assert!(resolv.contains("8.8.8.8"));

        let _ = std::fs::remove_dir_all(&rootfs);
        let _ = std::fs::remove_dir_all(m.config_path.parent().unwrap());
    }
}
