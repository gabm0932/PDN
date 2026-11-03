//! Network management for PN Next
//!
//! Provides network management with CNI (Container Network Interface) plugin support
//! for container networking.

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json;
use std::collections::HashMap;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::path::{Path, PathBuf};
use std::process::Command as StdCommand;
use std::sync::Arc;
use tokio::sync::RwLock;
use uuid::Uuid;

/// Network configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetworkConfig {
    pub bridge_name: String,
    pub subnet: String,
    pub gateway: Option<String>,
    pub dns_servers: Vec<String>,
    pub enable_ipv6: bool,
    pub mtu: u16,
    /// CNI configuration
    pub cni: CniConfig,
}

impl Default for NetworkConfig {
    fn default() -> Self {
        Self {
            bridge_name: "pn0".to_string(),
            subnet: "10.0.0.0/24".to_string(),
            gateway: Some("10.0.0.1".to_string()),
            dns_servers: vec!["8.8.8.8".to_string(), "1.1.1.1".to_string()],
            enable_ipv6: false,
            mtu: 1500,
            cni: CniConfig::default(),
        }
    }
}

/// CNI (Container Network Interface) configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CniConfig {
    pub plugin_dirs: Vec<PathBuf>,
    pub config_dir: PathBuf,
    pub default_network: String,
    pub plugins: Vec<CniPluginConfig>,
}

impl Default for CniConfig {
    fn default() -> Self {
        let mut dirs = Vec::new();
        dirs.push("/opt/cni/bin".into());
        dirs.push("/usr/libexec/cni".into());
        dirs.push("/usr/local/libexec/cni".into());

        Self {
            plugin_dirs: dirs,
            config_dir: "/etc/cni/net.d".into(),
            default_network: "cni-default".to_string(),
            plugins: vec![
                CniPluginConfig {
                    name: "bridge".to_string(),
                    type_: "bridge".to_string(),
                    config: serde_json::json!({
                        "isGateway": true,
                        "ipMasq": true,
                        "hairpinMode": true,
                        "ipam": {
                            "type": "host-local",
                            "subnet": "10.0.0.0/24",
                            "routes": [
                                { "dst": "0.0.0.0/0" }
                            ]
                        }
                    }),
                },
                CniPluginConfig {
                    name: "loopback".to_string(),
                    type_: "loopback".to_string(),
                    config: serde_json::json!({}),
                },
                CniPluginConfig {
                    name: "portmap".to_string(),
                    type_: "portmap".to_string(),
                    config: serde_json::json!({
                        "capabilities": { "portMappings": true }
                    }),
                },
                CniPluginConfig {
                    name: "bandwidth".to_string(),
                    type_: "bandwidth".to_string(),
                    config: serde_json::json!({
                        "capabilities": { "bandwidth": true }
                    }),
                },
            ],
        }
    }
}

/// CNI Plugin Configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CniPluginConfig {
    pub name: String,
    pub type_: String,
    pub config: serde_json::Value,
}

/// CNI Network configuration (CNI spec)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CniNetworkConfig {
    pub cni_version: String,
    pub name: String,
    pub plugins: Vec<CniPluginConfig>,
    pub ipam: Option<CniIpamConfig>,
}

/// CNI IPAM configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CniIpamConfig {
    pub type_: String,
    pub subnet: Option<String>,
    pub routes: Option<Vec<CniRoute>>,
    pub data_dir: Option<String>,
    pub ranges: Option<Vec<Vec<CniRange>>>,
}

/// CNI Route
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CniRoute {
    pub dst: String,
    pub gw: Option<String>,
}

/// CNI Range
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CniRange {
    pub subnet: String,
    pub range_start: Option<String>,
    pub range_end: Option<String>,
    pub gateway: Option<String>,
}

/// CNI Runtime arguments
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CniRuntimeArgs {
    pub cni_version: String,
    pub cni_command: String,
    pub cni_containerid: String,
    pub cni_netns: String,
    pub cni_ifname: String,
    pub cni_args: Option<String>,
    pub cni_path: Option<String>,
}

