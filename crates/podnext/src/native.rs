//! Native implementations for podnext
//!
//! This module provides native implementations of all podnext commands
//! using the Rust crates directly instead of wrapping proot-distro.

/// Trace-only logging, off unless `PN_DEBUG` is set in the environment.
///
/// These lines were unconditional upstream and flooded real output — a single
/// `backup create` emitted ~280 kB of DEBUG for one alpine container, and every
/// command printed a DEBUG banner, burying the actual result. Enable with
/// `PN_DEBUG=1` when diagnosing.
macro_rules! pn_trace {
    ($($arg:tt)*) => {
        if ::std::env::var_os("PN_DEBUG").is_some() {
            ::std::eprintln!($($arg)*);
        }
    };
}
use anyhow::{bail, Context, Result};
use flate2::read::GzDecoder;
use hex;
use indicatif::{ProgressBar, ProgressStyle};
use oci_spec::image::ImageManifest;
use reqwest::Client;
use sha2::{Digest, Sha256};
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use tar::Archive;
use tokio::signal::unix::{signal, SignalKind};
use urlencoding::encode;

// ===== Docker Hub Search =====

#[derive(serde::Deserialize, Debug)]
pub struct DockerHubSearchResponse {
    #[allow(dead_code)]
    pub count: usize,
    pub results: Vec<DockerHubImage>,
}

#[derive(serde::Deserialize, Debug)]
pub struct DockerHubImage {
    pub repo_name: String,
    pub short_description: String,
    pub star_count: u32,
    pub pull_count: u64,
    pub is_official: bool,
    pub is_automated: bool,
}

pub async fn search_docker_hub(query: &str, limit: usize) -> Result<Vec<DockerHubImage>> {
    let client = Client::new();
    let url = format!(
        "https://registry.hub.docker.com/v2/search/repositories/?query={}&page_size={}",
        encode(query),
        limit.min(100)
    );

    let response = client
        .get(&url)
        .header(
            "User-Agent",
            format!("podnext/{}", env!("CARGO_PKG_VERSION")),
        )
        .send()
        .await
        .context("Failed to send search request")?;

    if !response.status().is_success() {
        bail!("Docker Hub search failed: {}", response.status());
    }

    let search_response: DockerHubSearchResponse = response
        .json()
        .await
        .context("Failed to parse search response")?;

    Ok(search_response.results.into_iter().take(limit).collect())
}

// ===== OCI Image Download & Install =====

#[derive(serde::Deserialize, Debug)]
pub struct DockerHubTokenResponse {
    pub token: String,
}

#[derive(serde::Deserialize, Debug)]
pub struct ManifestList {
    pub manifests: Vec<ManifestDescriptor>,
}

#[derive(serde::Deserialize, Debug)]
pub struct ManifestDescriptor {
    pub digest: String,
    #[serde(rename = "mediaType")]
    #[allow(dead_code)]
    pub media_type: String,
    pub platform: Option<Platform>,
}

#[derive(serde::Deserialize, Debug)]
pub struct Platform {
    pub architecture: String,
    pub os: String,
}

#[allow(dead_code)]
struct ImageLayer {
    digest: String,
    size: u64,
    media_type: String,
}

