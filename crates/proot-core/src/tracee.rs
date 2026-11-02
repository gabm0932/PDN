//! Tracee management for PRoot

use crate::arch::Arch;
use crate::path::PathTranslator;
use crate::syscall::SyscallHandler;
use crate::tracee_state::TraceeStateManager;
use crate::vfs::VfsManager;
use anyhow::Result;
use nix::unistd::Pid;

/// Unique tracee identifier
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TraceeId(pub nix::unistd::Pid);

impl From<nix::unistd::Pid> for TraceeId {
    fn from(pid: nix::unistd::Pid) -> Self {
        TraceeId(pid)
    }
}

/// Tracee state
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TraceeState {
    Running,
    Stopped,
    SyscallEntry,
    SyscallExit,
    Signaled,
    Exited,
    Zombie,
}

/// Tracee representation
#[derive(Debug, Clone)]
pub struct Tracee {
    pub pid: nix::unistd::Pid,
    pub state: TraceeState,
    pub registers: Registers,
    pub syscall_info: Option<SyscallInfo>,
    pub path_info: PathInfo,
    pub vfs_context: VfsContext,
    pub arch: Arch,
    pub parent: Option<Pid>,
    pub children: Vec<Pid>,
    pub fd_table: FdTable,
    pub signal_mask: u64,
    pub ptrace_options: u32,
}

impl Tracee {
    pub fn new(pid: Pid, arch: Arch) -> Self {
        Self {
            pid,
            state: TraceeState::Running,
            registers: Registers::default(),
            syscall_info: None,
            path_info: PathInfo::default(),
            vfs_context: VfsContext::default(),
            arch,
            parent: None,
            children: Vec::new(),
            fd_table: FdTable::new(),
            signal_mask: 0,
            ptrace_options: 0,
        }
    }

    pub fn handle_syscall_entry(
        &mut self,
        _handler: &mut crate::syscall::SyscallHandler,
        translator: &mut crate::path::PathTranslator,
        _tracee_mgr: &mut crate::tracee_state::TraceeStateManager,
        _vfs_mgr: &mut crate::vfs::VfsManager,
    ) -> Result<()> {
        let syscall = self.read_syscall()?;
        self.syscall_info = Some(SyscallInfo {
            number: syscall.number,
            args: syscall.args,
            retval: 0,
        });

        translator.translate_syscall_args(
            &mut self.syscall_info.as_mut().unwrap().args,
            &self.path_info,
        )?;

        Ok(())
    }

    pub fn handle_syscall_exit(
        &mut self,
        _handler: &mut crate::syscall::SyscallHandler,
        _translator: &mut crate::path::PathTranslator,
        _tracee_mgr: &mut crate::tracee_state::TraceeStateManager,
        _vfs_mgr: &mut crate::vfs::VfsManager,
    ) -> Result<()> {
        let retval = self.read_retval()?;
        if let Some(syscall_info) = &mut self.syscall_info {
            syscall_info.retval = retval;
        }
        self.syscall_info = None;
        Ok(())
    }

    fn read_syscall(&self) -> Result<RawSyscall> {
        // Architecture-specific syscall reading
        Ok(RawSyscall {
            number: 0,
            args: [0; 6],
        })
    }

    fn read_retval(&self) -> Result<isize> {
        Ok(0)
    }

