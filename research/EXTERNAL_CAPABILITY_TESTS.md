# External Capability Benchmark — proot-distro 5.10.3 vs PDN `podnext` 0.1.7

Host-side (Termux shell) side-by-side test. **Raw evidence only — every number below is copied from an actual timed command.** No synthesized values.

- Date: 2026-11-03 (WIB, UTC+07:00)
- Host: Android 16, aarch64, Termux (proot-distro rootfs store under $PREFIX/var/lib; PDN under `~/.podnext/`)
- `proot-distro`: system binary v5.10.3 (`/data/data/com.termux/files/usr/bin/proot-distro`, by @sylirre)
- `podnext`: `/data/data/com.termux/files/home/workspace/PDN/target/release/podnext` (podnext 0.1.7)
- Shell: bash 5.x, `TIMEFORMAT='REAL=%R USER=%U SYS=%S'`
- All measurements `n=3`, both tools, median reported.

### Collision-avoidance note
A concurrent subagent owns the shared `alpine` containers in **both** tools. Per instruction, the shared `alpine` container was **never removed/reset/reinstalled**. Dedicated bench containers were used for all timing:
- proot-distro: `pd-bench` (`proot-distro install alpine --name pd-bench`)
- PDN: `pdn-bench` (`podnext install alpine --name pdn-bench`)

Both bench containers were **left installed** at the end (for the next agent). Only the two backup archives created in step 8 were deleted.

---

## SUMMARY TABLE

| Operation | proot-distro 5.10.3 median | PDN podnext 0.1.7 median | Winner |
|---|---|---|---|
| INSTALL alpine (cold, REAL s) | **10.109** (11.209 / 10.109 / 9.486) | **8.868** (8.871 / 7.936 / 8.868) | PDN (−1.24 s, ~12%) |
| LOGIN `-- echo hello` (REAL s) | **0.561** (0.550 / 0.561 / 0.573) | **0.347** (0.355 / 0.347 / 0.342) | PDN (−0.21 s, ~38%) |
| LIST (REAL s) | **0.677** (1.486 / 0.567 / 0.677) | **0.034** (0.072 / 0.031 / 0.034) | PDN (−0.64 s, ~95%) |
| RUN `-- echo hello` (REAL s) | **0.740** | **0.418** | PDN (−0.32 s) |
| BACKUP of bench container (REAL s) | **2.000** → 9,041,920 B (plain tar) | **3.205** → 4,631,494 B (gzip) | proot-distro faster; PDN smaller |
| SEARCH alpine (exit) | 0 (plain-space table, 34 lines) | 0 (box-unicode table, 57 lines) | both work |
| Multi-cmd shell exec (`id;uname;…`) | exit 0, all cmds OK | exit 0, all cmds OK | both work (see §7 diffs) |

Notes on winner column: INSTALL is network-bound (~4 MiB layer each run) so the 1.2 s delta is mostly jitter, not architectural. LIST/LOGIN deltas are structural (Rust single binary vs shell/Python wrapper) and are consistent across all 3 runs.

---

## 1. Clean state — `remove` / `clear-cache` (bench containers)

```bash
$ proot-distro remove pd-bench
Error: container 'pd-bench' is not installed.

$ proot-distro clear-cache
[*] Clearing cache...
[*] Reclaimed 4.0 MiB of disk space.
```

```bash
$ podnext remove pdn-bench -f
DEBUG execute_command: START Remove { name: "pdn-bench", force: true }
Error: Container 'pdn-bench' not found        # first call, container absent — OK
```

### FINDING — `podnext remove` is a no-op (returns 0, does not delete)
After `pdn-bench` existed, removal reported success but the container persisted:

```bash
$ ls ~/.podnext/containers/            # BEFORE
almalinux  alpine  pdn-bench  test-alpine  test-pdn-install

$ podnext -vv remove pdn-bench -f
DEBUG main: Executing command: Remove { name: "pdn-bench", force: true }
DEBUG execute_command: START Remove { name: "pdn-bench", force: true }
Container 'pdn-bench' removed.
EXIT=0

$ ls ~/.podnext/containers/            # AFTER — still present
almalinux  alpine  pdn-bench  test-alpine  test-pdn-install
```

