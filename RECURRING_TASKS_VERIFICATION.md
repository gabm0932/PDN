# Recurring Tasks Section 4 - Verification Report

**Date**: 2026-11-02
**Workspace**: /data/data/com.termux/files/home/workspace/PDN
**Git HEAD**: 9e778740d41738a37a98543c20ae2b22d207cda4 (master)
**Version**: 0.1.7

---

## 1. Dual-Layer Testing (Host & Container Shell)

### Host Tests ✅
- `cargo test --workspace`: **93 tests passed**, 1 ignored, 0 failed
- `cargo test --package pn-next --features cri`: **49 tests passed**, 0 failed
- Daemon IPC smoke test: Socket responds to `ping` → `{"type":"pong"}`, `list` → `{"type":"ok","data":[]}`

### Container Shell Tests ✅
- podnext binary executes inside container (verified via `podnext login alpine -- podnext --version`)
- All subcommands functional inside container (verified `list`, `service start`, `--help`)
- CLI behavior identical between host and container
- **Limitation**: `cargo test` times out in container due to build duration (120s+)

### Behavioral Differences Documented
| Aspect | Host | Container |
|--------|------|-----------|
| Filesystem access | Full host access | Full host access (no isolation) |
| Network | Host network | Host network (shared) |
| Process view | All host processes | All host processes |
| Binary execution | Native | Native (same binary via bind mount) |

---

## 2. CLI Polish & Command Usage

### Verified ✅
- All 30+ subcommands have `--help` with descriptions
- Required argument validation (exit code 2 for missing args)
- Runtime error handling (exit code 1 for service not found, etc.)
- Output formats: `table`, `json`, `yaml`, `text` via `--format`
- Shell completions: `bash`, `zsh`, `fish`, `powershell`, `elvish`
- Version flag works: `podnext --version` → `0.1.7`

### Stubs / Unimplemented Commands ⚠️
| Command | Status |
|---------|--------|
| `network create/ls/inspect/rm/connect/disconnect` | "not yet implemented" |
| `volume create/ls/inspect/rm/prune` | "not yet implemented" |
| `image push/tag/history/inspect` | "not yet implemented" |
| `image pull` | Works (delegates to install) |

### Missing vs proot-distro ❌
| Feature | proot-distro | PDN |
|---------|-------------|-----|
| `--isolated` mode | ✅ | ❌ |
| `--minimal` mode | ✅ | ❌ |
| `--shared-home` / `--shared-tmp` flags | ✅ | Hardcoded true |
| `--redirect-ports` / `-P` | ✅ | ❌ |
| `--detach` / `-d` | ✅ | ❌ |
| `--get-proot-cmd` | ✅ | ❌ |
| `--architecture` override | ✅ | ❌ |
| `--allow-insecure` registry | ✅ | ❌ |
| `-q` / `--quiet` | ✅ | ❌ |
| `list --image` cached images | ✅ | ❌ |
| `backup --compress` / `--output` | ✅ | ❌ |
| `install` from URL/file/digest | ✅ | ❌ |
| `rename`, `reset`, `copy`, `sync` | ✅ | ❌ |
| `build` (Dockerfile), `push` | ✅ | ❌ |

---

## 3. Container Isolation Verification

### Filesystem Isolation ❌
- **Default**: NO isolation - `shared_home=true`, `shared_tmp=true` in LoginOptions
- Container sees full host filesystem (`/data/data/com.termux/files/home/`)
- No chroot/pivot_root - proot-core `execute()` spawns process directly without ptrace
- Bind mounts from container config only (empty by default)

### Network Isolation ❌
- **No network namespace** - shares host network stack
- Container can bind to host ports (`python3 -c "socket.bind(('0.0.0.0', 18080))"` succeeds)
- `/proc/net/dev` permission denied (Android restriction, not isolation)
- `ip addr show` fails (netlink permission denied)

### IPC Isolation ⚠️
- Separate IPC namespace visible in `/proc/self/ns/ipc` (different inode)
- But `/dev/shm` not accessible in container
- Host semaphores/message queues accessible (same IPC namespace inode as host)

### PID Isolation ❌
- **No PID namespace** - `/proc/self/ns/pid` missing
- `ps aux` shows ALL host processes (Hermes, cargo, postgres, etc.)

### Mount Isolation ❌
- **No mount namespace** - `/proc/self/mounts` shows full host mount table
- All Android mounts visible (/data, /system, /vendor, /apex, etc.)

### UTS Isolation ✅
- Separate UTS namespace visible (different inode in `/proc/self/ns/uts`)
- Hostname can be customized via `--hostname`

### Cgroup Isolation ❌
- **No cgroup isolation** - shares host cgroup hierarchy (`/apps/uid_10396/pid_XXXXX`)
- Resource limits (quota set) not enforced at kernel level

### Privilege Escalation Protection ✅
- **No capabilities** - CapInh/Prm/Eff/Bnd/Amb all `0000000000000000`
- **Seccomp enabled** - Mode 2 (filtering), 1 filter installed
- **No new privileges** - Running as unprivileged user (UID 10396)
- **Rootless** - No root access possible

