//! D-Bus Broker - Inter-container IPC and service discovery
//!
//! Provides a lightweight D-Bus message broker for communication between
//! container services and the supervisor. Implements a subset of the D-Bus
//! specification suitable for Termux/Android environments.

pub mod services;
pub mod zbus_client;

use crate::supervisor::ServiceSupervisor;
use anyhow::{bail, Context, Result};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use services::*;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::net::{UnixListener, UnixStream};
use tokio::sync::{broadcast, mpsc, RwLock};
use tracing::{debug, error, info, instrument, warn};
use uuid::Uuid;

/// D-Bus message types
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MessageType {
    MethodCall,
    MethodReturn,
    Error,
    Signal,
}

/// D-Bus message header
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessageHeader {
    pub message_type: MessageType,
    pub flags: u8,
    pub version: u8,
    pub serial: u32,
    pub path: Option<String>,
    pub interface: Option<String>,
    pub member: Option<String>,
    pub destination: Option<String>,
    pub sender: Option<String>,
    pub signature: Option<String>,
    pub unix_fds: u32,
}

/// D-Bus message
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DBusMessage {
    pub header: MessageHeader,
    pub body: Vec<u8>,
}

/// Service registration info
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServiceRegistration {
    pub name: String,
    pub path: String,
    pub interface: String,
    pub pid: u32,
    pub container: Option<String>,
    pub registered_at: DateTime<Utc>,
    pub properties: HashMap<String, serde_json::Value>,
}

/// D-Bus connection info
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConnectionInfo {
    pub unique_name: String,
    pub bus_name: Option<String>,
    pub pid: u32,
    pub uid: u32,
    pub container: Option<String>,
    pub connected_at: DateTime<Utc>,
    pub is_monitor: bool,
}

/// D-Bus broker configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DBusBrokerConfig {
    /// Socket path for the broker
    pub socket_path: String,
    /// Maximum message size (bytes)
    pub max_message_size: usize,
    /// Maximum queued messages per connection
    pub max_queue_size: usize,
    /// Enable message logging
    pub log_messages: bool,
    /// Allowed well-known names
    pub allowed_names: HashSet<String>,
    /// Container isolation (separate bus per container)
    pub isolate_containers: bool,
}

impl Default for DBusBrokerConfig {
    fn default() -> Self {
        let mut allowed = HashSet::new();
        allowed.insert("org.freedesktop.DBus".to_string());
        allowed.insert("org.pnnext.*".to_string());

        Self {
            socket_path: "/run/pn-next/dbus/session".to_string(),
            max_message_size: 64 * 1024 * 1024, // 64MB
            max_queue_size: 1000,
            log_messages: false,
            allowed_names: allowed,
            isolate_containers: true,
        }
    }
}

/// Message bus for a container namespace
struct ContainerBus {
    connections: HashMap<String, ConnectionInfo>,
    services: HashMap<String, ServiceRegistration>,
    pending_replies: HashMap<u32, mpsc::Sender<DBusMessage>>,
    signal_subscriptions: HashMap<String, HashSet<String>>, // interface.member -> connection names
}

/// Main D-Bus broker
pub struct DBusBroker {
    config: DBusBrokerConfig,
    global_bus: Arc<RwLock<ContainerBus>>,
    container_buses: Arc<RwLock<HashMap<String, Arc<RwLock<ContainerBus>>>>>,
    event_tx: broadcast::Sender<DBusEvent>,
    serial_counter: Arc<RwLock<u32>>,
    /// The supervisor `org.pnnext.Supervisor` calls are forwarded to.
    ///
    /// Optional because `DBusBroker::new` is synchronous while
    /// `ServiceSupervisor::initialize` is not, so the handle is attached by the
    /// caller that already owns an initialized one (the daemon). A broker
    /// without it answers those calls with an error rather than pretending.
    supervisor: Option<Arc<ServiceSupervisor>>,
}

/// D-Bus broker events
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum DBusEvent {
    ConnectionOpened {
        unique_name: String,
        container: Option<String>,
    },
    ConnectionClosed {
        unique_name: String,
    },
    ServiceRegistered {
        name: String,
        container: Option<String>,
    },
    ServiceUnregistered {
        name: String,
    },
    MessageSent {
        from: String,
        to: String,
        message_type: MessageType,
    },
    MethodCall {
        sender: String,
        destination: String,
        path: String,
        interface: String,
        member: String,
    },
}

impl DBusBroker {
    /// Create a new D-Bus broker
    pub fn new(config: DBusBrokerConfig) -> Self {
        let (event_tx, _) = broadcast::channel(1024);

        Self {
            config,
            global_bus: Arc::new(RwLock::new(ContainerBus {
                connections: HashMap::new(),
                services: HashMap::new(),
                pending_replies: HashMap::new(),
                signal_subscriptions: HashMap::new(),
            })),
            container_buses: Arc::new(RwLock::new(HashMap::new())),
            event_tx,
            serial_counter: Arc::new(RwLock::new(1)),
            supervisor: None,
        }
    }

