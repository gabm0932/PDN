//! Image management for PRoot Distro

use anyhow::{bail, Result};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::json;
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::path::PathBuf;

/// Image source for downloading
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ImageSource {
    Local(PathBuf),
    Remote(String),
    Docker(String),
    Oci(String),
}

/// Image manager for container images
pub struct ImageManager {
    images: std::collections::HashMap<String, Image>,
    cache_dir: PathBuf,
}

impl ImageManager {
    pub fn new() -> Result<Self> {
        let cache_dir = dirs::cache_dir()
            .unwrap_or_else(|| PathBuf::from("/tmp"))
            .join("pn-next")
            .join("images");

        std::fs::create_dir_all(&cache_dir)?;

        Ok(Self {
            images: HashMap::new(),
            cache_dir,
        })
    }

    pub fn download_and_verify(&mut self, source: &str) -> Result<Image> {
        // Parse source (e.g., "ubuntu:22.04", "docker.io/library/ubuntu:22.04")
        let (name, tag) = Self::parse_image_source(source)?;

        // Check if already cached
        if let Some(image) = self.images.get(source) {
            return Ok(image.clone());
        }

        // Download image
        let image = self.download_image(&name, &tag)?;

        // Verify checksum
        self.verify_image(&image)?;

        // Store in cache
        self.images.insert(source.to_string(), image.clone());

        Ok(image)
    }

    pub fn download_and_verify_source(&mut self, source: &ImageSource) -> Result<Image> {
        let source_str = match source {
            ImageSource::Local(path) => path.to_string_lossy().to_string(),
            ImageSource::Remote(s) => s.clone(),
            ImageSource::Docker(s) => s.clone(),
            ImageSource::Oci(s) => s.clone(),
        };
        self.download_and_verify(&source_str)
    }

    fn parse_image_source(source: &str) -> Result<(String, String)> {
        let parts: Vec<&str> = source.split(':').collect();
        if parts.len() == 2 {
            Ok((parts[0].to_string(), parts[1].to_string()))
        } else {
            Ok((source.to_string(), "latest".to_string()))
        }
    }

    fn download_image(&self, name: &str, tag: &str) -> Result<Image> {
        // Implementation would download from registry
        // For now, return a placeholder
        Ok(Image {
            name: name.to_string(),
            tag: tag.to_string(),
            digest: String::new(),
            layers: Vec::new(),
            architecture: "aarch64".to_string(),
            os: "linux".to_string(),
            created: chrono::Utc::now(),
            size: 0,
        })
    }

    fn verify_image(&self, _image: &Image) -> Result<()> {
        // Verify image digest
        Ok(())
    }

    pub fn list(&self) -> Vec<&Image> {
        self.images.values().collect()
    }

    pub fn remove(&mut self, name: &str) -> Result<()> {
        self.images
            .remove(name)
            .ok_or_else(|| anyhow::anyhow!("Image not found"))?;
        Ok(())
    }

