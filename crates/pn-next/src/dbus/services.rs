//! D-Bus Service Definitions for PN Next Inter-Container Communication
//!
//! This module defines the standard D-Bus interfaces and service definitions
//! used for communication between containers and the supervisor.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Well-known service names for PN Next
pub mod service_names {
    /// PN Next Supervisor service
    pub const SUPERVISOR: &str = "org.pnnext.Supervisor";
    /// PN Next Container Manager service
    pub const CONTAINER_MANAGER: &str = "org.pnnext.ContainerManager";
    /// PN Next Network Manager service
    pub const NETWORK_MANAGER: &str = "org.pnnext.NetworkManager";
    /// PN Next Storage Manager service
    pub const STORAGE_MANAGER: &str = "org.pnnext.StorageManager";
    /// PN Next Cluster Manager service
    pub const CLUSTER_MANAGER: &str = "org.pnnext.ClusterManager";
    /// PN Next Metrics service
    pub const METRICS: &str = "org.pnnext.Metrics";
    /// PN Next Event Bus service
    pub const EVENT_BUS: &str = "org.pnnext.EventBus";
}

/// Object paths for PN Next services
pub mod object_paths {
    pub const SUPERVISOR: &str = "/org/pnnext/Supervisor";
    pub const CONTAINER_MANAGER: &str = "/org/pnnext/ContainerManager";
    pub const NETWORK_MANAGER: &str = "/org/pnnext/NetworkManager";
    pub const STORAGE_MANAGER: &str = "/org/pnnext/StorageManager";
    pub const CLUSTER_MANAGER: &str = "/org/pnnext/ClusterManager";
    pub const METRICS: &str = "/org/pnnext/Metrics";
    pub const EVENT_BUS: &str = "/org/pnnext/EventBus";
}

/// Interface names for PN Next services
pub mod interfaces {
    pub const SUPERVISOR: &str = "org.pnnext.Supervisor";
    pub const CONTAINER_MANAGER: &str = "org.pnnext.ContainerManager";
    pub const NETWORK_MANAGER: &str = "org.pnnext.NetworkManager";
    pub const STORAGE_MANAGER: &str = "org.pnnext.StorageManager";
    pub const CLUSTER_MANAGER: &str = "org.pnnext.ClusterManager";
    pub const METRICS: &str = "org.pnnext.Metrics";
    pub const EVENT_BUS: &str = "org.pnnext.EventBus";
    pub const CONTAINER: &str = "org.pnnext.Container";
    pub const SERVICE: &str = "org.pnnext.Service";
}

/// Standard D-Bus Introspection XML for PN Next services
pub mod introspection {
    use super::*;

    /// Supervisor service introspection
    pub const SUPERVISOR: &str = r#"
<!DOCTYPE node PUBLIC "-//freedesktop//DTD D-BUS Object Introspection 1.0//EN"
"http://www.freedesktop.org/standards/dbus/1.0/introspect.dtd">
<node>
  <interface name="org.pnnext.Supervisor">
    <method name="StartService">
      <arg name="name" type="s" direction="in"/>
      <arg name="config" type="a{sv}" direction="in"/>
      <arg name="result" type="b" direction="out"/>
    </method>
    <method name="StopService">
      <arg name="name" type="s" direction="in"/>
      <arg name="force" type="b" direction="in"/>
      <arg name="result" type="b" direction="out"/>
    </method>
    <method name="RestartService">
      <arg name="name" type="s" direction="in"/>
      <arg name="result" type="b" direction="out"/>
    </method>
    <method name="GetStatus">
      <arg name="name" type="s" direction="in"/>
      <arg name="status" type="a{sv}" direction="out"/>
    </method>
    <method name="ListServices">
      <arg name="services" type="a{sv}" direction="out"/>
    </method>
    <method name="GetLogs">
      <arg name="name" type="s" direction="in"/>
      <arg name="lines" type="u" direction="in"/>
      <arg name="logs" type="as" direction="out"/>
    </method>
    <signal name="ServiceStateChanged">
      <arg name="name" type="s"/>
      <arg name="state" type="s"/>
      <arg name="pid" type="u"/>
    </signal>
    <signal name="ServiceOutput">
      <arg name="name" type="s"/>
      <arg name="stream" type="s"/>
      <arg name="data" type="s"/>
    </signal>
  </interface>
  <interface name="org.freedesktop.DBus.Introspectable">
    <method name="Introspect">
      <arg name="xml_data" type="s" direction="out"/>
    </method>
  </interface>
  <interface name="org.freedesktop.DBus.Peer">
    <method name="Ping"/>
    <method name="GetMachineId">
      <arg name="machine_id" type="s" direction="out"/>
    </method>
  </interface>
</node>"#;

