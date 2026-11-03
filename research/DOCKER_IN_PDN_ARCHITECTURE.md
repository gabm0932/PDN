# Architecture: Docker Integration in PDN

Status: **Design specification**. Every claim regarding hardware limits and `uml4termux` is derived from empirical benchmarks and cited sources, not conjecture.

---

## 1. Measured Deterministic Constraints

PDN currently executes containers using **PRoot** (`ptrace` chroot emulation). While sufficient for basic filesystems and process execution, it is fundamentally incapable of running Docker. Measured directly on this device (uid 10396, unprivileged):

| Docker Requirement | Measured Outcome on Android | Evidence |
|---|---|---|
| `unshare(CLONE_NEWUSER)` | `EINVAL` | Direct syscall execution |
| `unshare(CLONE_NEWNS/NEWPID/NEWNET)` | `EPERM` | Direct syscall execution |
| `mount(2)` for overlayfs | **Process terminated with SIGSYS (31)** by Android seccomp | Direct syscall execution |
| `CAP_SYS_ADMIN` / `CAP_NET_ADMIN` | `CapEff = CapPrm = CapBnd = 0` | `/proc/self/status` inspection |
| Write access to `/sys/fs/cgroup` | Exists, but `touch` returns `Permission denied` | File write probe |
| Host `dockerd` binary | Absent; only client `docker` v24.0.6 exists | `command -v dockerd` |

PRoot **spoofs** `mount` and `unshare` by returning `rc=0` without performing any underlying VFS virtualization. The consequences are hazardous: `dockerd` appears to start inside a PRoot container, but subsequent container initialization fails completely. Running Docker directly inside PRoot is therefore not merely difficult—it is **architecturally flawed**.

The only viable execution path on Android is running a genuine Linux kernel in user-space: `uml4termux` (User-Mode Linux, arm64). That project has already validated on this identical hardware: `dockerd` 20.10.24 + containerd + runc, `overlay2` storage driver (`Native Overlay Diff: true`, `Backing Filesystem: extfs`), cgroups v2 controllers (`cpu io memory pids`), `docker pull/run/exec/logs`, inter-container networking on `docker0`, port publishing via `gvproxy`, Compose v2.29.7, and BuildKit.

---

## 2. Architectural Decision

**PDN does not reimplement Docker. PDN introduces a polymorphic secondary runtime backend.**

```
                    ┌──────────────────────────────┐
   podnext CLI ────► │  RuntimeBackend (trait)      │
   podnext-daemon   │                              │
                    ├──────────────┬───────────────┤
                    │ ProotBackend │  UmlBackend   │
                    └──────┬───────┴───────┬───────┘
                           │               │
                   ptrace chroot     uml4termuxd (UNIX socket)
                   (no namespaces)    └─ UML kernel → dockerd → containers
```

Rationale:

- **Do NOT** attempt to reimplement Docker: impossible without kernel namespaces; custom builders/overlay emulation on PRoot inevitably crash when running the first container.
- **Do NOT** invoke `buildctl` or `podman` on the host: both require kernel namespaces and cannot run unprivileged on Android.
- **Do NOT** strip `build` from the CLI: developers need local image builds. The proper solution is providing a backend capable of executing them.

Backends are configured per container (via the `runtime` field in `metadata.json`) and can be overridden via CLI (`--runtime uml`). The default remains `proot` for low latency and zero virtualization overhead; `uml` is selected when a workload strictly requires a genuine Linux kernel.

---

## 3. Daemon Wire Protocol (`uml4termuxd`)

`uml4termuxd` communicates via **newline-delimited text** over a UNIX domain socket inside `$UML4TERMUX_RUNTIME`; the daemon responds once and terminates the connection:

| Request | Response |
|---|---|
| `protocol` | `{"protocol":N,"runtime_contract":N,"commands":[...]}` |
| `status` | `{"state":"running\|idle\|completed","child_pid":N,"restarts":N,"kernel":"…","mem":"…","ncpus":"…","uptime_s":N}` |
| `exec <cmd>` | Output wrapped in `__EXEC_<seq>_BEGIN__ … __EXEC_<seq>_DONE_$?` |
| `stop` | `{"state":"stopping"}` |