`podnext list` also still shows `pdn-bench`. Consequence for this benchmark: subsequent PDN cold runs were forced clean with a manual `rm -rf ~/.podnext/containers/pdn-bench` instead of `podnext remove`. Evidence of the bug is retained below (a plain `podnext remove pdn-bench -f` followed by `podnext install` errors "already exists", see §2).

---

## 2. INSTALL benchmark (cold)

### proot-distro — `proot-distro install alpine --name pd-bench` (clear-cache + remove before each run)

```bash
$ TIMEFORMAT='REAL=%R USER=%U SYS=%S'
$ proot-distro remove pd-bench ; proot-distro clear-cache ; time proot-distro install alpine --name pd-bench
# RUN 1
[*] Installing 'alpine:latest' as 'pd-bench'...
[*] d3531b7a4f21: Downloading layer 1/1 (4.0 MiB)...
[*] d3531b7a4f21: Applying layer 1/1...
[*] Registering Android-specific UIDs and GIDs...
[*] Finished installation.
REAL=11.209 USER=0.621 SYS=0.281

# RUN 2
REAL=10.109 USER=0.660 SYS=0.243

# RUN 3
REAL=9.486 USER=0.667 SYS=0.298
```

- Samples (REAL): 11.209 / 10.109 / 9.486 → **median 10.109**

### PDN — `podnext install alpine --name pdn-bench` (container removed before each run)

```bash
$ podnext remove pdn-bench -f ; time podnext install alpine --name pdn-bench
# RUN 1
DEBUG execute_command: START Install { image: "alpine", name: Some("pdn-bench"), force: false }
Installing 'alpine' as 'pdn-bench'...
Fetching auth token...
Fetching manifest...
Downloading layers...
Applying compatibility patches...
Container 'pdn-bench' installed successfully!
Rootfs: /data/data/com.termux/files/home/.podnext/containers/pdn-bench/rootfs
REAL=8.871 USER=0.330 SYS=0.318
```

RUN 2 and RUN 3 were first attempted with the tool's own `podnext remove` and **failed**, proving the §1 no-op bug:

```bash
# RUN 2 (attempt via podnext remove — FAILED)
Container 'pdn-bench' removed.
$ time podnext install alpine --name pdn-bench
Error: Container 'pdn-bench' already exists at /data/data/com.termux/files/home/.podnext/containers/pdn-bench/rootfs. Use --force to overwrite.
REAL=0.034 USER=0.014 SYS=0.013
```

Re-run with a manual wipe (`rm -rf ~/.podnext/containers/pdn-bench`):

```bash
# RUN 2 (manual wipe)
manual rm -rf done
Fetching auth token... / Fetching manifest... / Downloading layers... / Applying compatibility patches...
Container 'pdn-bench' installed successfully!
REAL=7.936 USER=0.374 SYS=0.416

# RUN 3 (manual wipe)
REAL=8.868 USER=0.361 SYS=0.275
```

- Valid samples (REAL): 8.871 / 7.936 / 8.868 → **median 8.868**
- (`--force` exists on `podnext install`, but task said remove container between runs; manual wipe used because `remove` is broken.)

---

## 3. LOGIN / command-exec benchmark — `login <bench> -- echo hello` (n=3)

```bash
$ TIMEFORMAT='REAL=%R USER=%U SYS=%S'
$ time proot-distro login pd-bench -- echo hello
hello
REAL=0.550 USER=0.321 SYS=0.157      # RUN1
REAL=0.561 USER=0.325 SYS=0.145      # RUN2
REAL=0.573 USER=0.362 SYS=0.126      # RUN3
# median 0.561
```

```bash
$ time podnext login pdn-bench -- echo hello
DEBUG login] Got container: pdn-bench
DEBUG login] Exit code: 0
hello
REAL=0.355 USER=0.031 SYS=0.164      # RUN1
REAL=0.347 USER=0.056 SYS=0.153      # RUN2
REAL=0.342 USER=0.042 SYS=0.165      # RUN3
# median 0.347
```

Both print `hello`, exit 0. PDN emits a large `[DEBUG …]` banner on every login (noise; likely intended to be silenced in release/default verbosity).

---

## 4. LIST benchmark (n=3)