    /// Get event receiver
    pub fn subscribe_events(&self) -> broadcast::Receiver<DBusEvent> {
        self.event_tx.subscribe()
    }

    /// Start the broker listener
    pub async fn start(&self) -> Result<()> {
        // Ensure socket directory exists
        if let Some(parent) = std::path::Path::new(&self.config.socket_path).parent() {
            tokio::fs::create_dir_all(parent)
                .await
                .context("Failed to create socket directory")?;
        }

        // Remove existing socket
        let _ = tokio::fs::remove_file(&self.config.socket_path).await;

        let listener =
            UnixListener::bind(&self.config.socket_path).context("Failed to bind D-Bus socket")?;

        info!("D-Bus broker listening on {}", self.config.socket_path);

        // Set socket permissions for container access
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = std::fs::metadata(&self.config.socket_path)?.permissions();
            perms.set_mode(0o666);
            std::fs::set_permissions(&self.config.socket_path, perms)?;
        }

        loop {
            match listener.accept().await {
                Ok((stream, _)) => {
                    let broker = self.clone_for_connection();
                    tokio::spawn(async move {
                        if let Err(e) = broker.handle_connection(stream).await {
                            error!("D-Bus connection error: {}", e);
                        }
                    });
                }
                Err(e) => {
                    error!("D-Bus accept error: {}", e);
                    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
                }
            }
        }
    }

    fn clone_for_connection(&self) -> ConnectionHandler {
        ConnectionHandler {
            config: self.config.clone(),
            global_bus: self.global_bus.clone(),
            container_buses: self.container_buses.clone(),
            event_tx: self.event_tx.clone(),
            serial_counter: self.serial_counter.clone(),
            supervisor: self.supervisor.clone(),
        }
    }

    /// Attach the supervisor that `org.pnnext.Supervisor` calls act on.
    ///
    /// Separate from `new` because the supervisor must be initialized
    /// asynchronously first; see the field comment.
    pub fn with_supervisor(mut self, supervisor: Arc<ServiceSupervisor>) -> Self {
        self.supervisor = Some(supervisor);
        self
    }

    /// Register a well-known service name
    pub async fn register_service(&self, registration: ServiceRegistration) -> Result<()> {
        let container = registration.container.clone();
        let bus = if self.config.isolate_containers && container.is_some() {
            self.get_or_create_container_bus(container.as_ref().unwrap())
                .await
        } else {
            self.global_bus.clone()
        };

        let mut bus_guard = bus.write().await;

        // Check if name is allowed
        if !self.is_name_allowed(&registration.name) {
            bail!("Service name not allowed: {}", registration.name);
        }

        // Check for conflicts
        if bus_guard.services.contains_key(&registration.name) {
            bail!("Service name already registered: {}", registration.name);
        }

        bus_guard
            .services
            .insert(registration.name.clone(), registration.clone());

        let service_name = registration.name.clone();
        let container_for_event = container.clone();

        let _ = self.event_tx.send(DBusEvent::ServiceRegistered {
            name: service_name,
            container: container_for_event,
        });

        info!("Registered D-Bus service: {}", registration.name);
        Ok(())
    }

    /// Unregister a service
    pub async fn unregister_service(&self, name: &str, container: Option<&str>) -> Result<()> {
        let bus = if self.config.isolate_containers && container.is_some() {
            self.get_or_create_container_bus(container.unwrap()).await
        } else {
            self.global_bus.clone()
        };

        let mut bus_guard = bus.write().await;
        bus_guard.services.remove(name);

        let _ = self.event_tx.send(DBusEvent::ServiceUnregistered {
            name: name.to_string(),
        });

        info!("Unregistered D-Bus service: {}", name);
        Ok(())
    }

    /// Look up a service
    pub async fn lookup_service(
        &self,
        name: &str,
        container: Option<&str>,
    ) -> Option<ServiceRegistration> {
        let bus = if self.config.isolate_containers && container.is_some() {
            self.get_or_create_container_bus(container.unwrap()).await
        } else {
            self.global_bus.clone()
        };

        let bus_guard = bus.read().await;
        bus_guard.services.get(name).cloned()
    }

    /// List all services
    pub async fn list_services(&self, container: Option<&str>) -> Vec<ServiceRegistration> {
        let bus = if self.config.isolate_containers && container.is_some() {
            self.get_or_create_container_bus(container.unwrap()).await
        } else {
            self.global_bus.clone()
        };

        let bus_guard = bus.read().await;
        bus_guard.services.values().cloned().collect()
    }

    async fn get_or_create_container_bus(&self, container: &str) -> Arc<RwLock<ContainerBus>> {
        let mut buses = self.container_buses.write().await;
        buses
            .entry(container.to_string())
            .or_insert_with(|| {
                Arc::new(RwLock::new(ContainerBus {
                    connections: HashMap::new(),
                    services: HashMap::new(),
                    pending_replies: HashMap::new(),
                    signal_subscriptions: HashMap::new(),
                }))
            })
            .clone()
    }

    fn is_name_allowed(&self, name: &str) -> bool {
        for allowed in &self.config.allowed_names {
            if allowed.ends_with('*') {
                let prefix = &allowed[..allowed.len() - 1];
                if name.starts_with(prefix) {
                    return true;
                }
            } else if name == allowed {
                return true;
            }
        }
        false
    }

    /// Generate next serial number
    async fn next_serial(&self) -> u32 {
        let mut counter = self.serial_counter.write().await;
        let serial = *counter;
        *counter = counter.wrapping_add(1);
        if *counter == 0 {
            *counter = 1;
        }
        serial
    }
}

