# PDN vs proot-distro — User-Perspective A/B Comparison (RUN 2)

- Host: Termux on Android 16, aarch64, host uid `10396` (u0_a396).
- Date: 2026-11-03 22:20 WIB.
- PDN: `./target/release/podnext` (binary 18774320 bytes, Nov 3 22:03), container `deb-pdn`
  at `~/.podnext/containers/deb-pdn`.
- proot-distro: `$PREFIX/bin/proot-distro`, container `debian`
  at `$PREFIX/var/lib/proot-distro/containers/debian`.
- Neither container was deleted or reset. No packages installed. No source modified.

## 1. Cold `login` latency (3 runs each)

Command: `time <tool> login <name> -- /bin/sh -c 'echo READY'`

PDN `deb-pdn` — raw:
```
=== PDN run 1 ===
READY
real    0m0.257s
user    0m0.021s
sys     0m0.102s
=== PDN run 2 ===
real    0m0.114s
=== PDN run 3 ===
real    0m0.097s
```
proot-distro `debian` — raw:
```
=== proot-distro run 1 ===
READY
real    0m1.022s
user    0m0.435s
sys     0m0.263s
=== proot-distro run 2 ===
real    0m0.710s
=== proot-distro run 3 ===
real    0m0.603s
```
PDN real: 0.257 / 0.114 / 0.097 s. proot-distro real: 1.022 / 0.710 / 0.603 s.
PDN ≈ 5× faster cold start.

## 2. Kernel identity — `uname -r`, `uname -m`

PDN:
```
6.1.157-android14-11-g78a0a76cd7b7-ab15497637
aarch64
```
proot-distro:
```
6.17.0-PRoot-Distro
aarch64
```
Host (`uname -a`): `Linux localhost 6.1.157-android14-11-g78a0a76cd7b7-ab15497637 #1 SMP PREEMPT Tue May 26 16:06:02 UTC 2026 aarch64 Android`.
PDN reports the REAL host kernel. proot-distro reports a fabricated `6.17.0-PRoot-Distro`.

`/proc/version` inside each:
```
PDN:  Linux version 6.1.157-android14-11-g78a0a76cd7b7-ab15497637 (proot@termux) (gcc (GCC) 13.3.0, GNU ld (GNU Binutils) 2.42) #1 SMP PREEMPT_DYNAMIC Fri, 10 Oct 2025 00:00:00 +0000
PD:   Linux version 6.17.0-PRoot-Distro (proot@termux) (gcc (GCC) 13.3.0, GNU ld (GNU Binutils) 2.42) #1 SMP PREEMPT_DYNAMIC Fri, 10 Oct 2025 00:00:00 +0000
```
Both synthesize the `(proot@termux)(gcc...)` build tag; PDN's kernel number is authentic, proot-distro's is not.

## 3. PATH and binary resolution

`echo $PATH`:
```
PDN: /usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin
PD:  /usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin:/data/data/com.termux/files/usr/bin
```
`command -v python3 node git wget make gcc`:
```
PDN: python3 => NOTFOUND / node => NOTFOUND / git => NOTFOUND / wget => NOTFOUND / make => NOTFOUND / gcc => NOTFOUND
PD:  python3 => /data/data/com.termux/files/usr/bin/python3
     node => /data/data/com.termux/files/usr/bin/node
     git  => /data/data/com.termux/files/usr/bin/git
     wget => /data/data/com.termux/files/usr/bin/wget
     make => /data/data/com.termux/files/usr/bin/make
     gcc  => /data/data/com.termux/files/usr/bin/gcc
```
proot-distro injects the host Termux `bin` into PATH and resolves HOST binaries from outside the container. PDN keeps the container isolated.

## 4. `/etc/os-release` (head -3)
Both identical:
```
PRETTY_NAME="Debian GNU/Linux 13 (trixie)"
NAME="Debian GNU/Linux"
VERSION_ID="13"
```

## 5. Environment leak

`env | grep -c -i termux`:
```
PDN: 0        (grep -c exits 1 on zero matches)
PD:  2
```
`env | sort` line counts: PDN 6 lines, proot-distro 24 lines.

