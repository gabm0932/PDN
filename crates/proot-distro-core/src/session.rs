//! Session management for PRoot Distro

use anyhow::Result;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use uuid::Uuid;

/// Session manager
pub struct SessionManager {
    sessions: Arc<Mutex<HashMap<String, Session>>>,
}

impl SessionManager {
    pub fn new() -> Result<Self> {
        Ok(Self {
            sessions: Arc::new(Mutex::new(HashMap::new())),
        })
    }

    pub fn create(
        &self,
        _container_name: &str,
        options: &crate::login::LoginOptions,
    ) -> Result<Session> {
        let id = Uuid::new_v4().to_string();
        let session = Session {
            id: id.clone(),
            container_name: options.container_name.clone(),
            user: options.user.clone().unwrap_or_else(|| "root".to_string()),
            command: options.command.clone(),
            env: options.env.iter().cloned().collect(),
            cwd: options.cwd.clone().unwrap_or_else(|| "/".to_string()),
            created_at: Utc::now(),
            started_at: None,
            ended_at: None,
            status: SessionStatus::Created,
            pid: None,
        };

        let mut sessions = self.sessions.lock().unwrap();
        sessions.insert(id.clone(), session.clone());

        Ok(session)
    }

    pub fn get(&self, id: &str) -> Option<Session> {
        self.sessions.lock().unwrap().get(id).cloned()
    }

    pub fn list(&self) -> Vec<Session> {
        self.sessions.lock().unwrap().values().cloned().collect()
    }

    pub fn list_all(&self) -> Vec<crate::container::SessionInfo> {
        let sessions = self.sessions.lock().unwrap();
        sessions
            .values()
            .map(|s| crate::container::SessionInfo {
                container_name: s.container_name.clone(),
                user: s.user.clone(),
                pid: s.pid,
                status: format!("{:?}", s.status),
                started_at: s.started_at.unwrap_or(s.created_at),
            })
            .collect()
    }

    pub fn list_all_full(&self) -> Vec<Session> {
        let sessions = self.sessions.lock().unwrap();
        sessions.values().cloned().collect()
    }

    pub fn end(&self, id: &str) -> Result<()> {
        let mut sessions = self.sessions.lock().unwrap();
        if let Some(session) = sessions.get_mut(id) {
            session.status = SessionStatus::Ended;
            session.ended_at = Some(chrono::Utc::now());
        }
        Ok(())
    }
}

/// Session information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Session {
    pub id: String,
    pub container_name: String,
    pub user: String,
    pub command: Vec<String>,
    pub env: std::collections::HashMap<String, String>,
    pub cwd: String,
    pub created_at: DateTime<Utc>,
    pub started_at: Option<DateTime<Utc>>,
    pub ended_at: Option<DateTime<Utc>>,
    pub status: SessionStatus,
    pub pid: Option<u32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SessionStatus {
    Created,
    Running,
    Completed,
    Failed,
    Terminated,
    Ended,
}
