# PRoot Distro Next (PDN) - Next Generation Container Platform

[![Rust](https://img.shields.io/badge/rust-1.70%2B-orange.svg)](https://www.rust-lang.org)
[![License](https://img.shields.io/badge/license-GPL--2.0--or--later-blue.svg)](LICENSE)
[![CI/CD](https://img.shields.io/badge/CI%2FCD-100%25%20passing-brightgreen.svg)](https://github.com/michaelamoetoevsky/PDN/actions)
[![Platform](https://img.shields.io/badge/platform-Android%20%7C%20Linux%20aarch64%20%2F%20x86__64-blue.svg)](https://termux.dev)

PRoot Distro Next (PDN / `podnext`) is a next-generation container management and service orchestration platform engineered specifically for Termux on Android 16+ and unprivileged rootless Linux environments.

Built entirely in Rust, PDN unifies the PRoot (C) ecosystem, the `proot-distro` framework, and modern Android runtime compatibility shims into a single high-performance, memory-safe, and leak-proof platform.

---

## 1. Feature Comparison: PodNext vs PRoot-Distro

| Feature / Metric | `proot-distro` (Termux Default) | `podnext` (PRoot Distro Next) | PDN Advantage |
|---|---|---|---|
| **Language & Execution** | Multi-layer Bash / Python script | Native compiled Rust binary | Zero scripting overhead, memory safety |
| **Login Cold-Start Latency** | ~0.603 s | **~0.097 s** | **~5× faster execution** |
| **Container Listing Latency** | ~0.160 s | **~0.008 s** | **~20× faster execution** |
| **Environment Isolation** | Leaks 24 to 163 host env vars (PATH, ANDROID_ROOT, etc.) | **Strictly sanitized (6 essential clean vars)** | Pure container isolation, no host pollution |
| **Guest Root Identity** | Basic permissive emulation | Native `--change-id=0:0` emulation | Flawless `apt`, `dpkg`, and Linux package manager operations |
| **Shared Memory (`/dev/shm`)** | Overwrites host or unmanaged | **Isolated standalone POSIX shared memory (`1777`)** | **Chromium headless & Playwright DOM dump run out of the box** |
| **Exit Code Propagation** | Prone to truncation or collapsing to 1 | **Accurate child process exit code forwarding (`ChildExit`)** | CI/CD pipelines & automation scripts receive real exit codes |
| **File Transfer Security** | Standard tar pipe | **Strict path traversal enforcement (rejects `..`)** | Rejects directory traversal exploits into host rootfs |
| **Filesystem Synchronization** | No built-in hash-based sync | **`sync` with SHA-256 content hashes & `--delete`** | I/O-efficient delta-only directory transfers |
| **Service Supervision** | None (processes die when shell exits) | **Process supervisor, restart policies, watchdog** | Long-running background daemon services |
| **Resource Governance** | None | **cgroups v2 memory & CPU quota controllers** | Prevents mobile device OOM freezes |
| **Android Integration** | Manual | **Termux:Boot autostart, WakeLock engine** | Services survive device sleep states |
| **IPC & Automation** | None | **Standalone D-Bus broker & declarative GitOps manifests** | Production-grade automation on Android |
| **Kernel Docker Support** | Blocked (seccomp SIGSYS on `mount(2)`) | **Polymorphic `RuntimeBackend` architecture (UML bridge)** | Direct path to full Docker via User-Mode Linux VM |

---

## 2. Usage Tiers: From Core Distro to Advanced Orchestration

The command hierarchy in `podnext` is partitioned into three distinct tiers:

```
┌────────────────────────────────────────────────────────────────────────┐
│  TIER 1: SIMPLE & CORE DISTRO (Everyday proot-distro drop-in)          │
│  install · login · run · list · copy · sync · rename · reset · remove  │
├────────────────────────────────────────────────────────────────────────┤
│  TIER 2: INTERMEDIATE (Multi-Container, OCI Images, Volumes & Backups) │
│  image · volume · network · backup · ps · sessions · attach · kill    │
├────────────────────────────────────────────────────────────────────────┤
│  TIER 3: ADVANCED (Service Supervisor, Watchdog, Quota, D-Bus, GitOps) │
│  service · daemon · watchdog · quota · log · dbus · gitops · boot      │
└────────────────────────────────────────────────────────────────────────┘
```

---

### Tier 1: Core Distro Usage (Drop-in Replacement)

For everyday development environments, commands are intuitive and straightforward:

#### 1. Search and Install Distributions
```bash
# View curated official distributions
podnext search

# Search Docker Hub / OCI registries
podnext search debian --limit 5

# Install Debian 13 (Trixie)
podnext install debian:13

# Install Ubuntu with a custom container name
podnext install ubuntu:22.04 --name my-ubuntu
```

#### 2. Interactive Shell & Ad-hoc Execution
```bash
# Enter interactive root shell
podnext login debian

# Run commands directly from the host without spawning an interactive shell
podnext run debian -- apt update && apt install -y curl git
```

#### 3. Inspect Container Status & OS Version
```bash
# Display installed containers with genuine detected distro names and versions
podnext list
```

#### 4. Fast & Secure File Exchange (Copy & Sync)
```bash
# Copy files from container to host (traversal-safe)
podnext copy debian:/etc/hosts ./hosts.bak

# Recursively copy directory from host into container
podnext copy ./my-project debian:/root/my-project -R

# Sync directory using SHA-256 delta hashes with deletion propagation
podnext sync ./my-project debian:/root/my-project --delete
```

#### 5. Rename, Reset, or Remove Containers
```bash
# Rename an installed container
podnext rename debian deb-prod

# Reset container back to its original clean OCI state
podnext reset deb-prod

# Remove container and permanently clean up storage
podnext remove deb-prod --force
```

---

### Tier 2: Intermediate Storage & Backup (Multi-Container)

For advanced storage management, backup snapshots, and multi-container environments:

#### 1. Local OCI Image Cache Management
```bash
# List locally cached OCI images
podnext image ls

# Pull an image without immediately spinning up a container
podnext image pull alpine:latest
```

#### 2. Persistent Storage Volumes
```bash
# Create a named persistent volume
podnext volume create postgres-data

# List existing volumes
podnext volume ls

# Prune unused dangling volumes
podnext volume prune
```

#### 3. Isolated Bridge Networks
```bash
# Create an isolated bridge network
podnext network create internal-net

# List active container networks
podnext network ls
```

#### 4. Backup Snapshots & Restoration
```bash
# Create a full compressed snapshot archive of a container
podnext backup create deb-prod

# View available snapshot archives
podnext backup list

# Restore a snapshot to a new container
podnext backup restore <backup-id> --target deb-restored
```

---

### Tier 3: Advanced Systems (Supervision, Daemons & IPC)

For production services, mobile boot persistence, resource limits, and GitOps workflows:

#### 1. Background Supervisor Daemon
```bash
# Start background supervisor daemon
podnext daemon start

# Check supervisor daemon status
podnext daemon status
```

#### 2. Managed Services & Lifecycle
```bash
# Start a supervised background service with automatic restart policy
podnext service start web-server

# Inspect service state and process PID
podnext service status web-server

# View live service logs
podnext service logs web-server
```

#### 3. Resource Quotas (cgroups v2)
```bash
# Limit maximum memory usage to 512MB
podnext quota set web-server memory 512M

# Restrict CPU utilization (2 cores = 200%)
podnext quota set web-server cpu 200

# Monitor real-time CPU and RAM utilization
podnext quota usage web-server
```

#### 4. Watchdog & Circuit Breakers
```bash
# Configure automatic restart policy: up to 3 retries within 60 seconds
podnext watchdog policy set web-server OnFailure 3 60

# Inspect circuit breaker status
podnext watchdog status
```

#### 5. D-Bus IPC Broker & Declarative GitOps
```bash
# Run standalone D-Bus broker on a UNIX domain socket
podnext dbus serve --socket $TMPDIR/pdn-dbus.sock

# Call service methods over D-Bus
podnext dbus call org.pnnext.Service /org/pnnext/Service org.pnnext.Service.List --socket $TMPDIR/pdn-dbus.sock

# Synchronize declarative container & volume manifests via GitOps
podnext gitops sync --repo ./manifests

# Detect configuration drift
podnext gitops drift
```

#### 6. Android Keep-Alive & WakeLock
```bash
# Acquire Android Partial WakeLock to prevent container suspension during screen-off
podnext wakelock acquire web-server Partial

# Enable boot autostart via Termux:Boot
podnext boot enable web-server
```

---

## 3. Browser & Automation Compatibility: Playwright on Android

PDN has been verified to execute modern headless web browsers inside unprivileged rootless containers due to standalone `/dev/shm` provisioning (POSIX shared memory mode `1777`):

```bash
# Inside a PDN Debian container:
apt update && apt install -y chromium chromium-sandbox
chromium --headless=new --no-sandbox --disable-gpu --dump-dom "https://example.com"
```
*Result: Clean HTML DOM extraction without shared memory denial errors (`EACCES` / `ENOSPC`).*

---

## 4. Verification Status & CI/CD Pipeline

The GitHub Actions automated verification matrix is **100% Passing (Green)**:

| Workflow | Scope & Test Matrix | Status |
|---|---|---|
| **PDN CI/CD** | Host Build, Integration Test Suite (133 tests passed), Cross-Compile aarch64, Fuzzing, Docs mdBook, Security Audit | **SUCCESS** |
| **Android Cross-Compile** | Matrix: `aarch64-linux-android` & `x86_64-linux-android` with NDK `r27c` & `r30` | **SUCCESS** |
| **Benchmark Suite** | Performance & latency regression benchmarks | **SUCCESS** |

---

## 5. Crates Architecture

PDN is organized into 6 modular Rust crates:

| Crate | Responsibility |
|---|---|
| `proot-core` | Core PRoot engine: ptrace syscall tracing, VFS routing, path resolution, seccomp BPF filters. |
| `proot-distro-core` | Container lifecycle, SELinux-safe layer unpacking, OCI digest verification, `/dev/shm` management, OCI manifest handling. |
| `proot-compat` | Android compatibility layer: platform personality shims, mount helpers, cgroup v2 shims, sysfs stubs. |
| `pn-gateway` | Fail-safe supervisor, container guardrails, health monitors, Prometheus metrics. |
| `pn-next` | Advanced runtime orchestration: scheduler, bridge networking, volume drivers, D-Bus broker, supervisor service, GitOps engine. |
| `podnext` | Unified CLI frontend and supervisor daemon binaries (`podnext`, `podnext-daemon`). |

---

## 6. References & Further Reading

- [Migration Guide from proot-distro](docs/src/migration-guide.md)
- [Complete CLI Reference](docs/src/cli-reference.md)
- [Architectural Analysis & Roadmap](docs/ARCHITECTURAL_ANALYSIS_AND_ROADMAP.md)
- [Internal Capabilities & Testing Suite](research/INTERNAL_CAPABILITY_TESTS_RUN2.md)
- [Docker Runtime Integration Blueprint via UML](research/DOCKER_RUNTIME_INTEGRATION_PLAN.md)

---

## License

GPL-2.0-or-later.