    /// Container Manager service introspection
    pub const CONTAINER_MANAGER: &str = r#"
<!DOCTYPE node PUBLIC "-//freedesktop//DTD D-BUS Object Introspection 1.0//EN"
"http://www.freedesktop.org/standards/dbus/1.0/introspect.dtd">
<node>
  <interface name="org.pnnext.ContainerManager">
    <method name="CreateContainer">
      <arg name="name" type="s" direction="in"/>
      <arg name="image" type="s" direction="in"/>
      <arg name="config" type="a{sv}" direction="in"/>
      <arg name="container_id" type="s" direction="out"/>
    </method>
    <method name="StartContainer">
      <arg name="container_id" type="s" direction="in"/>
      <arg name="result" type="b" direction="out"/>
    </method>
    <method name="StopContainer">
      <arg name="container_id" type="s" direction="in"/>
      <arg name="timeout" type="u" direction="in"/>
      <arg name="result" type="b" direction="out"/>
    </method>
    <method name="RemoveContainer">
      <arg name="container_id" type="s" direction="in"/>
      <arg name="force" type="b" direction="in"/>
      <arg name="result" type="b" direction="out"/>
    </method>
    <method name="GetContainerInfo">
      <arg name="container_id" type="s" direction="in"/>
      <arg name="info" type="a{sv}" direction="out"/>
    </method>
    <method name="ListContainers">
      <arg name="containers" type="aa{sv}" direction="out"/>
    </method>
    <method name="ExecInContainer">
      <arg name="container_id" type="s" direction="in"/>
      <arg name="command" type="as" direction="in"/>
      <arg name="output" type="s" direction="out"/>
    </method>
    <signal name="ContainerStateChanged">
      <arg name="container_id" type="s"/>
      <arg name="state" type="s"/>
    </signal>
    <signal name="ContainerCreated">
      <arg name="container_id" type="s"/>
      <arg name="name" type="s"/>
    </signal>
    <signal name="ContainerRemoved">
      <arg name="container_id" type="s"/>
    </signal>
  </interface>
  <interface name="org.freedesktop.DBus.Introspectable">
    <method name="Introspect">
      <arg name="xml_data" type="s" direction="out"/>
    </method>
  </interface>
  <interface name="org.freedesktop.DBus.Peer">
    <method name="Ping"/>
    <method name="GetMachineId">
      <arg name="machine_id" type="s" direction="out"/>
    </method>
  </interface>
</node>"#;

    /// Network Manager service introspection
    pub const NETWORK_MANAGER: &str = r#"
<!DOCTYPE node PUBLIC "-//freedesktop//DTD D-BUS Object Introspection 1.0//EN"
"http://www.freedesktop.org/standards/dbus/1.0/introspect.dtd">
<node>
  <interface name="org.pnnext.NetworkManager">
    <method name="CreateNetwork">
      <arg name="name" type="s" direction="in"/>
      <arg name="driver" type="s" direction="in"/>
      <arg name="config" type="a{sv}" direction="in"/>
      <arg name="network_id" type="s" direction="out"/>
    </method>
    <method name="RemoveNetwork">
      <arg name="network_id" type="s" direction="in"/>
      <arg name="result" type="b" direction="out"/>
    </method>
    <method name="ConnectContainer">
      <arg name="network_id" type="s" direction="in"/>
      <arg name="container_id" type="s" direction="in"/>
      <arg name="config" type="a{sv}" direction="in"/>
      <arg name="endpoint_id" type="s" direction="out"/>
    </method>
    <method name="DisconnectContainer">
      <arg name="network_id" type="s" direction="in"/>
      <arg name="container_id" type="s" direction="in"/>
      <arg name="result" type="b" direction="out"/>
    </method>
    <method name="ListNetworks">
      <arg name="networks" type="aa{sv}" direction="out"/>
    </method>
    <method name="GetNetworkInfo">
      <arg name="network_id" type="s" direction="in"/>
      <arg name="info" type="a{sv}" direction="out"/>
    </method>
    <signal name="NetworkCreated">
      <arg name="network_id" type="s"/>
      <arg name="name" type="s"/>
    </signal>
    <signal name="NetworkRemoved">
      <arg name="network_id" type="s"/>
    </signal>
    <signal name="ContainerConnected">
      <arg name="network_id" type="s"/>
      <arg name="container_id" type="s"/>
    </signal>
    <signal name="ContainerDisconnected">
      <arg name="network_id" type="s"/>
      <arg name="container_id" type="s"/>
    </signal>
  </interface>
  <interface name="org.freedesktop.DBus.Introspectable">
    <method name="Introspect">
      <arg name="xml_data" type="s" direction="out"/>
    </method>
  </interface>
  <interface name="org.freedesktop.DBus.Peer">
    <method name="Ping"/>
    <method name="GetMachineId">
      <arg name="machine_id" type="s" direction="out"/>
    </method>
  </interface>
</node>"#;

