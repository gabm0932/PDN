# Internal Capability Tests (Run 2) — `deb-pdn` vs Host Environment

- Date: 2026-11-04 (WIB)
- Host: Android 16 (aarch64), Kernel `6.1.157-android14-11-g78a0a76cd7b7-ab15497637`
- Tool: PodNext (`podnext`) v0.1.7 (`/data/data/com.termux/files/home/workspace/PDN/target/release/podnext`)
- Target Container: `deb-pdn` (Debian GNU/Linux 13 / trixie rootfs at `~/.podnext/containers/deb-pdn/rootfs`)
- Host UID: `10396` (`u0_a396`)
- Container Fake UID: `0` (`root`)

---

## 1. Executive Summary

| Capability Subsystem | Outside (`deb-pdn` Host) | Inside `deb-pdn` | Status / Verdict |
|---|---|---|---|
| **Python `sys.platform`** | `android` | `android` (host binary) | Parity via bound host runtime |
| **Python `os.environ`** | 163 variables (host leak) | Sanitized: 6 keys (`HOME=/root`, `PATH`, `TERM`...) | **Clean isolation in Run 2** |
| **Hermes Agent CLI** | Functional (`v0.22.0+canary`) | Functional via host prefix | Works inside container |
| **Hermes `is_termux()`** | `True` | `False` | Correct guest environment isolation |
| **Hermes `is_container()`** | `False` | `False` | PRoot lacks container cgroup/env markers |
| **Hermes Config Resolution** | Default host profile | Container `/root/.hermes` or bound host | Configurable via `HERMES_HOME` |
| **Headless Chromium** | Functional (DOM dumped) | Functional (DOM dumped) | **100% Verified** |
| **Playwright Core (Node.js)** | Functional (`PW_OK`) | Functional (`PW_OK`) | **100% Verified** |
| **`/dev/shm` POSIX SHM** | Missing (`ENOENT`) | Mounted (`drwxrwxrwt`, host shm bound) | **Better inside PDN** |
| **Docker `unshare(2)`** | Fails (`EINVAL` / `EPERM`) | Fake `rc=0` (PRoot userspace trap) | Cannot run Docker natively |
| **Docker `mount(2)`** | Killed by **SIGSYS (31)** (exit 159) | Fake `rc=0` (PRoot trap, no kernel mount) | Cannot mount OverlayFS |
| **Linux Capabilities** | `CapEff = CapPrm = 0` | `CapEff = CapPrm = 0` | No real kernel privileges |
| **Cgroup v1/v2 Access** | Permission denied (exit 2) | Permission denied (exit 2) | Cannot allocate container cgroups |

---

## 2. Test 1: Platform Detection Probes

### 1.1 Python Platform & Environment Inspection

#### Inside `deb-pdn`
Command:
```bash
/data/data/com.termux/files/home/workspace/PDN/target/release/podnext login deb-pdn -- \
  /data/data/com.termux/files/usr/bin/python3 -c "import sys, platform, os, pprint; print('sys.platform:', sys.platform); print('platform.uname():', platform.uname()); print('os.uname():', os.uname()); pprint.pprint(dict(os.environ))"
```
Output:
```text
sys.platform: android
platform.uname(): uname_result(system='Android', node='localhost', release='16', version='#1 SMP PREEMPT Tue May 26 16:06:02 UTC 2026', machine='aarch64')
os.uname(): posix.uname_result(sysname='Linux', nodename='localhost', release='6.1.157-android14-11-g78a0a76cd7b7-ab15497637', version='#1 SMP PREEMPT Tue May 26 16:06:02 UTC 2026', machine='aarch64')
{'COLORTERM': 'truecolor',
 'HOME': '/root',
 'LANG': 'en_US.UTF-8',
 'PATH': '/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin',
 'PWD': '/',
 'TERM': 'xterm-256color'}
```
Exit Code: `0`