    pub fn handle_event(
        &mut self,
        event: crate::ptrace::PtraceEvent,
        syscall_handler: &mut SyscallHandler,
        path_translator: &mut PathTranslator,
        tracee_manager: &mut TraceeStateManager,
        vfs_manager: &mut VfsManager,
    ) -> Result<()> {
        match event {
            crate::ptrace::PtraceEvent::SyscallEntry => {
                self.handle_syscall_entry(
                    syscall_handler,
                    path_translator,
                    tracee_manager,
                    vfs_manager,
                )?;
            }
            crate::ptrace::PtraceEvent::SyscallExit => {
                self.handle_syscall_exit(
                    syscall_handler,
                    path_translator,
                    tracee_manager,
                    vfs_manager,
                )?;
            }
            crate::ptrace::PtraceEvent::Signal(sig) => {
                self.handle_signal(sig)?;
            }
            crate::ptrace::PtraceEvent::Clone
            | crate::ptrace::PtraceEvent::Fork
            | crate::ptrace::PtraceEvent::Vfork => {
                self.handle_clone()?;
            }
            crate::ptrace::PtraceEvent::Exec => {
                self.handle_exec()?;
            }
            crate::ptrace::PtraceEvent::Seccomp => {
                self.handle_seccomp()?;
            }
            crate::ptrace::PtraceEvent::Exit => {
                self.state = TraceeState::Exited;
            }
            _ => {}
        }
        Ok(())
    }

    fn handle_signal(&mut self, _sig: nix::sys::signal::Signal) -> Result<()> {
        self.state = TraceeState::Signaled;
        Ok(())
    }

    fn handle_clone(&mut self) -> Result<()> {
        // New tracee will be picked up by waitpid
        Ok(())
    }

    fn handle_exec(&mut self) -> Result<()> {
        // Reset path info on exec
        self.path_info = PathInfo::default();
        Ok(())
    }

    fn handle_seccomp(&mut self) -> Result<()> {
        // Handle seccomp trap
        Ok(())
    }
}

/// Raw syscall data
#[derive(Debug, Clone, Default)]
pub struct RawSyscall {
    pub number: i64,
    pub args: [u64; 6],
}

/// Syscall information
#[derive(Debug, Clone, Default)]
pub struct SyscallInfo {
    pub number: i64,
    pub args: [u64; 6],
    pub retval: isize,
}

/// Register state
#[derive(Debug, Clone, Default)]
pub struct Registers {
    pub regs: [u64; 31],
    pub pc: u64,
    pub sp: u64,
    pub pstate: u64,
}

/// Path information for tracee
#[derive(Debug, Clone, Default)]
pub struct PathInfo {
    pub cwd: String,
    pub root: String,
    pub mounts: Vec<MountInfo>,
}

/// Mount information
#[derive(Debug, Clone, Default)]
pub struct MountInfo {
    pub source: String,
    pub target: String,
    pub fstype: String,
    pub flags: u64,
}

/// VFS context for tracee
#[derive(Debug, Clone, Default)]
pub struct VfsContext {
    pub open_files: Vec<OpenFile>,
    pub mount_namespace: u64,
}

/// Open file entry
#[derive(Debug, Clone, Default)]
pub struct OpenFile {
    pub fd: i32,
    pub path: String,
    pub flags: i32,
    pub mode: u32,
}

/// File descriptor table
#[derive(Debug, Clone, Default)]
pub struct FdTable {
    pub entries: Vec<Option<FdEntry>>,
}

impl FdTable {
    pub fn new() -> Self {
        Self {
            entries: Vec::new(),
        }
    }

    pub fn allocate(&mut self, entry: FdEntry) -> i32 {
        for (i, entry_opt) in self.entries.iter_mut().enumerate() {
            if entry_opt.is_none() {
                *entry_opt = Some(entry);
                return i as i32;
            }
        }
        self.entries.push(Some(entry));
        (self.entries.len() - 1) as i32
    }

    pub fn get(&self, fd: i32) -> Option<&FdEntry> {
        self.entries.get(fd as usize).and_then(|e| e.as_ref())
    }

    pub fn get_mut(&mut self, fd: i32) -> Option<&mut FdEntry> {
        self.entries.get_mut(fd as usize).and_then(|e| e.as_mut())
    }

    pub fn close(&mut self, fd: i32) -> Option<FdEntry> {
        if let Some(entry) = self.entries.get_mut(fd as usize) {
            entry.take()
        } else {
            None
        }
    }
}

/// File descriptor entry
#[derive(Debug, Clone, Default)]
pub struct FdEntry {
    pub path: String,
    pub flags: i32,
    pub mode: u32,
    pub offset: u64,
    pub flags_open: i32,
}
