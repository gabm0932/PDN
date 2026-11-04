# Filesystem Fault Injection & Resilience Report: podnext (v0.1.7)

Target binary: `/data/data/com.termux/files/usr/bin/podnext`
Platform: Termux Android 16 (API 36), aarch64, Linux 6.1.157
Harness script: `fs_fault_injection.py`

---

## 1. Corrupted Container Metadata (`metadata.json`)

| Fault Injected | CLI Operation | Exit Code | Result / Error Output | Analysis |
|---|---|---|---|---|
| Truncated JSON | `podnext list` | 0 | Listed normally (fallback os-release) | `ContainerManager::load_containers` skips bad JSON quietly, inspects `/etc/os-release`. |
| Truncated JSON | `podnext run ...` | 0 | Executed `alive` | Metadata not strictly required for execution if rootfs valid. |
| Truncated JSON | `podnext reset ...` | 1 | `Error: EOF while parsing a value at line 1 column 55` | `cmd_reset` reads `metadata.json` directly without fallback or repair. |
| Truncated JSON | `podnext rename ...` | 0 | Renamed directory | Directory renamed; metadata update skipped silently. |
| Invalid Types | `podnext list` | 0 | Listed normally | Parsed types invalid; ignored by loader. |
| Invalid Types | `podnext run ...` | 0 | Executed `alive` | Runtime unaffected. |
| Invalid Types | `podnext reset ...` | 1 | `Error: Container 'fault-box' has no recorded source image...` | Cannot parse string `image`; reset fails. |
| Missing Keys (`{}`) | `podnext list` | 0 | Listed normally | Detects distro via rootfs `/etc/os-release`. |
| Missing Keys (`{}`) | `podnext reset ...` | 1 | `Error: Container 'fault-box' has no recorded source image...` | Field `image` empty; reset refuses. |
| Missing `image`/`tag` | `podnext reset ...` | 1 | `Error: Container 'fault-box' has no recorded source image...` | Reset blocked. |
| Random Binary Bytes | `podnext list` | 0 | Listed normally | Loader silently handles non-UTF8 read/parse failure. |
| Random Binary Bytes | `podnext run ...` | 0 | Executed `alive` | Runtime unaffected. |
| Random Binary Bytes | `podnext reset ...` | 1 | `Error: stream did not contain valid UTF-8` | `serde_json` fails on invalid byte stream. |
| Directory as `metadata.json` | `podnext reset ...` | 1 | `Error: Is a directory (os error 21)` | Unhandled filesystem type mismatch. |
| Symlink Loop on `metadata.json` | `podnext reset ...` | 1 | `Error: Too many symbolic links encountered (os error 40)` | OS errno 40 surfaced cleanly via anyhow. |

---

## 2. Degraded Rootfs

| Fault Injected | CLI Operation | Exit Code | Result / Error Output | Analysis |
|---|---|---|---|---|
| Missing `/bin/sh` | `podnext run <c> -- /bin/echo alive` | 1 | `proot error: '/bin/sh' not found (root = ..., cwd = /, $PATH=(null))` | All commands in `proot-core` wrapped with `/bin/sh -c`; missing shell blocks direct execution. |
| Missing `/bin/sh` | `podnext run <c>` | 1 | `proot error: '/bin/sh' not found ...` | Default shell command cannot execute. |
| Missing `/bin/sh` | `podnext login <c>` | 1 | `proot error: '/bin/sh' not found ...` | Interactive login shell fails cleanly. |
| Missing `/bin/sh` | `podnext reset <c>` | 0 | Container reinstalled from source image | Reset succeeds; wiped bad rootfs and restored `/bin/sh`. |
| `chmod 0000 rootfs` | `podnext list` | 0 | Container hidden from list | `is_valid_rootfs` check (`etc`/`bin`/`usr` existence) fails quietly. |
| `chmod 0000 rootfs` | `podnext run <c>` | 1 | `Error: Container 'fault-box' not found` | Loader ignores container entirely. |
| `chmod 0000 rootfs` | `podnext reset <c>` | 1 | `Error: Container 'fault-box' not found` | Cannot reset container because loader doesn't register it. |
| `chmod 0000 rootfs` | `podnext remove <c> -f` | 1 | `Error: Container 'fault-box' not found` | Cannot remove container via CLI; orphaned directory. |
| `chmod 0000 rootfs` | `podnext install <image> --name <c>` | 1 | `Error: Container '<c>' already exists at .../rootfs. Use --force to overwrite.` | Zombie state: install detects path, but loader ignores it. |
| `chmod 0000 rootfs` | `podnext install --force` | 1 | `Error: Permission denied (os error 13)` | Unpack fails; cannot write into 0000 directory. |
| Read-only rootfs (`chmod 0555 rootfs`) | `podnext run <c> -- touch /test` | 1 | `touch: /test: Permission denied` | Guest permissions respected by PRoot. |
| Read-only rootfs (`chmod 0555 rootfs`) | `podnext run <c> -- touch /tmp/w` | 0 | File created | Writable subdirectories remain usable. |