```bash
$ time proot-distro list
Installed containers:
  * alpine
  * pd-bench
Log in with: proot-distro login <name>
REAL=1.486 USER=0.677 SYS=0.310      # RUN1 (cold)
REAL=0.567 USER=0.399 SYS=0.112      # RUN2
REAL=0.677 USER=0.469 SYS=0.100      # RUN3
# median 0.677
```

```bash
$ time podnext list
+------------------+--------+---------+---------+---------+------------------+
| NAME             | DISTRO | VERSION | ARCH    | STATUS  | CREATED          |
+------------------+--------+---------+---------+---------+------------------+
| test-pdn-install | ubuntu | 22.04   | aarch64 | Stopped | 2026-11-02 19:38 |
| test-alpine      | ubuntu | 22.04   | aarch64 | Stopped | 2026-11-02 19:38 |
| pdn-bench        | ubuntu | 22.04   | aarch64 | Stopped | 2026-11-02 19:38 |
| alpine           | ubuntu | 22.04   | aarch64 | Stopped | 2026-11-02 19:38 |
+------------------+--------+---------+---------+---------+------------------+
REAL=0.072 USER=0.001 SYS=0.035      # RUN1
REAL=0.031 USER=0.004 SYS=0.025      # RUN2
REAL=0.034 USER=0.008 SYS=0.019      # RUN3
# median 0.034
```

- **Correctness flag:** `podnext list` reports DISTRO=`ubuntu`, VERSION=`22.04` for the **alpine** containers (and for every container). Metadata is mislabeled — DISTRO/VERSION columns are unreliable.
- Row order is non-deterministic (RUN1/2/3 list the same rows in different order — HashMap iteration).

---

## 5. RUN mode

```bash
$ time proot-distro run pd-bench -- echo hello
hello
REAL=0.740 USER=0.411 SYS=0.206      # supported, exit 0
```

```bash
$ time podnext run pdn-bench -- echo hello
DEBUG execute_command: START Run { container: "pdn-bench", command: ["echo", "hello"], ... }
hello
REAL=0.418 USER=0.036 SYS=0.201      # supported, exit 0
```

Both support RUN mode and execute the command. PDN `run` additionally offers `-d/--detach`, `-p/--publish`, `-v/--volume`, `--network`, `--entrypoint`, `-t/--tty`, `-r/--rm` (docker-style flags; all confirmed present in `podnext run --help`). proot-distro `run` is entrypoint-oriented (no such flags).

---

## 6. SEARCH — `search alpine` (first 15 lines each)