/// Connection handler for individual client connections
struct ConnectionHandler {
    config: DBusBrokerConfig,
    global_bus: Arc<RwLock<ContainerBus>>,
    container_buses: Arc<RwLock<HashMap<String, Arc<RwLock<ContainerBus>>>>>,
    event_tx: broadcast::Sender<DBusEvent>,
    serial_counter: Arc<RwLock<u32>>,
    supervisor: Option<Arc<ServiceSupervisor>>,
}

impl ConnectionHandler {
    async fn handle_connection(&self, mut stream: UnixStream) -> Result<()> {
        let (rd, wr) = stream.split();
        let mut reader = BufReader::new(rd);
        let mut writer = wr;

        // Generate unique connection name
        let unique_name = format!(
            ":1.{}",
            uuid::Uuid::new_v4().simple().to_string()[..8].to_string()
        );
        let container = None; // Would extract from credentials in real implementation

        // Send Hello reply
        let hello_reply = self.create_hello_reply(&unique_name).await?;
        self.send_message(&mut writer, &hello_reply).await?;

        // Register connection
        self.register_connection(&unique_name, &container).await?;

        let _ = self.event_tx.send(DBusEvent::ConnectionOpened {
            unique_name: unique_name.clone(),
            container: container.clone(),
        });

        // Message loop
        loop {
            let mut length_buf = [0u8; 4];
            if reader.read_exact(&mut length_buf).await.is_err() {
                break; // Connection closed
            }

            let length = u32::from_le_bytes(length_buf) as usize;
            if length > self.config.max_message_size {
                error!("Message too large: {} bytes", length);
                break;
            }

            let mut buf = vec![0u8; length];
            reader.read_exact(&mut buf).await?;

            let message: DBusMessage =
                serde_json::from_slice(&buf).context("Failed to parse D-Bus message")?;

            if self.config.log_messages {
                debug!("Received message: {:?}", message.header);
            }

            // Handle message
            let replies = self
                .process_message(&unique_name, &container, &message)
                .await?;

            for reply in replies {
                self.send_message(&mut writer, &reply).await?;
            }
        }

        // Cleanup
        self.unregister_connection(&unique_name).await?;

        let _ = self
            .event_tx
            .send(DBusEvent::ConnectionClosed { unique_name });

        Ok(())
    }

    async fn create_hello_reply(&self, unique_name: &str) -> Result<DBusMessage> {
        let serial = {
            let mut counter = self.serial_counter.write().await;
            let s = *counter;
            *counter = counter.wrapping_add(1);
            s
        };

        Ok(DBusMessage {
            header: MessageHeader {
                message_type: MessageType::MethodReturn,
                flags: 0,
                version: 1,
                serial,
                path: Some("/org/freedesktop/DBus".to_string()),
                interface: Some("org.freedesktop.DBus".to_string()),
                member: Some("Hello".to_string()),
                destination: Some(unique_name.to_string()),
                sender: Some("org.freedesktop.DBus".to_string()),
                signature: Some("s".to_string()),
                unix_fds: 0,
            },
            body: serde_json::to_vec(unique_name)?,
        })
    }

    async fn register_connection(
        &self,
        unique_name: &str,
        container: &Option<String>,
    ) -> Result<()> {
        let bus = if self.config.isolate_containers && container.is_some() {
            self.get_or_create_container_bus(container.as_ref().unwrap())
                .await
        } else {
            self.global_bus.clone()
        };

        let mut bus_guard = bus.write().await;
        bus_guard.connections.insert(
            unique_name.to_string(),
            ConnectionInfo {
                unique_name: unique_name.to_string(),
                bus_name: None,
                pid: 0, // Would get from socket credentials
                uid: 0,
                container: container.clone(),
                connected_at: Utc::now(),
                is_monitor: false,
            },
        );

        Ok(())
    }

    async fn unregister_connection(&self, unique_name: &str) -> Result<()> {
        // Try all buses
        let mut buses = vec![self.global_bus.clone()];

        if let Ok(container_buses) = self.container_buses.try_read() {
            for bus in container_buses.values() {
                buses.push(bus.clone());
            }
        }

        for bus in buses {
            let mut bus_guard = bus.write().await;
            bus_guard.connections.remove(unique_name);
        }

        Ok(())
    }

