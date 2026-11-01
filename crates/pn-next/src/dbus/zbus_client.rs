//! ZBus-based D-Bus Client for Inter-Container Communication
//!
//! This module provides a proper D-Bus client implementation using zbus
//! for communicating with the D-Bus broker and other containers.

use anyhow::{Context, Result};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{debug, error, info, warn};
use zbus::connection::Builder as ConnectionBuilder;
use zbus::{Connection, Proxy};

use super::{DBusBrokerConfig, DBusEvent, MessageType, ServiceRegistration};

/// ZBus-based D-Bus client for container communication
pub struct ZBusClient {
    connection: Option<Connection>,
    config: DBusBrokerConfig,
    event_tx: Option<tokio::sync::broadcast::Sender<DBusEvent>>,
    service_proxies: Arc<RwLock<HashMap<String, Proxy<'static>>>>,
    unique_name: Option<String>,
}

impl ZBusClient {
    /// Create a new ZBus client
    pub fn new(config: DBusBrokerConfig) -> Self {
        Self {
            connection: None,
            config,
            event_tx: None,
            service_proxies: Arc::new(RwLock::new(HashMap::new())),
            unique_name: None,
        }
    }

    /// Connect to the D-Bus broker via Unix socket
    pub async fn connect(&mut self) -> Result<()> {
        let socket_path = &self.config.socket_path;

        let stream = std::os::unix::net::UnixStream::connect(socket_path)
            .context("Failed to connect to D-Bus broker socket")?;

        let connection = ConnectionBuilder::unix_stream(stream)
            .build()
            .await
            .context("Failed to build D-Bus connection")?;

        self.unique_name = Some(connection.unique_name().expect("unique name").to_string());
        info!(
            "Connected to D-Bus broker as {}",
            self.unique_name.as_ref().unwrap()
        );

        self.connection = Some(connection);
        Ok(())
    }

    /// Connect to the session bus (for container-to-container communication)
    pub async fn connect_session(&mut self) -> Result<()> {
        let connection = ConnectionBuilder::session()
            .build()
            .await
            .context("Failed to connect to session bus")?;

        self.unique_name = Some(connection.unique_name().expect("unique name").to_string());
        info!(
            "Connected to session bus as {}",
            self.unique_name.as_ref().unwrap()
        );

        self.connection = Some(connection);
        Ok(())
    }

    /// Connect to the system bus
    pub async fn connect_system(&mut self) -> Result<()> {
        let connection = ConnectionBuilder::system()
            .build()
            .await
            .context("Failed to connect to system bus")?;

        self.unique_name = Some(connection.unique_name().expect("unique name").to_string());
        info!(
            "Connected to system bus as {}",
            self.unique_name.as_ref().unwrap()
        );

        self.connection = Some(connection);
        Ok(())
    }

    /// Get the unique name assigned by the bus
    pub fn unique_name(&self) -> Option<&str> {
        self.unique_name.as_deref()
    }

    /// Request a well-known name on the bus
    pub async fn request_name(
        &self,
        name: &str,
        allow_replacement: bool,
        replace_existing: bool,
        do_not_queue: bool,
    ) -> Result<u32> {
        let connection = self.connection.as_ref().context("Not connected")?;

        let mut flags = 0u32;
        if allow_replacement {
            flags |= 1;
        }
        if replace_existing {
            flags |= 2;
        }
        if do_not_queue {
            flags |= 4;
        }

        let proxy = Proxy::new(
            connection,
            "org.freedesktop.DBus",
            "/org/freedesktop/DBus",
            "org.freedesktop.DBus",
        )
        .await?;

        let reply = proxy.call_method("RequestName", &(name, flags)).await?;
        let result: u32 = reply.body().deserialize()?;
        Ok(result)
    }

    /// Release a well-known name
    pub async fn release_name(&self, name: &str) -> Result<u32> {
        let connection = self.connection.as_ref().context("Not connected")?;

        let proxy = Proxy::new(
            connection,
            "org.freedesktop.DBus",
            "/org/freedesktop/DBus",
            "org.freedesktop.DBus",
        )
        .await?;

        let reply = proxy.call_method("ReleaseName", &(name,)).await?;
        let result: u32 = reply.body().deserialize()?;
        Ok(result)
    }

    /// Call a method on a remote service with a single argument tuple
    pub async fn call_method<T, Args>(
        &self,
        destination: &str,
        path: &str,
        interface: &str,
        method: &str,
        args: Args,
    ) -> Result<T>
    where
        T: zbus::Type + for<'de> serde::Deserialize<'de>,
        Args: zbus::Type + serde::Serialize,
    {
        let connection = self.connection.as_ref().context("Not connected")?;

        let proxy = Proxy::new(connection, destination, path, interface).await?;

        let reply = proxy.call_method(method, &(args,)).await?;
        let result: T = reply.body().deserialize()?;
        Ok(result)
    }

