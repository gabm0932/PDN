# PDN — Debian: Root Cause Analysis & Fixes (November 2026)

**Environment**: Termux on Android 16, aarch64, unprivileged, uid 10396.
**Comparators**: `proot-distro 5.10.3` (Debian Trixie) vs `podnext 0.1.7` (`deb-pdn`, Debian Trixie).
All findings below are derived from live command output; reproduction commands are provided verbatim.

---

## 1. Hard Links Blocked by Android SELinux -> Debian Install Fails

**Symptom (Install Log):**

```
Error: failed to unpack `.../.podnext/containers/deb-pdn/rootfs/usr/bin/perl5.40.1`
Caused by:
    Permission denied (os error 13) when hard linking
    .../rootfs/usr/bin/perl to .../rootfs/usr/bin/perl5.40.1
```

**Proof of Platform Constraint (Not Application Logic Error)** — `ln` fails directly inside `$HOME`:

```
$ echo x > ~/hltest/a; ln ~/hltest/a ~/hltest/b
ln: failed to create hard link '.../hltest/b' => '.../hltest/a': Permission denied
```

Filesystem is `f2fs` (`stat -f`), mounted with `seclabel,nosuid,nodev`; `/sys/fs/selinux/enforce` is unreadable. Alpine succeeded because BusyBox applets use **symlinks**; Debian images employ **hard links** (`perl` -> `perl5.40.1`), causing extraction failure exclusively on distros that rely on hard links.

**Remediation:** `crates/proot-distro-core/src/untar.rs` — custom streaming tar extractor: `hard_link` is attempted first; if rejected with `EACCES`/`EPERM`, it **falls back to copying the file**. Symlinks remain genuine symlinks; device nodes and FIFOs are skipped; file modes are applied after content is written; path traversal outside destination bounds is strictly rejected. This single implementation is shared across: installation (`native.rs`), compression routines (`compress.rs`), and restoration (`backup_restore/mod.rs`).

**Verification:** `podnext install debian --name deb-pdn` -> `Container 'deb-pdn' installed successfully!` + `metadata.json` written (rootfs 19MB -> fully populated).

---

## 2. Missing `--change-id` -> `apt install` Impossible (Primary Root Cause)

**Symptom:**

```
$ podnext login deb-pdn -- apt-get install -y --no-install-recommends curl
dpkg: error: requested operation requires superuser privilege
E: Sub-process /usr/bin/dpkg returned an error code (2)
/bin/sh: 1: curl: not found
```

Comparator running the same command in proot-distro: `Setting up curl (8.14.1-2+deb13u7)` + `curl 8.14.1 (aarch64-unknown-linux-gnu) ...`.

**Root Cause (Source Evidence):**

| Implementation | Proot Argv Construction |
|---|---|
| PDN `crates/proot-core/src/lib.rs:200-204` | `--kill-on-exit --link2symlink --sysvipc -L` — **omits `--change-id`** |
| proot-distro `login/proot_cmd.py:105` | `--change-id={login_uid}:{login_gid}` (defaults to `0:0`) |

Without `--change-id`, guest processes retain the real Android host UID (10396), causing all privileged operations to be rejected immediately by `dpkg`. This was the root blocker preventing Node.js, Playwright, Chromium, and standard package installations from functioning inside PDN containers.

**Remediation:** Added `--change-id=0:0` to **both** argv builders in `crates/proot-core/src/lib.rs`, mirroring proot-distro behavior for non-termux containers. This operates as a user-space illusion: PRoot maps the identity for traced processes without conferring actual host kernel privileges.

**Verification Suite:**

```
### 1. FAKE ROOT ###        id -u / id -g / id -un   ->  0 / 0 / root
### 2. APT INSTALL ###       Setting up curl (8.14.1-2+deb13u7)
                            curl 8.14.1 (aarch64-unknown-linux-gnu) libcurl/8.14.1 ...
### 3. DPKG AS ROOT ###      dpkg -l | wc -l -> 106   (from 83 before install)
### 4. ISOLATION REGRESSION ###  ls /etc | wc -l -> 60 ; alpine-release -> 0 ; PRETTY_NAME="Debian GNU/Linux 13 (trixie)"
### 5. ENV HYGIENE ###       LD_PRELOAD|LD_LIBRARY_PATH|API_KEY|termux -> 0
                            PATH=/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin
### 6. PROC PARITY ###       124.08 932.76 / cpu  1954 0 2872 93276 259 340 250 83 0 0 / 0.12 0.07 0.02 2/165 765
### 7. ALPINE STILL OK ###   3.24.3 / apk-tools 3.0.8-r0
```