    async fn get_or_create_container_bus(&self, container: &str) -> Arc<RwLock<ContainerBus>> {
        let mut buses = self.container_buses.write().await;
        buses
            .entry(container.to_string())
            .or_insert_with(|| {
                Arc::new(RwLock::new(ContainerBus {
                    connections: HashMap::new(),
                    services: HashMap::new(),
                    pending_replies: HashMap::new(),
                    signal_subscriptions: HashMap::new(),
                }))
            })
            .clone()
    }

    async fn process_message(
        &self,
        sender: &str,
        container: &Option<String>,
        message: &DBusMessage,
    ) -> Result<Vec<DBusMessage>> {
        let mut replies = Vec::new();

        match message.header.message_type {
            MessageType::MethodCall => {
                replies.extend(self.handle_method_call(sender, container, message).await?);
            }
            MessageType::MethodReturn | MessageType::Error => {
                self.handle_reply(sender, message).await?;
            }
            MessageType::Signal => {
                self.handle_signal(sender, container, message).await?;
            }
        }

        Ok(replies)
    }

    async fn handle_method_call(
        &self,
        sender: &str,
        container: &Option<String>,
        message: &DBusMessage,
    ) -> Result<Vec<DBusMessage>> {
        let interface = message.header.interface.as_deref().unwrap_or("");
        let member = message.header.member.as_deref().unwrap_or("");
        let path = message.header.path.as_deref().unwrap_or("/");
        let destination = message.header.destination.as_deref();

        let _ = self.event_tx.send(DBusEvent::MethodCall {
            sender: sender.to_string(),
            destination: destination.unwrap_or("").to_string(),
            path: path.to_string(),
            interface: interface.to_string(),
            member: member.to_string(),
        });

        match (interface, member) {
            ("org.freedesktop.DBus", "RequestName") => {
                self.handle_request_name(sender, container, message).await
            }
            ("org.freedesktop.DBus", "ReleaseName") => {
                self.handle_release_name(sender, container, message).await
            }
            ("org.freedesktop.DBus", "NameHasOwner") => {
                self.handle_name_has_owner(sender, container, message).await
            }
            ("org.freedesktop.DBus", "ListNames") => {
                self.handle_list_names(sender, container, message).await
            }
            ("org.freedesktop.DBus", "AddMatch") => {
                self.handle_add_match(sender, container, message).await
            }
            ("org.freedesktop.DBus", "RemoveMatch") => {
                self.handle_remove_match(sender, container, message).await
            }
            ("org.freedesktop.DBus.Introspectable", "Introspect") => {
                self.handle_introspect(sender, container, message, path)
                    .await
            }
            ("org.pnnext.Supervisor", _) => {
                self.handle_supervisor_method(sender, container, message)
                    .await
            }
            _ => {
                // Forward to destination or return error
                if let Some(dest) = destination {
                    self.forward_message(sender, dest, message).await
                } else {
                    Ok(vec![self.create_error_reply(
                        &message,
                        "org.freedesktop.DBus.Error.UnknownMethod",
                        &format!("Unknown method {}.{}", interface, member),
                    )?])
                }
            }
        }
    }

    async fn handle_request_name(
        &self,
        sender: &str,
        container: &Option<String>,
        message: &DBusMessage,
    ) -> Result<Vec<DBusMessage>> {
        // Parse name from body (string)
        let name: String = serde_json::from_slice(&message.body)?;

        let bus = if self.config.isolate_containers && container.is_some() {
            self.get_or_create_container_bus(container.as_ref().unwrap())
                .await
        } else {
            self.global_bus.clone()
        };

        let mut bus_guard = bus.write().await;

        // Check if name is allowed
        if !self.is_name_allowed(&name) {
            return Ok(vec![self.create_error_reply(
                &message,
                "org.freedesktop.DBus.Error.AccessDenied",
                &format!("Name not allowed: {}", name),
            )?]);
        }

        // Check if already owned
        if bus_guard.services.contains_key(&name) {
            return Ok(vec![self.create_error_reply(
                &message,
                "org.freedesktop.DBus.Error.NameExists",
                &format!("Name already exists: {}", name),
            )?]);
        }

        // Register as a bus name for this connection
        bus_guard.services.insert(
            name.clone(),
            ServiceRegistration {
                name: name.clone(),
                path: "/".to_string(),
                interface: "org.freedesktop.DBus".to_string(),
                pid: 0,
                container: container.clone(),
                registered_at: Utc::now(),
                properties: HashMap::new(),
            },
        );

        // Update connection info
        if let Some(conn) = bus_guard.connections.get_mut(sender) {
            conn.bus_name = Some(name.clone());
        }

        // Reply with success (1 = primary owner)
        Ok(vec![DBusMessage {
            header: MessageHeader {
                message_type: MessageType::MethodReturn,
                flags: 0,
                version: 1,
                serial: {
                    let mut counter = self.serial_counter.write().await;
                    let s = *counter;
                    *counter = counter.wrapping_add(1);
                    s
                },
                path: message.header.path.clone(),
                interface: Some("org.freedesktop.DBus".to_string()),
                member: Some("RequestName".to_string()),
                destination: Some(sender.to_string()),
                sender: Some("org.freedesktop.DBus".to_string()),
                signature: Some("u".to_string()),
                unix_fds: 0,
            },
            body: serde_json::to_vec(&1u32)?,
        }])
    }

