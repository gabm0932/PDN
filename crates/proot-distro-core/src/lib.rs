//! PRoot Distro Core - Container management for PRoot

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
pub mod bindings;
pub mod compress;
pub mod config;
pub mod constants;
pub mod container;
pub mod dirfd;
pub mod gpu;
pub mod guestdirs;
pub mod guestfile;
pub mod image;
pub mod install;
pub mod locking;
pub mod login;
pub mod message;
pub mod names;
pub mod network;
pub mod paths;
pub mod progress;
pub mod security;
pub mod session;
pub mod shm;
pub mod signature;
pub mod storage;
pub mod sysdata;
pub mod untar;

pub use bindings::{BindConfig, BindingManager};
pub use compress::{CompressionFormat, Compressor};
pub use config::{
    Config, DistroConfig, DistroType, FileRotationConfig, InstallOptions, LoggingConfig,
    PathsConfig, Repository, Scripts,
};
pub use constants::constants::Constants;
pub use constants::termux::TermuxConstants;
pub use container::{
    BindMount, Container, ContainerConfig, ContainerInfo, ContainerManager, ContainerStatus,
    NetworkConfig, NetworkMode, PortMapping, ResourceLimits, SecurityConfig, SessionInfo,
};
pub use dirfd::DirFd;
pub use gpu::{
    apply_gpu_to_rootfs, detect_gpu_devices, generate_gpu_bind_mounts, GpuConfig, GpuDevice,
    GpuDeviceType,
};
pub use guestfile::GuestFile;
pub use image::{
    Image, ImageHistoryEntry, ImageInspectInfo, ImageManager, ImageSource, RootFSInfo,
};
pub use install::Installer;
pub use locking::LockManager;
pub use login::{LoginManager, LoginOptions};
pub use message::{Message, MessageLevel};
pub use names::NameValidator;
pub use network::{NetworkInfo, NetworkManager};
pub use paths::{ContainerPaths, PathResolver};
pub use progress::ProgressTracker;
pub use security::{
    AppArmorProfile, CapabilityConfig, SeccompAction, SeccompArch, SeccompArg, SeccompOp,
    SeccompProfile, SeccompSyscallRule, SecurityHardeningManager, SecurityProfileConfig,
    SecurityProfileType, SelLinuxProfile,
};
pub use session::{Session, SessionManager, SessionStatus};
pub use shm::ShmManager;
pub use signature::{
    extract_signature_from_annotations, ImageSignature, SignatureConfig, SignatureType,
    SignatureVerifier, TlogEntry, VerificationResult,
};
pub use storage::{StorageManager, VolumeConfig, VolumeInfo};

use anyhow::{bail, Result};

/// Main entry point for PRoot Distro Core
pub struct ProotDistroCore {
    container_manager: ContainerManager,
    image_manager: ImageManager,
    network_manager: NetworkManager,
    storage_manager: StorageManager,
    config_manager: ConfigManager,
    session_manager: SessionManager,
    lock_manager: LockManager,
    shm_manager: ShmManager,
}

impl ProotDistroCore {
    pub fn new() -> Result<Self> {
        pn_trace!("[DEBUG new] Creating ContainerManager");
        let container_manager = ContainerManager::new()?;
        pn_trace!("[DEBUG new] Creating ImageManager");
        let image_manager = ImageManager::new()?;
        pn_trace!("[DEBUG new] Creating NetworkManager");
        let network_manager = NetworkManager::new()?;
        pn_trace!("[DEBUG new] Creating StorageManager");
        let storage_manager = StorageManager::new()?;
        pn_trace!("[DEBUG new] Creating ConfigManager");
        let config_manager = ConfigManager::new()?;
        pn_trace!("[DEBUG new] Creating SessionManager");
        let session_manager = SessionManager::new()?;
        pn_trace!("[DEBUG new] Creating LockManager");
        let lock_manager = LockManager::new()?;
        pn_trace!("[DEBUG new] Creating ShmManager");
        let shm_manager = ShmManager::new()?;

        Ok(Self {
            container_manager,
            image_manager,
            network_manager,
            storage_manager,
            config_manager,
            session_manager,
            lock_manager,
            shm_manager,
        })
    }

    pub async fn install(&mut self, options: InstallOptions) -> Result<()> {
        self.image_manager
            .download_and_verify_source(&options.source)?;
        self.container_manager
            .create(&options.name, &options.config)
            .await?;
        self.shm_manager.create(&options.name, 1024 * 1024)?; // 1MB default
        Ok(())
    }

    pub async fn login(&mut self, options: LoginOptions) -> Result<()> {
        let container = self.container_manager.get(&options.container_name).await?;
        pn_trace!("[DEBUG login] Got container: {}", options.container_name);
        pn_trace!(
            "[DEBUG login] Container rootfs: {:?}",
            container.rootfs_path
        );
        let session = self
            .session_manager
            .create(&options.container_name, &options)?;
        pn_trace!("[DEBUG login] Created session: {}", session.id);
        self.lock_manager.acquire(&options.container_name)?;
        pn_trace!("[DEBUG login] Acquired lock");

        // Execute proot interactively with the provided command
        let command = if options.command.is_empty() {
            vec!["/bin/sh".to_string()]
        } else {
            options.command
        };
        pn_trace!("[DEBUG login] Command: {:?}", command);

        let exit_code = self
            .container_manager
            .execute(&options.container_name, command)
            .await?;
        pn_trace!("[DEBUG login] Exit code: {}", exit_code);
        if exit_code != 0 {
            // Return the code as a typed error so `main` can exit with it
            // instead of collapsing every non-zero code to 1.
            return Err(anyhow::Error::new(proot_core::ChildExit(exit_code)));
        }
        Ok(())
    }

    pub async fn remove(&mut self, name: &str) -> Result<()> {
        self.lock_manager.acquire(name)?;
        self.container_manager.remove(name).await?;
        self.shm_manager.remove(name)?;
        Ok(())
    }

    pub async fn list(&self) -> Result<Vec<ContainerInfo>> {
        self.container_manager.list().await
    }

    pub async fn backup(&self, name: &str, dest: &std::path::Path) -> Result<()> {
        let container = self.container_manager.get(name).await?;
        self.container_manager.backup(&container, dest)
    }

    pub async fn restore(&mut self, name: &str, src: &std::path::Path) -> Result<()> {
        self.container_manager.restore(name, src)
    }

    pub fn network_manager(&mut self) -> &mut NetworkManager {
        &mut self.network_manager
    }

    pub fn storage_manager(&mut self) -> &mut StorageManager {
        &mut self.storage_manager
    }
}

struct ConfigManager {
    // Configuration management
}

impl ConfigManager {
    fn new() -> Result<Self> {
        Ok(Self {})
    }
}

/// Initialize PRoot Distro Core
pub fn init() -> Result<ProotDistroCore> {
    ProotDistroCore::new()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test(flavor = "multi_thread")]
    async fn test_init() {
        // Don't set PN_TEST_ROOT - let it use temp_dir() fallback which is writable
        // std::env::set_var("PN_TEST_ROOT", "/tmp/pn-next-test-init");
        let core = init();
        if let Err(ref e) = core {
            eprintln!("Init error: {}", e);
            // Print full error chain
            let mut source = e.source();
            while let Some(err) = source {
                eprintln!("  Caused by: {}", err);
                source = err.source();
            }
        }
        assert!(core.is_ok());
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_proot_distro_core_new() {
        let core = ProotDistroCore::new();
        assert!(core.is_ok());
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_install_options_default() {
        let options = InstallOptions::default();
        assert!(options.name.is_empty());
        assert!(options.config.name.is_empty());
    }
}