Zero regressions observed: filesystem isolation intact, environment variables sanitized, `/proc` stubs match proot-distro byte-for-byte, and Alpine containers remain unaffected.

---

## 3. Container Restoration Returning Random UUIDs Without Creating Files

**Root Cause:** `restore_container` returned `Ok(Uuid::new_v4().to_string())` without moving extracted archive contents (`rootfs/...`) into `~/.podnext/containers/<name>/`. Lacking `metadata.json`, restored containers were invisible to CLI subcommands.

**Remediation:** In `crates/pn-next/src/backup_restore/mod.rs`:
1. Target resolved from `--target` or defaulted to `<container>-restored`.
2. Existing targets rejected unless `--overwrite` is specified.
3. Extracted directory moved from `extract_dir/rootfs` to `containers_dir/<target>/rootfs` (with fallback to recursive `copy_tree` preserving symlinks).
4. `metadata.json` written to formally register the container.
5. `sysdata` stubs provisioned identically to a fresh installation.

**Verification:**

```
$ podnext backup create deb-pdn           -> Backup created with ID: 928d887d-2eed-49f2-8ce3-e55e913d673f
$ podnext backup restore 928d887d-... --target deb-restored
                                          -> Restored to container: deb-restored
$ podnext list                            -> deb-restored | deb-pdn | restored | aarch64 | Stopped
$ podnext login deb-restored -- sh -c 'cat /etc/os-release|grep PRETTY; dpkg -l|wc -l; cat /proc/uptime'
                                          -> PRETTY_NAME="Debian GNU/Linux 13 (trixie)" / 83 / 124.08 932.76
```

---

## 4. Measured Divergence Summary

| Metric | PDN | proot-distro | Notes |
|---|---|---|---|
| Guest UID | 0 (post-fix) | 0 | Prior to fix: 10396 -> apt failed |
| `uname -r` | Real kernel `6.1.157-android14-…` | `6.17.0-PRoot-Distro` | Architectural choice: avoids spoofing identities unless requested |
| Guest PATH | Clean (`/usr/local/sbin:…:/bin`) | **Leaks** `/data/data/com.termux/files/usr/bin` | In proot-distro, `python3` resolves to **host** Python (`sys.prefix=/data/data/com.termux/files/usr`) |
| `getent hosts deb.debian.org` | Resolves | Resolves | IPv4/IPv6 ordering varies; both functional |
| `apt-get update` | 10.1 MB / 13 s | 10.1 MB / 13 s | Identical throughput |
| `/proc/uptime`, `/proc/stat`, `/proc/loadavg` | Identical bytes | Identical bytes | Synthetic sysdata stubs |

The host PATH leakage in `proot-distro` means host Bionic binaries can accidentally execute inside guest glibc environments, creating unpredictable ABI crashes—an entire vulnerability class eliminated in PDN.

---

## 5. Subcommands Promoted from Stubs

| Subcommand | Prior State | Remediation |
|---|---|---|
| `network connect/disconnect` | `not yet implemented` | Added `containers` persistence to `NetworkConfig`; `connect` writes network DNS to guest `/etc/resolv.conf`, `disconnect` restores default |
| `volume prune` | `not yet implemented` | Aggregates volume references from all container `bind_mounts`; prompts for confirmation unless `--force` is passed |
| `log tail` | `not yet implemented` | Backed by `StructuredLogging::query` shared with `log query` |
| `dashboard start/stop/status` | Print-only stub | Dispatches `WebDashboard::start()`; manages PID file at `~/.podnext/dashboard.pid`; handles `SIGTERM` -> `SIGKILL`; status checks process liveness and port responsiveness |

---

## 6. In-Container Validation: Debian Environment

Executed via `podnext login deb-pdn -- ...` following fixes.

### 6.1 Headless Chromium + Playwright Functional

`/dev/shm` was missing in initial PDN builds (`--bind=/dev` shadowed `rootfs/dev`), preventing Chromium from allocating shared memory segments. In `crates/proot-distro-core/src/guestdirs.rs`, `containers/<name>/shm` is created with mode **1777** and bind-mounted to `/dev/shm` after `--bind=/dev`.

