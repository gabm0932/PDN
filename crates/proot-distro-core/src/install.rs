//! Installer for PRoot Distro

use crate::config::InstallOptions;
use crate::image::ImageManager;
use anyhow::Result;

/// Installer for PRoot Distro
pub struct Installer {
    image_manager: ImageManager,
    options: InstallOptions,
}

impl Installer {
    pub fn new(options: InstallOptions) -> Result<Self> {
        let image_manager = ImageManager::new()?;
        Ok(Self {
            image_manager,
            options,
        })
    }

    pub fn install(&mut self) -> Result<()> {
        // Validate options
        self.validate()?;

        // Download and verify image
        let image = self
            .image_manager
            .download_and_verify_source(&self.options.source)?;

        // Extract rootfs
        self.extract_rootfs(&image)?;

        // Configure container
        self.configure_container()?;

        // Run post-install scripts
        self.run_post_install()?;

        Ok(())
    }

    fn validate(&self) -> Result<()> {
        if self.options.name.is_empty() {
            return Err(anyhow::anyhow!("Container name cannot be empty"));
        }

        Ok(())
    }

    fn extract_rootfs(&self, _image: &crate::image::Image) -> Result<()> {
        // Extract rootfs from image layers
        // This would extract tar.gz layers to the container rootfs
        Ok(())
    }

    fn configure_container(&self) -> Result<()> {
        // Configure container (fstab, hostname, hosts, etc.)
        Ok(())
    }

    fn run_post_install(&self) -> Result<()> {
        // Run post-install scripts
        Ok(())
    }
}
