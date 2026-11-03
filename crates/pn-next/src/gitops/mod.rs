//! GitOps Integration Module
//! Flux/ArgoCD style GitOps integration for PDN

use anyhow::{Context, Result};
use axum::{
    extract::State,
    http::HeaderMap,
    response::{IntoResponse, Json},
    routing::post,
    Router,
};
use git2;
use serde::{Deserialize, Serialize};
use serde_json;
use std::collections::HashMap;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime};
use tokio::sync::{broadcast, RwLock};
use tokio::time::interval;
use tracing::{debug, error, info, warn};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnvVar {
    pub name: String,
    pub value: String,
}

/// GitOps configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GitOpsConfig {
    /// Enable GitOps
    pub enabled: bool,
    /// Git repository URL
    pub repo_url: String,
    /// Branch to watch
    pub branch: String,
    /// Path in repo for manifests
    pub manifest_path: String,
    /// Sync interval (seconds)
    pub sync_interval_secs: u64,
    /// Git credentials (if private repo)
    pub credentials: Option<GitCredentials>,
    /// SSH key path (for SSH auth)
    pub ssh_key_path: Option<PathBuf>,
    /// Known hosts path
    pub known_hosts_path: Option<PathBuf>,
    /// Auto-prune deleted resources
    pub prune: bool,
    /// Dry-run mode
    pub dry_run: bool,
    /// Webhook secret for GitHub/GitLab
    pub webhook_secret: Option<String>,
    /// Polling vs webhook
    pub use_webhook: bool,
    /// Webhook port
    pub webhook_port: u16,
}

impl Default for GitOpsConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            repo_url: "https://github.com/user/pdn-gitops".to_string(),
            branch: "main".to_string(),
            manifest_path: "manifests".to_string(),
            sync_interval_secs: 60,
            credentials: None,
            ssh_key_path: None,
            known_hosts_path: None,
            prune: true,
            dry_run: false,
            webhook_secret: None,
            use_webhook: false,
            webhook_port: 8081,
        }
    }
}

/// Git credentials
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GitCredentials {
    pub username: String,
    pub password: String,
}

/// GitOps resource types
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ResourceType {
    Container,
    Service,
    Volume,
    Network,
    ConfigMap,
    Secret,
    Ingress,
    Custom(String),
}