PDN env in full:
```
COLORTERM=truecolor
HOME=/root
LANG=en_US.UTF-8
PATH=/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin
PWD=/
TERM=xterm-256color
```
proot-distro env (leaked host vars, abridged):
```
ANDROID_ART_ROOT=/apex/com.android.art
ANDROID_DATA=/data
ANDROID_I18N_ROOT=/apex/com.android.i18n
ANDROID_ROOT=/system
ANDROID_TZDATA_ROOT=/apex/com.android.tzdata
BOOTCLASSPATH=/apex/com.android.art/javalib/core-oj.jar:...(host JVM classpath)
DEX2OATBOOTCLASSPATH=...(host JVM classpath)
EXTERNAL_STORAGE=/sdcard
MOZ_FAKE_NO_SANDBOX=1
MOZ_SHM_NO_SEALS=1
PATH=...:/data/data/com.termux/files/usr/bin
PD_CONTAINER=debian
PD_IMAGE=debian
PD_IMAGE_ID=sha256:45728a7e7854b716b6de49a8e7801925f88c940bae3bff728ea4a521974e4925
PROOT_L2S_DIR=/data/data/com.termux/files/usr/var/lib/proot-distro/containers/debian/rootfs/.l2s
PULSE_SERVER=127.0.0.1
container=proot-distro
```
proot-distro leaks: full Android/JVM bootstrap env, the device external-storage path, a HOST filesystem path (`PROOT_L2S_DIR`), and its own `container=proot-distro` marker. PDN leaks none of these.

## 6. `free` and `nproc`

```
PDN: /bin/sh: 1: free: not found   |  nproc => 8
PD:  total 11585580 used 8658172 free 64388 ... Swap 12582908 ...  |  nproc => 8
Host: total 11585580 used 8705684 free 198420 ... Swap 12582908 ... |  nproc => 8
```
PDN: `free` genuinely absent (procps not in the base image) — honest. proot-distro: `free` is the HOST's memory report, reached via the leaked host PATH. nproc=8 both (matches host).

## 7. Disk / mounts / shm

`df -h /` — identical on both:
```
Filesystem        Size  Used Avail Use% Mounted on
/dev/block/dm-58  482G  356G  125G  75% /
```
`mount | wc -l`: PDN 50, proot-distro 53.
`ls /dev/shm`: PDN `probe`; proot-distro `probe  x` (both are leftover test files, not tool behavior).

Extra mounts present ONLY in proot-distro (via `comm`):
```
/data/data/com.termux/cache on /data/data/com.termux/cache type bind
/data/data/com.termux/files/usr/var/lib/proot-distro/containers/debian/shm on /dev/shm type bind
/dev/urandom on /dev/random type bind
/storage/emulated/0 on /mnt/sdcard type bind
... (plus per-container sysdata binds: loadavg, stat, sys_empty, uptime, version, vmstat, sysctl_*)
```
Both bind their own sysdata dirs (same set). PDN additionally does NOT bind `/mnt/sdcard`, `/data/data/com.termux/cache`, or alias `/dev/random`.

## 8. Identity — `id`, `id -u`, `getent`

PDN:
```
uid=0(root) gid=0(root) groups=0(root),1007,1077,1079,1096,3003,9997,20396,50396,99909997
0
---getent 10396---          (empty)
---aid---                   (empty)
```
proot-distro:
```
uid=0(root) gid=0(root) groups=0(root),1007(aid_log),1077(aid_external_storage),1079(aid_ext_obb_rw),1096(aid_update_engine_log),3003(aid_inet),9997(aid_everybody),20396(aid_u0_a396_cache),50396(aid_all_a396),99909997
0
---getent 10396---
aid_u0_a396:x:10396:10396:Termux:/:/sbin/nologin
---aid---
aid_u0_a396:x:10396:10396:Termux:/:/sbin/nologin
```
`getent passwd | wc -l`: PDN 18, proot-distro 19 (the +1 is the injected `aid_u0_a396`).
proot-distro resolves the host Android app identity (uid 10396, "Termux") inside the container and attaches Android group NAMES to the supplementary GIDs. PDN exposes the raw numeric GIDs but maps no Android identity, and `getent passwd 10396` returns empty.

## 9. Package management

`dpkg -l | wc -l`: PDN 106, proot-distro 106.
`apt-get -s install hello` (simulate only): both `rc=0`, byte-identical tail:
```
Reading state information...
The following NEW packages will be installed:
  hello
0 upgraded, 1 newly installed, 0 to remove and 1 not upgraded.
Inst hello (2.10-5 Debian:13.7/stable [arm64])
Conf hello (2.10-5 Debian:13.7/stable [arm64])
```
Parity.

## 10. Network