    /// Call a method with no arguments
    pub async fn call_method_no_args<T>(
        &self,
        destination: &str,
        path: &str,
        interface: &str,
        method: &str,
    ) -> Result<T>
    where
        T: zbus::Type + for<'de> serde::Deserialize<'de>,
    {
        let connection = self.connection.as_ref().context("Not connected")?;

        let proxy = Proxy::new(connection, destination, path, interface).await?;

        let reply = proxy.call_method(method, &()).await?;
        let result: T = reply.body().deserialize()?;
        Ok(result)
    }

    /// Subscribe to signals from a service
    pub async fn subscribe_signals(
        &self,
        sender: Option<&str>,
        interface: Option<&str>,
        member: Option<&str>,
        path: Option<&str>,
    ) -> Result<()> {
        let connection = self.connection.as_ref().context("Not connected")?;

        let mut rule = String::from("type='signal'");

        if let Some(s) = sender {
            rule.push_str(&format!(",sender='{}'", s));
        }
        if let Some(i) = interface {
            rule.push_str(&format!(",interface='{}'", i));
        }
        if let Some(m) = member {
            rule.push_str(&format!(",member='{}'", m));
        }
        if let Some(p) = path {
            rule.push_str(&format!(",path='{}'", p));
        }

        let proxy = Proxy::new(
            connection,
            "org.freedesktop.DBus",
            "/org/freedesktop/DBus",
            "org.freedesktop.DBus",
        )
        .await?;

        proxy.call_method("AddMatch", &(rule,)).await?;
        Ok(())
    }

    /// Get a proxy for a well-known service (cached)
    pub async fn get_proxy(
        &self,
        service: &str,
        path: &str,
        interface: &str,
    ) -> Result<Proxy<'static>> {
        let cache_key = format!("{}@{}/{}", service, path, interface);

        {
            let proxies = self.service_proxies.read().await;
            if let Some(proxy) = proxies.get(&cache_key) {
                return Ok(proxy.clone());
            }
        }

        let connection = self.connection.as_ref().context("Not connected")?;

        let proxy = Proxy::new(connection, service, path, interface).await?;

        let mut proxies = self.service_proxies.write().await;
        // We need to convert to 'static lifetime for caching
        // This is safe because the connection outlives the proxy
        let proxy_static = unsafe { std::mem::transmute::<Proxy<'_>, Proxy<'static>>(proxy) };
        proxies.insert(cache_key, proxy_static.clone());

        Ok(proxy_static)
    }

    /// List all names on the bus
    pub async fn list_names(&self) -> Result<Vec<String>> {
        let connection = self.connection.as_ref().context("Not connected")?;

        let proxy = Proxy::new(
            connection,
            "org.freedesktop.DBus",
            "/org/freedesktop/DBus",
            "org.freedesktop.DBus",
        )
        .await?;

        let reply = proxy.call_method("ListNames", &()).await?;
        let names: Vec<String> = reply.body().deserialize()?;
        Ok(names)
    }

    /// Check if a name has an owner
    pub async fn name_has_owner(&self, name: &str) -> Result<bool> {
        let connection = self.connection.as_ref().context("Not connected")?;

        let proxy = Proxy::new(
            connection,
            "org.freedesktop.DBus",
            "/org/freedesktop/DBus",
            "org.freedesktop.DBus",
        )
        .await?;

        let reply = proxy.call_method("NameHasOwner", &(name,)).await?;
        let has_owner: bool = reply.body().deserialize()?;
        Ok(has_owner)
    }

    /// Ping the bus
    pub async fn ping(&self) -> Result<()> {
        let connection = self.connection.as_ref().context("Not connected")?;

        let proxy = Proxy::new(
            connection,
            "org.freedesktop.DBus",
            "/org/freedesktop/DBus",
            "org.freedesktop.DBus.Peer",
        )
        .await?;

        proxy.call_method("Ping", &()).await?;
        Ok(())
    }

    /// Get machine ID
    pub async fn get_machine_id(&self) -> Result<String> {
        let connection = self.connection.as_ref().context("Not connected")?;

        let proxy = Proxy::new(
            connection,
            "org.freedesktop.DBus",
            "/org/freedesktop/DBus",
            "org.freedesktop.DBus.Peer",
        )
        .await?;

        let reply = proxy.call_method("GetMachineId", &()).await?;
        let machine_id: String = reply.body().deserialize()?;
        Ok(machine_id)
    }

    /// Introspect a remote object
    pub async fn introspect(&self, destination: &str, path: &str) -> Result<String> {
        let connection = self.connection.as_ref().context("Not connected")?;

        let proxy = Proxy::new(
            connection,
            destination,
            path,
            "org.freedesktop.DBus.Introspectable",
        )
        .await?;

        let reply = proxy.call_method("Introspect", &()).await?;
        let xml: String = reply.body().deserialize()?;
        Ok(xml)
    }

    /// Disconnect from the bus
    pub async fn disconnect(&mut self) -> Result<()> {
        if let Some(conn) = self.connection.take() {
            conn.close().await?;
            info!("Disconnected from D-Bus");
        }
        Ok(())
    }
}