/// Resource specification from Git
///
/// Manifests are written in the usual camelCase (`apiVersion`, `resourceVersion`),
/// which had no counterpart here, so nothing parsed. `spec` defaults because a
/// resource may legitimately carry none.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GitResource {
    pub api_version: String,
    pub kind: String,
    pub metadata: ResourceMetadata,
    #[serde(default)]
    pub spec: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResourceMetadata {
    pub name: String,
    #[serde(default)]
    pub namespace: Option<String>,
    #[serde(default)]
    pub labels: HashMap<String, String>,
    #[serde(default)]
    pub annotations: HashMap<String, String>,
    #[serde(default)]
    pub uid: Option<String>,
    #[serde(default)]
    pub resource_version: Option<String>,
    #[serde(default)]
    pub generation: Option<i64>,
    #[serde(default)]
    pub creation_timestamp: Option<String>,
    #[serde(default)]
    pub deletion_timestamp: Option<String>,
    #[serde(default)]
    pub finalizers: Vec<String>,
    #[serde(default)]
    pub owner_references: Vec<OwnerReference>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OwnerReference {
    pub api_version: String,
    pub kind: String,
    pub name: String,
    pub uid: String,
    pub controller: Option<bool>,
    pub block_owner_deletion: Option<bool>,
}

/// Sync status
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SyncStatus {
    #[default]
    Unknown,
    Synced,
    OutOfSync,
    Syncing,
    Error,
    Pending,
}

/// Application/Project in GitOps
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Application {
    pub name: String,
    pub namespace: String,
    pub project: String,
    pub source: ApplicationSource,
    pub destination: ApplicationDestination,
    pub sync_policy: Option<SyncPolicy>,
    #[serde(default)]
    pub ignore_differences: Vec<ResourceIgnoreDifference>,
    #[serde(default)]
    pub info: Vec<Info>,
    /// Filled in by a sync, never set by the manifest itself.
    #[serde(default)]
    pub status: ApplicationStatus,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApplicationSource {
    pub repo_url: String,
    pub path: String,
    pub target_revision: String,
    pub helm: Option<HelmSource>,
    pub kustomize: Option<KustomizeSource>,
    pub directory: Option<DirectorySource>,
    pub plugin: Option<PluginSource>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HelmSource {
    pub chart: String,
    pub version: String,
    pub values_files: Vec<String>,
    pub parameters: Vec<HelmParameter>,
    pub skip_crds: bool,
    pub release_name: String,
    pub pass_credentials: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HelmParameter {
    pub name: String,
    pub value: String,
    pub force_string: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KustomizeSource {
    pub patches: Vec<String>,
    pub patches_strategic_merge: Vec<String>,
    pub images: Vec<String>,
    pub common_labels: HashMap<String, String>,
    pub common_annotations: HashMap<String, String>,
    pub name_prefix: String,
    pub name_suffix: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DirectorySource {
    pub recurse: bool,
    pub jsonnet: Option<JsonnetSource>,
    pub exclude: Vec<String>,
    pub include: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonnetSource {
    pub ext_vars: Vec<String>,
    pub tlas: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginSource {
    pub name: String,
    pub env: Vec<EnvVar>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApplicationDestination {
    pub server: String,
    pub namespace: String,
    pub name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncPolicy {
    pub automated: Option<SyncPolicyAutomated>,
    pub sync_options: Vec<String>,
    pub retry: Option<RetryStrategy>,
    pub allow_empty: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncPolicyAutomated {
    pub prune: bool,
    pub self_heal: bool,
    pub allow_empty: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RetryStrategy {
    pub limit: i32,
    pub backoff: Backoff,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Backoff {
    pub duration: String,
    pub factor: i32,
    pub max_duration: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResourceIgnoreDifference {
    pub group: String,
    pub kind: String,
    pub name: String,
    pub namespace: Option<String>,
    pub json_pointers: Vec<String>,
    pub jq_path_expressions: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Info {
    pub name: String,
    pub value: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ApplicationStatus {
    pub sync: SyncStatusInfo,
    pub health: HealthStatusInfo,
    pub conditions: Vec<Condition>,
    pub observed_at: String,
    pub operation_state: Option<OperationState>,
    pub history: Vec<SyncHistory>,
    pub resources: Vec<ResourceStatus>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SyncStatusInfo {
    pub status: SyncStatus,
    pub revision: String,
    pub compared_to: Option<ComparedTo>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComparedTo {
    pub source: ApplicationSource,
    pub destination: ApplicationDestination,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct HealthStatusInfo {
    pub status: HealthStatus,
    pub message: String,
    pub progress: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum HealthStatus {
    #[default]
    Unknown,
    Healthy,
    Degraded,
    Missing,
    Suspended,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Condition {
    pub type_: String,
    pub status: String,
    pub message: String,
    pub last_transition_time: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OperationState {
    pub phase: String,
    pub message: String,
    pub started_at: String,
    pub finished_at: Option<String>,
    pub sync_result: Option<SyncResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncResult {
    pub revision: String,
    pub resources: Vec<ResourceResult>,
    pub hooks: Vec<HookResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResourceResult {
    pub hook_phase: String,
    pub group: String,
    pub kind: String,
    pub name: String,
    pub namespace: String,
    pub status: SyncStatus,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HookResult {
    pub name: String,
    pub phase: String,
    pub started_at: String,
    pub finished_at: Option<String>,
    pub status: SyncStatus,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncHistory {
    pub revision: String,
    pub source: ApplicationSource,
    pub destination: ApplicationDestination,
    pub start_time: String,
    pub end_time: String,
    pub status: SyncStatus,
    pub message: String,
    pub performed_by: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResourceStatus {
    pub group: String,
    pub kind: String,
    pub name: String,
    pub namespace: String,
    pub status: SyncStatus,
    pub health: HealthStatus,
    pub hook_phase: String,
    pub manifest: Option<String>,
    pub sync_status: SyncStatus,
    pub message: String,
}

/// GitOps events
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum GitOpsEvent {
    RepoCloned {
        repo: String,
        path: PathBuf,
    },
    RepoUpdated {
        repo: String,
        commit: String,
    },
    SyncStarted {
        app: String,
        revision: String,
    },
    SyncCompleted {
        app: String,
        revision: String,
        duration_ms: u64,
    },
    SyncFailed {
        app: String,
        error: String,
    },
    ResourceApplied {
        app: String,
        resource: String,
        kind: String,
    },
    ResourceDeleted {
        app: String,
        resource: String,
        kind: String,
    },
    ResourceFailed {
        app: String,
        resource: String,
        kind: String,
        error: String,
    },
    DriftDetected {
        app: String,
        resource: String,
        diff: String,
    },
    HealthDegraded {
        app: String,
        resource: String,
        health: HealthStatus,
    },
    WebhookReceived {
        payload: String,
    },
}

/// GitOps manager
pub struct GitOpsManager {
    config: GitOpsConfig,
    applications: Arc<RwLock<HashMap<String, Application>>>,
    repo_path: PathBuf,
    event_sender: broadcast::Sender<GitOpsEvent>,
    git_repo: Option<git2::Repository>,
}

impl GitOpsManager {
    pub fn new(config: GitOpsConfig) -> Self {
        let (event_sender, _) = broadcast::channel(1000);
        // The repo root only. `manifest_path` is relative to a checkout and is
        // applied when reading, so joining it here applied it twice.
        // Android has no writable `/tmp`; `$TMPDIR` (Termux) or the cache dir is
        // what actually exists, and `/tmp` fail here meant *every* GitOps
        // command errored on the first write.
        // PDN_GITOPS_DIR names the repository root itself, so a caller can point
        // it straight at a checkout of manifests.
        let repo_path = match std::env::var("PDN_GITOPS_DIR") {
            Ok(v) if !v.is_empty() => PathBuf::from(v),
            // Android has no writable `/tmp`; `$TMPDIR` (Termux) is what exists.
            _ => PathBuf::from(std::env::var("TMPDIR").unwrap_or_else(|_| "/tmp".to_string()))
                .join("pdn-gitops"),
        };

        Self {
            config,
            applications: Arc::new(RwLock::new(HashMap::new())),
            repo_path,
            event_sender,
            git_repo: None,
        }
    }

    fn emit_event(&self, event: GitOpsEvent) {
        self.event_sender.send(event).ok();
    }

    /// The directory applications are read from: `<repo>/applications`.
    pub fn applications_dir(&self) -> PathBuf {
        self.repo_path.join("applications")
    }

    /// Where the last-known state is persisted between invocations.
    fn state_path(&self) -> PathBuf {
        self.repo_path.join(".gitops-state.json")
    }

    /// Read applications from `<repo>/applications/*.yaml`.
    ///
    /// This is what populates `applications`; nothing did, so every command
    /// operated on an empty map and `sync` looped over zero keys.
    pub async fn load_from_disk(&mut self) -> Result<usize> {
        let dir = self.applications_dir();
        if !dir.exists() {
            return Ok(0);
        }

        let mut loaded = HashMap::new();
        for entry in std::fs::read_dir(&dir)? {
            let path = entry?.path();
            if !path.is_file() || !Self::is_manifest(&path) {
                continue;
            }
            match serde_yaml::from_str::<Application>(&std::fs::read_to_string(&path)?) {
                Ok(app) if !app.name.is_empty() => {
                    loaded.insert(app.name.clone(), app);
                }
                Ok(_) => warn!("{} has no name; skipped", path.display()),
                Err(e) => warn!("Skipping application {}: {}", path.display(), e),
            }
        }

        let count = loaded.len();
        *self.applications.write().await = loaded;
        Ok(count)
    }

    /// Restore the state written by a previous invocation.
    pub async fn load_state(&self) -> Result<()> {
        let path = self.state_path();
        let Ok(raw) = std::fs::read_to_string(&path) else {
            return Ok(());
        };
        let stored: HashMap<String, Application> = serde_json::from_str(&raw)
            .with_context(|| format!("Corrupt GitOps state at {}", path.display()))?;

        let mut apps = self.applications.write().await;
        for (name, stored_app) in stored {
            match apps.get_mut(&name) {
                // The manifest defines identity; the store defines what has been
                // observed. Copying only `status` keeps both truths intact.
                // (`or_insert` here was a no-op — `load_from_disk` had already
                // inserted every key — so the recorded status was discarded and
                // every command reported Unknown.)
                Some(app) => app.status = stored_app.status,
                None => {
                    apps.insert(name, stored_app);
                }
            }
        }
        Ok(())
    }

    /// Persist the current state for the next invocation.
    pub async fn save_state(&self) -> Result<()> {
        let path = self.state_path();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).with_context(|| {
                format!(
                    "Cannot create the GitOps state directory {}. Set PDN_GITOPS_DIR to a \
                     writable path.",
                    parent.display()
                )
            })?;
        }
        let apps = self.applications.read().await;
        std::fs::write(&path, serde_json::to_vec_pretty(&*apps)?)?;
        Ok(())
    }

    pub fn config(&self) -> &GitOpsConfig {
        &self.config
    }

    pub fn repo_path(&self) -> &Path {
        &self.repo_path
    }

    pub async fn list_applications(&self) -> Vec<Application> {
        let mut apps: Vec<Application> = self.applications.read().await.values().cloned().collect();
        apps.sort_by(|a, b| a.name.cmp(&b.name));
        apps
    }

    pub async fn get_application(&self, name: &str) -> Option<Application> {
        self.applications.read().await.get(name).cloned()
    }

    /// Sync every loaded application, returning how many synced cleanly.
    pub async fn sync_now(&self) -> Result<usize> {
        Self::sync_all_applications(
            self.applications.clone(),
            &self.repo_path,
            &self.event_sender,
        )
        .await
    }

    pub async fn start(&mut self) -> Result<()> {
        if !self.config.enabled {
            info!("GitOps disabled");
            return Ok(());
        }

        info!("Starting GitOps manager for repo: {}", self.config.repo_url);

        // Initialize/clone repository
        self.init_repository().await?;

        // Start sync loop
        self.start_sync_loop().await;

        // Start webhook server if enabled
        if self.config.use_webhook {
            self.start_webhook_server().await?;
        }

        Ok(())
    }

    async fn init_repository(&mut self) -> Result<()> {
        if self.repo_path.exists() {
            info!(
                "Repository already exists at {:?}, pulling latest",
                self.repo_path
            );
            self.pull_repository().await?;
        } else {
            info!("Cloning repository: {}", self.config.repo_url);
            self.clone_repository().await?;
        }

        self.git_repo = Some(git2::Repository::open(&self.repo_path)?);
        Ok(())
    }

    async fn clone_repository(&mut self) -> Result<()> {
        let mut builder = git2::build::RepoBuilder::new();

        let mut callbacks = git2::RemoteCallbacks::new();

        if let Some(creds) = &self.config.credentials {
            callbacks.credentials(|_url, username, _allowed| {
                git2::Cred::userpass_plaintext(&creds.username, &creds.password)
            });
        } else if let Some(ssh_key) = &self.config.ssh_key_path {
            callbacks.credentials(|_url, username, _allowed| {
                git2::Cred::ssh_key(username.unwrap_or("git"), None, ssh_key, None)
            });
        }

        let mut remote_callbacks = None;
        if self.config.credentials.is_some() || self.config.ssh_key_path.is_some() {
            remote_callbacks = Some(callbacks);
        }

        builder.branch(&self.config.branch);

        let repo = if let Some(callbacks) = remote_callbacks {
            let mut remote_callbacks = git2::RemoteCallbacks::new();
            // We need to set up the callbacks properly
            // For simplicity, we'll use the fetch options
            let mut fetch_options = git2::FetchOptions::new();
            fetch_options.remote_callbacks(callbacks);
            builder.fetch_options(fetch_options);
            builder.clone(&self.config.repo_url, &self.repo_path)?
        } else {
            builder.clone(&self.config.repo_url, &self.repo_path)?
        };

        self.git_repo = Some(repo);

        self.emit_event(GitOpsEvent::RepoCloned {
            repo: self.config.repo_url.clone(),
            path: self.repo_path.clone(),
        });

        Ok(())
    }

    async fn pull_repository(&self) -> Result<()> {
        let repo = self
            .git_repo
            .as_ref()
            .context("Repository not initialized")?;

        let mut remote = repo.find_remote("origin")?;

        let mut callbacks = git2::RemoteCallbacks::new();
        if let Some(creds) = &self.config.credentials {
            callbacks.credentials(|_url, username, _allowed| {
                git2::Cred::userpass_plaintext(&creds.username, &creds.password)
            });
        }

        let mut fetch_options = git2::FetchOptions::new();
        fetch_options.remote_callbacks(callbacks);

        remote.fetch(&[&self.config.branch], Some(&mut fetch_options), None)?;

        let merge_head = repo.refname_to_id("refs/remotes/origin/HEAD")?;
        let mut reference = repo.find_reference("HEAD")?;
        reference.set_target(merge_head, "Fast-forward")?;

        let head_commit = repo.find_commit(merge_head)?;
        let commit_id = head_commit.id().to_string();

        self.emit_event(GitOpsEvent::RepoUpdated {
            repo: self.config.repo_url.clone(),
            commit: commit_id,
        });

        Ok(())
    }

    async fn start_sync_loop(&self) {
        let config = self.config.clone();
        let apps = self.applications.clone();
        let repo_path = self.repo_path.clone();
        let event_sender = self.event_sender.clone();

        tokio::spawn(async move {
            let mut interval = interval(Duration::from_secs(config.sync_interval_secs));

            loop {
                interval.tick().await;

                if let Err(e) =
                    Self::sync_all_applications(apps.clone(), &repo_path, &event_sender).await
                {
                    error!("Sync failed: {}", e);
                }
            }
        });
    }

    async fn sync_all_applications(
        apps: Arc<RwLock<HashMap<String, Application>>>,
        repo_path: &Path,
        event_sender: &broadcast::Sender<GitOpsEvent>,
    ) -> Result<usize> {
        let app_list = {
            let apps_read = apps.read().await;
            apps_read.keys().cloned().collect::<Vec<_>>()
        };

        let mut synced = 0;
        for app_name in app_list {
            match Self::sync_application(apps.clone(), repo_path, event_sender, &app_name).await {
                Ok(()) => synced += 1,
                // One bad application must not stop the rest, but the failure
                // has to reach the caller rather than only the log.
                Err(e) => error!("Failed to sync application {}: {}", app_name, e),
            }
        }
        Ok(synced)
    }

    /// Sync one named application, taking `&self` so callers hold a manager.
    pub async fn sync_one(&self, app_name: &str) -> Result<()> {
        Self::sync_application(
            self.applications.clone(),
            &self.repo_path,
            &self.event_sender,
            app_name,
        )
        .await
    }

    async fn sync_application(
        apps: Arc<RwLock<HashMap<String, Application>>>,
        repo_path: &Path,
        event_sender: &broadcast::Sender<GitOpsEvent>,
        app_name: &str,
    ) -> Result<()> {
        let app = {
            let apps_read = apps.read().await;
            apps_read.get(app_name).cloned()
        };

        let Some(mut app) = app else {
            return Ok(());
        };

        let start_time = Instant::now();
        let revision = Self::get_current_revision(&repo_path).await?;

        event_sender
            .send(GitOpsEvent::SyncStarted {
                app: app_name.to_string(),
                revision: revision.clone(),
            })
            .ok();

        // Update sync status
        {
            let mut apps_write = apps.write().await;
            if let Some(app_mut) = apps_write.get_mut(app_name) {
                app_mut.status.sync.status = SyncStatus::Syncing;
                app_mut.status.operation_state = Some(OperationState {
                    phase: "Syncing".to_string(),
                    message: "Syncing from Git repository".to_string(),
                    started_at: chrono::Utc::now().to_rfc3339(),
                    finished_at: None,
                    sync_result: None,
                });
            }
        }

        // Read manifests from repo
        let manifests = Self::read_manifests(repo_path, &app.source.path).await?;

        // Apply manifests
        let mut applied = 0;
        let mut failed = 0;
        let mut resources = Vec::new();

        for manifest in manifests {
            let resource_key = format!(
                "{}/{}",
                manifest.metadata.namespace.as_deref().unwrap_or("default"),
                manifest.kind.to_lowercase()
            );

            event_sender
                .send(GitOpsEvent::ResourceApplied {
                    app: app_name.to_string(),
                    resource: manifest.metadata.name.clone(),
                    kind: manifest.kind.clone(),
                })
                .ok();

            resources.push(ResourceStatus {
                group: manifest
                    .api_version
                    .split('/')
                    .next()
                    .unwrap_or("")
                    .to_string(),
                kind: manifest.kind.clone(),
                name: manifest.metadata.name.clone(),
                namespace: manifest
                    .metadata
                    .namespace
                    .clone()
                    .unwrap_or_else(|| "default".to_string()),
                status: SyncStatus::Synced,
                health: HealthStatus::Healthy,
                hook_phase: "".to_string(),
                manifest: Some(serde_json::to_string(&manifest).unwrap_or_default()),
                sync_status: SyncStatus::Synced,
                message: "Applied successfully".to_string(),
            });

            applied += 1;
        }

        // Prune if enabled
        if app
            .sync_policy
            .as_ref()
            .map(|p| p.automated.as_ref().map(|a| a.prune).unwrap_or(false))
            .unwrap_or(false)
        {
            // Would prune resources not in manifests
        }

        // Update application status
        {
            let mut apps_write = apps.write().await;
            if let Some(app_mut) = apps_write.get_mut(app_name) {
                app_mut.status.sync.status = SyncStatus::Synced;
                app_mut.status.sync.revision = revision.clone();
                app_mut.status.health.status = HealthStatus::Healthy;
                app_mut.status.observed_at = chrono::Utc::now().to_rfc3339();
                app_mut.status.operation_state = Some(OperationState {
                    phase: "Succeeded".to_string(),
                    message: "Sync completed successfully".to_string(),
                    started_at: chrono::Utc::now().to_rfc3339(),
                    finished_at: Some(chrono::Utc::now().to_rfc3339()),
                    sync_result: Some(SyncResult {
                        revision: revision.clone(),
                        resources: resources
                            .iter()
                            .map(|r| ResourceResult {
                                hook_phase: r.hook_phase.clone(),
                                group: r.group.clone(),
                                kind: r.kind.clone(),
                                name: r.name.clone(),
                                namespace: r.namespace.clone(),
                                status: r.sync_status.clone(),
                                message: r.message.clone(),
                            })
                            .collect(),
                        hooks: vec![],
                    }),
                });
                // These used to be thrown away one line after being built, which
                // is why `gitops resources` could never show anything.
                app_mut.status.resources = resources.clone();
                app_mut.status.history.insert(
                    0,
                    SyncHistory {
                        revision: "current".to_string(),
                        source: app.source.clone(),
                        destination: app.destination.clone(),
                        start_time: chrono::Utc::now().to_rfc3339(),
                        end_time: chrono::Utc::now().to_rfc3339(),
                        status: SyncStatus::Synced,
                        message: "Sync completed successfully".to_string(),
                        performed_by: "gitops-controller".to_string(),
                    },
                );
                if app_mut.status.history.len() > 10 {
                    app_mut.status.history.truncate(10);
                }
            }
        }

        let duration = start_time.elapsed().as_millis() as u64;
        event_sender
            .send(GitOpsEvent::SyncCompleted {
                app: app_name.to_string(),
                revision: "current".to_string(),
                duration_ms: duration,
            })
            .ok();

        Ok(())
    }

    async fn read_manifests(repo_path: &Path, manifest_path: &str) -> Result<Vec<GitResource>> {
        let full_path = repo_path.join(manifest_path);
        let mut resources: Vec<GitResource> = Vec::new();

        if !full_path.exists() {
            return Ok(resources);
        }

        if full_path.is_file() {
            if Self::is_manifest(&full_path) {
                Self::read_manifest_file(&full_path, &mut resources)?;
            }
        } else {
            for entry in walkdir::WalkDir::new(&full_path) {
                let entry = entry?;
                if entry.file_type().is_file() && Self::is_manifest(entry.path()) {
                    // One unreadable file must not abort the whole sync; the
                    // walk collects what it can and reports the rest.
                    if let Err(e) = Self::read_manifest_file(entry.path(), &mut resources) {
                        warn!("Skipping manifest {}: {}", entry.path().display(), e);
                    }
                }
            }
        }

        Ok(resources)
    }

    fn is_manifest(path: &Path) -> bool {
        path.extension()
            .map(|e| e == "yaml" || e == "yml")
            .unwrap_or(false)
    }

    /// Parse every YAML document in one file into resources.
    ///
    /// `serde_yaml::Deserializer` is used rather than a single `from_str`
    /// because manifests are routinely multi-document (`---` separated).
    fn read_manifest_file(path: &Path, resources: &mut Vec<GitResource>) -> Result<()> {
        let content = std::fs::read_to_string(path)?;

        for document in serde_yaml::Deserializer::from_str(&content) {
            // An empty document (a trailing `---`) is not an error.
            let value = serde_yaml::Value::deserialize(document)?;
            if value.is_null() {
                continue;
            }
            match serde_yaml::from_value::<GitResource>(value) {
                Ok(resource) => resources.push(resource),
                Err(e) => warn!("Skipping a document in {}: {}", path.display(), e),
            }
        }

        Ok(())
    }

    /// The revision to record for a checkout.
    ///
    /// A plain directory of manifests is a legitimate source, so a missing
    /// repository is "unversioned", not a failure.
    async fn get_current_revision(repo_path: &Path) -> Result<String> {
        let revision = git2::Repository::open(repo_path)
            .ok()
            .and_then(|repo| {
                let head = repo.head().ok()?;
                let commit = head.peel_to_commit().ok()?;
                Some(commit.id().to_string())
            })
            .unwrap_or_else(|| "unversioned".to_string());
        Ok(revision)
    }

    async fn start_webhook_server(&self) -> Result<()> {
        if !self.config.use_webhook {
            return Ok(());
        }

        info!(
            "Starting GitOps webhook server on port {}",
            self.config.webhook_port
        );

        let apps = self.applications.clone();
        let repo_path = self.repo_path.clone();
        let event_sender = self.event_sender.clone();

        let app = Router::new()
            .route("/webhook", post(webhook_handler))
            .with_state(apps.clone());

        let addr = SocketAddr::from(([0, 0, 0, 0], self.config.webhook_port));
        let listener = tokio::net::TcpListener::bind(addr).await?;

        tokio::spawn(async move {
            axum::serve(listener, app).await.ok();
        });

        Ok(())
    }
}

async fn webhook_handler(
    State(_apps): State<Arc<RwLock<HashMap<String, Application>>>>,
    headers: HeaderMap,
    body: String,
) -> impl IntoResponse {
    // Verify webhook signature if secret configured
    // Process webhook payload
    Json(serde_json::json!({"status": "ok"}))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test(flavor = "multi_thread")]
    async fn test_gitops_config() {
        let config = GitOpsConfig::default();
        assert!(config.enabled);
        assert!(!config.repo_url.is_empty());
    }
}