    /// Push an image to a registry
    pub async fn push(
        &mut self,
        image_ref: &str,
        username: Option<&str>,
        password: Option<&str>,
    ) -> Result<()> {
        let (name, tag) = Self::parse_image_source(image_ref)?;
        let image = self
            .images
            .get(image_ref)
            .ok_or_else(|| anyhow::anyhow!("Image '{}' not found locally", image_ref))?
            .clone();

        let client = Client::new();

        // Determine registry URL
        let (registry_host, repo_name) = Self::parse_registry_and_repo(&name)?;
        let registry_url = format!("https://{}", registry_host);

        // Get auth token for push
        let token = if let (Some(user), Some(pass)) = (username, password) {
            Self::get_registry_token_basic(&client, &registry_host, &repo_name, user, pass, "push")
                .await?
        } else {
            Self::get_registry_token(&client, &registry_host, &repo_name, "push").await?
        };

        // Pre-flight. Docker uploads blobs first and the manifest last; this used
        // to PUT the manifest and *then* print "would be pushed (skipped - no
        // local layer data)" for every layer, so the registry kept a manifest
        // describing blobs that were never uploaded and `docker pull` failed
        // afterwards. Refuse before any network write instead.
        if image.layers.is_empty() {
            bail!("Cannot push '{}': image has no layers", image_ref);
        }
        for layer in &image.layers {
            if layer.diff_id.is_none() {
                bail!(
                    "Cannot push '{}': layer {} has no diffID (the uncompressed tar digest). \
                     A manifest built without it is rejected by any registry that verifies \
                     chain-IDs.",
                    image_ref,
                    layer.digest
                );
            }
        }
        bail!(
            "Cannot push '{}': PDN does not retain layer blobs after extraction, so there is \
             nothing to upload. Install the image and rebuild it into a pushed archive first \
             (layer retention is not implemented yet).",
            image_ref
        );

        #[allow(unreachable_code)]
        // Get image manifest
        let manifest = self.build_manifest(&image)?;
        let manifest_bytes = serde_json::to_vec(&manifest)?;
        let _manifest_digest = Self::compute_digest(&manifest_bytes);

        // Push manifest
        let manifest_url = format!("{}/v2/{}/manifests/{}", registry_url, repo_name, tag);
        let response = client
            .put(&manifest_url)
            .bearer_auth(&token)
            .header(
                "Content-Type",
                "application/vnd.docker.distribution.manifest.v2+json",
            )
            .body(manifest_bytes)
            .send()
            .await?;

        if !response.status().is_success() {
            bail!("Failed to push manifest: {}", response.status());
        }

        // Push layers
        for layer in &image.layers {
            if layer.digest.is_empty() {
                continue;
            }
            let layer_url = format!("{}/v2/{}/blobs/{}", registry_url, repo_name, layer.digest);

            // Check if layer already exists
            let head_response = client.head(&layer_url).bearer_auth(&token).send().await?;
            if head_response.status().is_success() {
                continue; // Layer already exists
            }

            bail!("Layer {} has no local blob to upload", layer.digest);
        }

        eprintln!("Image {} pushed successfully", image_ref);
        Ok(())
    }

    /// Tag an image locally
    pub fn tag(&mut self, source: &str, target: &str) -> Result<()> {
        let image = self
            .images
            .get(source)
            .ok_or_else(|| anyhow::anyhow!("Source image '{}' not found", source))?
            .clone();

        // Parse target to get name and tag
        let (target_name, target_tag) = Self::parse_image_source(target)?;

        // Create new image with target tag
        let mut new_image = image.clone();
        new_image.name = target_name;
        new_image.tag = target_tag;

        self.images.insert(target.to_string(), new_image);
        Ok(())
    }

    /// Get image history (layers with metadata)
    pub fn history(&self, image_ref: &str) -> Result<Vec<ImageHistoryEntry>> {
        let image = self
            .images
            .get(image_ref)
            .ok_or_else(|| anyhow::anyhow!("Image '{}' not found", image_ref))?;

        let mut history = Vec::new();

        // Add image config as first history entry
        history.push(ImageHistoryEntry {
            id: image.digest.clone(),
            created: image.created,
            created_by: "image config".to_string(),
            size: 0,
            comment: format!("Imported from {}", image_ref),
        });

        // Add each layer as history entry
        for layer in &image.layers {
            history.push(ImageHistoryEntry {
                id: layer.digest.clone(),
                created: image.created,
                created_by: format!("layer {}", layer.media_type),
                size: layer.size,
                comment: "".to_string(),
            });
        }

        Ok(history)
    }

    /// Inspect image - get detailed information
    pub fn inspect(&self, image_ref: &str) -> Result<ImageInspectInfo> {
        let image = self
            .images
            .get(image_ref)
            .ok_or_else(|| anyhow::anyhow!("Image '{}' not found", image_ref))?;

        // Inspect reports what is known: a freshly pulled image may not have
        // diffIDs recorded yet, and `inspect` should not fail for that reason.
        let config = self.build_config(image, false)?;
        let manifest = Self::manifest_from_config(image, &config)?;

        Ok(ImageInspectInfo {
            id: image.digest.clone(),
            repo_tags: vec![format!("{}:{}", image.name, image.tag)],
            repo_digests: vec![format!("{}@{}", image.name, image.digest)],
            parent: String::new(),
            comment: String::new(),
            created: image.created,
            container: String::new(),
            container_config: serde_json::json!({}),
            docker_version: String::new(),
            author: String::new(),
            config: config.clone(),
            architecture: image.architecture.clone(),
            os: image.os.clone(),
            size: image.size,
            virtual_size: image.size,
            rootfs: config.rootfs.clone(),
            manifest: serde_json::to_value(&manifest)?,
        })
    }