/// Container D-Bus client for inter-container communication
pub struct ContainerDBusClient {
    client: ZBusClient,
    container_id: String,
    container_name: String,
}

impl ContainerDBusClient {
    /// Create a new container D-Bus client
    pub fn new(container_id: String, container_name: String, config: DBusBrokerConfig) -> Self {
        Self {
            client: ZBusClient::new(config),
            container_id,
            container_name,
        }
    }

    /// Connect to the broker
    pub async fn connect(&mut self) -> Result<()> {
        self.client.connect().await
    }

    /// Register this container as a service
    pub async fn register_container_service(&mut self) -> Result<()> {
        let service_name = format!("org.pnnext.Container.{}", self.container_name);

        self.client
            .request_name(&service_name, true, true, false)
            .await?;

        // Subscribe to container management signals
        self.client
            .subscribe_signals(
                Some("org.pnnext.ContainerManager"),
                Some("org.pnnext.ContainerManager"),
                None,
                None,
            )
            .await?;

        info!("Registered container service: {}", service_name);
        Ok(())
    }

    /// Call supervisor to start a service
    pub async fn start_service(&self, name: &str, config: HashMap<String, String>) -> Result<bool> {
        #[derive(zbus::Type, serde::Serialize)]
        struct StartServiceArgs<'a> {
            name: &'a str,
            config: &'a HashMap<String, String>,
        }