pub async fn install_image_native(
    image_ref: &str,
    container_name: &str,
    force: bool,
    rootfs_path: Option<PathBuf>,
) -> Result<PathBuf, anyhow::Error> {
    // Parse image reference (e.g., "ubuntu:22.04")
    let (name, tag) = parse_image_ref(image_ref)?;

    let container_name = container_name.to_string();

    println!("Installing '{}' as '{}'...", image_ref, container_name);

    // Determine rootfs path
    let rootfs = rootfs_path.unwrap_or_else(|| {
        dirs::home_dir()
            .unwrap_or_else(|| PathBuf::from("/home/user"))
            .join(".podnext")
            .join("containers")
            .join(&container_name)
            .join("rootfs")
    });

    // Check if container already exists (before creating directory)
    if rootfs.exists() && !force {
        bail!(
            "Container '{}' already exists at {}. Use --force to overwrite.",
            container_name,
            rootfs.display()
        );
    }

    // Create rootfs directory
    fs::create_dir_all(&rootfs)?;

    // Step 1: Get auth token from Docker Hub
    println!("[1/4] Authenticating with Docker Hub registry...");
    let token = get_docker_hub_token(&name).await?;

    // Step 2: Get manifest list to find correct platform
    println!(
        "[2/4] Fetching image manifest (linux/{})...",
        std::env::consts::ARCH
    );
    let manifest = get_manifest(&name, &tag, &token).await?;

    // Step 3: Download and extract layers
    println!("[3/4] Downloading and extracting OCI layers (Android-safe)...");
    if let Err(e) = download_and_extract_layers(&name, &manifest, &token, &rootfs).await {
        // Clean up partial rootfs directory on failure so it does not leave a broken state
        let _ = fs::remove_dir_all(&rootfs);
        return Err(e);
    }

    // Step 4: Apply proot-compat patches
    println!("[4/4] Configuring guest environment, DNS, and /dev/shm...");
    proot_compat::patch_rootfs(&rootfs)?;

    // Step 4b: Set up guest network config files.
    // Without /etc/resolv.conf the guest has no DNS at all (docker images
    // ship none, and the host has none to inherit); `apk update` then fails
    // with "temporary error". /etc/hosts is normalised the same way.
    configure_guest_network(&rootfs, &container_name)?;

    // Prepare the fake /proc stubs alongside the rootfs, so the container is
    // complete before its first login. `login` regenerates `stat`/`uptime` for
    // whatever CPU set the device has at that moment.
    if let Some(container_dir) = rootfs.parent() {
        proot_distro_core::sysdata::ensure(container_dir)?;
        proot_distro_core::guestdirs::ensure(container_dir)?;
    }

    // Step 5: Save container metadata
    save_container_metadata(&container_name, &name, &tag, &rootfs)?;

    println!("\nContainer '{}' installed successfully!", container_name);
    println!("Rootfs: {}", rootfs.display());
    println!("\nTo start the container:");
    println!("  podnext login {}", container_name);

    Ok(rootfs)
}

/// Check that an image reference can actually be fetched, before anything destructive.
///
/// `reset` deletes the container directory and only then installs the base image, so
/// an unfetchable reference — including one a `backup restore` wrote as a *container*
/// name rather than a registry reference — destroyed the container and then failed to
/// replace it. Resolving the manifest here costs one round trip and lets the caller
/// fail first, while the container is still on disk.
pub async fn verify_image_available(image_ref: &str) -> Result<()> {
    let (name, tag) = parse_image_ref(image_ref)?;
    let token = get_docker_hub_token(&name).await?;
    get_manifest(&name, &tag, &token).await?;
    Ok(())
}

/// Write the guest's network configuration files.
///
/// Docker/OCI rootfs images ship no `/etc/resolv.conf`, and Termux has none to
/// inherit, so a freshly installed PDN container had zero DNS: `apk update`
/// died with "temporary error" and `getent`/`nslookup` failed. proot-distro
/// solves this by writing resolv.conf at install time; we do the same.
///
/// Nameservers come from the host when it has any (Android's own resolver is
/// not visible from Termux, so this rarely hits) and otherwise fall back to the
/// public resolvers. `/etc/hosts` gets a minimal loopback entry plus the
/// container's own hostname so `hostname`-based lookups resolve locally.
fn configure_guest_network(rootfs: &Path, container_name: &str) -> Result<()> {
    let etc = rootfs.join("etc");
    fs::create_dir_all(&etc)?;

    let resolv = etc.join("resolv.conf");
    if !resolv.exists() {
        let contents = "nameserver 8.8.8.8\nnameserver 8.8.4.4\nnameserver 1.1.1.1\n";
        fs::write(&resolv, contents)?;
    }

    let hosts = etc.join("hosts");
    if !hosts.exists() {
        let contents = format!(
            "127.0.0.1\tlocalhost localhost.localdomain\n\
             ::1\t\tlocalhost localhost.localdomain\n\
             127.0.0.1\t{}\n",
            container_name
        );
        fs::write(&hosts, contents)?;
    }

    let hostname_file = etc.join("hostname");
    if !hostname_file.exists() {
        fs::write(&hostname_file, format!("{}\n", container_name))?;
    }

    Ok(())
}