### Root Cause: proot-core execute() Bypass
The `ProotEngine::execute()` method in `crates/proot-core/src/lib.rs` **does not use ptrace/chroot** - it directly spawns the command via `tokio::process::Command`. The ptrace-based implementation exists in `run()` but is never called. This means:
- No chroot to container rootfs
- No bind mount enforcement
- No syscall interception/translation
- Container processes run natively on host

---

## 4. Upstream Comparison: PDN vs termux proot-distro 5.10.3

### Feature Comparison

| Category | proot-distro 5.10.3 | PDN (podnext) 0.1.7 |
|----------|---------------------|---------------------|
| **Container Management** | | |
| install | ✅ Docker/OCI/URL/file | ✅ Docker/OCI only |
| login | ✅ `--isolated`, `--minimal`, `--shared-*`, `-P`, `-d` | ⚠️ Basic, shared_home/tmp hardcoded |
| list | ✅ containers + images (`-i`) | ✅ containers only |
| run | ✅ entrypoint + port redirect | ✅ basic command |
| remove | ✅ | ✅ |
| rename | ✅ | ❌ |
| reset | ✅ | ❌ |
| backup/restore | ✅ tar + compress | ✅ incremental/differential |
| copy/sync | ✅ | ❌ |
| build (Dockerfile) | ✅ | ❌ |
| push (registry) | ✅ | ❌ |
| **Service Orchestration (v2.0)** | | |
| service start/stop/restart | ❌ | ✅ |
| service logs/status/enable | ❌ | ✅ |
| daemon management | ❌ | ✅ |
| watchdog policies | ❌ | ✅ |
| resource quotas (cgroups v2) | ❌ | ✅ (API only, not enforced) |
| structured logging | ❌ | ✅ |
| D-Bus IPC | ❌ | ✅ (API only) |
| socket activation | ❌ | ✅ (API only) |
| Termux:Boot integration | ❌ | ✅ |
| wake locks | ❌ | ✅ |
| policy engine | ❌ | ✅ |
| **Advanced Features** | | |
| multi-arch build service | ❌ | ✅ (stub) |
| GitOps integration | ❌ | ✅ (stub) |
| web dashboard | ❌ | ✅ (stub) |
| network management | ❌ | ✅ (stub) |
| volume management | ❌ | ✅ (stub) |

### Performance Notes
- proot-distro: Mature, optimized ptrace implementation, actual isolation modes
- PDN: proot-core ptrace implementation incomplete (`execute()` bypasses it), no actual isolation
- Startup time: Both similar (proot overhead)
- PDN daemon adds ~12MB binary overhead

### Compatibility
| Distro | proot-distro | PDN |
|--------|-------------|-----|
| Ubuntu (22.04, 24.04) | ✅ | ✅ |
| Debian (12, stable) | ✅ | ✅ |
| Alpine | ✅ | ✅ |
| Arch | ✅ | ✅ |
| Fedora | ✅ | ✅ |
| ArchLinuxARM | ✅ | ✅ |
| Void Linux | ✅ | ❌ (untested) |
| NixOS | ✅ | ❌ (untested) |
| Custom rootfs tarball | ✅ | ❌ |
| Cross-arch (QEMU) | ✅ | ⚠️ `--qemu` flag only |

### Isolation Comparison
| Isolation Type | proot-distro | PDN |
|----------------|-------------|-----|
| Filesystem (--isolated) | ✅ Real chroot | ❌ None |
| Network (--isolated) | ✅ Namespace | ❌ None |
| PID | ❌ | ❌ |
| IPC | ⚠️ Partial | ⚠️ Partial |
| UTS | ✅ | ✅ |
| Cgroups | ❌ | ❌ (API only) |
| Capabilities | ✅ Dropped | ✅ Dropped |
| Seccomp | ✅ | ✅ |

---

## Summary

| Task | Status | Notes |
|------|--------|-------|
| 1. Dual-layer testing | ✅ DONE | Host tests pass; container CLI verified; cargo test in container times out |
| 2. CLI Polish | ⚠️ PARTIAL | Help/validation/exit codes good; stubs remain; missing proot-distro features |
| 3. Isolation Verification | ✅ DONE | Critical finding: **proot-core execute() bypasses ptrace/chroot - no actual isolation** |
| 4. Upstream Comparison | ✅ DONE | Documented above; PDN has service orchestration but lacks core container isolation |

---

## Critical Blocker

**The proot-core `execute()` method does not implement actual container isolation.** It spawns processes directly on the host without:
- chroot/pivot_root to container rootfs
- ptrace-based syscall interception
- bind mount enforcement
- namespace separation

This affects ALL container operations (login, run, service start). The isolation features (quotas, watchdog, network, volumes) are API-only stubs without kernel enforcement.

**Recommendation**: Complete the ptrace-based `run_loop()` integration in `ProotEngine::execute()` before relying on any isolation guarantees.