#### Outside Container (Host)
Command:
```bash
env -u LD_LIBRARY_PATH python3 -c "import sys, platform, os, pprint; print('sys.platform:', sys.platform); print('platform.uname():', platform.uname()); print('os.uname():', os.uname()); print('os.environ count:', len(os.environ)); print('Sample environ:', {k: os.environ[k] for k in ['HOME', 'PATH', 'PREFIX', 'TMPDIR'] if k in os.environ})"
```
Output:
```text
sys.platform: android
platform.uname(): uname_result(system='Android', node='localhost', release='16', version='#1 SMP PREEMPT Tue May 26 16:06:02 UTC 2026', machine='aarch64')
os.uname(): posix.uname_result(sysname='Linux', nodename='localhost', release='6.1.157-android14-11-g78a0a76cd7b7-ab15497637', version='#1 SMP PREEMPT Tue May 26 16:06:02 UTC 2026', machine='aarch64')
os.environ count: 163
Sample environ: {'HOME': '/data/data/com.termux/files/home', 'PATH': '/data/data/com.termux/files/home/.local/bin:...', 'PREFIX': '/data/data/com.termux/files/usr', 'TMPDIR': '/data/data/com.termux/files/usr/tmp'}
```
Exit Code: `0`

---

### 1.2 Hermes Agent Execution via Host Prefix

#### Inside `deb-pdn`
Command:
```bash
/data/data/com.termux/files/home/workspace/PDN/target/release/podnext login deb-pdn -- /bin/sh -c "export LD_LIBRARY_PATH=/data/data/com.termux/files/usr/lib; export PATH=\$PATH:/data/data/com.termux/files/usr/bin; /data/data/com.termux/files/usr/bin/hermes --version"
```
Output:
```text
Hermes Agent v0.22.0+canary.20261014T070054Z (2026.9.24) · canary
Install directory: /data/data/com.termux/files/usr/lib/hermes-agent/app
Install method: apt
Python: 3.14.6
OpenAI SDK: 2.24.0
```
Exit Code: `0`

#### Outside Container (Host)
Command:
```bash
hermes --version
```
Output:
```text
Hermes Agent v0.22.0+canary.20261014T070054Z (2026.9.24) · canary
Install directory: /data/data/com.termux/files/usr/lib/hermes-agent/app
Install method: apt
Python: 3.14.6
OpenAI SDK: 2.24.0
```
Exit Code: `0`

---

### 1.3 Hermes Config & Platform Detection Routines

#### Inside `deb-pdn`
Command:
```bash
/data/data/com.termux/files/home/workspace/PDN/target/release/podnext login deb-pdn -- /bin/sh -c "export LD_LIBRARY_PATH=/data/data/com.termux/files/usr/lib; export PATH=\$PATH:/data/data/com.termux/files/usr/bin; /data/data/com.termux/files/usr/bin/python3 -c \"
import sys
sys.path.insert(0, '/data/data/com.termux/files/usr/lib/hermes-agent/app')
import hermes_constants as hc
print('is_termux():', hc.is_termux())
print('is_container():', hc.is_container())
print('is_wsl():', hc.is_wsl())
print('get_hermes_home():', hc.get_hermes_home())
print('get_real_home():', hc.get_real_home())
\""
```
Output:
```text
is_termux(): False
is_container(): False
is_wsl(): False
get_hermes_home(): /root/.hermes
get_real_home(): /root
```
Exit Code: `0`

Config lookup inside container:
```bash
# Isolated container profile:
/data/data/com.termux/files/home/workspace/PDN/target/release/podnext login deb-pdn -- /bin/sh -c "export LD_LIBRARY_PATH=/data/data/com.termux/files/usr/lib; export PATH=\$PATH:/data/data/com.termux/files/usr/bin; /data/data/com.termux/files/usr/bin/hermes config get terminal.backend"
# Output:
local

# Bound host profile via HERMES_HOME:
/data/data/com.termux/files/home/workspace/PDN/target/release/podnext login deb-pdn -- /bin/sh -c "export LD_LIBRARY_PATH=/data/data/com.termux/files/usr/lib; export PATH=\$PATH:/data/data/com.termux/files/usr/bin; export HERMES_HOME=/data/data/com.termux/files/home/.hermes; /data/data/com.termux/files/usr/bin/hermes config get model"
# Output:
default: gpt-6-luna
provider: custom
base_url: https://ai.genzicorp.com/v1
api_key: gzi_...ef50
api_mode: chat_completions
```

#### Outside Container (Host)
Command:
```bash
env -u LD_LIBRARY_PATH python3 -c "
import sys
sys.path.insert(0, '/data/data/com.termux/files/usr/lib/hermes-agent/app')
import hermes_constants as hc
print('is_termux():', hc.is_termux())
print('is_container():', hc.is_container())
print('is_wsl():', hc.is_wsl())
print('get_hermes_home():', hc.get_hermes_home())
print('get_real_home():', hc.get_real_home())
"
```
Output:
```text
is_termux(): True
is_container(): False
is_wsl(): False
get_hermes_home(): /data/data/com.termux/files/home/.hermes
get_real_home(): /data/data/com.termux/files/home
```
Exit Code: `0`

