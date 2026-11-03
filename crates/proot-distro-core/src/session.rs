//! Session management for PRoot Distro

use anyhow::Result;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::os::unix::io::AsRawFd;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use uuid::Uuid;

/// Persistent session record for active container instances.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SessionRecord {
    pub container_name: String,
    pub pid: u32,
    pub started_at: u64,
    pub login_uid: String,
    pub cmdline: Vec<String>,
}

/// Directory storing runtime session files.
pub fn sessions_dir() -> PathBuf {
    if let Ok(dir) = std::env::var("RUNTIME_DIR") {
        if !dir.trim().is_empty() {
            return PathBuf::from(dir).join("sessions");
        }
    }
    let home = dirs::home_dir()
        .or_else(|| std::env::var("HOME").ok().map(PathBuf::from))
        .unwrap_or_else(|| PathBuf::from("/tmp"));
    home.join(".podnext").join("sessions")
}

/// Session file path for a given PID.
pub fn session_file_path(pid: u32) -> PathBuf {
    sessions_dir().join(format!("{}.json", pid))
}

/// Register a new running session, taking an inherited exclusive lock.
pub fn register_session(
    container: &str,
    pid: u32,
    uid: &str,
    cmd: &[String],
) -> Result<std::fs::File> {
    let dir = sessions_dir();
    std::fs::create_dir_all(&dir)?;

    let path = session_file_path(pid);
    let file = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(true)
        .open(&path)?;

    let fd = file.as_raw_fd();

    // Acquire exclusive advisory lock
    let ret = unsafe { libc::flock(fd, libc::LOCK_EX) };
    if ret != 0 {
        let err = std::io::Error::last_os_error();
        let _ = std::fs::remove_file(&path);
        return Err(anyhow::anyhow!(
            "Failed to acquire LOCK_EX on session file '{}': {}",
            path.display(),
            err
        ));
    }

    // Unset FD_CLOEXEC so child process inherits descriptor
    unsafe {
        let flags = libc::fcntl(fd, libc::F_GETFD);
        if flags < 0 {
            return Err(anyhow::anyhow!(
                "fcntl(F_GETFD) failed on session fd: {}",
                std::io::Error::last_os_error()
            ));
        }
        if libc::fcntl(fd, libc::F_SETFD, flags & !libc::FD_CLOEXEC) < 0 {
            return Err(anyhow::anyhow!(
                "fcntl(F_SETFD) failed to clear FD_CLOEXEC: {}",
                std::io::Error::last_os_error()
            ));
        }
    }

    let started_at = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    let record = SessionRecord {
        container_name: container.to_string(),
        pid,
        started_at,
        login_uid: uid.to_string(),
        cmdline: cmd.to_vec(),
    };

    use std::io::Write;
    let data = serde_json::to_vec_pretty(&record)?;
    (&file).write_all(&data)?;
    (&file).flush()?;

    Ok(file)
}

/// Query active sessions by probing non-blocking shared locks on session files.
pub fn active_sessions() -> Result<Vec<SessionRecord>> {
    let dir = sessions_dir();
    if !dir.exists() {
        return Ok(Vec::new());
    }

    let mut sessions = Vec::new();
    for entry in std::fs::read_dir(&dir)? {
        let entry = match entry {
            Ok(e) => e,
            Err(_) => continue,
        };
        let path = entry.path();
        if path.extension().and_then(|s| s.to_str()) != Some("json") {
            continue;
        }

        let file = match std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(&path)
        {
            Ok(f) => f,
            Err(_) => match std::fs::File::open(&path) {
                Ok(f) => f,
                Err(_) => continue,
            },
        };

        let fd = file.as_raw_fd();
        let ret = unsafe { libc::flock(fd, libc::LOCK_SH | libc::LOCK_NB) };
        if ret == 0 {
            // Lock succeeded: process holding LOCK_EX has terminated. Unlink file.
            unsafe {
                libc::flock(fd, libc::LOCK_UN);
            }
            drop(file);
            let _ = std::fs::remove_file(&path);
        } else {
            let err = std::io::Error::last_os_error();
            let errno = err.raw_os_error().unwrap_or(0);
            if errno == libc::EWOULDBLOCK || errno == libc::EAGAIN {
                // Session is alive (LOCK_EX held)
                if let Ok(content) = std::fs::read_to_string(&path) {
                    if let Ok(record) = serde_json::from_str::<SessionRecord>(&content) {
                        sessions.push(record);
                    }
                }
            }
        }
    }

    Ok(sessions)
}

/// Send a signal to a session process.
pub fn kill_session(pid: u32, signal: i32) -> Result<()> {
    let ret = unsafe { libc::kill(pid as libc::pid_t, signal) };
    if ret != 0 {
        return Err(anyhow::anyhow!(
            "kill_session({}, {}) failed: {}",
            pid,
            signal,
            std::io::Error::last_os_error()
        ));
    }
    Ok(())
}

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn session_record_serialization() {
        let record = SessionRecord {
            container_name: "test-box".to_string(),
            pid: 12345,
            started_at: 1700000000,
            login_uid: "1000".to_string(),
            cmdline: vec!["/bin/sh".to_string(), "-l".to_string()],
        };
        let json = serde_json::to_string(&record).unwrap();
        let parsed: SessionRecord = serde_json::from_str(&json).unwrap();
        assert_eq!(record, parsed);
    }

    #[test]
    fn session_registration_and_probing() {
        let tmp = std::env::temp_dir().join(format!("pdn-sess-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();

        std::env::set_var("RUNTIME_DIR", &tmp);

        let my_pid = std::process::id();
        let cmd = vec!["bash".to_string()];
        let file = register_session("c1", my_pid, "1000", &cmd).expect("register session");

        // The session file is locked by `file`
        let active = active_sessions().expect("active sessions");
        assert_eq!(active.len(), 1);
        assert_eq!(active[0].container_name, "c1");
        assert_eq!(active[0].pid, my_pid);

        // Drop the file to release the lock
        drop(file);

        // Now probing should find the lock free, prune the file, and return empty
        let active_after = active_sessions().expect("active sessions after drop");
        assert_eq!(active_after.len(), 0);

        let _ = std::fs::remove_dir_all(&tmp);
    }
}