---

## 3. Broken Symlink Loops & Dangling Symlinks

| Fault Injected | CLI Operation | Exit Code | Result / Error Output | Analysis |
|---|---|---|---|---|
| Circular Loop (`/a -> /b -> /a`) | `podnext run <c> -- /bin/ls -l /a` | 0 | `lrwxrwxrwx 1 root root 7 /a -> /b` | `ls -l` inspects link without dereferencing target. |
| Circular Loop (`/a -> /b -> /a`) | `podnext run <c> -- /bin/cat /a` | 1 | `cat: can't open '/a': Symbolic link loop` | PRoot/guest kernel surfaces `ELOOP` (errno 40). |
| Circular Loop (`/a -> /b -> /a`) | `podnext copy <c>:/a /tmp/dst` | 1 | `Error: Source '<c>:/a' does not exist` | Absolute guest link `/b` interpreted as host path; missing on host. |
| Circular Loop (relative `a -> b -> a`) | `podnext copy <c>:/a $SCRATCH/dst` | 0 | `Copied <c>:/a -> ...` | Relative link copied as symlink without following. |
| Circular Loop in directory tree | `podnext sync <c>:/sym_dir $SCRATCH/dst -d` | 0 | Synced without recursion | `sync` walks entries without infinite loop. |
| Circular Directory Loop (`sub/loop -> ../..`) | `podnext sync <c>:/dir_loop $SCRATCH/dst -d` | 0 | Synced without recursion | Symlink recorded without recursive descent. |
| Circular Executable Loop (`/bin/sh -> /bin/bash -> /bin/sh`) | `podnext run <c> -- /bin/sh` | 1 | `proot error: '/bin/sh' not found ...` | PRoot fails executable resolution cleanly with error code 1. |
| Dangling Symlink (`/etc/dangling -> /nonexistent`) | `podnext run <c> -- /bin/cat /etc/dangling` | 1 | `cat: can't open '/etc/dangling': No such file or directory` | Clean `ENOENT`. |
| Dangling `/etc/resolv.conf` | `podnext run <c> -- /bin/cat /etc/resolv.conf` | 1 | `cat: can't open '/etc/resolv.conf': No such file or directory` | Standard file access error. |
| Dangling `/etc/resolv.conf` | `podnext run <c> -- nslookup google.com` | 0 | DNS resolved via 127.0.0.1:53 | Musl libc falls back to local stub resolver. |
| Dangling `/etc/passwd` | `podnext run <c> -- id` | 0 | `uid=0 gid=0(root) groups=...` | Name lookup fails (uid=0 without name); process runs. |
| Dangling `/etc/os-release` | `podnext list` | 0 | Listed normally | Distro detector returns `None`; falls back to `metadata.json`. |

---

## 4. Diagnosis & Recovery Assessment

1. **Crash Immunity**: 0 unhandled panics, 0 segfaults, 0 infinite recursion loops detected across 40+ injected faults. All errors handled via `anyhow::Result` or proot child exit status.
2. **Diagnostic Quality**:
   - `metadata.json` errors surface raw Serde errors (`EOF while parsing...`, `invalid UTF-8`) rather than identifying file corruption.
   - `chmod 0000 rootfs` triggers false "Container not found" because loader silently ignores directories where rootfs validation fails.
3. **Recovery Capabilities**:
   - `podnext reset`: Successfully recovers containers with missing binaries or broken guest rootfs symlinks. Fails completely when `metadata.json` is corrupt or rootfs is unreadable (`0000`), causing recovery deadlock.
   - `podnext remove -f`: Successfully removes containers with corrupt `metadata.json`, but fails on `chmod 0000 rootfs` ("Container not found"), creating unmanaged zombie directories in `~/.podnext/containers/`.