---

## 3. Test 2: Browser & Playwright Execution

### 2.1 Headless Chromium DOM Dump

#### Inside `deb-pdn`
Command:
```bash
/data/data/com.termux/files/home/workspace/PDN/target/release/podnext login deb-pdn -- \
  /data/data/com.termux/files/usr/lib/chromium/chrome --headless=new --no-sandbox --disable-gpu --user-data-dir=/root/cud --dump-dom "data:text/html,<h1>PDN_BROWSER_VERIFIED</h1>"
```
Output:
```text
[31421:31438:1104/025625.768605:ERROR:./../src/dbus/bus.cc:405] Failed to connect to the bus: Failed to connect to socket /data/data/com.termux/files/usr/var/run/dbus/system_bus_socket: No such file or directory
... (dbus warning chatter omitted) ...
<html><head></head><body><h1>PDN_BROWSER_VERIFIED</h1></body></html>
```
Exit Code: `0`

#### Outside Container (Host)
Command:
```bash
/data/data/com.termux/files/usr/lib/chromium/chrome --headless=new --no-sandbox --disable-gpu --user-data-dir=/data/data/com.termux/files/home/.podnext/cud-host --dump-dom "data:text/html,<h1>PDN_BROWSER_VERIFIED</h1>"
```
Output:
```text
<html><head></head><body><h1>PDN_BROWSER_VERIFIED</h1></body></html>
```
Exit Code: `0`

---

### 2.2 Playwright Automation via Node.js

Script (`/root/pwtest.js`):
```javascript
const { chromium } = require('playwright-core');
(async () => {
  const exe = process.env.PW_EXE || '/data/data/com.termux/files/usr/lib/chromium/chrome';
  const b = await chromium.launch({
    executablePath: exe,
    args: ['--no-sandbox', '--disable-setuid-sandbox', '--disable-gpu'],
  });
  const p = await b.newPage();
  await p.setContent('<h1>PW_OK</h1>');
  const t = await p.textContent('h1');
  console.log('PLAYWRIGHT_DOM=' + t);
  console.log('UA=' + await p.evaluate(() => navigator.userAgent));
  await b.close();
  process.exit(0);
})().catch(e => { console.error('PW_FAIL=' + e.message); process.exit(1); });
```

#### Inside `deb-pdn`
Command:
```bash
/data/data/com.termux/files/home/workspace/PDN/target/release/podnext login deb-pdn -- /bin/sh -c \
  "export NODE_PATH=/data/data/com.termux/files/usr/lib/node_modules; /data/data/com.termux/files/usr/bin/node /root/pwtest.js"
```
Output:
```text
PLAYWRIGHT_DOM=PW_OK
UA=Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) HeadlessChrome/156.0.0.0 Safari/537.36
```
Exit Code: `0`

#### Outside Container (Host)
Command:
```bash
NODE_PATH=/data/data/com.termux/files/usr/lib/node_modules node /data/data/com.termux/files/home/.podnext/containers/deb-pdn/rootfs/root/pwtest.js
```
Output:
```text
PLAYWRIGHT_DOM=PW_OK
UA=Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) HeadlessChrome/156.0.0.0 Safari/537.36
```
Exit Code: `0`

---

### 2.3 `/dev/shm` Access & POSIX Shared Memory

#### Status & Permissions
- Host: `/dev/shm` does not exist (`ls: cannot access '/dev/shm': No such file or directory`, exit code 2).
- Inside `deb-pdn`: `/dev/shm` exists as `drwxrwxrwt 2 root root 3452`, bound to host directory `~/.podnext/containers/deb-pdn/shm`.

#### Functional POSIX Shared Memory Test (Python `multiprocessing.shared_memory`)
Command inside `deb-pdn`:
```bash
/data/data/com.termux/files/home/workspace/PDN/target/release/podnext login deb-pdn -- \
  /data/data/com.termux/files/usr/bin/python3 -c "
import multiprocessing.shared_memory as sm
try:
    s = sm.SharedMemory(create=True, size=1024, name='test_pdn_shm')
    s.buf[:5] = b'HELLO'
    print('SHM_CREATE_OK: readback =', bytes(s.buf[:5]))
    s.close()
    s.unlink()
except Exception as e:
    print('SHM_ERROR:', type(e), e)
"
```
Output:
```text
SHM_CREATE_OK: readback = b'HELLO'
```
Exit Code: `0`

---