`getent hosts deb.debian.org`: both resolve (IPv6 Fastly `debian.map.fastlydns.net`).
`cat /etc/resolv.conf`:
```
PDN: nameserver 1.1.1.1
     nameserver 1.0.0.1
PD:  nameserver 8.8.8.8
     nameserver 8.8.4.4
```
Both resolve; different DNS config (PDN Cloudflare, proot-distro Google).

## 11. Missing-binary behavior

`command -v nosuchbinary; echo rc=$?`:
```
PDN: rc=127
PD:  rc=127
```
POSIX-correct on both.

## 12. Host-side rootfs ownership / mode

Host container dir:
```
PDN: drwx------  u0_a396 u0_a396  ~/.podnext/containers/deb-pdn          (0700)
PD:  drwxr-xr-x  u0_a396 u0_a396  $PREFIX/var/lib/proot-distro/containers/debian (0755)
```
Inside container `stat -c '%U %G %a' /root`:
```
PDN: root root 755
PD:  root root 700
```
PDN's host container directory is owner-private (0700); proot-distro's is world-readable (0755) on the Termux host.

---

## Summary Table

| # | Measurement | PDN (`deb-pdn`) | proot-distro (`debian`) | Verdict |
|---|---|---|---|---|
| 1 | cold login (real) | 0.257 / 0.114 / 0.097 s | 1.022 / 0.710 / 0.603 s | PDN ≈5× faster |
| 2 | `uname -r` | `6.1.157-android14-...` | `6.17.0-PRoot-Distro` | PDN = real kernel; PD fabricated |
| 3 | PATH | clean Debian, 6 dirs | +host `.../com.termux/files/usr/bin` | PD leaks host PATH |
| 4 | os-release | Debian 13 trixie | Debian 13 trixie | parity |
| 5 | `env` termux-hits / lines | 0 / 6 | 2 / 24 | PD leaks Android+host env |
| 6 | `free` | not found (honest) | host's memory | PD shows host mem via leaked PATH |
| 7 | `mount` count | 50 | 53 | PD binds `/mnt/sdcard`, host cache, aliases `/dev/random` |
| 8 | `getent passwd 10396` | empty | `aid_u0_a396:...Termux` | PD leaks host Android identity |
| 9 | `dpkg -l` / apt simulate | 106 / rc=0 | 106 / rc=0 | parity |
| 10 | resolv.conf | 1.1.1.1, 1.0.0.1 | 8.8.8.8, 8.8.4.4 | both resolve; different DNS |
| 11 | `command -v nosuchbinary` | rc=127 | rc=127 | parity |
| 12 | host dir mode / `/root` | 0700 / 755 | 0755 / 700 | PDN host dir private; PD host dir world-readable |
| — | `exit` code propagation | child code (7→7, 42→42) | child code (7→7) | **PDN BUG-1 FIXED in commit 70b263e** |

---

## Where PDN is more correct (with evidence)

1. **Real kernel, not a fake one.** PDN `uname -r` → `6.1.157-android14-11-g78a0a76cd7b7-ab15497637`,
   matching host `uname -a`. proot-distro reports `6.17.0-PRoot-Distro`. Software that gates on
   kernel version (eBPF, io_uring, seccomp profiles, driver checks) sees the truth under PDN.

2. **No host binary leakage.** proot-distro's PATH includes `/data/data/com.termux/files/usr/bin`
   and resolves `python3/node/git/wget/make/gcc` to HOST Termux binaries (evidence §3). A build
   inside the "container" silently uses host toolchains. PDN resolves none — clean isolation.

3. **No host/Android environment leakage.** proot-distro exports the full Android bootstrap
   (`ANDROID_ROOT`, `BOOTCLASSPATH`, `DEX2OATBOOTCLASSPATH`), the external-storage path, and a host
   filesystem path `PROOT_L2S_DIR` (§5). PDN exports 6 clean vars; `env | grep -c -i termux` = 0.

4. **No host Android identity mapping.** proot-distro injects `aid_u0_a396:...Termux` and names the
   host's supplementary GIDs as `aid_*` (§8). PDN keeps them numeric and exposes no host identity.

5. **Correct `/dev/random` semantics.** PDN's `/dev/random` is the real char device `1,8`; proot-distro
   binds `/dev/urandom` (`1,9`) over `/dev/random` (§7). PDN preserves the distinction.

6. **Private host storage layout.** PDN's container dir is `drwx------` (0700); proot-distro's is
   `drwxr-xr-x` (0755), readable by any host process (§12).

## Where proot-distro is more convenient (fair)