    /// Build OCI manifest from image (returns JSON value for serialization)
    /// The container config blob for an image.
    ///
    /// `diff_ids` must hold the digest of each *uncompressed* layer tar. This
    /// used to be filled with the compressed layer digest, which is a different
    /// value; a registry that verifies chain-IDs rejects such a manifest. Rather
    /// than emit a plausible-looking wrong number, refuse.
    fn build_config(&self, image: &Image, require_diff_ids: bool) -> Result<ImageConfigInfo> {
        let mut diff_ids = Vec::with_capacity(image.layers.len());
        for layer in &image.layers {
            match (&layer.diff_id, require_diff_ids) {
                (Some(id), _) => diff_ids.push(id.clone()),
                (None, true) => anyhow::bail!(
                    "layer {} has no diffID (the uncompressed tar digest); it cannot be \
                     derived from the compressed digest",
                    layer.digest
                ),
                // Reporting only: show the layer digest in place of the unknown
                // diffID so the gap is visible rather than hidden. `push` refuses
                // before it can ship this.
                (None, false) => diff_ids.push(layer.digest.clone()),
            }
        }

        Ok(ImageConfigInfo {
            architecture: image.architecture.clone(),
            os: image.os.clone(),
            rootfs: RootFSInfo {
                typ: "layers".to_string(),
                diff_ids,
            },
            config: ContainerConfig {
                env: Vec::new(),
                cmd: Vec::new(),
                entrypoint: Vec::new(),
                working_dir: "/".to_string(),
                user: "root".to_string(),
                exposed_ports: HashMap::new(),
                volumes: HashMap::new(),
                labels: HashMap::new(),
            },
            created: image.created,
        })
    }

    fn build_manifest(&self, image: &Image) -> Result<serde_json::Value> {
        let config = self.build_config(image, true)?;
        Self::manifest_from_config(image, &config)
    }

    /// Manifest schema 2 for an image and its config blob.
    ///
    /// The config's digest must describe the blob we would upload, and its size
    /// must be the real length; this previously wrote `"size": 0` and borrowed
    /// `image.digest` (a *layer* digest) as the config digest.
    fn manifest_from_config(image: &Image, config: &ImageConfigInfo) -> Result<serde_json::Value> {
        let config_bytes = serde_json::to_vec(config)?;
        let config_digest = Self::compute_digest(&config_bytes);

        let layers: Vec<serde_json::Value> = image
            .layers
            .iter()
            .map(|l| {
                json!({
                    "mediaType": l.media_type,
                    "size": l.size,
                    "digest": l.digest,
                })
            })
            .collect();

        Ok(json!({
            "schemaVersion": 2,
            "mediaType": "application/vnd.docker.distribution.manifest.v2+json",
            "config": {
                "mediaType": "application/vnd.docker.container.image.v1+json",
                "size": config_bytes.len(),
                "digest": config_digest,
            },
            "layers": layers,
        }))
    }

    fn parse_registry_and_repo(name: &str) -> Result<(String, String)> {
        let parts: Vec<&str> = name.split('/').collect();
        if parts.len() >= 2 && parts[0].contains('.') {
            // Has registry host (e.g., docker.io/library/ubuntu)
            let registry = parts[0].to_string();
            let repo = parts[1..].join("/");
            Ok((registry, repo))
        } else {
            // Default to Docker Hub
            let repo = if parts.len() == 1 {
                format!("library/{}", name)
            } else {
                name.to_string()
            };
            Ok(("docker.io".to_string(), repo))
        }
    }