## 4. Test 3: Docker Capability Forensics Inside PRoot / PDN

### 3.1 Namespace Isolation (`unshare`)

Raw libc syscall probe:
```python
import ctypes, os
libc = ctypes.CDLL(None, use_errno=True)
flags = {
    'CLONE_NEWUSER': 0x10000000,
    'CLONE_NEWNS':   0x00020000,
    'CLONE_NEWPID':  0x20000000,
    'CLONE_NEWNET':  0x40000000,
    'CLONE_NEWIPC':  0x08000000,
    'CLONE_NEWUTS':  0x04000000
}
for name, flag in flags.items():
    rc = libc.unshare(flag)
    err = ctypes.get_errno() if rc != 0 else 0
    print(f'{name}: rc={rc}, errno={err} ({os.strerror(err) if err else "OK"})')
```

#### Results Comparison
| Flag | Host Syscall Result | Container (PRoot) Result | Root Cause Analysis |
|---|---|---|---|
| `CLONE_NEWUSER` | `rc=-1, errno=22 (Invalid argument)` | `rc=0, errno=0 (OK)` | Kernel has user namespaces disabled/restricted (`EINVAL`); PRoot fakes success |
| `CLONE_NEWNS` | `rc=-1, errno=1 (Operation not permitted)` | `rc=0, errno=0 (OK)` | Requires unprivileged user ns or `CAP_SYS_ADMIN` (`EPERM`); PRoot fakes success |
| `CLONE_NEWPID` | `rc=-1, errno=1 (Operation not permitted)` | `rc=0, errno=0 (OK)` | Requires `CAP_SYS_ADMIN` (`EPERM`); PRoot fakes success |
| `CLONE_NEWNET` | `rc=-1, errno=1 (Operation not permitted)` | `rc=0, errno=0 (OK)` | Requires `CAP_NET_ADMIN` (`EPERM`); PRoot fakes success |
| `CLONE_NEWIPC` | `rc=-1, errno=1 (Operation not permitted)` | `rc=0, errno=0 (OK)` | Requires `CAP_SYS_ADMIN` (`EPERM`); PRoot fakes success |
| `CLONE_NEWUTS` | `rc=-1, errno=1 (Operation not permitted)` | `rc=0, errno=0 (OK)` | Requires `CAP_SYS_ADMIN` (`EPERM`); PRoot fakes success |

CLI verification outside:
```bash
unshare -U -m /bin/true
# Output:
unshare: unshare failed: Invalid argument
# Exit code: 1
```

---

### 3.2 Filesystem Mount & OverlayFS (`mount`)

#### Host Syscall Termination by Android Seccomp
Command on host:
```bash
env -u LD_LIBRARY_PATH python3 -c "
import ctypes, os
libc = ctypes.CDLL(None)
res = libc.mount(b'none', b'/mnt', b'tmpfs', 0, None)
"
```
Output:
```text
/data/data/com.termux/files/usr/bin/bash: line 12: 13847 Unknown signal 31
```
Exit Code: `159` (`128 + 31` = **SIGSYS**)
The Android kernel seccomp filter intercepts the `mount(2)` syscall from an application UID and terminates the process immediately with signal 31 (`SIGSYS`).

#### Container Mount Interception & OverlayFS Proof of Fake
Inside `deb-pdn`, PRoot intercepts `SYS_mount` and returns `0` to prevent the host kernel from terminating the process. However, **no mount is established**.

Proof via OverlayFS test inside `deb-pdn`:
```bash
/data/data/com.termux/files/home/workspace/PDN/target/release/podnext login deb-pdn -- /bin/sh -c \
  "mkdir -p /tmp/lower /tmp/upper /tmp/work /tmp/merged && \
   mount -t overlay overlay -o lowerdir=/tmp/lower,upperdir=/tmp/upper,workdir=/tmp/work /tmp/merged && \
   echo 'LOWER_DATA' > /tmp/lower/file.txt && \
   ls -la /tmp/merged"
```
Output:
```text
total 7
drwx------. 2 root root 3452 Nov  3 20:02 .
drwxr-xr-x. 7 root root 3452 Nov  3 20:02 ..
```
Exit Code: `0`
**Outcome:** `mount` returned `0`, but `/tmp/merged` contains nothing (`total 7`, only `.` and `..`). The lower file `file.txt` is completely missing. Docker's storage driver (`overlay2`) cannot assemble layer hierarchies under PRoot.

---

### 3.3 Linux Capabilities & Privilege Elevation

