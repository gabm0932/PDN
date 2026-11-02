//! Constants for PRoot Distro

/// Default constants
pub mod constants {
    pub const DEFAULT_ROOT_DIR: &str = ".pn-next";
    pub const DEFAULT_CONTAINERS_DIR: &str = "containers";
    pub const DEFAULT_IMAGES_DIR: &str = "images";
    pub const DEFAULT_CACHE_DIR: &str = "cache";
    pub const DEFAULT_LOG_DIR: &str = "logs";
    pub const DEFAULT_CONFIG_DIR: &str = "config";

    pub const DEFAULT_DISTRO: &str = "ubuntu";
    pub const DEFAULT_VERSION: &str = "22.04";
    pub const DEFAULT_ARCH: &str = "aarch64";

    pub const DEFAULT_NETWORK_BRIDGE: &str = "pn0";
    pub const DEFAULT_SUBNET: &str = "10.0.0.0/24";

    pub const DEFAULT_CPU_LIMIT: f64 = 1.0;
    pub const DEFAULT_MEMORY_LIMIT: u64 = 1024 * 1024 * 1024; // 1 GB
    pub const DEFAULT_DISK_LIMIT: u64 = 10 * 1024 * 1024 * 1024; // 10 GB
    pub const DEFAULT_PIDS_LIMIT: u64 = 1000;

    pub const DEFAULT_DNS_SERVERS: &[&str] = &["8.8.8.8", "1.1.1.1"];
    pub const DEFAULT_BRIDGE: &str = "pn0";

    /// Constants container for runtime access
    #[derive(Debug, Clone)]
    pub struct Constants {
        pub root_dir: String,
        pub containers_dir: String,
        pub images_dir: String,
        pub cache_dir: String,
        pub log_dir: String,
        pub config_dir: String,
        pub distro: String,
        pub version: String,
        pub arch: String,
        pub network_bridge: String,
        pub subnet: String,
        pub cpu_limit: f64,
        pub memory_limit: u64,
        pub disk_limit: u64,
        pub pids_limit: u64,
        pub dns_servers: Vec<String>,
        pub bridge: String,
    }

    impl Default for Constants {
        fn default() -> Self {
            Self {
                root_dir: DEFAULT_ROOT_DIR.to_string(),
                containers_dir: DEFAULT_CONTAINERS_DIR.to_string(),
                images_dir: DEFAULT_IMAGES_DIR.to_string(),
                cache_dir: DEFAULT_CACHE_DIR.to_string(),
                log_dir: DEFAULT_LOG_DIR.to_string(),
                config_dir: DEFAULT_CONFIG_DIR.to_string(),
                distro: DEFAULT_DISTRO.to_string(),
                version: DEFAULT_VERSION.to_string(),
                arch: DEFAULT_ARCH.to_string(),
                network_bridge: DEFAULT_NETWORK_BRIDGE.to_string(),
                subnet: DEFAULT_SUBNET.to_string(),
                cpu_limit: DEFAULT_CPU_LIMIT,
                memory_limit: DEFAULT_MEMORY_LIMIT,
                disk_limit: DEFAULT_DISK_LIMIT,
                pids_limit: DEFAULT_PIDS_LIMIT,
                dns_servers: DEFAULT_DNS_SERVERS.iter().map(|s| s.to_string()).collect(),
                bridge: DEFAULT_BRIDGE.to_string(),
            }
        }
    }
}

/// Termux-specific constants
pub mod termux {
    pub const PREFIX: &str = "/data/data/com.termux/files/usr";
    pub const HOME: &str = "/data/data/com.termux/files/home";
    pub const TMP: &str = "/data/data/com.termux/files/usr/tmp";
    pub const PACKAGE: &str = "com.termux";

    pub const STORAGE_PERMISSIONS: &[&str] =
        &["/sdcard", "/storage/emulated/0", "/storage/self/primary"];

    /// Termux constants container for runtime access
    #[derive(Debug, Clone)]
    pub struct TermuxConstants {
        pub prefix: String,
        pub home: String,
        pub tmp: String,
        pub package: String,
        pub storage_permissions: Vec<String>,
    }

    impl Default for TermuxConstants {
        fn default() -> Self {
            Self {
                prefix: PREFIX.to_string(),
                home: HOME.to_string(),
                tmp: TMP.to_string(),
                package: PACKAGE.to_string(),
                storage_permissions: STORAGE_PERMISSIONS.iter().map(|s| s.to_string()).collect(),
            }
        }
    }
}