/// Network manager with CNI support
pub struct NetworkManager {
    config: NetworkConfig,
    networks: Arc<tokio::sync::RwLock<HashMap<String, Network>>>,
    interfaces: Arc<tokio::sync::RwLock<HashMap<String, NetworkInterface>>>,
    cni_cache: Arc<tokio::sync::RwLock<HashMap<String, CniNetworkConfig>>>,
}

impl NetworkManager {
    pub fn new(config: NetworkConfig) -> Result<Self> {
        Ok(Self {
            config,
            networks: Arc::new(tokio::sync::RwLock::new(HashMap::new())),
            interfaces: Arc::new(tokio::sync::RwLock::new(HashMap::new())),
            cni_cache: Arc::new(tokio::sync::RwLock::new(HashMap::new())),
        })
    }

    pub async fn initialize(&self) -> Result<()> {
        // Create default bridge network
        self.create_network(Network {
            id: Uuid::new_v4(),
            name: self.config.bridge_name.clone(),
            driver: "bridge".to_string(),
            subnet: self.config.subnet.clone(),
            gateway: self.config.gateway.clone(),
            dns_servers: self.config.dns_servers.clone(),
            enable_ipv6: self.config.enable_ipv6,
            mtu: self.config.mtu,
            created_at: chrono::Utc::now(),
            containers: Vec::new(),
        })
        .await?;

        // Load CNI configurations
        self.load_cni_configs().await?;

        Ok(())
    }

    /// Load CNI configurations from config directory
    async fn load_cni_configs(&self) -> Result<()> {
        let config_dir = &self.config.cni.config_dir;
        if !config_dir.exists() {
            // Create default CNI config
            self.create_default_cni_config().await?;
        }

        // Load existing CNI configs
        if config_dir.is_dir() {
            let mut entries = tokio::fs::read_dir(config_dir).await?;
            while let Some(entry) = entries.next_entry().await? {
                let path = entry.path();
                if path
                    .extension()
                    .map_or(false, |ext| ext == "conf" || ext == "json")
                {
                    if let Ok(content) = tokio::fs::read_to_string(&path).await {
                        if let Ok(config) =
                            serde_json::from_slice::<CniNetworkConfig>(content.as_bytes())
                        {
                            self.cni_cache
                                .write()
                                .await
                                .insert(config.name.clone(), config);
                        }
                    }
                }
            }
        }

        Ok(())
    }

    /// Create default CNI configuration
    async fn create_default_cni_config(&self) -> Result<()> {
        let config_dir = &self.config.cni.config_dir;
        tokio::fs::create_dir_all(config_dir).await?;

        let default_config = CniNetworkConfig {
            cni_version: "0.4.0".to_string(),
            name: self.config.cni.default_network.clone(),
            plugins: self.config.cni.plugins.clone(),
            ipam: Some(CniIpamConfig {
                type_: "host-local".to_string(),
                subnet: Some(self.config.subnet.clone()),
                routes: Some(vec![CniRoute {
                    dst: "0.0.0.0/0".to_string(),
                    gw: self.config.gateway.clone(),
                }]),
                data_dir: Some("/var/lib/cni/networks".to_string()),
                ranges: None,
            }),
        };

        let config_path = config_dir.join("10-default.conf");
        let content = serde_json::to_string_pretty(&default_config)?;
        tokio::fs::write(config_path, content).await?;

        self.cni_cache
            .write()
            .await
            .insert(default_config.name.clone(), default_config);

        Ok(())
    }

    pub async fn shutdown(&self) -> Result<()> {
        Ok(())
    }

    pub async fn create_network(&self, network: Network) -> Result<()> {
        let mut networks = self.networks.write().await;
        networks.insert(network.name.clone(), network);
        Ok(())
    }

    pub async fn remove_network(&self, name: &str) -> Result<()> {
        let mut networks = self.networks.write().await;
        networks.remove(name);
        Ok(())
    }