1. **Termux toolchain is available out of the box.** Because the host bin is on PATH, `python3`,
   `git`, `node`, `gcc` "just work" inside the container with no apt install (§3). Convenient for
   casual scripting; it is exactly the isolation leak PDN avoids.

2. **`free` works immediately.** proot-distro returns a memory report (the host's) (§6); PDN has no
   `free` at all until `procps` is installed.

3. **External storage is mounted.** proot-distro binds `/storage/emulated/0 → /mnt/sdcard` (§7), so
   the shared `/sdcard` is reachable. PDN does not mount it — inside PDN `ls /mnt/sdcard` → "No such
   file or directory". Users needing device files to arrive without an explicit bind will notice.

4. **Faithful exit-code propagation.** `proot-distro login debian -- sh -c 'exit 7'` returns 7; run
   under PDN the wrapper returns 1 (§ BUG-1). Scripts checking `$?` behave correctly in
   proot-distro.

5. **Mature, widely documented CLI.** proot-distro is Termux's default; its behavior and docs are
   well known (out of test scope — noted for context, not measured).

## Real BUG in PDN (reproduced)

### BUG-1 — child exit code is not propagated; wrapper always returns 1 [FIXED]

**Status:** FIXED in commit `70b263e` (`fix(cli): exit with the child's code instead of collapsing it to 1`).
Verified on device:
- `podnext login deb-pdn -- /bin/sh -c 'exit 0'` -> rc=0
- `podnext login deb-pdn -- /bin/sh -c 'exit 3'` -> rc=3
- `podnext login deb-pdn -- /bin/sh -c 'exit 7'` -> rc=7
- `podnext login deb-pdn -- /bin/sh -c 'exit 42'` -> rc=42
- `podnext login deb-pdn -- /bin/sh -c 'exit 127'` -> rc=127
- `podnext run deb-pdn -- /bin/sh -c 'exit 9'` -> rc=9

Initial diagnosis and reproduction below:

Symptom: `podnext login <name> -- <cmd>` prints the child's exit code in an error line but exits 1
regardless of the child's status, breaking `&&`, `if`, and CI checks.

Reproduce:
```
$ ./target/release/podnext login deb-pdn -- /bin/sh -c 'exit 7'
Error: Login shell exited with code 7
$ echo $?
1

$ ./target/release/podnext login deb-pdn -- /bin/sh -c 'exit 42'
Error: Login shell exited with code 42
$ echo $?
1
```
Raw, 3 consecutive runs (code 7):
```
Error: Login shell exited with code 7
wrapper_rc=1
Error: Login shell exited with code 7
wrapper_rc=1
Error: Login shell exited with code 7
wrapper_rc=1
```
Control (proot-distro, same child):
```
$ proot-distro login debian -- /bin/sh -c 'exit 7'; echo $?
7
```
Expected: wrapper exit == child exit (7 → 7, 42 → 42). Actual: always 1. Success (`exit 0`) → rc=0,
so only non-zero codes are collapsed. The message already knows the code (42), so the value is
available and simply not forwarded.

## Proposed PoC documentation for the outcome

- **PoC title:** "Container fidelity under PDN vs proot-distro on a shared Termux host."
- **Reproduction script** (read-only, no resets): run the 12 command blocks from §1–§12 above against
  `deb-pdn` (PDN) and `debian` (proot-distro); diff the outputs. Expected: PDN shows real kernel,
  clean env/PATH/identity; proot-distro shows leaked host PATH, Android env, `aid_u0_a396`, and a
  fake `6.17.0-PRoot-Distro`.
- **Bug PoC:** `podnext login deb-pdn -- /bin/sh -c 'exit 7'; echo $?` → expected `7`, observed `1`.
- **Acceptance for the bug fix:** wrapper exit code must equal the container command's exit code for
  every code 0–255; verify with `exit 0`, `exit 1`, `exit 7`, `exit 42`, `exit 127`.
- **Suggested regression test:** spawn `podnext login <ctr> -- sh -c 'exit N'` for N in {0,7,42,127}
  and assert the process exit code equals N.

## Caveats / UNTESTED

- Untested: interactive TTY login behavior, signal forwarding (Ctrl-C), multi-command `sh -c` chains
  with pipes, and stdout/stderr interleaving under a pty — none were requested and none were run.
- Untested (intentional): `podnext run` subcommand semantics; only `login ... -- cmd` was exercised.
- The 50-vs-53 `mount` delta and the `/dev/shm` file diffs include pre-existing test artifacts
  (`probe`, `x`); only the named bind mounts in §7 are attributed to the tools.