    async fn get_registry_token(
        client: &Client,
        registry: &str,
        repo: &str,
        action: &str,
    ) -> Result<String> {
        let scope = format!("repository:{}:{}", repo, action);
        let url = format!(
            "https://auth.{}/token?service=registry.{}&scope={}",
            registry, registry, scope
        );

        let response = client.get(&url).send().await?;
        let token_resp: DockerHubTokenResponse = response.json().await?;
        Ok(token_resp.token)
    }

    async fn get_registry_token_basic(
        client: &Client,
        registry: &str,
        repo: &str,
        username: &str,
        password: &str,
        action: &str,
    ) -> Result<String> {
        let scope = format!("repository:{}:{}", repo, action);
        let url = format!(
            "https://auth.{}/token?service=registry.{}&scope={}",
            registry, registry, scope
        );

        let response = client
            .get(&url)
            .basic_auth(username, Some(password))
            .send()
            .await?;
        let token_resp: DockerHubTokenResponse = response.json().await?;
        Ok(token_resp.token)
    }

    fn compute_digest(data: &[u8]) -> String {
        let mut hasher = Sha256::new();
        hasher.update(data);
        format!("sha256:{}", hex::encode(hasher.finalize()))
    }
}

#[derive(serde::Deserialize, Debug)]
struct DockerHubTokenResponse {
    pub token: String,
}

/// Image history entry
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImageHistoryEntry {
    pub id: String,
    pub created: chrono::DateTime<chrono::Utc>,
    pub created_by: String,
    pub size: u64,
    pub comment: String,
}

/// Image inspect info
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImageInspectInfo {
    pub id: String,
    pub repo_tags: Vec<String>,
    pub repo_digests: Vec<String>,
    pub parent: String,
    pub comment: String,
    pub created: chrono::DateTime<chrono::Utc>,
    pub container: String,
    pub container_config: serde_json::Value,
    pub docker_version: String,
    pub author: String,
    pub config: ImageConfigInfo,
    pub architecture: String,
    pub os: String,
    pub size: u64,
    pub virtual_size: u64,
    pub rootfs: RootFSInfo,
    pub manifest: serde_json::Value,
}

/// Simplified image config for inspect output
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImageConfigInfo {
    pub architecture: String,
    pub os: String,
    pub rootfs: RootFSInfo,
    pub config: ContainerConfig,
    pub created: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RootFSInfo {
    pub typ: String,
    pub diff_ids: Vec<String>,
}

/// Container image
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Image {
    pub name: String,
    pub tag: String,
    pub digest: String,
    pub layers: Vec<Layer>,
    pub architecture: String,
    pub os: String,
    pub created: chrono::DateTime<chrono::Utc>,
    pub size: u64,
}

/// Image layer
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Layer {
    /// Digest of the *compressed* blob as stored in the registry.
    pub digest: String,
    pub size: u64,
    pub media_type: String,
    /// Digest of the *uncompressed* layer tar (the OCI "diffID").
    ///
    /// This is a different value from `digest` and cannot be derived from it.
    /// It used to be filled with `digest`, which is a lie a registry that
    /// verifies chain-IDs will reject; `push` now refuses when it is absent
    /// rather than shipping the wrong number.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub diff_id: Option<String>,
}

/// Local image manifest (for storage)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LocalImageManifest {
    pub schema_version: u32,
    pub media_type: String,
    pub config: ImageConfig,
    pub layers: Vec<Layer>,
}

/// Image configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImageConfig {
    pub architecture: String,
    pub os: String,
    pub rootfs: RootFS,
    pub config: ContainerConfig,
    pub created: chrono::DateTime<chrono::Utc>,
}

/// Root filesystem info
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RootFS {
    pub typ: String,
    pub diff_ids: Vec<String>,
}

/// Container configuration from image
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContainerConfig {
    pub env: Vec<String>,
    pub cmd: Vec<String>,
    pub entrypoint: Vec<String>,
    pub working_dir: String,
    pub user: String,
    pub exposed_ports: HashMap<String, PortInfo>,
    pub volumes: HashMap<String, VolumeInfo>,
    pub labels: HashMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PortInfo {
    pub protocol: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VolumeInfo {}