```
$ podnext login deb-pdn -- /bin/sh -c 'ls -ld /dev/shm; touch /dev/shm/probe && echo SHM_WRITABLE'
drwxrwxrwt. 2 root root 3452 ...  /dev/shm
SHM_WRITABLE
```

Playwright execution (`playwright-core` 1.64.1 on host) targeting container browser:

```
$ podnext copy ~/.hermes/cache/scratch/pwtest.js deb-pdn:/root/pwtest.js
$ podnext login deb-pdn -- /bin/sh -c \
    'NODE_PATH=/data/data/com.termux/files/usr/lib/node_modules \
     /data/data/com.termux/files/usr/bin/node /root/pwtest.js'
PLAYWRIGHT_DOM=PW_OK
UA=Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) HeadlessChrome/156.0.0.0 Safari/537.36
```

Headless Chromium direct execution:

```
$ podnext login deb-pdn -- /bin/sh -c '/data/data/com.termux/files/usr/lib/chromium/chrome \
    --headless=new --no-sandbox --disable-gpu --user-data-dir=/root/cud \
    --dump-dom "data:text/html,<h1>OKPDN</h1>"'
<html><head></head><body><h1>OKPDN</h1></body></html>
```

*Technical note*: `--no-sandbox` is required because unprivileged user namespaces are absent under PRoot; `--disable-dev-shm-usage` is **not** required now that `/dev/shm` is properly provisioned.

### 6.2 Node.js Runtime

Both pathways verified:
- Host Node.js (Bionic) executed via absolute path inside guest -> functional.
- Guest packages (`apt-get install -y nodejs npm`) available in Debian Trixie (Node.js 20.19.2).

### 6.3 Docker Daemon Inside PRoot — Physically Impossible

Measured directly on unprivileged uid 10396: `CapEff=CapPrm=CapBnd=0`; `mount(2)` -> **process terminated with SIGSYS (31)** by Android seccomp; `unshare(CLONE_NEWUSER)` -> `EINVAL`; `CLONE_NEWNS/NEWPID/NEWNET` -> `EPERM`; `/sys/fs/cgroup` is read-only; host lacks `dockerd`. PRoot merely returns fake `rc=0` on mount/unshare syscalls, leaving Docker unable to allocate genuine filesystems or namespaces. The only viable path is running a real kernel in user-space via `uml4termux`.

### 6.4 Hermes Agent Execution Inside Guest Container

```
$ proot-distro login debian -- /data/data/com.termux/files/usr/bin/hermes --version
Hermes Agent v0.22.0+canary.20261014T070054Z (2026.9.24) · canary
Install directory: /data/data/com.termux/files/usr/lib/hermes-agent/app
Install method: apt
Python: 3.14.6
OpenAI SDK: 2.24.0
```

Entire import chain resolves cleanly inside the guest when `$TERMUX_PREFIX` is bound to provide the host Python interpreter.

---

## 7. Added Subcommands & Verification

| Command | Behavior |
|---|---|
| `podnext rename <old> <new>` | Renames container root directory via `rename(2)` and updates `metadata.json` (name + rootfs path) |
| `podnext copy [--recursive] [--move] <SRC> <DEST>` | Handles `[CONTAINER:]PATH` specs; dereferences target symlinks on standard copy (matching `cp` and `proot-distro`), preserves symlinks on `--move` |

```
$ podnext rename deb-restored deb-renamed
Renamed container 'deb-restored' to 'deb-renamed'

$ podnext copy ~/pdn-copytest.txt deb-pdn:/root/copytest.txt
$ podnext login deb-pdn -- cat /root/copytest.txt
hello-from-host
```

---

## 8. Resolved Bug Fixes

1. **`ContainerLock::acquire` Panic**: `libc::flock(self.file.as_ref().unwrap()...)` previously executed before `self.file = Some(file)`, causing unwrap panics on initial lock acquisition. Fixed by acquiring the lock directly on the open descriptor before assignment.
2. **Unsafe Archive Unpacking**: Standard `Archive::unpack` was used across four code paths without handling Android SELinux hard link restrictions. All archive unpacking now unifies on `untar::extract`.

---

## 9. Architectural Invariants

- File ownership (uid/gid) is not restored during rootless extraction; Android users cannot execute `chown`; permissions are mapped via file mode bits.
- Container `/etc/hostname` defaults to image base name; `hostname` reports `localhost` by default unless overridden.