    async fn handle_release_name(
        &self,
        sender: &str,
        container: &Option<String>,
        message: &DBusMessage,
    ) -> Result<Vec<DBusMessage>> {
        let name: String = serde_json::from_slice(&message.body)?;

        let bus = if self.config.isolate_containers && container.is_some() {
            self.get_or_create_container_bus(container.as_ref().unwrap())
                .await
        } else {
            self.global_bus.clone()
        };

        let mut bus_guard = bus.write().await;
        bus_guard.services.remove(&name);

        // Update connection info
        if let Some(conn) = bus_guard.connections.get_mut(sender) {
            conn.bus_name = None;
        }

        Ok(vec![DBusMessage {
            header: MessageHeader {
                message_type: MessageType::MethodReturn,
                flags: 0,
                version: 1,
                serial: {
                    let mut counter = self.serial_counter.write().await;
                    let s = *counter;
                    *counter = counter.wrapping_add(1);
                    s
                },
                path: message.header.path.clone(),
                interface: Some("org.freedesktop.DBus".to_string()),
                member: Some("ReleaseName".to_string()),
                destination: Some(sender.to_string()),
                sender: Some("org.freedesktop.DBus".to_string()),
                signature: Some("u".to_string()),
                unix_fds: 0,
            },
            body: serde_json::to_vec(&1u32)?,
        }])
    }

    async fn handle_name_has_owner(
        &self,
        sender: &str,
        container: &Option<String>,
        message: &DBusMessage,
    ) -> Result<Vec<DBusMessage>> {
        let name: String = serde_json::from_slice(&message.body)?;

        let bus = if self.config.isolate_containers && container.is_some() {
            self.get_or_create_container_bus(container.as_ref().unwrap())
                .await
        } else {
            self.global_bus.clone()
        };

        let bus_guard = bus.read().await;
        let has_owner = bus_guard.services.contains_key(&name);

        Ok(vec![DBusMessage {
            header: MessageHeader {
                message_type: MessageType::MethodReturn,
                flags: 0,
                version: 1,
                serial: {
                    let mut counter = self.serial_counter.write().await;
                    let s = *counter;
                    *counter = counter.wrapping_add(1);
                    s
                },
                path: message.header.path.clone(),
                interface: Some("org.freedesktop.DBus".to_string()),
                member: Some("NameHasOwner".to_string()),
                destination: Some(sender.to_string()),
                sender: Some("org.freedesktop.DBus".to_string()),
                signature: Some("b".to_string()),
                unix_fds: 0,
            },
            body: serde_json::to_vec(&has_owner)?,
        }])
    }

    async fn handle_list_names(
        &self,
        sender: &str,
        container: &Option<String>,
        message: &DBusMessage,
    ) -> Result<Vec<DBusMessage>> {
        let bus = if self.config.isolate_containers && container.is_some() {
            self.get_or_create_container_bus(container.as_ref().unwrap())
                .await
        } else {
            self.global_bus.clone()
        };

        let bus_guard = bus.read().await;
        let names: Vec<String> = bus_guard.services.keys().cloned().collect();

        Ok(vec![DBusMessage {
            header: MessageHeader {
                message_type: MessageType::MethodReturn,
                flags: 0,
                version: 1,
                serial: {
                    let mut counter = self.serial_counter.write().await;
                    let s = *counter;
                    *counter = counter.wrapping_add(1);
                    s
                },
                path: message.header.path.clone(),
                interface: Some("org.freedesktop.DBus".to_string()),
                member: Some("ListNames".to_string()),
                destination: Some(sender.to_string()),
                sender: Some("org.freedesktop.DBus".to_string()),
                signature: Some("as".to_string()),
                unix_fds: 0,
            },
            body: serde_json::to_vec(&names)?,
        }])
    }