fn parse_image_ref(image_ref: &str) -> Result<(String, String)> {
    let parts: Vec<&str> = image_ref.split(':').collect();
    match parts.len() {
        1 => Ok((parts[0].to_string(), "latest".to_string())),
        2 => Ok((parts[0].to_string(), parts[1].to_string())),
        _ => bail!("Invalid image reference: {}", image_ref),
    }
}

async fn get_docker_hub_token(repo: &str) -> Result<String> {
    let client = Client::new();
    // Use library/ prefix for official images in token scope
    let scope_repo = if repo.contains('/') {
        repo.to_string()
    } else {
        format!("library/{}", repo)
    };
    let url = format!(
        "https://auth.docker.io/token?service=registry.docker.io&scope=repository:{}:pull",
        scope_repo
    );

    let response = client.get(&url).send().await?;
    let token_resp: DockerHubTokenResponse = response.json().await?;
    Ok(token_resp.token)
}

async fn get_manifest(name: &str, tag: &str, token: &str) -> Result<ImageManifest> {
    let client = Client::new();
    // Add library/ prefix for official images (no slash in name)
    let manifest_name = if name.contains('/') {
        name.to_string()
    } else {
        format!("library/{}", name)
    };
    let url = format!(
        "https://registry.hub.docker.com/v2/{}/manifests/{}",
        manifest_name, tag
    );

    let response = client
        .get(&url)
        .bearer_auth(token)
        .header(
            "Accept",
            "application/vnd.docker.distribution.manifest.v2+json",
        )
        .header(
            "Accept",
            "application/vnd.docker.distribution.manifest.list.v2+json",
        )
        .header("Accept", "application/vnd.oci.image.manifest.v1+json")
        .send()
        .await?;

    if !response.status().is_success() {
        bail!("Failed to fetch manifest: {}", response.status());
    }

    let content_type = {
        let headers = response.headers();
        headers
            .get("content-type")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .to_string()
    };

    let body = response.bytes().await?;

    // Handle both Docker manifest list and OCI image index
    if content_type.contains("manifest.list")
        || content_type.contains("manifest.list.v2")
        || content_type.contains("oci.image.index")
    {
        // Multi-platform manifest - find linux/arm64
        let manifest_list: ManifestList = serde_json::from_slice(&body)?;
        let arm64_manifest = manifest_list
            .manifests
            .iter()
            .find(|m| {
                m.platform
                    .as_ref()
                    .map(|p| p.architecture == "arm64" && p.os == "linux")
                    .unwrap_or(false)
            })
            .ok_or_else(|| anyhow::anyhow!("No linux/arm64 manifest found"))?;

        // Fetch the specific manifest
        return get_manifest_by_digest(name, &arm64_manifest.digest, token).await;
    }

    // Single platform manifest
    let manifest: ImageManifest = serde_json::from_slice(&body)?;
    Ok(manifest)
}

async fn get_manifest_by_digest(name: &str, digest: &str, token: &str) -> Result<ImageManifest> {
    let client = Client::new();
    // Add library/ prefix for official images (no slash in name)
    let manifest_name = if name.contains('/') {
        name.to_string()
    } else {
        format!("library/{}", name)
    };
    let url = format!(
        "https://registry.hub.docker.com/v2/{}/manifests/{}",
        manifest_name, digest
    );

    let response = client
        .get(&url)
        .bearer_auth(token)
        .header(
            "Accept",
            "application/vnd.docker.distribution.manifest.v2+json",
        )
        .header("Accept", "application/vnd.oci.image.manifest.v1+json")
        .send()
        .await?;

    if !response.status().is_success() {
        bail!("Failed to fetch manifest by digest: {}", response.status());
    }

    let body = response.bytes().await?;
    let manifest: ImageManifest = serde_json::from_slice(&body)?;
    Ok(manifest)
}