#### Capability Set (`/proc/self/status`)
Inspecting capabilities both outside and inside container:
```bash
grep -E "Cap(Inh|Prm|Eff|Bnd|Amb)" /proc/self/status
```
Output (identical on host and inside `deb-pdn`):
```text
CapInh:	0000000000000000
CapPrm:	0000000000000000
CapEff:	0000000000000000
CapBnd:	0000000000000000
CapAmb:	0000000000000000
```

#### Ambient Capability Elevation Attempt
Command:
```python
import ctypes, os
libc = ctypes.CDLL(None, use_errno=True)
PR_CAP_AMBIENT = 47
PR_CAP_AMBIENT_RAISE = 2
CAP_SYS_ADMIN = 21
res = libc.prctl(PR_CAP_AMBIENT, PR_CAP_AMBIENT_RAISE, CAP_SYS_ADMIN, 0, 0)
err = ctypes.get_errno() if res != 0 else 0
print(f'prctl PR_CAP_AMBIENT_RAISE CAP_SYS_ADMIN: res={res}, errno={err} ({os.strerror(err) if err else "OK"})')
```
Output inside `deb-pdn`:
```text
prctl PR_CAP_AMBIENT_RAISE CAP_SYS_ADMIN: res=-1, errno=1 (Operation not permitted)
```
Exit Code: `0` (`res=-1`, `errno=1` `EPERM`).

---

### 3.4 Control Groups (Cgroups)

#### Directory Access & Kernel Visibility
1. Host `/sys/fs/cgroup`:
   ```bash
   ls -la /sys/fs/cgroup
   # Output: ls: cannot open directory '/sys/fs/cgroup': Permission denied
   # Exit code: 2
   ```
2. Container `/sys/fs/cgroup`:
   ```bash
   /data/data/com.termux/files/home/workspace/PDN/target/release/podnext login deb-pdn -- ls -la /sys/fs/cgroup
   # Output: ls: cannot open directory '/sys/fs/cgroup': Permission denied
   # Exit code: 2
   ```
3. `/proc/cgroups` (both host and container):
   ```bash
   cat /proc/cgroups
   # Output: cat: /proc/cgroups: Permission denied
   # Exit code: 1
   ```

Dockerd and container runtimes (such as `runc`) depend strictly on writing to cgroup controllers (`cpu`, `memory`, `pids`). On Android 16 without root, cgroup hierarchies are inaccessible.

---

### 3.5 Docker Client vs Daemon Execution

Command:
```bash
/data/data/com.termux/files/home/workspace/PDN/target/release/podnext login deb-pdn -- /bin/sh -c \
  "export PATH=\$PATH:/data/data/com.termux/files/usr/bin; docker --version 2>&1; docker info 2>&1 || true"
```
Output:
```text
Docker version v24.0.6-ce, build 
Client:
 Version:    v24.0.6-ce
 Context:    default
 Debug Mode: false

Server:
ERROR: Cannot connect to the Docker daemon at unix:///data/data/com.termux/files/usr/var/run/docker.sock. Is the docker daemon running?
errors pretty printing info
```
Exit Code: `0`

---

## 5. Architectural Conclusion

1. **Host-bound AI & Browser Stack: Fully Operational**
   - Headless Chromium and Playwright run with 100% parity inside `deb-pdn` and outside.
   - `/dev/shm` is correctly provisioned by PDN with `01777` permissions, fixing standard Linux POSIX IPC crashes.
   - Hermes Agent CLI and configuration resolution work inside `deb-pdn`. Platform detection routines correctly distinguish the container environment (`is_termux() == False`).

2. **Native In-PRoot Docker: Structurally Impossible**
   - **Kernel namespaces**: Blocked with `EINVAL` (`CLONE_NEWUSER`) and `EPERM`. PRoot fakes `rc=0`, meaning no actual isolation occurs.
   - **Filesystems (`mount`)**: Host kernel enforces seccomp `SIGSYS` (exit code 159). PRoot suppresses `SIGSYS` and fakes `rc=0`, but OverlayFS layer merging does not occur (`/tmp/merged` remains empty).
   - **Capabilities**: Effective and permitted capabilities are strictly `0`. Elevation attempts fail with `EPERM`.
   - **Cgroups**: Inaccessible (`Permission denied`, exit codes 1 and 2).
   - **Path forward**: Docker inside PDN requires an independent guest kernel in user-space (such as `uml4termux` User-Mode Linux) as documented in `DOCKER_IN_PDN_ARCHITECTURE.md`.