    pub async fn get_network(&self, name: &str) -> Option<Network> {
        self.networks.read().await.get(name).cloned()
    }

    pub async fn list_networks(&self) -> Vec<Network> {
        self.networks.read().await.values().cloned().collect()
    }

    /// Connect a container to a network using CNI
    pub async fn connect_container(
        &self,
        network_name: &str,
        container_name: &str,
    ) -> Result<NetworkInterface> {
        // Get network
        let network = self
            .get_network(network_name)
            .await
            .ok_or_else(|| anyhow::anyhow!("Network not found: {}", network_name))?;

        // Get CNI config for network
        let cni_config = self
            .cni_cache
            .read()
            .await
            .get(network_name)
            .cloned()
            .unwrap_or_else(|| CniNetworkConfig {
                cni_version: "0.4.0".to_string(),
                name: network_name.to_string(),
                plugins: self.config.cni.plugins.clone(),
                ipam: Some(CniIpamConfig {
                    type_: "host-local".to_string(),
                    subnet: Some(network.subnet.clone()),
                    routes: Some(vec![CniRoute {
                        dst: "0.0.0.0/0".to_string(),
                        gw: network.gateway.clone(),
                    }]),
                    data_dir: Some("/var/lib/cni/networks".to_string()),
                    ranges: None,
                }),
            });

        // Create container network namespace
        let container_id = Uuid::new_v4().to_string();
        let netns_path = format!("/var/run/netns/{}", container_name);

        // Create network namespace (simulated for now)
        tokio::fs::create_dir_all("/var/run/netns").await?;
        tokio::fs::write(&netns_path, "").await?;

        // Execute CNI ADD command
        let cni_result = self
            .cni_add(&cni_config, &container_id, container_name, &netns_path)
            .await?;

        // Extract first interface from CNI result
        let first_interface = cni_result
            .interfaces
            .into_iter()
            .next()
            .ok_or_else(|| anyhow::anyhow!("No interface returned from CNI plugin"))?;

        // Get IP address from CNI result
        let ip_address = cni_result
            .ips
            .into_iter()
            .next()
            .and_then(|ip| ip.address.parse().ok())
            .unwrap_or(IpAddr::V4(Ipv4Addr::new(10, 0, 0, 2)));

        // Store interface
        let net_iface = NetworkInterface {
            id: Uuid::new_v4(),
            name: first_interface.name.clone(),
            network_id: network.id,
            container_id: Some(Uuid::parse_str(&container_id).unwrap_or(Uuid::nil())),
            ip_address,
            mac_address: first_interface.mac.unwrap_or_else(|| {
                format!(
                    "02:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}",
                    rand::random::<u8>(),
                    rand::random::<u8>(),
                    rand::random::<u8>(),
                    rand::random::<u8>(),
                    rand::random::<u8>()
                )
            }),
            macvlan: false,
        };

        // Store interface
        self.interfaces
            .write()
            .await
            .insert(first_interface.name.clone(), net_iface.clone());

        // Update network
        let mut networks = self.networks.write().await;
        if let Some(net) = networks.get_mut(network_name) {
            net.containers.push(container_name.to_string());
        }

        Ok(net_iface)
    }

