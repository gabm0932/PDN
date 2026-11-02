//! Extension system for PRoot

/// Extension trait for PRoot extensions
pub trait Extension: Send + Sync {
    fn name(&self) -> &str;
    fn init(&mut self) -> anyhow::Result<()>;
    fn handle_syscall_entry(
        &mut self,
        tracee: &mut crate::tracee::Tracee,
        handler: &mut crate::syscall::SyscallHandler,
        translator: &mut crate::path::PathTranslator,
        tracee_mgr: &mut crate::tracee_state::TraceeStateManager,
        vfs_mgr: &mut crate::vfs::VfsManager,
    ) -> anyhow::Result<()>;
    fn handle_syscall_exit(
        &mut self,
        tracee: &mut crate::tracee::Tracee,
        handler: &mut crate::syscall::SyscallHandler,
        translator: &mut crate::path::PathTranslator,
        tracee_mgr: &mut crate::tracee_state::TraceeStateManager,
        vfs_mgr: &mut crate::vfs::VfsManager,
    ) -> anyhow::Result<()>;
    fn on_tracee_create(&mut self, _tracee: &mut crate::tracee::Tracee) -> anyhow::Result<()> {
        Ok(())
    }
    fn on_tracee_exit(&mut self, _tracee: &mut crate::tracee::Tracee) -> anyhow::Result<()> {
        Ok(())
    }
}

/// Extension manager
pub struct ExtensionManager {
    extensions: Vec<Box<dyn Extension>>,
}

impl ExtensionManager {
    pub fn new() -> Self {
        Self {
            extensions: Vec::new(),
        }
    }

    pub fn register(&mut self, extension: Box<dyn Extension>) {
        self.extensions.push(extension);
    }

    pub fn init_all(&mut self) -> anyhow::Result<()> {
        for ext in &mut self.extensions {
            ext.init()?;
        }
        Ok(())
    }

    pub fn handle_syscall_entry_all(
        &mut self,
        tracee: &mut crate::tracee::Tracee,
        handler: &mut crate::syscall::SyscallHandler,
        translator: &mut crate::path::PathTranslator,
        tracee_mgr: &mut crate::tracee_state::TraceeStateManager,
        vfs_mgr: &mut crate::vfs::VfsManager,
    ) -> anyhow::Result<()> {
        for ext in &mut self.extensions {
            ext.handle_syscall_entry(tracee, handler, translator, tracee_mgr, vfs_mgr)?;
        }
        Ok(())
    }

    pub fn handle_syscall_exit_all(
        &mut self,
        tracee: &mut crate::tracee::Tracee,
        handler: &mut crate::syscall::SyscallHandler,
        translator: &mut crate::path::PathTranslator,
        tracee_mgr: &mut crate::tracee_state::TraceeStateManager,
        vfs_mgr: &mut crate::vfs::VfsManager,
    ) -> anyhow::Result<()> {
        for ext in &mut self.extensions {
            ext.handle_syscall_exit(tracee, handler, translator, tracee_mgr, vfs_mgr)?;
        }
        Ok(())
    }
}

/// Built-in extensions
pub mod builtin {
    use super::*;

    /// Path translation extension
    pub struct PathTranslationExtension;

    impl Extension for PathTranslationExtension {
        fn name(&self) -> &str {
            "path_translation"
        }
        fn init(&mut self) -> anyhow::Result<()> {
            Ok(())
        }
        fn handle_syscall_entry(
            &mut self,
            _tracee: &mut crate::tracee::Tracee,
            _handler: &mut crate::syscall::SyscallHandler,
            _translator: &mut crate::path::PathTranslator,
            _tracee_mgr: &mut crate::tracee_state::TraceeStateManager,
            _vfs_mgr: &mut crate::vfs::VfsManager,
        ) -> anyhow::Result<()> {
            Ok(())
        }
        fn handle_syscall_exit(
            &mut self,
            _tracee: &mut crate::tracee::Tracee,
            _handler: &mut crate::syscall::SyscallHandler,
            _translator: &mut crate::path::PathTranslator,
            _tracee_mgr: &mut crate::tracee_state::TraceeStateManager,
            _vfs_mgr: &mut crate::vfs::VfsManager,
        ) -> anyhow::Result<()> {
            Ok(())
        }
    }

    /// VFS extension
    pub struct VFSExtension;

    impl Extension for VFSExtension {
        fn name(&self) -> &str {
            "vfs"
        }
        fn init(&mut self) -> anyhow::Result<()> {
            Ok(())
        }
        fn handle_syscall_entry(
            &mut self,
            _tracee: &mut crate::tracee::Tracee,
            _handler: &mut crate::syscall::SyscallHandler,
            _translator: &mut crate::path::PathTranslator,
            _tracee_mgr: &mut crate::tracee_state::TraceeStateManager,
            _vfs_mgr: &mut crate::vfs::VfsManager,
        ) -> anyhow::Result<()> {
            Ok(())
        }
        fn handle_syscall_exit(
            &mut self,
            _tracee: &mut crate::tracee::Tracee,
            _handler: &mut crate::syscall::SyscallHandler,
            _translator: &mut crate::path::PathTranslator,
            _tracee_mgr: &mut crate::tracee_state::TraceeStateManager,
            _vfs_mgr: &mut crate::vfs::VfsManager,
        ) -> anyhow::Result<()> {
            Ok(())
        }
    }

    /// Seccomp extension
    pub struct SeccompExtension;

    impl Extension for SeccompExtension {
        fn name(&self) -> &str {
            "seccomp"
        }
        fn init(&mut self) -> anyhow::Result<()> {
            Ok(())
        }
        fn handle_syscall_entry(
            &mut self,
            _tracee: &mut crate::tracee::Tracee,
            _handler: &mut crate::syscall::SyscallHandler,
            _translator: &mut crate::path::PathTranslator,
            _tracee_mgr: &mut crate::tracee_state::TraceeStateManager,
            _vfs_mgr: &mut crate::vfs::VfsManager,
        ) -> anyhow::Result<()> {
            Ok(())
        }
        fn handle_syscall_exit(
            &mut self,
            _tracee: &mut crate::tracee::Tracee,
            _handler: &mut crate::syscall::SyscallHandler,
            _translator: &mut crate::path::PathTranslator,
            _tracee_mgr: &mut crate::tracee_state::TraceeStateManager,
            _vfs_mgr: &mut crate::vfs::VfsManager,
        ) -> anyhow::Result<()> {
            Ok(())
        }
    }
}