        let args = StartServiceArgs {
            name,
            config: &config,
        };
        let result: bool = self
            .client
            .call_method(
                "org.pnnext.Supervisor",
                "/org/pnnext/Supervisor",
                "org.pnnext.Supervisor",
                "StartService",
                args,
            )
            .await?;
        Ok(result)
    }

    /// Call supervisor to stop a service
    pub async fn stop_service(&self, name: &str, force: bool) -> Result<bool> {
        #[derive(zbus::Type, serde::Serialize)]
        struct StopServiceArgs<'a> {
            name: &'a str,
            force: bool,
        }

        let args = StopServiceArgs { name, force };
        let result: bool = self
            .client
            .call_method(
                "org.pnnext.Supervisor",
                "/org/pnnext/Supervisor",
                "org.pnnext.Supervisor",
                "StopService",
                args,
            )
            .await?;
        Ok(result)
    }

    /// Get service status from supervisor
    pub async fn get_service_status(&self, name: &str) -> Result<HashMap<String, String>> {
        #[derive(zbus::Type, serde::Serialize)]
        struct GetStatusArgs<'a> {
            name: &'a str,
        }

        let args = GetStatusArgs { name };
        let result: HashMap<String, String> = self
            .client
            .call_method(
                "org.pnnext.Supervisor",
                "/org/pnnext/Supervisor",
                "org.pnnext.Supervisor",
                "GetStatus",
                args,
            )
            .await?;
        Ok(result)
    }

    /// Create a container via container manager
    pub async fn create_container(
        &self,
        name: &str,
        image: &str,
        config: HashMap<String, String>,
    ) -> Result<String> {
        #[derive(zbus::Type, serde::Serialize)]
        struct CreateContainerArgs<'a> {
            name: &'a str,
            image: &'a str,
            config: &'a HashMap<String, String>,
        }

        let args = CreateContainerArgs {
            name,
            image,
            config: &config,
        };
        let result: String = self
            .client
            .call_method(
                "org.pnnext.ContainerManager",
                "/org/pnnext/ContainerManager",
                "org.pnnext.ContainerManager",
                "CreateContainer",
                args,
            )
            .await?;
        Ok(result)
    }

    /// Start a container
    pub async fn start_container(&self, container_id: &str) -> Result<bool> {
        #[derive(zbus::Type, serde::Serialize)]
        struct StartContainerArgs<'a> {
            container_id: &'a str,
        }

        let args = StartContainerArgs { container_id };
        let result: bool = self
            .client
            .call_method(
                "org.pnnext.ContainerManager",
                "/org/pnnext/ContainerManager",
                "org.pnnext.ContainerManager",
                "StartContainer",
                args,
            )
            .await?;
        Ok(result)
    }

    /// Stop a container
    pub async fn stop_container(&self, container_id: &str, timeout: u32) -> Result<bool> {
        #[derive(zbus::Type, serde::Serialize)]
        struct StopContainerArgs<'a> {
            container_id: &'a str,
            timeout: u32,
        }

        let args = StopContainerArgs {
            container_id,
            timeout,
        };
        let result: bool = self
            .client
            .call_method(
                "org.pnnext.ContainerManager",
                "/org/pnnext/ContainerManager",
                "org.pnnext.ContainerManager",
                "StopContainer",
                args,
            )
            .await?;
        Ok(result)
    }

    /// List all containers
    pub async fn list_containers(&self) -> Result<Vec<HashMap<String, String>>> {
        let result: Vec<HashMap<String, String>> = self
            .client
            .call_method_no_args(
                "org.pnnext.ContainerManager",
                "/org/pnnext/ContainerManager",
                "org.pnnext.ContainerManager",
                "ListContainers",
            )
            .await?;
        Ok(result)
    }

    /// Execute command in container
    pub async fn exec_in_container(
        &self,
        container_id: &str,
        command: Vec<String>,
    ) -> Result<String> {
        #[derive(zbus::Type, serde::Serialize)]
        struct ExecArgs<'a> {
            container_id: &'a str,
            command: &'a Vec<String>,
        }

        let args = ExecArgs {
            container_id,
            command: &command,
        };
        let result: String = self
            .client
            .call_method(
                "org.pnnext.ContainerManager",
                "/org/pnnext/ContainerManager",
                "org.pnnext.ContainerManager",
                "ExecInContainer",
                args,
            )
            .await?;
        Ok(result)
    }

    /// Publish an event to the event bus
    pub async fn publish_event(&self, topic: &str, event: HashMap<String, String>) -> Result<bool> {
        #[derive(zbus::Type, serde::Serialize)]
        struct PublishArgs<'a> {
            topic: &'a str,
            event: &'a HashMap<String, String>,
        }

        let args = PublishArgs {
            topic,
            event: &event,
        };
        let result: bool = self
            .client
            .call_method(
                "org.pnnext.EventBus",
                "/org/pnnext/EventBus",
                "org.pnnext.EventBus",
                "Publish",
                args,
            )
            .await?;
        Ok(result)
    }

    /// Subscribe to events
    pub async fn subscribe_events(&self, topic: &str, filter: &str) -> Result<String> {
        #[derive(zbus::Type, serde::Serialize)]
        struct SubscribeArgs<'a> {
            topic: &'a str,
            filter: &'a str,
        }

        let args = SubscribeArgs { topic, filter };
        let result: String = self
            .client
            .call_method(
                "org.pnnext.EventBus",
                "/org/pnnext/EventBus",
                "org.pnnext.EventBus",
                "Subscribe",
                args,
            )
            .await?;
        Ok(result)
    }

    /// Get underlying client for advanced usage
    pub fn client(&self) -> &ZBusClient {
        &self.client
    }

    /// Get mutable underlying client
    pub fn client_mut(&mut self) -> &mut ZBusClient {
        &mut self.client
    }
}

/// Helper to create a D-Bus connection for a container
pub async fn create_container_connection(
    container_id: &str,
    container_name: &str,
    socket_path: &str,
) -> Result<ZBusClient> {
    let config = DBusBrokerConfig {
        socket_path: socket_path.to_string(),
        ..Default::default()
    };

    let mut client = ZBusClient::new(config);
    client.connect().await?;

    // Request a name for this container
    let service_name = format!("org.pnnext.Container.{}", container_name);
    client
        .request_name(&service_name, true, true, false)
        .await?;

    Ok(client)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[tokio::test(flavor = "multi_thread")]
    async fn test_zbus_client_creation() {
        let temp_dir = tempdir().unwrap();
        let socket_path = temp_dir
            .path()
            .join("dbus.sock")
            .to_string_lossy()
            .to_string();

        let config = DBusBrokerConfig {
            socket_path: socket_path.clone(),
            ..Default::default()
        };

        let client = ZBusClient::new(config);
        assert!(client.connection.is_none());
        assert!(client.unique_name().is_none());
    }
}