    /// Storage Manager service introspection
    pub const STORAGE_MANAGER: &str = r#"
<!DOCTYPE node PUBLIC "-//freedesktop//DTD D-BUS Object Introspection 1.0//EN"
"http://www.freedesktop.org/standards/dbus/1.0/introspect.dtd">
<node>
  <interface name="org.pnnext.StorageManager">
    <method name="CreateVolume">
      <arg name="name" type="s" direction="in"/>
      <arg name="driver" type="s" direction="in"/>
      <arg name="config" type="a{sv}" direction="in"/>
      <arg name="volume_id" type="s" direction="out"/>
    </method>
    <method name="RemoveVolume">
      <arg name="volume_id" type="s" direction="in"/>
      <arg name="force" type="b" direction="in"/>
      <arg name="result" type="b" direction="out"/>
    </method>
    <method name="MountVolume">
      <arg name="volume_id" type="s" direction="in"/>
      <arg name="container_id" type="s" direction="in"/>
      <arg name="mount_path" type="s" direction="in"/>
      <arg name="result" type="b" direction="out"/>
    </method>
    <method name="UnmountVolume">
      <arg name="volume_id" type="s" direction="in"/>
      <arg name="container_id" type="s" direction="in"/>
      <arg name="result" type="b" direction="out"/>
    </method>
    <method name="ListVolumes">
      <arg name="volumes" type="aa{sv}" direction="out"/>
    </method>
    <method name="GetVolumeInfo">
      <arg name="volume_id" type="s" direction="in"/>
      <arg name="info" type="a{sv}" direction="out"/>
    </method>
    <signal name="VolumeCreated">
      <arg name="volume_id" type="s"/>
      <arg name="name" type="s"/>
    </signal>
    <signal name="VolumeRemoved">
      <arg name="volume_id" type="s"/>
    </signal>
    <signal name="VolumeMounted">
      <arg name="volume_id" type="s"/>
      <arg name="container_id" type="s"/>
      <arg name="mount_path" type="s"/>
    </signal>
    <signal name="VolumeUnmounted">
      <arg name="volume_id" type="s"/>
      <arg name="container_id" type="s"/>
    </signal>
  </interface>
  <interface name="org.freedesktop.DBus.Introspectable">
    <method name="Introspect">
      <arg name="xml_data" type="s" direction="out"/>
    </method>
  </interface>
  <interface name="org.freedesktop.DBus.Peer">
    <method name="Ping"/>
    <method name="GetMachineId">
      <arg name="machine_id" type="s" direction="out"/>
    </method>
  </interface>
</node>"#;

    /// Event Bus service introspection
    pub const EVENT_BUS: &str = r#"
<!DOCTYPE node PUBLIC "-//freedesktop//DTD D-BUS Object Introspection 1.0//EN"
"http://www.freedesktop.org/standards/dbus/1.0/introspect.dtd">
<node>
  <interface name="org.pnnext.EventBus">
    <method name="Publish">
      <arg name="topic" type="s" direction="in"/>
      <arg name="event" type="a{sv}" direction="in"/>
      <arg name="result" type="b" direction="out"/>
    </method>
    <method name="Subscribe">
      <arg name="topic" type="s" direction="in"/>
      <arg name="filter" type="s" direction="in"/>
      <arg name="subscription_id" type="s" direction="out"/>
    </method>
    <method name="Unsubscribe">
      <arg name="subscription_id" type="s" direction="in"/>
      <arg name="result" type="b" direction="out"/>
    </method>
    <method name="GetSubscriptions">
      <arg name="subscriptions" type="aa{sv}" direction="out"/>
    </method>
    <signal name="EventPublished">
      <arg name="topic" type="s"/>
      <arg name="event" type="a{sv}"/>
      <arg name="timestamp" type="t"/>
    </signal>
  </interface>
  <interface name="org.freedesktop.DBus.Introspectable">
    <method name="Introspect">
      <arg name="xml_data" type="s" direction="out"/>
    </method>
  </interface>
  <interface name="org.freedesktop.DBus.Peer">
    <method name="Ping"/>
    <method name="GetMachineId">
      <arg name="machine_id" type="s" direction="out"/>
    </method>
  </interface>
</node>"#;