    async fn handle_add_match(
        &self,
        sender: &str,
        container: &Option<String>,
        message: &DBusMessage,
    ) -> Result<Vec<DBusMessage>> {
        // Parse match rule (simplified)
        let rule: String = serde_json::from_slice(&message.body)?;

        let bus = if self.config.isolate_containers && container.is_some() {
            self.get_or_create_container_bus(container.as_ref().unwrap())
                .await
        } else {
            self.global_bus.clone()
        };

        let mut bus_guard = bus.write().await;

        // Extract interface.member from rule
        if let Some(member_start) = rule.find("member='") {
            let member = &rule[member_start + 8..];
            if let Some(end) = member.find('\'') {
                let member = &member[..end];
                bus_guard
                    .signal_subscriptions
                    .entry(member.to_string())
                    .or_default()
                    .insert(sender.to_string());
            }
        }

        Ok(vec![DBusMessage {
            header: MessageHeader {
                message_type: MessageType::MethodReturn,
                flags: 0,
                version: 1,
                serial: {
                    let mut counter = self.serial_counter.write().await;
                    let s = *counter;
                    *counter = counter.wrapping_add(1);
                    s
                },
                path: message.header.path.clone(),
                interface: Some("org.freedesktop.DBus".to_string()),
                member: Some("AddMatch".to_string()),
                destination: Some(sender.to_string()),
                sender: Some("org.freedesktop.DBus".to_string()),
                signature: None,
                unix_fds: 0,
            },
            body: Vec::new(),
        }])
    }

    async fn handle_remove_match(
        &self,
        sender: &str,
        container: &Option<String>,
        message: &DBusMessage,
    ) -> Result<Vec<DBusMessage>> {
        let rule: String = serde_json::from_slice(&message.body)?;

        let bus = if self.config.isolate_containers && container.is_some() {
            self.get_or_create_container_bus(container.as_ref().unwrap())
                .await
        } else {
            self.global_bus.clone()
        };

        let mut bus_guard = bus.write().await;

        // Extract and remove
        if let Some(member_start) = rule.find("member='") {
            let member = &rule[member_start + 8..];
            if let Some(end) = member.find('\'') {
                let member = &member[..end];
                if let Some(subs) = bus_guard.signal_subscriptions.get_mut(member) {
                    subs.remove(sender);
                    if subs.is_empty() {
                        bus_guard.signal_subscriptions.remove(member);
                    }
                }
            }
        }

        Ok(vec![DBusMessage {
            header: MessageHeader {
                message_type: MessageType::MethodReturn,
                flags: 0,
                version: 1,
                serial: {
                    let mut counter = self.serial_counter.write().await;
                    let s = *counter;
                    *counter = counter.wrapping_add(1);
                    s
                },
                path: message.header.path.clone(),
                interface: Some("org.freedesktop.DBus".to_string()),
                member: Some("RemoveMatch".to_string()),
                destination: Some(sender.to_string()),
                sender: Some("org.freedesktop.DBus".to_string()),
                signature: None,
                unix_fds: 0,
            },
            body: Vec::new(),
        }])
    }

    async fn handle_introspect(
        &self,
        sender: &str,
        container: &Option<String>,
        message: &DBusMessage,
        path: &str,
    ) -> Result<Vec<DBusMessage>> {
        let introspection = if path == "/org/freedesktop/DBus" {
            r#"<!DOCTYPE node PUBLIC "-//freedesktop//DTD D-BUS Object Introspection 1.0//EN"
"http://www.freedesktop.org/standards/dbus/1.0/introspect.dtd">
<node>
  <interface name="org.freedesktop.DBus">
    <method name="Hello">
      <arg name="name" type="s" direction="out"/>
    </method>
    <method name="RequestName">
      <arg name="name" type="s" direction="in"/>
      <arg name="flags" type="u" direction="in"/>
      <arg name="result" type="u" direction="out"/>
    </method>
    <method name="ReleaseName">
      <arg name="name" type="s" direction="in"/>
      <arg name="result" type="u" direction="out"/>
    </method>
    <method name="NameHasOwner">
      <arg name="name" type="s" direction="in"/>
      <arg name="has_owner" type="b" direction="out"/>
    </method>
    <method name="ListNames">
      <arg name="names" type="as" direction="out"/>
    </method>
    <method name="AddMatch">
      <arg name="rule" type="s" direction="in"/>
    </method>
    <method name="RemoveMatch">
      <arg name="rule" type="s" direction="in"/>
    </method>
  </interface>
  <interface name="org.freedesktop.DBus.Introspectable">
    <method name="Introspect">
      <arg name="xml_data" type="s" direction="out"/>
    </method>
  </interface>
</node>"#
        } else if path.starts_with("/org/pnnext") {
            r#"<!DOCTYPE node PUBLIC "-//freedesktop//DTD D-BUS Object Introspection 1.0//EN"
"http://www.freedesktop.org/standards/dbus/1.0/introspect.dtd">
<node>
  <interface name="org.pnnext.Supervisor">
    <method name="StartService">
      <arg name="name" type="s" direction="in"/>
    </method>
    <method name="StopService">
      <arg name="name" type="s" direction="in"/>
    </method>
    <method name="RestartService">
      <arg name="name" type="s" direction="in"/>
    </method>
    <method name="GetStatus">
      <arg name="name" type="s" direction="in"/>
      <arg name="status" type="a{sv}" direction="out"/>
    </method>
    <signal name="ServiceStateChanged">
      <arg name="name" type="s"/>
      <arg name="state" type="s"/>
    </signal>
  </interface>
  <interface name="org.freedesktop.DBus.Introspectable">
    <method name="Introspect">
      <arg name="xml_data" type="s" direction="out"/>
    </method>
  </interface>
</node>"#
        } else {
            ""
        };

        Ok(vec![DBusMessage {
            header: MessageHeader {
                message_type: MessageType::MethodReturn,
                flags: 0,
                version: 1,
                serial: {
                    let mut counter = self.serial_counter.write().await;
                    let s = *counter;
                    *counter = counter.wrapping_add(1);
                    s
                },
                path: message.header.path.clone(),
                interface: Some("org.freedesktop.DBus.Introspectable".to_string()),
                member: Some("Introspect".to_string()),
                destination: Some(sender.to_string()),
                sender: Some("org.freedesktop.DBus".to_string()),
                signature: Some("s".to_string()),
                unix_fds: 0,
            },
            body: serde_json::to_vec(&introspection)?,
        }])
    }

