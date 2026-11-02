//! Progress tracking for PRoot Distro

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Progress tracker
pub struct ProgressTracker {
    operations: std::sync::Mutex<HashMap<String, ProgressInfo>>,
}

impl ProgressTracker {
    pub fn new() -> Self {
        Self {
            operations: std::sync::Mutex::new(HashMap::new()),
        }
    }

    pub fn start(&self, id: &str, total: u64, description: String) {
        let mut ops = self.operations.lock().unwrap();
        ops.insert(
            id.to_string(),
            ProgressInfo {
                id: id.to_string(),
                description,
                total,
                current: 0,
                started_at: std::time::Instant::now(),
                status: ProgressStatus::Running,
            },
        );
    }

    pub fn update(&self, id: &str, current: u64) {
        let mut ops = self.operations.lock().unwrap();
        if let Some(op) = ops.get_mut(id) {
            op.current = current;
        }
    }

    pub fn finish(&self, id: &str, success: bool) {
        let mut ops = self.operations.lock().unwrap();
        if let Some(op) = ops.get_mut(id) {
            op.status = if success {
                ProgressStatus::Completed
            } else {
                ProgressStatus::Failed
            };
        }
    }

    pub fn get(&self, id: &str) -> Option<ProgressInfo> {
        self.operations.lock().unwrap().get(id).cloned()
    }

    pub fn list(&self) -> Vec<ProgressInfo> {
        self.operations.lock().unwrap().values().cloned().collect()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProgressInfo {
    pub id: String,
    pub description: String,
    pub total: u64,
    pub current: u64,
    #[serde(skip, default = "default_instant")]
    pub started_at: std::time::Instant,
    pub status: ProgressStatus,
}

fn default_instant() -> std::time::Instant {
    std::time::Instant::now()
}

impl ProgressInfo {
    pub fn progress_percent(&self) -> f64 {
        if self.total == 0 {
            0.0
        } else {
            (self.current as f64 / self.total as f64) * 100.0
        }
    }

    pub fn elapsed(&self) -> std::time::Duration {
        self.started_at.elapsed()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ProgressStatus {
    Pending,
    Running,
    Completed,
    Failed,
    Cancelled,
}

/// Progress bar for CLI
pub struct ProgressBar {
    total: u64,
    current: u64,
    width: usize,
    prefix: String,
}

impl ProgressBar {
    pub fn new(total: u64, prefix: String) -> Self {
        Self {
            total,
            current: 0,
            width: 50,
            prefix,
        }
    }

    pub fn increment(&mut self, amount: u64) {
        self.current = (self.current + amount).min(self.total);
    }

    pub fn finish(&self) {
        println!();
    }

    pub fn draw(&self) {
        let percent = if self.total == 0 {
            0.0
        } else {
            self.current as f64 / self.total as f64
        };
        let filled = (percent * self.width as f64) as usize;
        let empty = self.width - filled;

        let bar = "█".repeat(filled) + &"░".repeat(empty);
        let percent = (self.current as f64 / self.total as f64 * 100.0) as u32;

        print!("\r{} [{}] {}%", self.prefix, bar, percent);
        use std::io::Write;
        std::io::stdout().flush().ok();
    }
}
