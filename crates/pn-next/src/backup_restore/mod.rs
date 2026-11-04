//! Backup/Restore Module
//! Container snapshots and disaster recovery for PDN

/// Trace-only logging, off unless `PN_DEBUG` is set in the environment.
///
/// These lines were unconditional upstream and flooded real output — a single
/// `backup create` emitted ~280 kB of DEBUG for one alpine container, burying
/// the result. Enable with `PN_DEBUG=1` when diagnosing.
macro_rules! pn_trace {
    ($($arg:tt)*) => {
        if ::std::env::var_os("PN_DEBUG").is_some() {
            ::std::eprintln!($($arg)*);
        }
    };
}
use anyhow::{Context, Result};
use flate2::read::GzDecoder;
use flate2::write::GzEncoder;
use flate2::Compression;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime};
use tar::Archive;
use tar::Builder;
use tokio::fs;
use tokio::sync::{broadcast, Mutex, RwLock};
use tokio::time::interval;
use tracing::{debug, error, info, warn};
use uuid::Uuid;

/// Container info for backup/restore (extended version with all needed fields)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContainerInfo {
    pub id: String,
    pub name: String,
    pub image: String,
    pub command: Vec<String>,
    pub args: Vec<String>,
    pub env: HashMap<String, String>,
    pub labels: HashMap<String, String>,
    pub mounts: Vec<MountSpec>,
    pub network: String,
    pub ports: Vec<PortSpec>,
    pub resources: ResourceLimits,
    pub restart_policy: String,
}

/// Backup configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackupConfig {
    /// Enable backup system
    pub enabled: bool,
    /// Backup storage directory
    pub backup_dir: PathBuf,
    /// Containers directory (where container rootfs are stored)
    pub containers_dir: PathBuf,
    /// Maximum backups to keep per container
    pub max_backups_per_container: usize,
    /// Maximum total backup size (bytes)
    pub max_total_size: u64,
    /// Backup interval (seconds) for scheduled backups
    pub schedule_interval_secs: u64,
    /// Compression level (1-9)
    pub compression_level: u32,
    /// Include volumes in backup
    pub include_volumes: bool,
    /// Include container config
    pub include_config: bool,
    /// Include container logs
    pub include_logs: bool,
    /// Encryption enabled
    pub encryption_enabled: bool,
    /// Encryption key path
    pub encryption_key_path: Option<PathBuf>,
    /// Retention policy
    pub retention: RetentionPolicy,
    /// Backup hooks
    pub hooks: BackupHooks,
}

impl Default for BackupConfig {
    fn default() -> Self {
        // Determine default containers directory from environment or use standard location
        let containers_dir = std::env::var("PDN_CONTAINERS_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|_| {
                pn_trace!("DEBUG: dirs::home_dir() = {:?}", dirs::home_dir());
                dirs::home_dir()
                    .unwrap_or_else(|| PathBuf::from("/"))
                    .join(".podnext")
                    .join("containers")
            });

        // Determine default backup directory from environment or use standard location
        let backup_dir = std::env::var("PDN_BACKUP_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|_| {
                pn_trace!("DEBUG: dirs::home_dir() = {:?}", dirs::home_dir());
                dirs::home_dir()
                    .unwrap_or_else(|| PathBuf::from("/"))
                    .join(".podnext")
                    .join("backups")
            });

        pn_trace!(
            "DEBUG BackupConfig::default: backup_dir = {:?}, containers_dir = {:?}",
            backup_dir,
            containers_dir
        );

        Self {
            enabled: true,
            backup_dir,
            containers_dir,
            max_backups_per_container: 10,
            max_total_size: 50 * 1024 * 1024 * 1024, // 50GB
            schedule_interval_secs: 3600,            // 1 hour
            compression_level: 6,
            include_volumes: true,
            include_config: true,
            include_logs: false,
            encryption_enabled: false,
            encryption_key_path: None,
            retention: RetentionPolicy::default(),
            hooks: BackupHooks::default(),
        }
    }
}

/// Retention policy
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RetentionPolicy {
    /// Keep hourly backups for N hours
    pub hourly: u32,
    /// Keep daily backups for N days
    pub daily: u32,
    /// Keep weekly backups for N weeks
    pub weekly: u32,
    /// Keep monthly backups for N months
    pub monthly: u32,
    /// Keep yearly backups for N years
    pub yearly: u32,
    /// Maximum total backups
    pub max_total: usize,
}

impl Default for RetentionPolicy {
    fn default() -> Self {
        Self {
            hourly: 24,
            daily: 7,
            weekly: 4,
            monthly: 6,
            yearly: 2,
            max_total: 100,
        }
    }
}

/// Backup hooks
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackupHooks {
    /// Pre-backup command
    pub pre_backup: Option<String>,
    /// Post-backup command
    pub post_backup: Option<String>,
    /// Pre-restore command
    pub pre_restore: Option<String>,
    /// Post-restore command
    pub post_restore: Option<String>,
    /// On failure command
    pub on_failure: Option<String>,
}

impl Default for BackupHooks {
    fn default() -> Self {
        Self {
            pre_backup: None,
            post_backup: None,
            pre_restore: None,
            post_restore: None,
            on_failure: None,
        }
    }
}