    async fn handle_supervisor_method(
        &self,
        sender: &str,
        container: &Option<String>,
        message: &DBusMessage,
    ) -> Result<Vec<DBusMessage>> {
        // Bodies are a single JSON-encoded string, the same convention
        // `handle_request_name` uses.
        let name: String = serde_json::from_slice(&message.body).map_err(|e| {
            anyhow::anyhow!(
                "{} expects one JSON-encoded service name in the body: {}",
                message.header.member.as_deref().unwrap_or("?"),
                e
            )
        })?;

        let Some(supervisor) = self.supervisor.as_ref() else {
            return Ok(vec![self.create_error_reply(
                &message,
                "org.freedesktop.DBus.Error.NotSupported",
                "This broker was started without a supervisor: no service manager is \
                 running to forward this call to. Start the daemon, or use \
                 'podnext service <action>' directly.",
            )?]);
        };

        let member = message.header.member.as_deref().unwrap_or_default();
        let _ = sender;
        let _ = container;

        match member {
            "StartService" => {
                supervisor.start_service(&name).await?;
                let body = serde_json::to_vec(&serde_json::json!({
                    "service": name,
                    "state": "started",
                }))?;
                Ok(vec![self.create_method_return(message, "s", body)?])
            }
            // The introspection XML advertises a plain Stop; force stays
            // reserved for callers that ask for it, so a normal stop keeps the
            // supervisor's graceful path.
            "StopService" => {
                supervisor.stop_service(&name, false).await?;
                let body = serde_json::to_vec(&serde_json::json!({
                    "service": name,
                    "state": "stopped",
                }))?;
                Ok(vec![self.create_method_return(message, "s", body)?])
            }
            "RestartService" => {
                supervisor.restart_service(&name).await?;
                let body = serde_json::to_vec(&serde_json::json!({
                    "service": name,
                    "state": "restarted",
                }))?;
                Ok(vec![self.create_method_return(message, "s", body)?])
            }
            "GetStatus" => {
                let status = supervisor.get_status(&name).await?;
                let body = serde_json::to_vec(&status)?;
                Ok(vec![self.create_method_return(message, "s", body)?])
            }
            other => Ok(vec![self.create_error_reply(
                &message,
                "org.freedesktop.DBus.Error.UnknownMethod",
                &format!("Unknown supervisor method '{}'", other),
            )?]),
        }
    }

    async fn handle_reply(&self, _sender: &str, _message: &DBusMessage) -> Result<()> {
        // Handle pending replies - would look up by serial
        Ok(())
    }

    async fn handle_signal(
        &self,
        sender: &str,
        container: &Option<String>,
        message: &DBusMessage,
    ) -> Result<()> {
        let interface = message.header.interface.as_deref().unwrap_or("");
        let member = message.header.member.as_deref().unwrap_or("");
        let signal_key = format!("{}.{}", interface, member);

        let bus = if self.config.isolate_containers && container.is_some() {
            self.get_or_create_container_bus(container.as_ref().unwrap())
                .await
        } else {
            self.global_bus.clone()
        };

        let bus_guard = bus.read().await;

        if let Some(subscribers) = bus_guard.signal_subscriptions.get(&signal_key) {
            for subscriber in subscribers {
                // Would forward signal to subscriber
                // For now just log
                debug!("Signal {} forwarded to {}", signal_key, subscriber);
            }
        }

        Ok(())
    }

    async fn forward_message(
        &self,
        sender: &str,
        destination: &str,
        message: &DBusMessage,
    ) -> Result<Vec<DBusMessage>> {
        let bus = self.global_bus.read().await;

        // Check if destination is a known connection or service
        if bus.connections.contains_key(destination) || bus.services.contains_key(destination) {
            let _ = self.event_tx.send(DBusEvent::MessageSent {
                from: sender.to_string(),
                to: destination.to_string(),
                message_type: message.header.message_type,
            });

            // In real implementation, would queue to destination's connection
            Ok(vec![])
        } else {
            Ok(vec![self.create_error_reply(
                &message,
                "org.freedesktop.DBus.Error.ServiceUnknown",
                &format!("Service unknown: {}", destination),
            )?])
        }
    }

