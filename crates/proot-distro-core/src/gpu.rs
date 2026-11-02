//! GPU acceleration support for PRoot containers
//!
//! Provides device detection and passthrough for GPU acceleration
//! including Vulkan, DRM/KMS, and OpenGL ES on Android.

use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

/// GPU device information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GpuDevice {
    pub path: PathBuf,
    pub device_type: GpuDeviceType,
    pub driver: String,
    pub vendor_id: Option<u16>,
    pub device_id: Option<u16>,
    pub render_nodes: Vec<PathBuf>,
}

/// GPU device types
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum GpuDeviceType {
    Drm,
    Vulkan,
    OpenGLES,
    Compute,
    Video,
}

/// GPU configuration for container
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GpuConfig {
    pub enabled: bool,
    pub devices: Vec<String>, // Specific device paths to pass through
    pub all: bool,            // Pass through all available GPUs
    pub vulkan: bool,
    pub drm: bool,
    pub opengl: bool,
    pub compute: bool,
}

impl Default for GpuConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            devices: Vec::new(),
            all: true,
            vulkan: true,
            drm: true,
            opengl: true,
            compute: true,
        }
    }
}

/// Detect available GPU devices on the host
pub fn detect_gpu_devices() -> Result<Vec<GpuDevice>> {
    let mut devices = Vec::new();

    // Scan DRM devices
    let drm_path = Path::new("/dev/dri");
    if drm_path.exists() {
        for entry in fs::read_dir(drm_path)? {
            let entry = entry?;
            let path = entry.path();
            let name = entry.file_name().to_string_lossy().to_string();

            if name.starts_with("card") || name.starts_with("renderD") {
                let device_type = if name.starts_with("renderD") {
                    GpuDeviceType::Drm
                } else {
                    GpuDeviceType::Drm
                };

                devices.push(GpuDevice {
                    path: path.clone(),
                    device_type,
                    driver: detect_drm_driver(&path)?,
                    vendor_id: None,
                    device_id: None,
                    render_nodes: if name.starts_with("card") {
                        find_render_nodes(&name)?
                    } else {
                        Vec::new()
                    },
                });
            }
        }
    }

    // Scan Vulkan devices (ICD)
    let vulkan_icd_paths = [
        "/etc/vulkan/icd.d",
        "/usr/share/vulkan/icd.d",
        "/usr/local/share/vulkan/icd.d",
    ];

    for icd_path in vulkan_icd_paths {
        if let Ok(entries) = fs::read_dir(icd_path) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().map_or(false, |ext| ext == "json") {
                    if let Ok(content) = fs::read_to_string(&path) {
                        if let Ok(icd) = serde_json::from_str::<serde_json::Value>(&content) {
                            if let Some(library_path) = icd
                                .get("ICD")
                                .and_then(|v| v.get("library_path"))
                                .and_then(|v| v.as_str())
                            {
                                if Path::new(library_path).exists() {
                                    devices.push(GpuDevice {
                                        path: PathBuf::from(library_path),
                                        device_type: GpuDeviceType::Vulkan,
                                        driver: "vulkan".to_string(),
                                        vendor_id: None,
                                        device_id: None,
                                        render_nodes: Vec::new(),
                                    });
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    // Android GPU devices (ion, gralloc, etc.)
    let android_gpu_paths = ["/dev/ion", "/dev/gpu0", "/dev/mali0", "/dev/kgsl-3d0"];

    for gpu_path in android_gpu_paths {
        if Path::new(gpu_path).exists() {
            devices.push(GpuDevice {
                path: PathBuf::from(gpu_path),
                device_type: GpuDeviceType::OpenGLES,
                driver: "android".to_string(),
                vendor_id: None,
                device_id: None,
                render_nodes: Vec::new(),
            });
        }
    }

    Ok(devices)
}

/// Detect DRM driver for a device
fn detect_drm_driver(device_path: &Path) -> Result<String> {
    let sys_path = device_path
        .to_string_lossy()
        .replace("/dev/dri/", "/sys/class/dri/");
    let driver_path = Path::new(&sys_path).join("device/driver");

    if driver_path.exists() {
        if let Ok(target) = fs::read_link(&driver_path) {
            return Ok(target
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("unknown")
                .to_string());
        }
    }

    Ok("unknown".to_string())
}

/// Find render nodes for a DRM card
fn find_render_nodes(card_name: &str) -> Result<Vec<PathBuf>> {
    let mut render_nodes = Vec::new();
    let drm_path = Path::new("/dev/dri");

    if let Ok(entries) = fs::read_dir(drm_path) {
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            if name.starts_with("renderD") {
                // Check if it belongs to the same card
                let card_num: u32 = card_name.trim_start_matches("card").parse().unwrap_or(0);
                let render_num: u32 = name.trim_start_matches("renderD").parse().unwrap_or(0);
                // renderD128+ corresponds to card0, renderD129+ to card1, etc.
                if render_num >= 128 && (render_num - 128) == card_num {
                    render_nodes.push(entry.path());
                }
            }
        }
    }

    Ok(render_nodes)
}

/// Generate bind mount arguments for GPU devices
pub fn generate_gpu_bind_mounts(
    config: &GpuConfig,
    devices: &[GpuDevice],
) -> Result<Vec<(String, String)>> {
    let mut mounts = Vec::new();

    if !config.enabled {
        return Ok(mounts);
    }

    let target_devices: Vec<&GpuDevice> = if config.all {
        devices.iter().collect()
    } else {
        devices
            .iter()
            .filter(|d| {
                config
                    .devices
                    .contains(&d.path.to_string_lossy().to_string())
            })
            .collect()
    };

    for device in target_devices {
        let source = device.path.to_string_lossy().to_string();
        let target = format!("/dev{}", source.strip_prefix("/dev").unwrap_or(&source));
        mounts.push((source, target));

        // Also add render nodes for DRM devices
        if device.device_type == GpuDeviceType::Drm {
            for render_node in &device.render_nodes {
                let source = render_node.to_string_lossy().to_string();
                let target = format!("/dev{}", source.strip_prefix("/dev").unwrap_or(&source));
                mounts.push((source, target));
            }
        }
    }

    // Add common GPU-related paths
    if config.vulkan {
        let vulkan_paths = [
            "/etc/vulkan/icd.d",
            "/usr/share/vulkan/icd.d",
            "/usr/local/share/vulkan/icd.d",
        ];
        for path in vulkan_paths {
            if Path::new(path).exists() {
                mounts.push((path.to_string(), path.to_string()));
            }
        }
    }

    if config.opengl {
        let gl_paths = [
            "/dev/dri",
            "/dev/ion",
            "/dev/gpu0",
            "/dev/mali0",
            "/dev/kgsl-3d0",
        ];
        for path in gl_paths {
            if Path::new(path).exists() {
                mounts.push((path.to_string(), path.to_string()));
            }
        }
    }

    Ok(mounts)
}

/// Apply GPU configuration to container rootfs
pub fn apply_gpu_to_rootfs(rootfs: &Path, config: &GpuConfig) -> Result<()> {
    if !config.enabled {
        return Ok(());
    }

    // Create device nodes in rootfs
    let dev_path = rootfs.join("dev");
    fs::create_dir_all(&dev_path)?;

    // Create dri directory
    let dri_path = rootfs.join("dev/dri");
    fs::create_dir_all(&dri_path)?;

    // Create symlinks for render nodes
    let host_dri = Path::new("/dev/dri");
    if host_dri.exists() {
        for entry in fs::read_dir(host_dri)? {
            let entry = entry?;
            let name = entry.file_name();
            let target = dri_path.join(&name);
            if !target.exists() {
                // Create device node or symlink
                std::os::unix::fs::symlink(entry.path(), &target)?;
            }
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_gpu_config_default() {
        let config = GpuConfig::default();
        assert!(!config.enabled);
        assert!(config.all);
    }

    #[test]
    fn test_detect_gpu_devices() {
        // This may return empty on systems without GPU
        let devices = detect_gpu_devices();
        assert!(devices.is_ok());
    }
}