async fn download_and_extract_layers(
    name: &str,
    manifest: &ImageManifest,
    token: &str,
    rootfs: &Path,
) -> Result<()> {
    let _client = Client::new();
    let layers = manifest.layers();
    let pb = ProgressBar::new(layers.len() as u64);
    pb.set_style(ProgressStyle::default_bar()
        .template("{spinner:.green} [{elapsed_precise}] [{bar:40.cyan/blue}] {pos}/{len} layers ({msg})")
        .unwrap()
        .progress_chars("#>-"));

    for layer in layers {
        let digest_str = layer.digest().to_string();
        pb.set_message(format!("Downloading {}", &digest_str[..20]));

        let layer_data = download_layer(name, &digest_str, token).await?;

        // Verify digest
        let mut hasher = Sha256::new();
        hasher.update(&layer_data);
        let computed = hex::encode(hasher.finalize());
        let expected = digest_str.strip_prefix("sha256:").unwrap_or(&digest_str);

        if computed != expected {
            bail!(
                "Layer digest mismatch: expected {}, got {}",
                expected,
                computed
            );
        }

        pb.set_message(format!("Extracting {}", &digest_str[..20]));
        extract_layer(&layer_data, &layer.media_type().to_string(), rootfs)?;

        pb.inc(1);
    }

    pb.finish_with_message("All layers extracted");
    Ok(())
}

async fn download_layer(name: &str, digest: &str, token: &str) -> Result<Vec<u8>> {
    let client = Client::new();
    // Add library/ prefix for official images (no slash in name)
    let manifest_name = if name.contains('/') {
        name.to_string()
    } else {
        format!("library/{}", name)
    };
    let url = format!(
        "https://registry.hub.docker.com/v2/{}/blobs/{}",
        manifest_name, digest
    );

    let response = client.get(&url).bearer_auth(token).send().await?;

    if !response.status().is_success() {
        bail!("Failed to download layer {}: {}", digest, response.status());
    }

    let bytes = response.bytes().await?;
    Ok(bytes.to_vec())
}

fn extract_layer(data: &[u8], _media_type: &str, rootfs: &Path) -> Result<()> {
    // Not `archive.unpack()`: this device denies `link(2)`, so layers carrying
    // hard links (Debian's perl, for one) abort the install. See the module
    // docs in proot_distro_core::untar.
    proot_distro_core::untar::extract(GzDecoder::new(data), rootfs)
}

fn save_container_metadata(
    container_name: &str,
    image_name: &str,
    image_tag: &str,
    rootfs: &Path,
) -> Result<()> {
    let metadata_dir = rootfs.parent().unwrap();
    let metadata_path = metadata_dir.join("metadata.json");

    let metadata = serde_json::json!({
        "name": container_name,
        "image": image_name,
        "tag": image_tag,
        "rootfs": rootfs.to_string_lossy(),
        "created_at": chrono::Utc::now().to_rfc3339(),
        "status": "Stopped"
    });

    fs::write(metadata_path, serde_json::to_string_pretty(&metadata)?)?;
    Ok(())
}

// ===== Native Container Operations =====

pub async fn list_containers_native(all: bool) -> Result<Vec<proot_distro_core::ContainerInfo>> {
    let container_mgr = proot_distro_core::ContainerManager::new()?;
    let containers = container_mgr.list().await?;

    if !all {
        // Filter to only running
        let containers: Vec<_> = containers
            .into_iter()
            .filter(|c| c.status == proot_distro_core::ContainerStatus::Running)
            .collect();
        return Ok(containers);
    }

    Ok(containers)
}