Protocol Constraints:
- `exec` is **rate-limited** (`{"error":"rate-limited","min_interval_ms":N}`) and **length-bounded** (`{"error":"command too long"}`). Both conditions surface explicit errors.
- Sockets are mode `0600`, verified via `SO_PEERCRED`, and every request is recorded in `audit.log`.
- Clients must complete the `protocol` handshake before dispatching commands.

---

## 4. Subcommand Mapping to Backends

| PDN Subcommand | Proot Backend Behavior | UML Backend Behavior |
|---|---|---|
| `login` / `run` | PRoot ptrace emulation | `exec` into UML guest shell |
| `build` | Rejects `RUN` directives | Executes `DOCKER_BUILDKIT=1 docker build`, then extracts layers via `docker save` |
| `docker` (passthrough) | Rejected with explanatory notice | Direct execution: `exec "docker <args>"` |
| `network connect` | Writes to guest `/etc/resolv.conf` | Native `docker network connect` inside UML guest |
| `volume` | Host directory bind mount | Native `docker volume` with Docker reference counts |
| `quota` | JSON state + `collect_usage` | Mapped to `--cpus`/`--memory` (`cpu.max`/`memory.max` verified: OOM 137 on `--memory 64m`) |

---

## 5. Measured Physical Limits (Even via UML)

The following constraints are hardware and architecture invariants:
- `io.max` cgroup — guest kernel cgroup v2 only exposes `io.stat`; `--device-*-bps` cannot be enforced.
- Swap accounting — disabled in guest kernel.
- Charging tmpfs/page-cache to memory cgroup — `CONFIG_MEMCG_KMEM` disabled.
- Multi-CPU in `ptrace` mode — single CPU; requires `seccomp=on`.
- Inbound forwarding via `passt`/`umnet` — outbound only; supported inbound path is `gvproxy` + `POST /services/forwarder/expose`.
- i386/armv7 native — `CONFIG_COMPAT` disabled; requires `qemu-user-static` inside guest.

---

## 6. Target Code Structure

```rust
// crates/proot-distro-core/src/runtime.rs
pub enum RuntimeKind { Proot, Uml }

pub trait RuntimeBackend: Send + Sync {
    fn kind(&self) -> RuntimeKind;
    fn supports_kernel_features(&self) -> bool;
    async fn exec(&self, container: &str, argv: &[String]) -> Result<ExitStatus>;
    fn status(&self) -> Result<RuntimeStatus>;
}
```

- `ProotBackend`: Migrated directly from current `container.rs::execute`.
- `UmlBackend`: Thin client to `uml4termuxd.sock`: handshakes `protocol`, then dispatches `exec` with rate-limit retries.
- Selection: `metadata.json` -> `"runtime": "proot" | "uml"`; overridden via `--runtime`.
- `podnext docker ...` rejects execution under the `proot` backend with a clear message: `"requires the uml runtime: docker needs namespaces; see 'podnext runtime info'"`.

---

## 6a. Safe Dockerfile Subsets (Without `RUN`)

Deconstructing the Docker image build pipeline reveals that only **command execution** requires namespaces; artifact assembly does not. `RUN` requires `fork` + `exec` inside a mount namespace (`unshare` EPERM, `mount(2)` SIGSYS). The remaining directives (`FROM`, `COPY`, `ADD`, `ENV`, `LABEL`, `WORKDIR`, `USER`, `EXPOSE`, `CMD`, `ENTRYPOINT`) are purely tar operations and JSON manipulation.

**ImageManager Fixes Applied**:
1. `diff_ids` populated with uncompressed tar SHA-256 hashes (not compressed layer digests).
2. Config serialization correctly sets real `size` and computes SHA-256 `digest`.
3. Push operations reject missing blobs prior to dispatching manifests to registries.

---

## 7. Phased Implementation Plan

1. **`RuntimeBackend` + `ProotBackend`**: Pure refactoring; all 128 tests pass with unchanged CLI semantics.
2. **Minimal `UmlBackend`**: Implements `protocol` + `status` + `exec`; verifies `uname -a` returns the UML kernel.
3. **`podnext docker` Passthrough**: Enabled exclusively on the UML backend (`podnext docker run --rm hello-world`).
4. **Production `build`**: Executes `docker build` inside UML guest and exports real OCI layers.
5. **Mapping Subsystems**: Bridges `network`, `volume`, and `quota` to native Docker resources when `backend = UML`.
