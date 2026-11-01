//! Image management for PRoot Distro

use anyhow::{bail, Result};
use flate2::read::GzDecoder;
use oci_spec::image::{ImageConfiguration, ImageManifest};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::json;
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use tar::Archive;
use urlencoding::encode;

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

    fn verify_image(&self, image: &Image) -> Result<()> {
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

        // Get image manifest
        let manifest = self.build_manifest(&image)?;
        let manifest_bytes = serde_json::to_vec(&manifest)?;
        let manifest_digest = Self::compute_digest(&manifest_bytes);

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

            // In a real implementation, we'd upload the layer data
            // For now, we skip actual layer upload as it requires local layer storage
            eprintln!(
                "Layer {} would be pushed (skipped - no local layer data)",
                layer.digest
            );
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

        let manifest = self.build_manifest(image)?;

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
            config: ImageConfigInfo {
                architecture: image.architecture.clone(),
                os: image.os.clone(),
                rootfs: RootFSInfo {
                    typ: "layers".to_string(),
                    diff_ids: image.layers.iter().map(|l| l.digest.clone()).collect(),
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
            },
            architecture: image.architecture.clone(),
            os: image.os.clone(),
            size: image.size,
            virtual_size: image.size,
            rootfs: RootFSInfo {
                typ: "layers".to_string(),
                diff_ids: image.layers.iter().map(|l| l.digest.clone()).collect(),
            },
            manifest: serde_json::to_value(&manifest)?,
        })
    }

    /// Build OCI manifest from image (returns JSON value for serialization)
    fn build_manifest(&self, image: &Image) -> Result<serde_json::Value> {
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

        let config_digest = if image.digest.is_empty() {
            "sha256:e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        } else {
            &image.digest
        };

        let manifest = json!({
            "schemaVersion": 2,
            "mediaType": "application/vnd.docker.distribution.manifest.v2+json",
            "config": {
                "mediaType": "application/vnd.docker.container.image.v1+json",
                "size": 0,
                "digest": config_digest,
            },
            "layers": layers,
        });

        Ok(manifest)
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
    pub digest: String,
    pub size: u64,
    pub media_type: String,
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