pub async fn login_container_native(
    container_name: &str,
    user: Option<String>,
    isolated: bool,
    command: Vec<String>,
) -> Result<()> {
    pn_trace!("[DEBUG native] Creating ProotDistroCore");
    let mut distro_core = proot_distro_core::ProotDistroCore::new()?;
    pn_trace!("[DEBUG native] ProotDistroCore created");

    let mut options = proot_distro_core::LoginOptions::default();
    options.container_name = container_name.to_string();
    options.isolated = isolated;
    pn_trace!("[DEBUG native] Default command: {:?}", options.command);
    options.command = if command.is_empty() {
        vec!["/bin/sh".to_string()]
    } else {
        command
    };
    pn_trace!("[DEBUG native] Final command: {:?}", options.command);

    if let Some(u) = user {
        options.user = Some(u);
    }

    pn_trace!("[DEBUG native] Calling login");
    distro_core.login(options).await?;
    pn_trace!("[DEBUG native] Login completed");
    Ok(())
}

/// PID of this process's only direct child during a foreground run — the proot
/// process the runtime spawned. proot-core keeps the child pid private, so the
/// run path has to discover it from `/proc` (matching `PPid`). Returns `None`
/// when no child is currently visible.
fn direct_child_pid() -> Option<i32> {
    let me = std::process::id() as i32;
    for entry in std::fs::read_dir("/proc").ok()?.flatten() {
        let Some(pid) = entry
            .file_name()
            .to_str()
            .and_then(|s| s.parse::<i32>().ok())
        else {
            continue;
        };
        let Ok(stat) = std::fs::read_to_string(entry.path().join("stat")) else {
            continue;
        };
        // `comm` (field 2) is parenthesised and may contain spaces/parens, so
        // the remaining fields only start reliably after the last ')'.
        let Some(rest) = stat.rfind(')').map(|i| &stat[i + 1..]) else {
            continue;
        };
        let mut fields = rest.split_whitespace();
        let _state = fields.next();
        if fields.next().and_then(|s| s.parse::<i32>().ok()) == Some(me) {
            return Some(pid);
        }
    }
    None
}

/// Forward `sig` to the running guest.
///
/// Sent to the child's *process group* so the whole traced tree is signalled at
/// once. The runtime is being given `process_group(0)` on spawn (seat
/// kill_correct); until that lands the child is not a group leader, `kill(-pid)`
/// returns ESRCH, and this falls back to signalling the child pid directly —
/// proot's own `--kill-on-exit` then tears down its tracees.
fn forward_to_guest(sig: i32) {
    let Some(pid) = direct_child_pid() else {
        return;
    };
    unsafe {
        if libc::kill(-pid, sig) != 0 {
            libc::kill(pid, sig);
        }
    }
}

pub async fn run_container_native(
    container_name: &str,
    command: Vec<String>,
    isolated: bool,
) -> Result<()> {
    let container_mgr = proot_distro_core::ContainerManager::new()?;
    let mut container = container_mgr.get(container_name).await?;
    if isolated {
        container.config.isolated = true;
    }
    let backend = container_mgr.get_backend(&container);

    // A foreground run must survive a SIGTERM/SIGINT long enough to kill the
    // guest. Without these handlers the default action terminated podnext
    // immediately and orphaned the proot process (and everything it traced).
    // `signal()`'s handler registration is dropped when this fn returns.
    let mut sigterm = signal(SignalKind::terminate())?;
    let mut sigint = signal(SignalKind::interrupt())?;

    let exec = backend.execute(&container, command);
    tokio::pin!(exec);

    let mut forwarded: Option<i32> = None;
    let result = loop {
        tokio::select! {
            res = &mut exec => break res,
            _ = sigterm.recv() => {
                forwarded = Some(libc::SIGTERM);
                forward_to_guest(libc::SIGTERM);
            }
            _ = sigint.recv() => {
                forwarded = Some(libc::SIGINT);
                forward_to_guest(libc::SIGINT);
            }
        }
    };

    // The runtime registered a session record for this pid and it is now spent;
    // remove it so `podnext ps`/sessions do not accumulate a stale entry. Runs
    // on normal, signalled and error exits alike.
    let _ = std::fs::remove_file(proot_distro_core::session::session_file_path(
        std::process::id(),
    ));

    let exit_code = result?;
    // A guest killed by signal N surfaces as 128+N (the shell convention), not
    // the -1/255 the runtime reports for a signal-terminated child.
    if let Some(sig) = forwarded {
        if exit_code != 0 {
            return Err(anyhow::Error::new(proot_core::ChildExit(128 + sig)));
        }
    }
    if exit_code != 0 {
        return Err(anyhow::Error::new(proot_core::ChildExit(exit_code)));
    }
    Ok(())
}