    fn create_error_reply(
        &self,
        original: &DBusMessage,
        error_name: &str,
        error_msg: &str,
    ) -> Result<DBusMessage> {
        let serial = self
            .serial_counter
            .try_write()
            .map(|mut c| {
                let s = *c;
                *c = c.wrapping_add(1);
                s
            })
            .unwrap_or(0);

        let body = serde_json::to_vec(&(error_name, error_msg))?;

        Ok(DBusMessage {
            header: MessageHeader {
                message_type: MessageType::Error,
                flags: 0,
                version: 1,
                serial,
                path: original.header.path.clone(),
                interface: original.header.interface.clone(),
                member: original.header.member.clone(),
                destination: original.header.sender.clone(),
                sender: Some("org.freedesktop.DBus".to_string()),
                signature: Some("ss".to_string()),
                unix_fds: 0,
            },
            body,
        })
    }

    /// A successful reply to a method call.
    ///
    /// Only `create_error_reply` existed, so every method that needed to answer
    /// with data had to either lie or go unimplemented.
    fn create_method_return(
        &self,
        original: &DBusMessage,
        signature: &str,
        body: Vec<u8>,
    ) -> Result<DBusMessage> {
        let serial = self
            .serial_counter
            .try_write()
            .map(|mut c| {
                let s = *c;
                *c = c.wrapping_add(1);
                s
            })
            .unwrap_or(0);

        Ok(DBusMessage {
            header: MessageHeader {
                message_type: MessageType::MethodReturn,
                flags: 0,
                version: 1,
                serial,
                path: original.header.path.clone(),
                interface: original.header.interface.clone(),
                member: original.header.member.clone(),
                destination: original.header.sender.clone(),
                sender: Some("org.pnnext.Supervisor".to_string()),
                signature: Some(signature.to_string()),
                unix_fds: 0,
            },
            body,
        })
    }

    fn is_name_allowed(&self, name: &str) -> bool {
        for allowed in &self.config.allowed_names {
            if allowed.ends_with('*') {
                let prefix = &allowed[..allowed.len() - 1];
                if name.starts_with(prefix) {
                    return true;
                }
            } else if name == allowed {
                return true;
            }
        }
        false
    }

    async fn send_message(
        &self,
        writer: &mut tokio::net::unix::WriteHalf<'_>,
        message: &DBusMessage,
    ) -> Result<()> {
        let data = serde_json::to_vec(message)?;
        let length = data.len() as u32;
        writer.write_all(&length.to_le_bytes()).await?;
        writer.write_all(&data).await?;
        writer.flush().await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[tokio::test(flavor = "multi_thread")]
    async fn test_dbus_broker_creation() {
        let temp_dir = tempdir().unwrap();
        let socket_path = temp_dir
            .path()
            .join("dbus.sock")
            .to_string_lossy()
            .to_string();

        let config = DBusBrokerConfig {
            socket_path: socket_path.clone(),
            allowed_names: [
                "org.freedesktop.DBus".to_string(),
                "org.pnnext.*".to_string(),
                "org.test.*".to_string(),
            ]
            .into(),
            ..Default::default()
        };

        let broker = DBusBroker::new(config);

        // Test service registration
        let registration = ServiceRegistration {
            name: "org.test.Service".to_string(),
            path: "/org/test/Service".to_string(),
            interface: "org.test.Service".to_string(),
            pid: 1234,
            container: Some("test-container".to_string()),
            registered_at: Utc::now(),
            properties: HashMap::new(),
        };

        let result = broker.register_service(registration).await;
        assert!(result.is_ok());

        // Test lookup
        let service = broker
            .lookup_service("org.test.Service", Some("test-container"))
            .await;
        assert!(service.is_some());
        assert_eq!(service.unwrap().name, "org.test.Service");
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_dbus_broker_list_services() {
        let temp_dir = tempdir().unwrap();
        let socket_path = temp_dir
            .path()
            .join("dbus.sock")
            .to_string_lossy()
            .to_string();

        let config = DBusBrokerConfig {
            socket_path,
            allowed_names: [
                "org.freedesktop.DBus".to_string(),
                "org.pnnext.*".to_string(),
                "org.test.*".to_string(),
            ]
            .into(),
            ..Default::default()
        };

        let broker = DBusBroker::new(config);

        // Register multiple services
        for i in 0..3 {
            let registration = ServiceRegistration {
                name: format!("org.test.Service{}", i),
                path: format!("/org/test/Service{}", i),
                interface: format!("org.test.Service{}", i),
                pid: 1000 + i,
                container: None,
                registered_at: Utc::now(),
                properties: HashMap::new(),
            };
            broker.register_service(registration).await.unwrap();
        }

        let services = broker.list_services(None).await;
        assert_eq!(services.len(), 3);
    }
}
