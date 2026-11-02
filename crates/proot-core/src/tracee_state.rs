//! Tracee state management for PRoot

use crate::tracee::{Tracee, TraceeState};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

/// Tracee state manager
pub struct TraceeStateManager {
    tracees: Arc<Mutex<HashMap<nix::unistd::Pid, Tracee>>>,
    state_history: Arc<Mutex<Vec<StateEvent>>>,
}

#[derive(Debug, Clone)]
pub struct StateEvent {
    pub timestamp: chrono::DateTime<chrono::Utc>,
    pub pid: nix::unistd::Pid,
    pub from: TraceeState,
    pub to: TraceeState,
    pub syscall: Option<i64>,
}

impl TraceeStateManager {
    pub fn new() -> Self {
        Self {
            tracees: Arc::new(Mutex::new(HashMap::new())),
            state_history: Arc::new(Mutex::new(Vec::new())),
        }
    }

    pub fn add_tracee(&self, tracee: crate::tracee::Tracee) {
        let pid = tracee.pid;
        self.tracees.lock().unwrap().insert(pid, tracee);
        self.record_state_change(pid, TraceeState::Running, TraceeState::Running, None);
    }

    pub fn remove_tracee(&self, pid: nix::unistd::Pid) -> Option<crate::tracee::Tracee> {
        self.tracees.lock().unwrap().remove(&pid)
    }

    pub fn get_tracee(&self, pid: nix::unistd::Pid) -> Option<crate::tracee::Tracee> {
        self.tracees.lock().unwrap().get(&pid).cloned()
    }

    pub fn get_all_tracees(&self) -> Vec<crate::tracee::Tracee> {
        self.tracees.lock().unwrap().values().cloned().collect()
    }

    pub fn update_state(
        &self,
        pid: nix::unistd::Pid,
        new_state: crate::tracee::TraceeState,
    ) -> bool {
        let mut tracees = self.tracees.lock().unwrap();
        if let Some(tracee) = tracees.get_mut(&pid) {
            let old_state = tracee.state;
            tracee.state = new_state;
            self.record_state_change(pid, old_state, new_state, None);
            true
        } else {
            false
        }
    }

    pub fn update_syscall_info(
        &self,
        pid: nix::unistd::Pid,
        info: crate::tracee::SyscallInfo,
    ) -> bool {
        let mut tracees = self.tracees.lock().unwrap();
        if let Some(tracee) = tracees.get_mut(&pid) {
            tracee.syscall_info = Some(info);
            true
        } else {
            false
        }
    }

    pub fn update_path_info(&self, pid: nix::unistd::Pid, info: crate::tracee::PathInfo) -> bool {
        let mut tracees = self.tracees.lock().unwrap();
        if let Some(tracee) = tracees.get_mut(&pid) {
            tracee.path_info = info;
            true
        } else {
            false
        }
    }

    pub fn record_state_change(
        &self,
        pid: nix::unistd::Pid,
        from: TraceeState,
        to: TraceeState,
        syscall: Option<i64>,
    ) {
        let event = StateEvent {
            timestamp: chrono::Utc::now(),
            pid,
            from,
            to,
            syscall,
        };
        self.state_history.lock().unwrap().push(event);
    }

    pub fn get_tracee_count(&self) -> usize {
        self.tracees.lock().unwrap().len()
    }

    pub fn get_running_tracees(&self) -> Vec<nix::unistd::Pid> {
        self.tracees
            .lock()
            .unwrap()
            .iter()
            .filter(|(_, t)| t.state == TraceeState::Running)
            .map(|(pid, _)| *pid)
            .collect()
    }

    pub fn get_syscall_stats(&self) -> HashMap<i64, u64> {
        let history = self.state_history.lock().unwrap();
        let mut stats = HashMap::new();
        for event in history.iter() {
            if let Some(syscall) = event.syscall {
                *stats.entry(syscall).or_insert(0) += 1;
            }
        }
        stats
    }
}