pub async fn list_sessions_native() -> Result<Vec<proot_distro_core::SessionInfo>> {
    let session_mgr = proot_distro_core::SessionManager::new()?;
    Ok(session_mgr.list_all())
}

pub async fn list_sessions_native_full() -> Result<Vec<proot_distro_core::Session>> {
    let session_mgr = proot_distro_core::SessionManager::new()?;
    Ok(session_mgr.list_all_full())
}

pub async fn kill_container_native(container_name: &str) -> Result<()> {
    let container_mgr = proot_distro_core::ContainerManager::new()?;
    container_mgr.kill(container_name).await
}

pub async fn remove_container_native(name: String, force: bool) -> Result<()> {
    let container_mgr = proot_distro_core::ContainerManager::new()?;

    if !force {
        print!("Remove container '{}'? [y/N]: ", name);
        io::stdout().flush()?;
        let mut input = String::new();
        io::stdin().read_line(&mut input)?;
        if !input.trim().eq_ignore_ascii_case("y") {
            println!("Cancelled.");
            return Ok(());
        }
    }

    container_mgr.remove(&name).await?;

    // ContainerManager::remove() only drops the in-memory entry; the rootfs
    // stayed on disk, so `remove` printed success while the container was
    // still installed (and the next `install` said "already exists").
    // Delete the on-disk container directory to make removal real.
    if let Some(home) = dirs::home_dir() {
        let dir = home.join(".podnext").join("containers").join(&name);
        if dir.exists() {
            fs::remove_dir_all(&dir).map_err(|e| {
                anyhow::anyhow!(
                    "removed '{}' from the index but failed to delete {}: {}",
                    name,
                    dir.display(),
                    e
                )
            })?;
        }
    }

    println!("Container '{}' removed.", name);
    Ok(())
}

#[cfg(all(test, unix))]
mod signal_tests {
    use super::*;
    use std::os::unix::process::CommandExt;

    /// Poll `f` until it is true or `deadline` passes; returns whether it became
    /// true. Asserts on the observable state change, not on a fixed sleep.
    fn eventually(deadline: std::time::Duration, mut f: impl FnMut() -> bool) -> bool {
        let end = std::time::Instant::now() + deadline;
        loop {
            if f() {
                return true;
            }
            if std::time::Instant::now() >= end {
                return false;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
    }

    /// The run path discovers the proot child from /proc and signals it; the
    /// guest must actually die. The child is spawned with `process_group(0)`,
    /// the same way proot-core will.
    #[test]
    fn forward_to_guest_terminates_child_group() {
        let mut child = std::process::Command::new("/bin/sh")
            .arg("-c")
            .arg("sleep 60")
            .process_group(0)
            .spawn()
            .expect("spawn child");
        let pid = child.id() as i32;

        // Observable: the child becomes visible to the run path's discovery.
        assert!(
            eventually(std::time::Duration::from_secs(5), || direct_child_pid()
                == Some(pid)),
            "child pid {pid} never showed up as this process's direct child"
        );

        forward_to_guest(libc::SIGTERM);

        // Observable: the child is reaped (gone), not a time elapsed.
        assert!(
            eventually(std::time::Duration::from_secs(5), || child
                .try_wait()
                .expect("try_wait")
                .is_some()),
            "child survived SIGTERM forwarded to its process group"
        );
    }
}