    /// Execute CNI ADD command
    async fn cni_add(
        &self,
        cni_config: &CniNetworkConfig,
        container_id: &str,
        container_name: &str,
        netns_path: &str,
    ) -> Result<CniResult> {
        // Build CNI args
        let args = CniRuntimeArgs {
            cni_version: cni_config.cni_version.clone(),
            cni_command: "ADD".to_string(),
            cni_containerid: container_id.to_string(),
            cni_netns: netns_path.to_string(),
            cni_ifname: "eth0".to_string(),
            cni_args: None,
            cni_path: Some(
                self.config
                    .cni
                    .plugin_dirs
                    .iter()
                    .map(|p| p.to_string_lossy())
                    .collect::<Vec<_>>()
                    .join(":"),
            ),
        };

        // Serialize network config
        let config_json = serde_json::to_string(cni_config)?;

        // Execute first plugin (typically bridge)
        if let Some(plugin) = cni_config.plugins.first() {
            let plugin_path = self.find_plugin(&plugin.type_)?;

            let mut cmd = StdCommand::new(&plugin_path);
            cmd.env("CNI_VERSION", &args.cni_version)
                .env("CNI_COMMAND", &args.cni_command)
                .env("CNI_CONTAINERID", &args.cni_containerid)
                .env("CNI_NETNS", &args.cni_netns)
                .env("CNI_IFNAME", &args.cni_ifname)
                .env("CNI_PATH", args.cni_path.as_deref().unwrap_or(""))
                .stdin(std::process::Stdio::piped())
                .stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::piped());

            let mut child = cmd.spawn()?;
            let mut stdin = child.stdin.take().unwrap();
            use std::io::Write;
            stdin.write_all(config_json.as_bytes())?;
            drop(stdin);

            let output = child.wait_with_output()?;

            if !output.status.success() {
                let stderr = String::from_utf8_lossy(&output.stderr);
                bail!("CNI plugin {} failed: {}", plugin.name, stderr);
            }

            // Parse result
            let result: CniResult = serde_json::from_slice(&output.stdout)?;
            return Ok(result);
        }

        bail!("No CNI plugins configured")
    }

    /// Execute CNI DEL command
    async fn cni_del(
        &self,
        cni_config: &CniNetworkConfig,
        container_id: &str,
        container_name: &str,
        netns_path: &str,
    ) -> Result<()> {
        let args = CniRuntimeArgs {
            cni_version: cni_config.cni_version.clone(),
            cni_command: "DEL".to_string(),
            cni_containerid: container_id.to_string(),
            cni_netns: netns_path.to_string(),
            cni_ifname: "eth0".to_string(),
            cni_args: None,
            cni_path: Some(
                self.config
                    .cni
                    .plugin_dirs
                    .iter()
                    .map(|p| p.to_string_lossy())
                    .collect::<Vec<_>>()
                    .join(":"),
            ),
        };

        let config_json = serde_json::to_string(cni_config)?;

        // Execute plugins in reverse order
        for plugin in cni_config.plugins.iter().rev() {
            let plugin_path = self.find_plugin(&plugin.type_)?;

            let mut cmd = StdCommand::new(&plugin_path);
            cmd.env("CNI_VERSION", &args.cni_version)
                .env("CNI_COMMAND", &args.cni_command)
                .env("CNI_CONTAINERID", &args.cni_containerid)
                .env("CNI_NETNS", &args.cni_netns)
                .env("CNI_IFNAME", &args.cni_ifname)
                .env("CNI_PATH", args.cni_path.as_deref().unwrap_or(""))
                .stdin(std::process::Stdio::piped())
                .stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::piped());

            let mut child = cmd.spawn()?;
            let mut stdin = child.stdin.take().unwrap();
            use std::io::Write;
            stdin.write_all(config_json.as_bytes())?;
            drop(stdin);

            let output = child.wait_with_output()?;

            if !output.status.success() {
                let stderr = String::from_utf8_lossy(&output.stderr);
                eprintln!("CNI plugin {} DEL failed: {}", plugin.name, stderr);
            }
        }

        Ok(())
    }

    /// Find CNI plugin binary
    fn find_plugin(&self, plugin_type: &str) -> Result<PathBuf> {
        for dir in &self.config.cni.plugin_dirs {
            let path = dir.join(plugin_type);
            if path.exists() {
                return Ok(path);
            }
        }

        // Try with CNI prefix
        for dir in &self.config.cni.plugin_dirs {
            let path = dir.join(format!("cni-{}", plugin_type));
            if path.exists() {
                return Ok(path);
            }
        }

        bail!("CNI plugin not found: {}", plugin_type)
    }

    /// Disconnect container from network
    pub async fn disconnect_container(
        &self,
        network_name: &str,
        container_name: &str,
    ) -> Result<()> {
        // Get interface
        let interfaces = self.interfaces.read().await;
        let interface = interfaces
            .values()
            .find(|i| {
                // Find interface for this container
                i.name.contains(container_name)
            })
            .cloned();

        if let Some(interface) = interface {
            let network = self.get_network(network_name).await;
            if let Some(network) = network {
                // Get CNI config
                let cni_config = self
                    .cni_cache
                    .read()
                    .await
                    .get(network_name)
                    .cloned()
                    .unwrap_or_else(|| CniNetworkConfig {
                        cni_version: "0.4.0".to_string(),
                        name: network_name.to_string(),
                        plugins: self.config.cni.plugins.clone(),
                        ipam: Some(CniIpamConfig {
                            type_: "host-local".to_string(),
                            subnet: Some(network.subnet.clone()),
                            routes: Some(vec![CniRoute {
                                dst: "0.0.0.0/0".to_string(),
                                gw: network.gateway.clone(),
                            }]),
                            data_dir: Some("/var/lib/cni/networks".to_string()),
                            ranges: None,
                        }),
                    });

                let container_id = Uuid::new_v4().to_string();
                let netns_path = format!("/var/run/netns/{}", container_name);

                // Execute CNI DEL
                self.cni_del(&cni_config, &container_id, container_name, &netns_path)
                    .await?;

                // Remove interface
                self.interfaces.write().await.remove(&interface.name);

                // Update network
                let mut networks = self.networks.write().await;
                if let Some(net) = networks.get_mut(network_name) {
                    net.containers.retain(|c| c != container_name);
                }
            }
        }

        Ok(())
    }

    /// Get container network interface
    pub async fn get_container_interface(&self, container_name: &str) -> Option<NetworkInterface> {
        let interfaces = self.interfaces.read().await;
        interfaces
            .values()
            .find(|i| i.name.contains(container_name))
            .cloned()
    }

    /// List all interfaces
    pub async fn list_interfaces(&self) -> Vec<NetworkInterface> {
        self.interfaces.read().await.values().cloned().collect()
    }
}

