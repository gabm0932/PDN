//! Utility functions

use anyhow::Result;
use std::fs;
use std::path::Path;

/// Parse container:path format
#[allow(dead_code)]
pub fn parse_container_path(path: &str) -> Result<(Option<String>, String)> {
    if let Some(idx) = path.find(':') {
        let container = &path[..idx];
        let path = &path[idx + 1..];
        Ok((Some(container.to_string()), path.to_string()))
    } else {
        Ok((None, path.to_string()))
    }
}

/// Calculate directory size
#[allow(dead_code)]
pub fn dir_size(dir: &Path) -> Result<u64> {
    let mut size = 0;
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let meta = entry.metadata()?;
        if meta.is_file() {
            size += meta.len();
        } else if meta.is_dir() {
            size += dir_size(&entry.path())?;
        }
    }
    Ok(size)
}

/// Format size in human-readable format
pub fn format_size(bytes: u64) -> String {
    const UNITS: &[&str] = &["B", "KB", "MB", "GB", "TB"];
    let mut size = bytes as f64;
    let mut unit = 0;
    while size >= 1024.0 && unit < UNITS.len() - 1 {
        size /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{} {}", size as u64, UNITS[unit])
    } else {
        format!("{:.1} {}", size, UNITS[unit])
    }
}

/// Parse size string (e.g., "2G", "512M", "1024K")
#[allow(dead_code)]
pub fn parse_size(s: &str) -> Result<u64> {
    let s = s.trim().to_lowercase();
    let (num, unit) = if s.ends_with('k') {
        (&s[..s.len() - 1], 1024)
    } else if s.ends_with('m') {
        (&s[..s.len() - 1], 1024 * 1024)
    } else if s.ends_with('g') {
        (&s[..s.len() - 1], 1024 * 1024 * 1024)
    } else if s.ends_with('t') {
        (&s[..s.len() - 1], 1024 * 1024 * 1024 * 1024)
    } else {
        (s.as_str(), 1)
    };

    let num: u64 = num.parse()?;
    Ok(num * unit)
}