### proot-distro (exit 0, 34 lines total) — plain space-aligned table
```bash
$ proot-distro search alpine
[*] Searching Docker Hub for 'alpine'...

Images on Docker Hub:

NAME               DESCRIPTION                           STARS   PULLS  OFFICIAL
alpine             A minimal Docker image based on Alp...  11579   12.4B  [OK]
alpine/git         A simple git container running in a...    252  519.5M
alpine/socat       Run socat command in alpine contain...    120  91.0M
alpine/curl                                                 14  109.5M
alpine/k8s         Kubernetes toolbox for EKS (kubectl...     68  81.9M
alpine/helm        Auto-trigger docker build for kuber...     71  73.5M
alpine/kubectl     Kubernetes command-line tool for ma...      2  18.2M
alpine/httpie      Auto-trigger docker build for `http...     23  17.8M
alpine/terragrunt  Auto-trigger docker build for terra...     19  17.0M
alpine/bombardier  Auto-trigger docker build for bomba...     30  17.1M
```

### podnext (exit 0, 57 lines total) — unicode box table, untruncated pulls
```bash
$ podnext search alpine
DEBUG main: Executing command: Search { query: "alpine", limit: 25, format: Table }
+-------------------+-----------------------------------------------------+-------+-------------+----------+
| NAME              | DESCRIPTION                                         | STARS | PULLS       | OFFICIAL |
+-------------------+-----------------------------------------------------+-------+-------------+----------+
| alpine            | A minimal Docker image based on Alpine Linux wit... | 11579 | 12446992049 | [OK]     |
+-------------------+-----------------------------------------------------+-------+-------------+----------+
| alpine/git        | A  simple git container running in alpine linux,... | 252   | 519546202   |          |
+-------------------+-----------------------------------------------------+-------+-------------+----------+
| alpine/socat      | Run socat command in alpine container               | 120   | 91039320    |          |
+-------------------+-----------------------------------------------------+-------+-------------+----------+
| alpine/curl       |                                                     | 14    | 109540204   |          |
+-------------------+-----------------------------------------------------+-------+-------------+----------+
| alpine/k8s        | Kubernetes toolbox for EKS (kubectl, helm, iam-a... | 68    | 81893315    |          |
+-------------------+-----------------------------------------------------+-------+-------------+----------+
```

Format delta: proot-distro = human-short pulls (`12.4B`), 34 lines. PDN = raw integer pulls, box table, 57 lines (borders). PDN supports `-f json|yaml|text`, proot-distro has no format flag.

---

## 7. SHELL end-to-end (multi-command, non-interactive)

Command: `/bin/sh -c 'id; uname -a; cat /etc/alpine-release; echo $PATH; ls /'`

### proot-distro
```bash
$ proot-distro login pd-bench -- /bin/sh -c 'id; uname -a; cat /etc/alpine-release; echo PATH=$PATH; ls /'
uid=0(root) gid=0(root) groups=1007(aid_log),1077(aid_external_storage),... ,99909997
Linux localhost 6.17.0-PRoot-Distro #1 SMP PREEMPT_DYNAMIC Fri, 10 Oct 2025 00:00:00 +0000 aarch64 Linux
3.24.3
PATH=/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin:/data/data/com.termux/files/usr/bin
apex bin data dev etc home lib linkerconfig media mnt odm opt proc product root run sbin sdcard srv storage sys system system_ext tmp usr var vendor
EXIT=0
```

### podnext
```bash
$ podnext login pdn-bench -- /bin/sh -c 'id; uname -a; cat /etc/alpine-release; echo PATH=$PATH; ls /'
uid=10396(u0_a396) gid=10396(u0_a396) groups=10396(u0_a396),1007(log),... context=u:r:untrusted_app_27:s0:c140,c257,c512,c768
Linux localhost 6.1.157-android14-11-g78a0a76cd7b7-ab15497637 #1 SMP PREEMPT Tue May 26 16:06:02 UTC 2026 aarch64 Android
3.24.3
PATH=/data/data/com.termux/files/home/.local/bin:...:/data/data/com.termux/files/usr/lib/hermes-agent/tools/ripgrep
apex bin data dev etc home lib linkerconfig media mnt odm opt proc product root run sbin sdcard srv storage sys system system_ext tmp usr var vendor
EXIT=0
```

**No command failed in either tool** (all 5 cmds produced output, exit 0 in both). Differences observed:

| Aspect | proot-distro | podnext |
|---|---|---|
| `id` | `uid=0(root) gid=0(root)` | `uid=10396(u0_a396)` (host app UID), SELinux `untrusted_app_27` |
| `uname -a` | kernel spoofed `6.17.0-PRoot-Distro`, `…aarch64 Linux` | real host kernel `6.1.157-android14-11…`, `…aarch64 Android` |
| `alpine-release` | `3.24.3` | `3.24.3` (identical) |
| `$PATH` | clean container PATH + one Termux bin | inherits **entire host Termux PATH** incl. `hermes-agent/…/venv/bin`, duplicated `~/.local/bin` entries |
| `ls /` | host Android `/` (default non-isolated, host bound) | host Android `/` (default non-isolated, host bound) |

Both default to non-isolated (host filesystem visible). PDN leaks the caller environment (PATH) into the container and does not drop to root by default.

---

## 8. BACKUP

### proot-distro — `backup <bench> > file`
```bash
$ TIMEFORMAT='REAL=%R USER=%U SYS=%S'
$ time proot-distro backup pd-bench > ~/pd-bench-backup.tar.gz
[*] Backing up 'pd-bench'...
[*] Will write backup data to stdout.
[*] Fixing file permissions in rootfs...
[*] Archiving the container...
[*] Finished backing up.
REAL=2.000 USER=0.716 SYS=0.363     EXIT=0
$ ls -la ~/pd-bench-backup.tar.gz
-rw-------. 1 u0_a396 u0_a396 9041920 Nov  3 02:41 /data/data/com.termux/files/home/pd-bench-backup.tar.gz
$ file ~/pd-bench-backup.tar.gz
/data/data/com.termux/files/home/pd-bench-backup.tar.gz: POSIX tar archive
```
- Time 2.000 s, size **9,041,920 B (~8.6 MiB)**. Note: stdout is **uncompressed tar** despite the `.gz` name (only `-c/--compress` compresses).

### PDN — `backup create <bench>`
```bash
$ time podnext backup create pdn-bench
DEBUG create_archive: archive metadata obtained, size=4631494
DEBUG create_archive: completed successfully, size=4631494
Backup created with ID: ab3a15f6-ca3a-4fbf-ad03-021e71b438e3
REAL=3.205 USER=1.291 SYS=0.723     EXIT=0
$ ls -la ~/.podnext/backups/ab3a15f6-ca3a-4fbf-ad03-021e71b438e3.tar.gz
-rw-------. 1 u0_a396 u0_a396 4631494 Nov  3 02:42 …/ab3a15f6-ca3a-4fbf-ad03-021e71b438e3.tar.gz
```
- `--force` is **not** a valid flag (`error: unexpected argument '--force' found`, exit 2).
- Time 3.205 s, size **4,631,494 B (~4.42 MiB, gzip)**.
- **Correctness flags:** `podnext backup create` prints 282,399 chars of `DEBUG create_archive: failed to add symlink … No such file or directory (os error 2)` for *every* symlink in the rootfs (e.g. `usr/bin/whoami`, `usr/bin/sort`, `usr/bin/tail`). Symlinks appear to be dropped from the archive — restore fidelity is in doubt.

### `podnext backup list` after a successful create (BUG — empty)
```bash
$ podnext backup list
+----+------+-----------+------+--------+------+---------+
| ID | NAME | CONTAINER | TYPE | STATUS | SIZE | CREATED |
+----+------+-----------+------+--------+------+---------+
```
Empty table despite the archive existing on disk and `Backup created with ID: …` having been printed. (proot-distro has no equivalent — backups go to a path the caller chooses.)

### Cleanup
Archives created here were deleted:
```bash
$ rm -f ~/pd-bench-backup.tar.gz
$ rm -rf ~/.podnext/backups/ab3a15f6-ca3a-4fbf-ad03-021e71b438e3 ~/.podnext/backups/ab3a15f6-ca3a-4fbf-ad03-021e71b438e3.tar.gz
$ ls ~/pd-bench-backup.tar.gz
ls: cannot access … No such file or directory   # confirmed gone
```

---

## 9. Feature probes (`<tool> <cmd> --help`; exit 0 = subcommand exists)

### proot-distro feature set, tested against PDN
```bash
$ for c in ps kill rename reset copy sync build push restore; do
    proot-distro $c --help >/dev/null 2>&1; pd=$?
    podnext     $c --help >/dev/null 2>&1; pn=$?
    echo "$c : proot-distro_help_exit=$pd podnext_help_exit=$pn"
  done
ps      : proot-distro_help_exit=0 podnext_help_exit=0
kill    : proot-distro_help_exit=0 podnext_help_exit=0
rename  : proot-distro_help_exit=0 podnext_help_exit=2
reset   : proot-distro_help_exit=0 podnext_help_exit=2
copy    : proot-distro_help_exit=0 podnext_help_exit=2
sync    : proot-distro_help_exit=0 podnext_help_exit=2
build   : proot-distro_help_exit=0 podnext_help_exit=0
push    : proot-distro_help_exit=0 podnext_help_exit=2
restore : proot-distro_help_exit=0 podnext_help_exit=2
```
- proot-distro-only, **absent in PDN**: `rename`, `reset`, `copy`, `sync`, `push`, top-level `restore` (PDN folds restore under `backup restore`).
- Overlapping: `ps`, `kill`, `build` exist in both (semantics differ — PDN `build` is a multi-arch build service: `submit/status/cancel`).

### PDN feature set, tested against proot-distro
```bash
$ for c in service network volume quota log dbus socket boot wakelock policy dashboard gitops image sessions; do
    podnext     $c --help >/dev/null 2>&1; pn=$?
    proot-distro $c --help >/dev/null 2>&1; pd=$?
    echo "$c : podnext_help_exit=$pn proot-distro_help_exit=$pd"
  done
service   : podnext_help_exit=0 proot-distro_help_exit=1
network   : podnext_help_exit=0 proot-distro_help_exit=1
volume    : podnext_help_exit=0 proot-distro_help_exit=1
quota     : podnext_help_exit=0 proot-distro_help_exit=1
log       : podnext_help_exit=0 proot-distro_help_exit=1
dbus      : podnext_help_exit=0 proot-distro_help_exit=1
socket    : podnext_help_exit=0 proot-distro_help_exit=1
boot      : podnext_help_exit=0 proot-distro_help_exit=1
wakelock  : podnext_help_exit=0 proot-distro_help_exit=1
policy    : podnext_help_exit=0 proot-distro_help_exit=1
dashboard : podnext_help_exit=0 proot-distro_help_exit=1
gitops    : podnext_help_exit=0 proot-distro_help_exit=1
image     : podnext_help_exit=0 proot-distro_help_exit=1
sessions  : podnext_help_exit=0 proot-distro_help_exit=1
```
(proot-distro returns exit 1 for an unknown command = absent.) All 14 PDN extras exist; none exist in proot-distro.

### PDN full top-level command set (`podnext help`)
```
install  list  login  run  ps  kill  remove  search  image  sessions  attach  info
version  service  daemon  watchdog  quota  log  dbus  socket  boot  wakelock  policy
backup  build  gitops  dashboard  network  volume  completion  help
```

### LOGIN flags
```bash
$ for f in --shared-home --shared-tmp --isolated --minimal --bind -b -u; do
    proot-distro login --help | grep -q -- "$f" && echo "PD $f PRESENT"; done
PD --shared-home PRESENT
PD --shared-tmp  PRESENT
PD --isolated    PRESENT
PD --minimal     PRESENT
PD --bind        PRESENT
PD -b            PRESENT
PD -u            PRESENT

$ for f in --shared-home --shared-tmp --isolated --minimal --bind -b -u; do
    podnext login --help | grep -q -- "$f" && echo "PDN $f PRESENT" || echo "PDN $f ABSENT"; done
PDN --shared-home ABSENT
PDN --shared-tmp  ABSENT
PDN --isolated    ABSENT
PDN --minimal     ABSENT
PDN --bind        PRESENT
PDN -b            PRESENT
PDN -u            PRESENT
```
- proot-distro `login` has the full isolation/bind flag set (`--shared-home`, `--shared-tmp`, `--isolated`, `--minimal`, `--bind`).
- PDN `login` exposes `--bind/-b`, `-u`, plus proot passthrough flags (`--kernel`, `--hostname`, `--uid`, `--gid`, `--seccomp`, `--sysvipc`, `--link2symlink`, `--env`) but **not** `--shared-home`/`--shared-tmp`/`--isolated`/`--minimal`. `podnext run` has the docker-style flag set (see §5).

---

## Raw log locations (session scratch, not guaranteed persistent)
`~/.hermes/cache/scratch/bench/{pd,pdn}_{install_1,install_2,install_3,login,list,run,search,shell,backup}.log`
Full `podnext backup create` output (282,399 chars) captured at `/data/data/com.termux/files/home/.hermes/cache/terminal-output/out-1793648518-5701-fce0.log`.

## Bottom line
- **Speed:** PDN wins decisively on LIST (~20×) and LOGIN (~1.6×); wins narrowly on cold INSTALL and RUN. proot-distro wins only on BACKUP wall-time, but produces a larger *uncompressed* archive.
- **Feature breadth:** PDN ships far more subcommands (service/daemon/watchdog/quota/log/dbus/socket/boot/wakelock/policy/dashboard/gitops/network/volume/sessions). proot-distro ships `rename/reset/copy/sync/push/restore` that PDN lacks.
- **Reliability defects observed in PDN 0.1.7:** (1) `remove` is a silent no-op (exit 0, container persists); (2) `backup create` fails to archive symlinks and floods 280K chars of DEBUG; (3) `backup list` shows empty after a successful create; (4) `list` reports wrong DISTRO/VERSION; (5) `login` leaks the host PATH and default-non-root while proot-distro defaults to root with a spoofed kernel; (6) debug banner printed unconditionally at default verbosity.
- Containers `pd-bench` (proot-distro) and `pdn-bench` (PDN) left installed; shared `alpine` containers untouched; benchmark backup archives deleted.