/// Backup metadata
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackupMetadata {
    pub id: String,
    pub name: String,
    pub container_id: String,
    pub container_name: String,
    pub backup_type: BackupType,
    pub status: BackupStatus,
    pub created_at: SystemTime,
    pub completed_at: Option<SystemTime>,
    pub size_bytes: u64,
    pub compressed_size: u64,
    pub checksum: String,
    pub manifest: BackupManifest,
    pub labels: HashMap<String, String>,
    pub annotations: HashMap<String, String>,
    pub error_message: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BackupType {
    Full,
    Incremental,
    Differential,
    Scheduled,
    Manual,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum BackupStatus {
    Pending,
    Running,
    Completed,
    Failed,
    Cancelled,
    Expired,
}

/// Backup manifest (list of files in backup)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackupManifest {
    pub files: Vec<ManifestEntry>,
    pub total_files: usize,
    pub total_size: u64,
    pub volumes: Vec<VolumeInfo>,
    pub config: Option<ContainerConfigSnapshot>,
    pub logs: Option<LogsSnapshot>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ManifestEntry {
    pub path: String,
    pub size: u64,
    pub mode: u32,
    pub uid: u32,
    pub gid: u32,
    pub mtime: SystemTime,
    pub checksum: String,
    pub file_type: FileType,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum FileType {
    File,
    Directory,
    Symlink,
    Hardlink,
    BlockDevice,
    CharDevice,
    Fifo,
    Socket,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VolumeInfo {
    pub name: String,
    pub mount_path: String,
    pub driver: String,
    pub size: u64,
    pub backup_path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContainerConfigSnapshot {
    pub id: String,
    pub name: String,
    pub image: String,
    pub command: Vec<String>,
    pub args: Vec<String>,
    pub env: HashMap<String, String>,
    pub labels: HashMap<String, String>,
    pub mounts: Vec<MountSpec>,
    pub network: String,
    pub ports: Vec<PortSpec>,
    pub resources: ResourceLimits,
    pub restart_policy: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MountSpec {
    pub source: String,
    pub destination: String,
    pub read_only: bool,
    pub propagation: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PortSpec {
    pub container_port: u16,
    pub host_port: Option<u16>,
    pub protocol: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResourceLimits {
    pub cpu_quota: Option<u64>,
    pub memory_limit: Option<u64>,
    pub memory_swap_limit: Option<u64>,
    pub pids_limit: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogsSnapshot {
    pub path: String,
    pub size: u64,
    pub lines: usize,
}

/// Restore options
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RestoreOptions {
    /// Target container name (empty = original name)
    pub target_name: Option<String>,
    /// Target container ID (empty = new container)
    pub target_id: Option<String>,
    /// Restore volumes
    pub restore_volumes: bool,
    /// Restore config
    pub restore_config: bool,
    /// Restore logs
    pub restore_logs: bool,
    /// Overwrite existing
    pub overwrite: bool,
    /// Start container after restore
    pub auto_start: bool,
    /// New network
    pub network: Option<String>,
    /// New labels
    pub labels: HashMap<String, String>,
}

/// Backup events
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum BackupEvent {
    BackupStarted {
        backup_id: String,
        container_id: String,
        backup_type: BackupType,
    },
    BackupCompleted {
        backup_id: String,
        size: u64,
        duration_ms: u64,
    },
    BackupFailed {
        backup_id: String,
        error: String,
    },
    BackupCancelled {
        backup_id: String,
    },
    BackupExpired {
        backup_id: String,
    },
    RestoreStarted {
        backup_id: String,
        target_name: String,
    },
    RestoreCompleted {
        backup_id: String,
        target_name: String,
        duration_ms: u64,
    },
    RestoreFailed {
        backup_id: String,
        target_name: String,
        error: String,
    },
    PruningStarted,
    PruningCompleted {
        removed_count: usize,
        freed_bytes: u64,
    },
}

/// Backup manager
pub struct BackupManager {
    config: BackupConfig,
    backups: Arc<RwLock<HashMap<String, BackupMetadata>>>,
    active_backups: Arc<Mutex<HashMap<String, BackupProgress>>>,
    event_sender: broadcast::Sender<BackupEvent>,
    cancel_tokens: Arc<RwLock<HashMap<String, tokio::sync::oneshot::Sender<()>>>>,
}

#[derive(Debug, Clone)]
pub struct BackupProgress {
    pub backup_id: String,
    pub container_id: String,
    pub started_at: SystemTime,
    pub processed_bytes: u64,
    pub total_bytes: u64,
    pub current_file: String,
    pub status: BackupStatus,
}

impl BackupManager {
    pub fn new(config: BackupConfig) -> Self {
        let (event_sender, _) = broadcast::channel(1000);

        // Ensure backup directory exists
        std::fs::create_dir_all(&config.backup_dir).ok();

        Self {
            config,
            backups: Arc::new(RwLock::new(HashMap::new())),
            active_backups: Arc::new(Mutex::new(HashMap::new())),
            event_sender,
            cancel_tokens: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    pub async fn start(&self) -> Result<()> {
        if !self.config.enabled {
            info!("Backup system disabled");
            return Ok(());
        }

        info!("Starting backup manager");

        // Load existing backups
        self.load_backups().await?;

        // Start scheduled backup task
        if self.config.schedule_interval_secs > 0 {
            self.start_scheduled_backups().await;
        }

        // Start retention/pruning task
        self.start_retention_task().await;

        Ok(())
    }

    pub async fn create_backup(
        &self,
        container_id: &str,
        container_name: &str,
        backup_type: BackupType,
    ) -> Result<String> {
        let backup_id = Uuid::new_v4().to_string();
        let now = SystemTime::now();

        // Create backup metadata
        let mut metadata = BackupMetadata {
            id: backup_id.clone(),
            name: format!(
                "{}-{}",
                container_name,
                now.duration_since(SystemTime::UNIX_EPOCH)
                    .unwrap()
                    .as_secs()
            ),
            container_id: container_id.to_string(),
            container_name: container_name.to_string(),
            backup_type,
            status: BackupStatus::Pending,
            created_at: now,
            completed_at: None,
            size_bytes: 0,
            compressed_size: 0,
            checksum: String::new(),
            manifest: BackupManifest {
                files: Vec::new(),
                total_files: 0,
                total_size: 0,
                volumes: Vec::new(),
                config: None,
                logs: None,
            },
            labels: HashMap::new(),
            annotations: HashMap::new(),
            error_message: None,
        };

        // Store initial metadata
        {
            let mut backups = self.backups.write().await;
            backups.insert(backup_id.clone(), metadata.clone());
        }

        // Create cancel token
        let (cancel_tx, cancel_rx) = tokio::sync::oneshot::channel();
        {
            let mut tokens = self.cancel_tokens.write().await;
            tokens.insert(backup_id.clone(), cancel_tx);
        }

        // Emit start event
        self.emit_event(BackupEvent::BackupStarted {
            backup_id: backup_id.clone(),
            container_id: container_id.to_string(),
            backup_type,
        })
        .await;

        // Run backup synchronously
        pn_trace!("DEBUG create_backup: running backup synchronously");
        match self
            .run_backup(
                backup_id.clone(),
                container_id,
                container_name,
                backup_type,
                cancel_rx,
            )
            .await
        {
            Ok(_) => {
                pn_trace!("DEBUG create_backup: backup completed successfully");
            }
            Err(e) => {
                pn_trace!("DEBUG create_backup: backup failed with error: {:?}", e);
                error!("Backup failed: {}", e);
            }
        }
        pn_trace!("DEBUG create_backup: backup completed");

        Ok(backup_id)
    }

    async fn run_backup(
        &self,
        backup_id: String,
        container_id: &str,
        container_name: &str,
        backup_type: BackupType,
        mut cancel_rx: tokio::sync::oneshot::Receiver<()>,
    ) -> Result<()> {
        pn_trace!(
            "DEBUG run_backup: START backup_id={}, container_id={}",
            backup_id,
            container_id
        );
        let start_time = Instant::now();

        // Update status to running
        self.update_backup_status(&backup_id, BackupStatus::Running)
            .await;
        pn_trace!("DEBUG run_backup: status updated to Running");

        // Get container info (would come from container runtime)
        let container_info = self.get_container_info(container_id).await?;
        pn_trace!("DEBUG run_backup: got container_info");

        // Run pre-backup hook
        if let Some(hook) = &self.config.hooks.pre_backup {
            self.run_hook(hook).await?;
        }
        pn_trace!("DEBUG run_backup: pre-backup hook done");

        // Create backup directory
        let backup_dir = self.config.backup_dir.join(&backup_id);
        pn_trace!("DEBUG run_backup: creating backup_dir = {:?}", backup_dir);
        fs::create_dir_all(&backup_dir).await?;
        pn_trace!("DEBUG run_backup: backup_dir created");

        let mut manifest = BackupManifest {
            files: Vec::new(),
            total_files: 0,
            total_size: 0,
            volumes: Vec::new(),
            config: None,
            logs: None,
        };

        let mut total_size = 0u64;
        let mut processed = 0u64;

        // Backup container rootfs
        if self.config.include_volumes {
            let rootfs_path = self.config.containers_dir.join(container_id).join("rootfs");
            pn_trace!(
                "DEBUG run_backup: rootfs_path = {:?}, exists = {}",
                rootfs_path,
                rootfs_path.exists()
            );
            if rootfs_path.exists() {
                let (files, size) = self
                    .backup_directory(&rootfs_path, &backup_dir.join("rootfs"), &mut cancel_rx)
                    .await?;
                pn_trace!(
                    "DEBUG run_backup: backup_directory returned {} files, size={}",
                    files.len(),
                    size
                );
                manifest.files.extend(files);
                total_size += size;
            } else {
                pn_trace!("DEBUG run_backup: rootfs_path does not exist!");
            }
        }

        // Backup container config
        if self.config.include_config {
            let config = self.backup_container_config(&container_info).await?;
            manifest.config = Some(config);
        }

        // Backup logs
        if self.config.include_logs {
            let logs = self.backup_logs(container_id).await?;
            manifest.logs = Some(logs);
        }

        // Update manifest totals
        manifest.total_files = manifest.files.len();
        manifest.total_size = total_size;

        // Write manifest
        let manifest_path = backup_dir.join("manifest.json");
        fs::write(&manifest_path, serde_json::to_vec_pretty(&manifest)?).await?;

        // Record the container identity next to the archive, in the backup root.
        //
        // NOT inside `backup_dir`: that directory is the staging area and is
        // deleted right after the archive is written, which is why the manifest
        // never survived on disk and `backup list` had nothing to read.
        let sidecar = serde_json::json!({
            "container_id": container_id,
            "container_name": container_name,
            "backup_type": format!("{:?}", backup_type),
            "size_bytes": total_size,
            "total_files": manifest.total_files,
        });
        fs::write(
            self.config
                .backup_dir
                .join(format!("{}.meta.json", backup_id)),
            serde_json::to_vec_pretty(&sidecar)?,
        )
        .await?;

        // Create compressed archive
        let archive_path = self.config.backup_dir.join(format!("{}.tar.gz", backup_id));
        let compressed_size = self
            .create_archive(&backup_dir, &archive_path, &mut cancel_rx)
            .await?;

        // Calculate checksum
        let checksum = self.calculate_checksum(&archive_path).await?;

        // Clean up temp directory
        fs::remove_dir_all(&backup_dir).await.ok();

        // Update metadata
        let completed_at = SystemTime::now();
        let metadata = BackupMetadata {
            id: backup_id.clone(),
            name: format!(
                "{}-{}",
                container_name,
                completed_at
                    .duration_since(SystemTime::UNIX_EPOCH)
                    .unwrap()
                    .as_secs()
            ),
            container_id: container_id.to_string(),
            container_name: container_name.to_string(),
            backup_type,
            status: BackupStatus::Completed,
            created_at: SystemTime::now() - start_time.elapsed(),
            completed_at: Some(completed_at),
            size_bytes: total_size,
            compressed_size,
            checksum,
            manifest,
            labels: HashMap::new(),
            annotations: HashMap::new(),
            error_message: None,
        };

        self.update_backup_metadata(&backup_id, metadata).await;

        // Run post-backup hook
        if let Some(hook) = &self.config.hooks.post_backup {
            self.run_hook(hook).await.ok();
        }

        // Emit completion event
        self.emit_event(BackupEvent::BackupCompleted {
            backup_id: backup_id.clone(),
            size: compressed_size,
            duration_ms: start_time.elapsed().as_millis() as u64,
        })
        .await;

        Ok(())
    }

    async fn get_container_info(&self, container_id: &str) -> Result<ContainerInfo> {
        pn_trace!("DEBUG get_container_info: container_id={}", container_id);

        // Try to get actual container info from proot-distro-core
        // For now, construct basic info using the container_id
        Ok(ContainerInfo {
            id: container_id.to_string(),
            name: container_id.to_string(),
            image: "alpine".to_string(),
            command: Vec::new(),
            args: Vec::new(),
            env: HashMap::new(),
            labels: HashMap::new(),
            mounts: Vec::new(),
            network: "".to_string(),
            ports: Vec::new(),
            resources: ResourceLimits::default(),
            restart_policy: "".to_string(),
        })
    }

    async fn backup_directory(
        &self,
        src: &Path,
        dst: &Path,
        cancel_rx: &mut tokio::sync::oneshot::Receiver<()>,
    ) -> Result<(Vec<ManifestEntry>, u64)> {
        pn_trace!("DEBUG backup_directory: src={:?}, dst={:?}", src, dst);
        fs::create_dir_all(dst).await?;
        pn_trace!("DEBUG backup_directory: dst created");

        let mut manifest_entries = Vec::new();
        let mut total_size = 0u64;

        let mut stack = vec![src.to_path_buf()];

        while let Some(current) = stack.pop() {
            // Check for cancellation
            if cancel_rx.try_recv().is_ok() {
                return Err(anyhow::anyhow!("Backup cancelled"));
            }

            pn_trace!("DEBUG backup_directory: reading dir {:?}", current);
            let mut dir_entries = fs::read_dir(&current).await?;
            pn_trace!("DEBUG backup_directory: dir entries obtained");
            while let Some(entry) = dir_entries.next_entry().await? {
                let path = entry.path();
                let rel_path = path.strip_prefix(src).unwrap().to_path_buf();
                let dst_path = dst.join(&rel_path);
                pn_trace!("DEBUG backup_directory: processing entry {:?}", path);

                let metadata = entry.metadata().await?;
                pn_trace!(
                    "DEBUG backup_directory: got metadata, is_dir={}, is_file={}, is_symlink={}",
                    metadata.is_dir(),
                    metadata.is_file(),
                    metadata.is_symlink()
                );

                let entry_info = ManifestEntry {
                    path: rel_path.to_string_lossy().to_string(),
                    size: metadata.len(),
                    mode: metadata.permissions().mode(),
                    uid: metadata.uid(),
                    gid: metadata.gid(),
                    mtime: metadata.modified().unwrap_or(SystemTime::now()),
                    checksum: if metadata.is_file() {
                        self.calculate_file_checksum(&path).await?
                    } else {
                        String::new()
                    },
                    file_type: if metadata.is_dir() {
                        FileType::Directory
                    } else if metadata.is_symlink() {
                        FileType::Symlink
                    } else {
                        FileType::File
                    },
                };

                manifest_entries.push(entry_info);
                total_size += metadata.len();

                if metadata.is_dir() {
                    let file_name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
                    pn_trace!("DEBUG backup_directory: dir file_name={:?}", file_name);
                    if matches!(file_name, "proc" | "sys" | "dev") {
                        pn_trace!("DEBUG backup_directory: skipping virtual fs {:?}", path);
                    } else {
                        pn_trace!("DEBUG backup_directory: creating dir {:?}", dst_path);
                        fs::create_dir_all(&dst_path).await?;
                        stack.push(path);
                    }
                } else if metadata.is_symlink() {
                    let target = fs::read_link(&path).await?;
                    std::os::unix::fs::symlink(&target, &dst_path)?;
                } else if metadata.is_file() {
                    pn_trace!(
                        "DEBUG backup_directory: copying file {:?} -> {:?}",
                        path,
                        dst_path
                    );
                    let copy_result = fs::copy(&path, &dst_path).await;
                    pn_trace!("DEBUG backup_directory: copy result: {:?}", copy_result);
                    copy_result?;
                    pn_trace!("DEBUG backup_directory: copied file successfully");
                } else {
                    pn_trace!(
                        "DEBUG backup_directory: skipping special file {:?} (type: {:?})",
                        path,
                        metadata.file_type()
                    );
                }
            }
        }
        pn_trace!("DEBUG backup_directory: completed successfully");

        Ok((manifest_entries, total_size))
    }

    async fn backup_container_config(
        &self,
        _info: &ContainerInfo,
    ) -> Result<ContainerConfigSnapshot> {
        Ok(ContainerConfigSnapshot::default())
    }

    async fn backup_logs(&self, _container_id: &str) -> Result<LogsSnapshot> {
        Ok(LogsSnapshot {
            path: String::new(),
            size: 0,
            lines: 0,
        })
    }

    async fn create_archive(
        &self,
        src_dir: &Path,
        archive_path: &Path,
        cancel_rx: &mut tokio::sync::oneshot::Receiver<()>,
    ) -> Result<u64> {
        pn_trace!(
            "DEBUG create_archive: src_dir={:?}, archive_path={:?}",
            src_dir,
            archive_path
        );
        let src_dir = src_dir.to_path_buf();
        let archive_path = archive_path.to_path_buf();
        let compression_level = self.config.compression_level;

        // Use spawn_blocking for synchronous tar.gz creation with manual walk to handle errors
        pn_trace!("DEBUG create_archive: spawning blocking task");
        let size = tokio::task::spawn_blocking(move || -> Result<u64> {
            pn_trace!(
                "DEBUG create_archive: blocking task started, src_dir={:?}",
                src_dir
            );
            use flate2::write::GzEncoder;
            use flate2::Compression;
            use std::fs::File;
            use std::io::Write;
            use tar::Builder;
            use walkdir::WalkDir;

            pn_trace!("DEBUG create_archive: creating file {:?}", archive_path);
            let file = File::create(&archive_path)?;
            pn_trace!("DEBUG create_archive: file created");
            let encoder = GzEncoder::new(file, Compression::new(compression_level));
            let mut archive = Builder::new(encoder);
            // Store symlinks as symlinks. With the default follow_symlinks(true)
            // tar resolves each link first, so the absolute busybox links
            // (bin/ls -> /bin/busybox) dangled inside the staging dir and were
            // silently dropped: ~300 entries missing per alpine backup plus a
            // flood of "No such file or directory when getting metadata".
            archive.follow_symlinks(false);

            pn_trace!("DEBUG create_archive: walking dir {:?}", src_dir);
            for entry in WalkDir::new(&src_dir).into_iter().filter_map(|e| e.ok()) {
                let path = entry.path();
                if path == src_dir {
                    continue;
                }
                let rel_path = path.strip_prefix(&src_dir).unwrap();

                if entry.file_type().is_dir() {
                    // For directories, we don't need to explicitly add them - tar will create them when adding files
                    continue;
                } else if entry.file_type().is_file() {
                    pn_trace!("DEBUG create_archive: adding file {:?}", rel_path);
                    if let Err(e) = archive.append_path_with_name(path, rel_path) {
                        pn_trace!(
                            "DEBUG create_archive: failed to add file {:?}: {}",
                            rel_path,
                            e
                        );
                        // Skip files that can't be read (sockets, device files, etc.)
                        continue;
                    }
                } else if entry.file_type().is_symlink() {
                    pn_trace!("DEBUG create_archive: adding symlink {:?}", rel_path);
                    if let Err(e) = archive.append_path_with_name(path, rel_path) {
                        pn_trace!(
                            "DEBUG create_archive: failed to add symlink {:?}: {}",
                            rel_path,
                            e
                        );
                        continue;
                    }
                }
            }
            pn_trace!("DEBUG create_archive: all files processed");
            let encoded = archive.into_inner()?;
            pn_trace!("DEBUG create_archive: archive finished");
            encoded.finish()?;
            pn_trace!("DEBUG create_archive: encoder finished");

            let metadata = std::fs::metadata(&archive_path)?;
            pn_trace!(
                "DEBUG create_archive: archive metadata obtained, size={}",
                metadata.len()
            );
            Ok(metadata.len())
        })
        .await??;

        pn_trace!(
            "DEBUG create_archive: completed successfully, size={}",
            size
        );
        Ok(size)
    }

    async fn calculate_checksum(&self, path: &Path) -> Result<String> {
        use sha2::{Digest, Sha256};
        let content = fs::read(path).await?;
        let mut hasher = Sha256::new();
        hasher.update(&content);
        Ok(format!("{:x}", hasher.finalize()))
    }

    async fn calculate_file_checksum(&self, path: &Path) -> Result<String> {
        use sha2::{Digest, Sha256};
        let content = fs::read(path).await?;
        let mut hasher = Sha256::new();
        hasher.update(&content);
        Ok(format!("{:x}", hasher.finalize()))
    }

    async fn run_hook(&self, hook: &str) -> Result<()> {
        let output = tokio::process::Command::new("sh")
            .arg("-c")
            .arg(hook)
            .output()
            .await?;

        if !output.status.success() {
            warn!("Hook failed: {}", String::from_utf8_lossy(&output.stderr));
        }

        Ok(())
    }

    pub async fn restore_backup(&self, backup_id: &str, options: RestoreOptions) -> Result<String> {
        let backup = {
            let backups = self.backups.read().await;
            backups.get(backup_id).cloned()
        }
        .context("Backup not found")?;

        if backup.status != BackupStatus::Completed {
            return Err(anyhow::anyhow!("Backup not completed"));
        }

        let target_name = options
            .target_name
            .clone()
            .unwrap_or_else(|| format!("{}-restored", backup.container_name));

        self.emit_event(BackupEvent::RestoreStarted {
            backup_id: backup_id.to_string(),
            target_name: target_name.clone(),
        })
        .await;

        // Run pre-restore hook
        if let Some(hook) = &self.config.hooks.pre_restore {
            self.run_hook(hook).await.ok();
        }

        // Extract archive
        let archive_path = self.config.backup_dir.join(format!("{}.tar.gz", backup_id));
        let extract_dir = self
            .config
            .backup_dir
            .join(format!("restore-{}", backup_id));

        // Extract using tar (spawn_blocking for sync I/O)
        let archive_path_clone = archive_path.clone();
        let extract_dir_clone = extract_dir.clone();
        tokio::task::spawn_blocking(move || -> Result<()> {
            use flate2::read::GzDecoder;
            use std::fs::File;

            let file = File::open(&archive_path_clone)?;
            let decoder = GzDecoder::new(file);
            // Shares the Android-safe extractor: `Archive::unpack` refuses to
            // create hard links here, which is why restoring a Debian backup
            // would fail the same way installing one used to.
            proot_distro_core::untar::extract(decoder, &extract_dir_clone)
        })
        .await??;

        // Restore container from extracted data
        let container_id = self
            .restore_container(&extract_dir, &backup, &options)
            .await?;

        // Run post-restore hook
        if let Some(hook) = &self.config.hooks.post_restore {
            self.run_hook(hook).await.ok();
        }

        // Cleanup
        fs::remove_dir_all(&extract_dir).await.ok();

        self.emit_event(BackupEvent::RestoreCompleted {
            backup_id: backup_id.to_string(),
            target_name: target_name.clone(),
            duration_ms: 0,
        })
        .await;

        Ok(container_id)
    }

    async fn restore_container(
        &self,
        extract_dir: &Path,
        backup: &BackupMetadata,
        options: &RestoreOptions,
    ) -> Result<String> {
        let target_name = options
            .target_name
            .clone()
            .unwrap_or_else(|| format!("{}-restored", backup.container_name));

        let dest = self.config.containers_dir.join(&target_name);

        if dest.exists() {
            if !options.overwrite {
                anyhow::bail!(
                    "Container '{}' already exists; pass --overwrite to replace it",
                    target_name
                );
            }
            fs::remove_dir_all(&dest).await?;
        }

        // The archive keeps the tree under `rootfs/`, so the staged directory is
        // not itself the rootfs. Older archives that were written flat are still
        // accepted.
        let staged = if extract_dir.join("rootfs").is_dir() {
            extract_dir.join("rootfs")
        } else {
            extract_dir.to_path_buf()
        };

        fs::create_dir_all(&dest).await?;
        let rootfs = dest.join("rootfs");
        if fs::rename(&staged, &rootfs).await.is_err() {
            copy_tree(&staged, &rootfs).await?;
        }

        // `metadata.json` is what makes ContainerManager list the container at
        // all — without it the restored tree is invisible to every other
        // command, which is what "restore returns a random UUID" really meant.
        let metadata = serde_json::json!({
            "name": target_name,
            "image": backup.container_name,
            "tag": "restored",
            "rootfs": rootfs.to_string_lossy(),
            "created_at": chrono::Utc::now().to_rfc3339(),
            "status": "Stopped",
        });
        fs::write(
            dest.join("metadata.json"),
            serde_json::to_string_pretty(&metadata)?,
        )
        .await?;

        // Same fake /proc stubs a fresh install gets, regenerated for this
        // device's CPU set.
        proot_distro_core::sysdata::ensure(&dest).ok();

        Ok(target_name)
    }

    pub async fn list_backups(&self, container_id: Option<&str>) -> Vec<BackupMetadata> {
        let backups = self.backups.read().await;
        backups
            .values()
            .filter(|b| container_id.map_or(true, |id| b.container_id == id))
            .cloned()
            .collect()
    }

    pub async fn get_backup(&self, backup_id: &str) -> Option<BackupMetadata> {
        self.backups.read().await.get(backup_id).cloned()
    }

    pub async fn delete_backup(&self, backup_id: &str) -> Result<()> {
        let backups = self.backups.read().await;
        let exists = backups.contains_key(backup_id);
        // Release the read guard before touching anything else: `backups` is a
        // tokio RwLock (not reentrant), so taking write() below while still
        // holding read() would wait on ourselves forever.
        drop(backups);
        if exists {
            let archive_path = self.config.backup_dir.join(format!("{}.tar.gz", backup_id));
            if archive_path.exists() {
                fs::remove_file(archive_path).await?;
            }
            // Remove the sidecar too, or the next `backup list` reports an entry
            // whose archive is gone.
            let sidecar_path = self
                .config
                .backup_dir
                .join(format!("{}.meta.json", backup_id));
            if sidecar_path.exists() {
                fs::remove_file(sidecar_path).await.ok();
            }
            let staging_dir = self.config.backup_dir.join(backup_id);
            if staging_dir.is_dir() {
                fs::remove_dir_all(staging_dir).await.ok();
            }
        }

        self.backups.write().await.remove(backup_id);
        Ok(())
    }

    pub async fn cancel_backup(&self, backup_id: &str) -> Result<()> {
        let mut tokens = self.cancel_tokens.write().await;
        if let Some(tx) = tokens.remove(backup_id) {
            tx.send(()).ok();
        }

        self.update_backup_status(backup_id, BackupStatus::Cancelled)
            .await;
        Ok(())
    }

    pub async fn get_backup_progress(&self, backup_id: &str) -> Option<BackupProgress> {
        self.active_backups.lock().await.get(backup_id).cloned()
    }

    async fn start_scheduled_backups(&self) {
        let config = self.config.clone();
        let manager = self.clone();

        tokio::spawn(async move {
            let mut interval = interval(Duration::from_secs(config.schedule_interval_secs));

            loop {
                interval.tick().await;

                // Get all containers and create scheduled backups
                // In real implementation, would iterate over running containers
                info!("Scheduled backup tick");
            }
        });
    }

    async fn start_retention_task(&self) {
        let config = self.config.clone();
        let manager = self.clone();

        tokio::spawn(async move {
            let mut interval = interval(Duration::from_secs(3600)); // Every hour

            loop {
                interval.tick().await;
                manager.apply_retention_policy().await.ok();
            }
        });
    }

    async fn apply_retention_policy(&self) -> Result<()> {
        let mut removed = 0;
        let mut freed = 0u64;

        let backups = self.backups.read().await;
        let mut to_remove = Vec::new();

        // Group by container
        let mut by_container: HashMap<String, Vec<&BackupMetadata>> = HashMap::new();
        for backup in backups.values() {
            by_container
                .entry(backup.container_id.clone())
                .or_default()
                .push(backup);
        }

        // Apply retention per container
        for (container_id, mut backups) in by_container {
            backups.sort_by(|a, b| b.created_at.cmp(&a.created_at));

            let mut keep = Vec::new();
            let mut hourly = 0;
            let mut daily = 0;
            let mut weekly = 0;
            let mut monthly = 0;
            let mut yearly = 0;

            for backup in backups {
                let age = SystemTime::now()
                    .duration_since(backup.created_at)
                    .unwrap_or_default();

                let keep_backup = match age {
                    d if d < Duration::from_secs(3600) => true, // Last hour
                    d if d < Duration::from_secs(86400)
                        && hourly < self.config.retention.hourly =>
                    {
                        hourly += 1;
                        true
                    }
                    d if d < Duration::from_secs(86400 * 7)
                        && daily < self.config.retention.daily =>
                    {
                        daily += 1;
                        true
                    }
                    d if d < Duration::from_secs(86400 * 30)
                        && weekly < self.config.retention.weekly =>
                    {
                        weekly += 1;
                        true
                    }
                    d if d < Duration::from_secs(86400 * 365)
                        && monthly < self.config.retention.monthly =>
                    {
                        monthly += 1;
                        true
                    }
                    d if yearly < self.config.retention.yearly => {
                        yearly += 1;
                        true
                    }
                    _ => false,
                };

                if keep_backup && keep.len() < self.config.retention.max_total {
                    keep.push(backup.id.clone());
                } else {
                    to_remove.push(backup.id.clone());
                }
            }
        }

        drop(backups);

        // Remove expired backups
        for id in to_remove {
            if let Err(e) = self.delete_backup(&id).await {
                error!("Failed to delete backup {}: {}", id, e);
            } else {
                removed += 1;
                // Would add size to freed
            }
        }

        if removed > 0 {
            self.emit_event(BackupEvent::PruningCompleted {
                removed_count: removed,
                freed_bytes: freed,
            })
            .await;
        }

        Ok(())
    }

    pub async fn load_backups(&self) -> Result<()> {
        // Ensure backup directory exists
        std::fs::create_dir_all(&self.config.backup_dir).ok();

        let mut entries = match fs::read_dir(&self.config.backup_dir).await {
            Ok(entries) => entries,
            Err(_) => {
                // Directory doesn't exist or can't be read, return empty
                return Ok(());
            }
        };

        // Rebuild the in-memory index from what is on disk.
        //
        // Two records exist per backup:
        //   * `<id>.tar.gz`     — the archive itself (the durable artefact)
        //   * `<id>.meta.json`  — container identity and sizes, written beside it
        // plus, from older runs, leftover staging dirs `<id>/manifest.json`.
        //
        // Scanning only for directories was wrong: the staging directory is
        // deleted as soon as the archive is written, so almost nothing was ever
        // found. Archives are the source of truth; metadata is enrichment.
        while let Some(entry) = entries.next_entry().await? {
            let path = entry.path();
            let file_name = path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            if file_name.is_empty() {
                continue;
            }

            let id = if let Some(stripped) = file_name.strip_suffix(".tar.gz") {
                stripped.to_string()
            } else if file_name.ends_with(".meta.json") {
                // Visited together with its archive; nothing to do here.
                continue;
            } else if path.is_dir() {
                file_name.clone()
            } else {
                continue;
            };

            let archive_path = self.config.backup_dir.join(format!("{}.tar.gz", id));
            let Ok(archive_meta) = std::fs::metadata(&archive_path) else {
                continue;
            };
            if !archive_meta.is_file() {
                continue;
            }
            let compressed_size = archive_meta.len();
            let created_at = archive_meta
                .modified()
                .unwrap_or_else(|_| SystemTime::now());

            let sidecar: Option<serde_json::Value> =
                std::fs::read_to_string(self.config.backup_dir.join(format!("{}.meta.json", id)))
                    .ok()
                    .and_then(|s| serde_json::from_str(&s).ok());

            let manifest: BackupManifest =
                std::fs::read_to_string(self.config.backup_dir.join(&id).join("manifest.json"))
                    .ok()
                    .and_then(|s| serde_json::from_str(&s).ok())
                    .unwrap_or(BackupManifest {
                        files: Vec::new(),
                        total_files: 0,
                        total_size: 0,
                        volumes: Vec::new(),
                        config: None,
                        logs: None,
                    });

            let container_id = sidecar
                .as_ref()
                .and_then(|v| v.get("container_name").or_else(|| v.get("container_id")))
                .and_then(|v| v.as_str())
                .filter(|s| !s.is_empty())
                .map(|s| s.to_string())
                .or_else(|| {
                    manifest
                        .config
                        .as_ref()
                        .map(|c| c.name.clone())
                        .filter(|n| !n.is_empty())
                })
                .unwrap_or_else(|| id.clone());

            let size_bytes = sidecar
                .as_ref()
                .and_then(|v| v.get("size_bytes"))
                .and_then(|v| v.as_u64())
                .filter(|n| *n > 0)
                .unwrap_or(manifest.total_size);

            let metadata = BackupMetadata {
                id: id.clone(),
                name: id.clone(),
                container_id: container_id.clone(),
                container_name: container_id,
                backup_type: BackupType::Full,
                status: BackupStatus::Completed,
                created_at,
                completed_at: Some(created_at),
                size_bytes,
                compressed_size,
                checksum: String::new(),
                manifest,
                labels: HashMap::new(),
                annotations: HashMap::new(),
                error_message: None,
            };

            self.backups.write().await.insert(id, metadata);
        }

        Ok(())
    }

    async fn update_backup_status(&self, backup_id: &str, status: BackupStatus) {
        let mut backups = self.backups.write().await;
        if let Some(backup) = backups.get_mut(backup_id) {
            backup.status = status;
        }
    }

    async fn update_backup_metadata(&self, backup_id: &str, metadata: BackupMetadata) {
        let mut backups = self.backups.write().await;
        backups.insert(backup_id.to_string(), metadata);
    }

    async fn emit_event(&self, event: BackupEvent) {
        self.event_sender.send(event).ok();
    }

    pub fn subscribe_events(&self) -> broadcast::Receiver<BackupEvent> {
        self.event_sender.subscribe()
    }
}

impl Clone for BackupManager {
    fn clone(&self) -> Self {
        Self {
            config: self.config.clone(),
            backups: self.backups.clone(),
            active_backups: self.active_backups.clone(),
            event_sender: self.event_sender.clone(),
            cancel_tokens: self.cancel_tokens.clone(),
        }
    }
}

impl Default for ContainerConfigSnapshot {
    fn default() -> Self {
        Self {
            id: String::new(),
            name: String::new(),
            image: String::new(),
            command: Vec::new(),
            args: Vec::new(),
            env: HashMap::new(),
            labels: HashMap::new(),
            mounts: Vec::new(),
            network: String::new(),
            ports: Vec::new(),
            resources: ResourceLimits::default(),
            restart_policy: String::new(),
        }
    }
}

impl Default for ResourceLimits {
    fn default() -> Self {
        Self {
            cpu_quota: None,
            memory_limit: None,
            memory_swap_limit: None,
            pids_limit: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test(flavor = "multi_thread")]
    async fn test_backup_config() {
        let config = BackupConfig::default();
        assert!(config.enabled);
        assert_eq!(config.compression_level, 6);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_retention_policy() {
        let policy = RetentionPolicy::default();
        assert_eq!(policy.hourly, 24);
        assert_eq!(policy.daily, 7);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_delete_backup_does_not_deadlock() {
        let mut config = BackupConfig::default();
        config.backup_dir =
            std::env::temp_dir().join(format!("pn-delete-test-{}", Uuid::new_v4()));
        let manager = BackupManager::new(config);

        let id = "deadlock-regression";
        manager
            .update_backup_metadata(
                id,
                BackupMetadata {
                    id: id.to_string(),
                    name: id.to_string(),
                    container_id: "c1".to_string(),
                    container_name: "c1".to_string(),
                    backup_type: BackupType::Full,
                    status: BackupStatus::Completed,
                    created_at: SystemTime::now(),
                    completed_at: Some(SystemTime::now()),
                    size_bytes: 0,
                    compressed_size: 0,
                    checksum: String::new(),
                    manifest: BackupManifest {
                        files: Vec::new(),
                        total_files: 0,
                        total_size: 0,
                        volumes: Vec::new(),
                        config: None,
                        logs: None,
                    },
                    labels: HashMap::new(),
                    annotations: HashMap::new(),
                    error_message: None,
                },
            )
            .await;

        // Issue #12: delete_backup used to take the write lock while still
        // holding its own read guard, so it blocked forever. A bounded timeout
        // turns that hang into a test failure instead of wedging the suite.
        let result = tokio::time::timeout(Duration::from_secs(5), manager.delete_backup(id)).await;
        std::fs::remove_dir_all(&manager.config.backup_dir).ok();

        result.expect("delete_backup deadlocked: it waited on its own read guard");
        assert!(manager.get_backup(id).await.is_none());
    }
}

/// Recursive copy, used when `rename` cannot move the staged tree (a different
/// filesystem, for instance). Symlinks are recreated as symlinks so a restored
/// rootfs keeps its busybox applet links pointing where they should.
async fn copy_tree(from: &Path, to: &Path) -> Result<()> {
    let mut stack = vec![(from.to_path_buf(), to.to_path_buf())];

    while let Some((src, dst)) = stack.pop() {
        fs::create_dir_all(&dst).await?;
        let mut entries = fs::read_dir(&src).await?;

        while let Some(entry) = entries.next_entry().await? {
            let path = entry.path();
            let target = dst.join(entry.file_name());
            let file_type = entry.file_type().await?;

            if file_type.is_dir() {
                stack.push((path, target));
            } else if file_type.is_symlink() {
                let link = fs::read_link(&path).await?;
                let _ = fs::remove_file(&target).await;
                tokio::fs::symlink(&link, &target).await?;
            } else {
                fs::copy(&path, &target).await?;
                if let Ok(meta) = fs::metadata(&path).await {
                    let _ = fs::set_permissions(&target, meta.permissions()).await;
                }
            }
        }
    }

    Ok(())
}