    /// Metrics service introspection
    pub const METRICS: &str = r#"
<!DOCTYPE node PUBLIC "-//freedesktop//DTD D-BUS Object Introspection 1.0//EN"
"http://www.freedesktop.org/standards/dbus/1.0/introspect.dtd">
<node>
  <interface name="org.pnnext.Metrics">
    <method name="GetMetrics">
      <arg name="prefix" type="s" direction="in"/>
      <arg name="metrics" type="a{sv}" direction="out"/>
    </method>
    <method name="GetContainerMetrics">
      <arg name="container_id" type="s" direction="in"/>
      <arg name="metrics" type="a{sv}" direction="out"/>
    </method>
    <method name="GetSystemMetrics">
      <arg name="metrics" type="a{sv}" direction="out"/>
    </method>
    <signal name="MetricsUpdated">
      <arg name="metrics" type="a{sv}"/>
      <arg name="timestamp" type="t"/>
    </signal>
  </interface>
  <interface name="org.freedesktop.DBus.Introspectable">
    <method name="Introspect">
      <arg name="xml_data" type="s" direction="out"/>
    </method>
  </interface>
  <interface name="org.freedesktop.DBus.Peer">
    <method name="Ping"/>
    <method name="GetMachineId">
      <arg name="machine_id" type="s" direction="out"/>
    </method>
  </interface>
</node>"#;

    /// Get introspection XML for a service
    pub fn get_introspection(service: &str) -> Option<&'static str> {
        match service {
            "org.pnnext.Supervisor" => Some(SUPERVISOR),
            "org.pnnext.ContainerManager" => Some(CONTAINER_MANAGER),
            "org.pnnext.NetworkManager" => Some(NETWORK_MANAGER),
            "org.pnnext.StorageManager" => Some(STORAGE_MANAGER),
            "org.pnnext.EventBus" => Some(EVENT_BUS),
            "org.pnnext.Metrics" => Some(METRICS),
            _ => None,
        }
    }
}

/// Container configuration for D-Bus
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContainerConfig {
    pub name: String,
    pub image: String,
    pub command: Option<Vec<String>>,
    pub env: HashMap<String, String>,
    pub mounts: Vec<MountConfig>,
    pub network: Option<String>,
    pub resources: Option<ResourceLimits>,
    pub labels: HashMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MountConfig {
    pub source: String,
    pub target: String,
    pub readonly: bool,
    pub mount_type: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResourceLimits {
    pub cpu_limit: Option<f64>,
    pub memory_limit: Option<u64>,
    pub io_limit: Option<u64>,
    pub pids_limit: Option<u64>,
}

/// Network configuration for D-Bus
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetworkConfig {
    pub name: String,
    pub driver: String,
    pub subnet: Option<String>,
    pub gateway: Option<String>,
    pub labels: HashMap<String, String>,
}

/// Volume configuration for D-Bus
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VolumeConfig {
    pub name: String,
    pub driver: String,
    pub size: Option<u64>,
    pub labels: HashMap<String, String>,
}

/// Service status for D-Bus
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServiceStatus {
    pub name: String,
    pub state: String,
    pub pid: Option<u32>,
    pub exit_code: Option<i32>,
    pub started_at: Option<String>,
    pub restarted_count: u32,
}

/// Container info for D-Bus
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContainerInfo {
    pub id: String,
    pub name: String,
    pub image: String,
    pub state: String,
    pub pid: Option<u32>,
    pub ip_address: Option<String>,
    pub created_at: String,
    pub labels: HashMap<String, String>,
}

/// Network info for D-Bus
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetworkInfo {
    pub id: String,
    pub name: String,
    pub driver: String,
    pub subnet: Option<String>,
    pub containers: Vec<String>,
}

/// Volume info for D-Bus
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VolumeInfo {
    pub id: String,
    pub name: String,
    pub driver: String,
    pub mount_point: Option<String>,
    pub size: Option<u64>,
    pub containers: Vec<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_introspection_retrieval() {
        assert!(introspection::get_introspection("org.pnnext.Supervisor").is_some());
        assert!(introspection::get_introspection("org.pnnext.ContainerManager").is_some());
        assert!(introspection::get_introspection("org.pnnext.NetworkManager").is_some());
        assert!(introspection::get_introspection("org.pnnext.StorageManager").is_some());
        assert!(introspection::get_introspection("org.pnnext.EventBus").is_some());
        assert!(introspection::get_introspection("org.pnnext.Metrics").is_some());
        assert!(introspection::get_introspection("org.unknown.Service").is_none());
    }

    #[test]
    fn test_service_constants() {
        assert_eq!(service_names::SUPERVISOR, "org.pnnext.Supervisor");
        assert_eq!(object_paths::SUPERVISOR, "/org/pnnext/Supervisor");
        assert_eq!(interfaces::SUPERVISOR, "org.pnnext.Supervisor");
    }
}