/// CNI Result from plugin execution
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CniResult {
    pub cni_version: String,
    pub interfaces: Vec<CniInterface>,
    pub ips: Vec<CniIp>,
    pub routes: Option<Vec<CniRoute>>,
    pub dns: Option<CniDns>,
}

/// CNI Interface
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CniInterface {
    pub name: String,
    pub mac: Option<String>,
    pub sandbox: String,
}

/// CNI IP
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CniIp {
    pub version: String,
    pub address: String,
    pub gateway: Option<String>,
    pub interface: Option<u32>,
}

/// CNI DNS
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CniDns {
    pub nameservers: Vec<String>,
    pub domain: String,
    pub search: Vec<String>,
    pub options: Vec<String>,
}

/// Network
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Network {
    pub id: Uuid,
    pub name: String,
    pub driver: String,
    pub subnet: String,
    pub gateway: Option<String>,
    pub dns_servers: Vec<String>,
    pub enable_ipv6: bool,
    pub mtu: u16,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub containers: Vec<String>,
}

/// Network Interface
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetworkInterface {
    pub id: Uuid,
    pub name: String,
    pub network_id: Uuid,
    pub container_id: Option<Uuid>,
    pub ip_address: IpAddr,
    pub mac_address: String,
    pub macvlan: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_network_manager_creation() {
        let config = NetworkConfig::default();
        let manager = NetworkManager::new(config);
        assert!(manager.is_ok());
    }

    #[tokio::test]
    async fn test_cni_config_default() {
        let config = CniConfig::default();
        assert!(!config.plugin_dirs.is_empty());
        assert!(!config.plugins.is_empty());
    }

    #[test]
    fn test_cni_network_config_serialization() {
        let config = CniNetworkConfig {
            cni_version: "0.4.0".to_string(),
            name: "test".to_string(),
            plugins: vec![],
            ipam: None,
        };
        let json = serde_json::to_string(&config).unwrap();
        assert!(json.contains("cni_version"));
    }
}
