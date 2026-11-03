# Internal (in-container) capability test matrix — proot-distro 5.10.3 vs PDN `podnext` 0.1.7

- Date: 2026-11-03 (WIB), 2026-11-02T19:42Z
- Host: Termux / Android 16, aarch64, kernel `6.1.157-android14-11-...`
- Tools under test:
  - `/data/data/com.termux/files/usr/bin/proot-distro` (v5.10.3)
  - `/data/data/com.termux/files/home/workspace/PDN/target/release/podnext` (`podnext 0.1.7`)
- Both containers: `alpine`, rootfs Alpine `3.24.3`
  - proot-distro rootfs: `/data/data/com.termux/files/usr/var/lib/proot-distro/containers/alpine/rootfs`
  - PDN rootfs: `/data/data/com.termux/files/home/.podnext/containers/alpine/rootfs`
- Method: for every test the **identical** command was run both ways:
  - `proot-distro login alpine -- <CMD>`
  - `~/workspace/PDN/target/release/podnext login alpine -- <CMD>`

Notes on notation used below:

- `podnext` always emits a block of `DEBUG ...` / `[DEBUG ...]` chatter on startup. Those lines are elided for readability; **every other byte is verbatim**. The non-DEBUG errors (`Error: Login shell exited with code N`, `Error: ...`) are kept.
- Because PDN re-wraps argv (see finding #2), shell-script tests were run **three** ways:
  - `[PD]` — proot-distro, form `-- /bin/sh -c '<script>'`
  - `[PDN-literal]` — podnext, identical form `-- /bin/sh -c '<script>'`
  - `[PDN-single]` — podnext, `-- '<script>'` (whole script as one argv word; works around the wrap bug and exposes the *underlying* capability)

---

## Headline findings

1. **BUG — command re-wrap destroys quoting (PDN).** PDN joins the post-`--` argv with spaces and wraps the joined string in `/bin/sh -c` **without re-quoting**. Consequence: any argv token containing spaces/operators is re-split. A shell script passed as `/bin/sh -c '<script>'` collapses — the inner `-c` consumes only the next word. Simple single tokens (`id`, `uname -a`, `ls /etc`) are unaffected. Proof:
   ```
   $ podnext login alpine -- /bin/sh -c 'echo X'
   <blank line>
   ```
   ```
   $ podnext login alpine -- echo hello world
   hello world
   ```
   ```
   $ podnext login alpine -- /bin/echo A B
   A B
   ```
2. **BUG — host environment leaks into the container (PDN).** PDN inherits the caller's environment verbatim, including `LD_PRELOAD=/data/data/com.termux/files/usr/lib/libtermux-exec-ld-preload.so` and `LD_LIBRARY_PATH=…/hermes-agent/runtime-libs/lib:…`. Container-native musl binaries then fail to load. `apk` is completely broken (`exit 127`). Proof that the env is the sole cause:
   ```
   $ podnext login alpine -- 'env -u LD_PRELOAD -u LD_LIBRARY_PATH apk --version'
   apk-tools 3.0.8-r0, compiled for aarch64.
   ```
   The same leak exposes **live host secrets** (many `HERMES_*_API_KEY` values) inside the container (test J).
3. **GAP — no `/etc/resolv.conf` provisioning (PDN).** proot-distro writes a `resolv.conf` into its rootfs; PDN does not, and the host has none. `cat /etc/resolv.conf` fails in PDN.
4. **Identity differs by design-ish:** PDN runs as the **host uid** (`10396`/`u0_a396`), keeps **host `HOME`** and **host `PATH`**; proot-distro fakes **uid 0** with `HOME=/root` and a clean container env.
5. **Kernel/`/proc` masking differs:** proot-distro fakes `uname` (`6.17.0-PRoot-Distro`) and can read `/proc/version`; PDN reports the **real Android kernel** and gets `Permission denied` on `/proc/version`.
6. Parity (identical behaviour) on: `/etc/alpine-release`, `hostname`, `/dev` listing (denied both), `/sys/class` (denied both), `/proc/mounts`, symlink resolution, exit-code propagation, host-process visibility via `ps`.

---

## A. Identity / kernel

### A1 `id`
```
$ proot-distro login alpine -- id
uid=0(root) gid=0(root) groups=1007(aid_log),1077(aid_external_storage),1079(aid_ext_obb_rw),1096(aid_update_engine_log),3003(aid_inet),9997(aid_everybody),20396(aid_u0_a396_cache),50396(aid_all_a396),99909997
```
```
$ podnext login alpine -- id
uid=10396(u0_a396) gid=10396(u0_a396) groups=10396(u0_a396),1007(log),1077(external_storage),1079(ext_obb_rw),1096(update_engine_log),3003(inet),9997(everybody),20396(u0_a396_cache),50396(all_a396),99909997 context=u:r:untrusted_app_27:s0:c140,c257,c512,c768
```
**DIFFERENT.** proot-distro presents uid 0; PDN presents the real Termux uid and the SELinux context.

### A2 `uname -a`
```
$ proot-distro login alpine -- uname -a
Linux localhost 6.17.0-PRoot-Distro #1 SMP PREEMPT_DYNAMIC Fri, 10 Oct 2025 00:00:00 +0000 aarch64 Linux
```
```
$ podnext login alpine -- uname -a
Linux localhost 6.1.157-android14-11-g78a0a76cd7b7-ab15497637 #1 SMP PREEMPT Tue May 26 16:06:02 UTC 2026 aarch64 Android
```
**DIFFERENT.** proot-distro masks the kernel release; PDN leaks the host kernel string (and the literal token `Android`).

### A3 `cat /etc/alpine-release`
```
$ proot-distro login alpine -- cat /etc/alpine-release
3.24.3
```
```
$ podnext login alpine -- cat /etc/alpine-release
3.24.3
```
**PASS.**

### A4 `hostname`
```
$ proot-distro login alpine -- hostname
localhost
```
```
$ podnext login alpine -- hostname
localhost
```
**PASS.**

---

## B. Filesystem isolation

### B1 `ls /etc/` — must show `alpine-release`, not android `aconfig`/`audio_effects`
```
$ proot-distro login alpine -- /bin/sh -c 'ls /etc/'
alpine-release
apk
busybox-paths.d
crontabs
fstab
group
hostname
hosts
inittab
issue
logrotate.d
modprobe.d
modules
modules-load.d
motd
mtab
network
nsswitch.conf
opt
os-release
passwd
periodic
profile
profile.d
protocols
resolv.conf
secfixes.d
securetty
services
shadow
shells
ssl
ssl1.1
sysctl.conf
sysctl.d
udhcpc
```

`[PDN-literal]` — wrap bug fired; `/bin/sh -c ls` ran `ls` on cwd `/` instead of `/etc/`:
```
$ podnext login alpine -- /bin/sh -c 'ls /etc/'
apex
bin
data
dev
etc
home
lib
linkerconfig
media
mnt
odm
opt
proc
product
root
run
sbin
sdcard
srv
storage
sys
system
system_ext
tmp
usr
var
vendor
```

`[PDN-single]` — correct isolation, identical to proot-distro (34 alpine entries, **no android `aconfig`/`audio_effects`**):
```
$ podnext login alpine -- 'ls /etc/'
alpine-release
apk
busybox-paths.d
crontabs
fstab
group
hostname
hosts
inittab
issue
logrotate.d
modprobe.d
modules
modules-load.d
motd
mtab
network
nsswitch.conf
opt
os-release
passwd
periodic
profile
profile.d
protocols
secfixes.d
securetty
services
shadow
shells
ssl
ssl1.1
sysctl.conf
sysctl.d
udhcpc
```
**`/etc` isolation: PASS** (no host-android files leak into `/etc` in either). PDN-literal form FAILs only because of the wrap bug (finding #1). Note PDN's `/etc` also lacks `resolv.conf` (see E1) — the one file proot-distro injects.

### B2 `cat /etc/alpine-release`
Same as A3 — **PASS** both (`3.24.3`).

### B3 `ls / | head -30`
```
$ proot-distro login alpine -- /bin/sh -c 'ls / | head -30'
apex
bin
data
dev
etc
home
lib
linkerconfig
media
mnt
odm
opt
proc
product
root
run
sbin
sdcard
srv
storage
sys
system
system_ext
tmp
usr
var
vendor
```
`[PDN-single]` produced the **identical 27 entries** (same android dirs `apex/system/system_ext/vendor/odm/product` bound at `/`, same alpine dirs). `[PDN-literal]` also matched (by coincidence — cwd is `/`). **PASS / parity.**

---

## C. Filesystem write

```
$ proot-distro login alpine -- /bin/sh -c 'echo hi > /tmp/t.txt && cat /tmp/t.txt && rm /tmp/t.txt'
hi
```
```
$ podnext login alpine -- /bin/sh -c 'echo hi > /tmp/t.txt && cat /tmp/t.txt && rm /tmp/t.txt'

(blank line; wrap bug: `/bin/sh -c echo` with $0=hi)
```
```
$ podnext login alpine -- 'echo hi > /tmp/t.txt && cat /tmp/t.txt && rm /tmp/t.txt'
hi
```
**PASS** (write/read/remove works) — the lone PDN-literal blank line is the wrap bug, not a write failure.

---

## D. Process visibility

```
$ proot-distro login alpine -- /bin/sh -c 'ps aux 2>/dev/null | head -5; echo "--- fallback ---"; ps | head -5'
PID   USER     TIME  COMMAND
 3344 root      0:09 /data/data/com.termux/files/usr/lib/hermes-agent/venv/bin/python -m tui_gateway.slash_worker --session-key 20261027_142904_c87a7f --parent-pid 5701 --model tokenharbor/deepseek-v4.1-flash:free --provider custom:9router
 3809 root      0:00 {node-MainThread} node /data/data/com.termux/files/home/.npm/_npx/cd1ce99963b5e8b1/node_modules/.bin/mcp-server-postgres postgresql://u0_a396@127.0.0.1:5432/honcho
 3818 root      0:01 {node-MainThread} node /data/data/com.termux/files/home/.npm/_npx/a3241bba59c344f5/node_modules/.bin/mcp-server-filesystem /data/data/com.termux/files/home/workspace
 3820 root      0:05 npm exec @playwright/mcp@latest --executable-path /data/data/com.termux/files/usr/bin/chromium-browser --headless
--- fallback ---
... (same 4 lines)
```
```
$ podnext login alpine -- 'ps aux 2>/dev/null | head -5; echo "--- fallback ---"; ps | head -5'
USER       PID %CPU %MEM    VSZ   RSS TTY      STAT START   TIME COMMAND
u0_a396   3344  1.4  0.2 12092668 33140 ?      S<sl  2022   0:09 /data/data/com.termux/files/usr/lib/hermes-agent/venv/bin/python -m tui_gateway.slash_worker --session-key 20261027_142904_c87a7f --parent-pid 5701 --model tokenharbor/deepseek-v4.1-flash:free --provider custom:9router
u0_a396   3809  0.0  0.0 11948108 10064 ?      S<sl  2022   0:00 node /data/data/com.termux/files/home/.npm/_npx/cd1ce99963b5e8b1/node_modules/.bin/mcp-server-postgres postgresql://u0_a396@127.0.0.1:5432/honcho
u0_a396   3818  0.2  0.0 11925816 10040 ?      S<sl  2022   0:01 node /data/data/com.termux/files/home/.npm/_npx/a3241bba59c344f5/node_modules/.bin/mcp-server-filesystem /data/data/com.termux/files/home/workspace
u0_a396   3820  0.7  0.1 12030768 16244 ?      S<sl  2022   0:05 npm exec @playwright/mcp@latest --executable-path /data/data/com.termux/files/usr/bin/chromium-browser --headless
--- fallback ---
... (same 4 lines)
```
**PASS / parity.** Neither container has a PID namespace; both expose the full host process list. Only cosmetic format differs (proot-distro's rootfs carries `procps`; PDN's uses busybox `ps`, and PDN reports the real uid `u0_a396`).

---

## E. Network config

### E1 `cat /etc/resolv.conf`
```
$ proot-distro login alpine -- cat /etc/resolv.conf
nameserver 8.8.8.8
nameserver 8.8.4.4
```
```
$ podnext login alpine -- cat /etc/resolv.conf
cat: /etc/resolv.conf: No such file or directory
Error: Login shell exited with code 1
```
**FAIL (PDN).** Host has no `/etc/resolv.conf` either (`cat: /etc/resolv.conf: No such file or directory`). proot-distro **generates** one in its rootfs (38 bytes, present in the proot-distro rootfs at `.../rootfs/etc/resolv.conf`); PDN does not create one. Root cause for both containers: the host relies on Android netd, not a file.

### E2 `cat /etc/hosts`
```
$ proot-distro login alpine -- cat /etc/hosts
# IPv4.
127.0.0.1   localhost.localdomain localhost

# IPv6.
::1         localhost.localdomain localhost ip6-localhost ip6-loopback
fe00::0     ip6-localnet
ff00::0     ip6-mcastprefix
ff02::1     ip6-allnodes
ff02::2     ip6-allrouters
ff02::3     ip6-allhosts
```
```
$ podnext login alpine -- cat /etc/hosts
127.0.0.1	localhost localhost.localdomain
::1		localhost localhost.localdomain
```
**DIFFERENT.** proot-distro uses the Alpine default hosts file. PDN's 2-line file (`.../rootfs/etc/hosts`) is a reduced variant. Host's own `/etc/hosts` is `127.0.0.1 localhost` / `::1 ip6-localhost`, so PDN is *not* simply binding the host file either.

### E3 DNS tools
```
$ proot-distro login alpine -- /bin/sh -c 'command -v nslookup || echo NO_nslookup; command -v getent || echo NO_getent; nslookup example.com 2>&1 | head -10'
/usr/bin/nslookup
/usr/bin/getent
Server:		8.8.8.8
Address:	8.8.8.8:53

Non-authoritative answer:
Name:	example.com
Address: 2606:4700:10::ac42:93f3
Name:	example.com
Address: 2606:4700:10::6814:179a

Non-authoritative answer:
```
```
$ podnext login alpine -- /bin/sh -c 'command -v nslookup || echo NO_nslookup; command -v getent || echo NO_getent; nslookup example.com 2>&1 | head -10'
/usr/bin/getent
/bin/sh: 1: nslookup: not found
```
`[PDN-single]`:
```
$ podnext login alpine -- 'command -v nslookup || echo NO_nslookup; command -v getent || echo NO_getent; nslookup example.com 2>&1 | head -10'
/usr/bin/nslookup
/usr/bin/getent
/bin/sh: 1: nslookup: not found
```
**FAIL (PDN).** In PDN, `nslookup` (a symlink → `/bin/busybox`) fails to exec (`not found`, exit 127) even though `command -v` resolves it, and `getent` (a real 67528-byte musl ELF at `/usr/bin/getent`) also fails to exec:
```
$ podnext login alpine -- 'env -u LD_PRELOAD -u LD_LIBRARY_PATH /usr/bin/getent hosts one.one.one.one; echo rc=$?'
env: ‘/usr/bin/getent’: No such file or directory
rc=127
```
The binaries are present and exec'able-looking on disk (`/bin/busybox` 919304 bytes, `/usr/bin/getent` 67528 bytes) yet return `ENOENT` at exec time under PDN — consistent with the host `LD_PRELOAD`/loader leak (finding #2). **DNS is effectively unusable inside PDN.** Verification of DNS through PDN was therefore **not established**.

---

## F. Package manager

### F1 `apk --version`
```
$ proot-distro login alpine -- apk --version
apk-tools 3.0.8-r0, compiled for aarch64.
```
```
$ podnext login alpine -- apk --version
Error relocating /data/data/com.termux/files/usr/lib/libtermux-exec-ld-preload.so: __register_atfork: symbol not found
Error relocating /data/data/com.termux/files/usr/lib/libtermux-exec-ld-preload.so: __errno: symbol not found
Error relocating /data/data/com.termux/files/usr/lib/libtermux-exec-ld-preload.so: __strlen_chk: symbol not found
Error relocating /data/data/com.termux/files/usr/lib/libtermux-exec-ld-preload.so: __open_2: symbol not found
Error relocating /data/data/com.termux/files/usr/lib/libtermux-exec-ld-preload.so: __read_chk: symbol not found
Error relocating /data/data/com.termux/files/usr/lib/libtermux-exec-ld-preload.so: __strcpy_chk: symbol not found
Error relocating /data/data/com.termux/files/usr/lib/libtermux-exec-ld-preload.so: __system_property_get: symbol not found
Error relocating /data/data/com.termux/files/usr/lib/libtermux-exec-ld-preload.so: __vsnprintf_chk: symbol not found
Error relocating /data/data/com.termux/files/usr/lib/libtermux-exec-ld-preload.so: __gnu_basename: symbol not found
Error relocating /data/data/com.termux/files/usr/lib/libtermux-exec-ld-preload.so: __readlink_chk: symbol not found
Error relocating /data/data/com.termux/files/usr/lib/libtermux-exec-ld-preload.so: __umask_chk: symbol not found
Error relocating /data/data/com.termux/files/usr/lib/hermes-agent/runtime-libs/lib/libz.so.1: __register_atfork: symbol not found
Error relocating /data/data/com.termux/files/usr/lib/hermes-agent/runtime-libs/lib/libz.so.1: __errno: symbol not found
Error relocating /data/data/com.termux/files/usr/lib/hermes-agent/runtime-libs/lib/libssl.so.3: __register_atfork: symbol not found
Error relocating /data/data/com.termux/files/usr/lib/hermes-agent/runtime-libs/lib/libssl.so.3: __errno: symbol not found
Error relocating /data/data/com.termux/files/usr/lib/hermes-agent/runtime-libs/lib/libssl.so.3: __system_property_get: symbol not found
Error relocating /data/data/com.termux/files/usr/lib/hermes-agent/runtime-libs/lib/libcrypto.so.3: __register_atfork: symbol not found
Error relocating /data/data/com.termux/files/usr/lib/hermes-agent/runtime-libs/lib/libcrypto.so.3: __errno: symbol not found
Error relocating /data/data/com.termux/files/usr/lib/hermes-agent/runtime-libs/lib/libcrypto.so.3: __cmsg_nxthdr: symbol not found
Error relocating /data/data/com.termux/files/usr/lib/hermes-agent/runtime-libs/lib/libcrypto.so.3: __system_property_get: symbol not found
Error: Login shell exited with code 127
```
**FAIL (PDN), exit 127.** Cause confirmed to be the leaked preload/library env:
```
$ podnext login alpine -- 'env -u LD_PRELOAD -u LD_LIBRARY_PATH apk --version'
apk-tools 3.0.8-r0, compiled for aarch64.
```

### F2 `apk update` (network)
```
$ proot-distro login alpine -- apk update
v3.24.3-108-gaf0fa42036c [https://dl-cdn.alpinelinux.org/alpine/v3.24/main]
v3.24.3-110-g168708e274c [https://dl-cdn.alpinelinux.org/alpine/v3.24/community]
OK: 28571 distinct packages available
```
```
$ podnext login alpine -- apk update
Error relocating /data/data/com.termux/files/usr/lib/libtermux-exec-ld-preload.so: __register_atfork: symbol not found
Error relocating /data/data/com.termux/files/usr/lib/libtermux-exec-ld-preload.so: __errno: symbol not found
Error relocating /data/data/com.termux/files/usr/lib/libtermux-exec-ld-preload.so: __strlen_chk: symbol not found
Error relocating /data/data/com.termux/files/usr/lib/libtermux-exec-ld-preload.so: __open_2: symbol not found
Error relocating /data/data/com.termux/files/usr/lib/libtermux-exec-ld-preload.so: __read_chk: symbol not found
Error relocating /data/data/com.termux/files/usr/lib/libtermux-exec-ld-preload.so: __strcpy_chk: symbol not found
Error relocating /data/data/com.termux/files/usr/lib/libtermux-exec-ld-preload.so: __system_property_get: symbol not found
Error relocating /data/data/com.termux/files/usr/lib/libtermux-exec-ld-preload.so: __vsnprintf_chk: symbol not found
Error relocating /data/data/com.termux/files/usr/lib/libtermux-exec-ld-preload.so: __gnu_basename: symbol not found
Error relocating /data/data/com.termux/files/usr/lib/libtermux-exec-ld-preload.so: __readlink_chk: symbol not found
Error relocating /data/data/com.termux/files/usr/lib/libtermux-exec-ld-preload.so: __umask_chk: symbol not found
Error relocating /data/data/com.termux/files/usr/lib/hermes-agent/runtime-libs/lib/libz.so.1: __register_atfork: symbol not found
Error relocating /data/data/com.termux/files/usr/lib/hermes-agent/runtime-libs/lib/libz.so.1: __errno: symbol not found
Error relocating /data/data/com.termux/files/usr/lib/hermes-agent/runtime-libs/lib/libssl.so.3: __register_atfork: symbol not found
Error relocating /data/data/com.termux/files/usr/lib/hermes-agent/runtime-libs/lib/libssl.so.3: __errno: symbol not found
Error relocating /data/data/com.termux/files/usr/lib/hermes-agent/runtime-libs/lib/libssl.so.3: __system_property_get: symbol not found
Error relocating /data/data/com.termux/files/usr/lib/hermes-agent/runtime-libs/lib/libcrypto.so.3: __register_atfork: symbol not found
Error relocating /data/data/com.termux/files/usr/lib/hermes-agent/runtime-libs/lib/libcrypto.so.3: __errno: symbol not found
Error relocating /data/data/com.termux/files/usr/lib/hermes-agent/runtime-libs/lib/libcrypto.so.3: __cmsg_nxthdr: symbol not found
Error relocating /data/data/com.termux/files/usr/lib/hermes-agent/runtime-libs/lib/libcrypto.so.3: __system_property_get: symbol not found
Error: Login shell exited with code 127
```
**FAIL (PDN), exit 127** — same env-leak cause as F1, not a network problem (proot-distro reached the CDN fine, so the host network path works; PDN never got that far).

---

## G. Shell builtins + PATH

```
$ proot-distro login alpine -- /bin/sh -c 'echo $PATH; which sh; command -v ls'
/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin:/data/data/com.termux/files/usr/bin
/bin/sh
/bin/ls
```
```
$ podnext login alpine -- 'echo $PATH; which sh; command -v ls'
/data/data/com.termux/files/home/.local/bin:/data/data/com.termux/files/home/.local/bin:/data/data/com.termux/files/usr/lib/hermes-agent/bin:/data/data/com.termux/files/home/.hermes/installs/1765bb811b147ca4/environments/0c31fdd4fe4d4362bafe3753a3835051/venv/bin:/data/data/com.termux/files/home/.local/bin:/data/data/com.termux/files/usr/bin:/opt/homebrew/bin:/opt/homebrew/sbin:/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin:/data/data/com.termux/files/usr/lib/hermes-agent/tools/python/data/data/com.termux/files/usr/bin:/data/data/com.termux/files/usr/lib/hermes-agent/tools/ffmpeg/data/data/com.termux/files/usr/bin:/data/data/com.termux/files/usr/lib/hermes-agent/tools/npm/bin:/data/data/com.termux/files/usr/lib/hermes-agent/tools/node/data/data/com.termux/files/usr/bin:/data/data/com.termux/files/usr/lib/hermes-agent/tools/ripgrep
/data/data/com.termux/files/usr/bin/sh
/data/data/com.termux/files/usr/bin/ls
```
**DIFFERENT.** `[PDN-literal]` mangled this (wrap bug → blank first line). `[PDN-single]` runs but with the **host** `PATH` and resolves `sh`/`ls` to **host Termux binaries** (`/data/data/com.termux/files/usr/bin/sh`, not `/bin/sh`). proot-distro prepends clean container paths and resolves to container binaries. The host PATH entries that win are unsafe combined with the env leak.

---

## H. `/proc` and `/sys` visibility

```
$ proot-distro login alpine -- /bin/sh -c 'ls /proc | head -5; head -1 /proc/version; ls /sys/class | head -5'
ls: /proc/fm: Permission denied
ls: /proc/mv: Permission denied
... (many "Permission denied", then:)
12033
12041
12167
12283
12285
Linux version 6.17.0-PRoot-Distro (proot@termux) (gcc (GCC) 13.3.0, GNU ld (GNU Binutils) 2.42) #1 SMP PREEMPT_DYNAMIC Fri, 10 Oct 2025 00:00:00 +0000
ls: can't open '/sys/class': Permission denied
```
`[PDN-single]`:
```
$ podnext login alpine -- 'ls /proc | head -5; head -1 /proc/version; ls /sys/class | head -5'
12033
12041
12167
12283
12285
head: cannot open '/proc/version' for reading: Permission denied
ls: cannot open directory '/sys/class': Permission denied
```
**DIFFERENT.** proot-distro can read `/proc/version` and returns its **faked** kernel banner; PDN gets `Permission denied`. `/sys/class` is denied in both. Both expose host PIDs under `/proc`. (`[PDN-literal]` was corrupted by the wrap bug.)

---

## I. `/dev`

```
$ proot-distro login alpine -- /bin/sh -c 'ls /dev | head -10'
ls: can't open '/dev': Permission denied
```
`[PDN-single]`:
```
$ podnext login alpine -- 'ls /dev | head -10'
ls: cannot open directory '/dev': Permission denied
```
**PASS / parity** — both deny `/dev` listing. (`[PDN-literal]` was corrupted by the wrap bug.)

---

## J. Environment variables

proot-distro container env (`env | sort | head -40`) — clean, container-owned, plus the standard `PD_*` variables:
```
ANDROID_ART_ROOT=/apex/com.android.art
ANDROID_DATA=/data
ANDROID_I18N_ROOT=/apex/com.android.i18n
ANDROID_ROOT=/system
ANDROID_TZDATA_ROOT=/apex/com.android.tzdata
BOOTCLASSPATH=/apex/... [long dalvik classpath, truncated] ...
COLORTERM=truecolor
DEX2OATBOOTCLASSPATH=/apex/... [long dalvik classpath, truncated] ...
EXTERNAL_STORAGE=/sdcard
HOME=/root
MOZ_FAKE_NO_SANDBOX=1
MOZ_SHM_NO_SEALS=1
PATH=/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin:/data/data/com.termux/files/usr/bin
PD_CONTAINER=alpine
PD_IMAGE=alpine
PD_IMAGE_ID=sha256:f7bbaec1bd0330a8dba26f03e05f421bb3c8b807bb41873df3c3b0c3d1083d8f
PROOT_L2S_DIR=/data/data/com.termux/files/usr/var/lib/proot-distro/containers/alpine/rootfs/.l2s
PULSE_SERVER=127.0.0.1
PWD=/root
SHLVL=2
TERM=xterm-256color
USER=root
container=proot-distro
```

PDN container env (`env | sort | head -40`) — **verbatim copy of the caller's host environment** (values redacted below where they are credentials; two long `*CLASSPATH` variables truncated):
```
AI_AGENT=hermes-agent
ANDROID_ART_ROOT=/apex/com.android.art
ANDROID_ASSETS=/system/app
ANDROID_DATA=/data
ANDROID_I18N_ROOT=/apex/com.android.i18n
ANDROID_ROOT=/system
ANDROID_STORAGE=/storage
ANDROID_TZDATA_ROOT=/apex/com.android.tzdata
ANDROID__BUILD_VERSION_SDK=36
APHRODITE_DIRECTIVES_DIR=/data/data/com.termux/files/home/.hermes/aphrodite/directives
APHRODITE_HOME=/data/data/com.termux/files/home/.hermes/aphrodite
ASEC_MOUNTPOINT=/mnt/asec
BOOTCLASSPATH=/apex/... [long dalvik classpath, truncated] ...
BROWSER_INACTIVITY_TIMEOUT=120
COLORTERM=truecolor
DEX2OATBOOTCLASSPATH=/apex/... [long dalvik classpath, truncated] ...
EXTERNAL_STORAGE=/sdcard
GIT_PAGER=cat
HERMES_ACCEPT_HOOKS=1
HERMES_AGENT=true
HERMES_BROWSER_CONTROL_PRINCIPAL=
HERMES_BROWSER_CONTROL_TRANSPORT_FAMILY=
HERMES_CRON_SESSION=
HERMES_CUSTOM_API_B_AI_API_KEY=«redacted»
HERMES_CUSTOM_API_ORCAROUTER_AI_API_KEY=«redacted»
HERMES_CUSTOM_DASHSCOPE_INTL_ALIYUNCS_COM_API_KEY=«redacted»
HERMES_CUSTOM_INTEGRATE_API_NVIDIA_COM_API_KEY=«redacted»
HERMES_CUSTOM_LOCALHOST_20128_API_KEY=«redacted»
HERMES_CUSTOM_OPENCODE_AI_API_KEY=«redacted»
HERMES_CUSTOM_TOKENHARBOR_AI_API_KEY=«redacted»
HERMES_CUSTOM_WS_BHT4N9KRA4TKJKDW_AP_SOUTHEAST_1_MAAS_ALIYUNCS_COM_API_KEY=«redacted»
HERMES_CUSTOM_WS_WENGCC4NZSHEM5FJ_AP_SOUTHEAST_1_MAAS_ALIYUNCS_COM_API_KEY=«redacted»
HERMES_CWD=/data/data/com.termux/files/home
HERMES_DELEGATED_CHILD_CONTEXT=/data/data/com.termux/files/home/.hermes
HERMES_EXEC_ASK=1
HERMES_GATEWAY_BUSY_INPUT_MODE=queue
HERMES_GATEWAY_PLATFORM_CONNECT_TIMEOUT=30
HERMES_GATEWAY_SESSION=1
HERMES_INTERACTIVE=1
HERMES_KANBAN_BOARD=default
...
```
**DIFFERENT.** proot-distro builds a clean, minimal, container-scoped env. PDN passes the **entire host environment** through — including `LD_PRELOAD`/`LD_LIBRARY_PATH` (breaks F1/F2/E3) and multiple live credential variables (`*_API_KEY`). The full host env was captured; only synthetic redaction of credential values is applied above. Related identity values inside PDN: `pwd=/`, `HOME=/data/data/com.termux/files/home` (host home).

---

## K. Interactive-shell emulation (multi-command through a shell)

```
$ proot-distro login alpine -- /bin/sh -c 'cd / && pwd && echo OK'
/
OK
```
```
$ podnext login alpine -- /bin/sh -c 'cd / && pwd && echo OK'
/
OK
```
**PASS** (both forms). (The PDN-literal form happened to survive here.)

---

## L. Exit codes

```
$ proot-distro login alpine -- /bin/sh -c '/bin/false; echo "exit=$?"; /bin/true; echo "exit=$?"'
exit=1
exit=0
```
```
$ podnext login alpine -- '/bin/false; echo "exit=$?"; /bin/true; echo "exit=$?"'
exit=1
exit=0
```
**PASS / parity.**

---

## M. Mount visibility

`[PD]` and `[PDN-single]` returned the **identical** first 20 lines of `/proc/mounts` (host view; no container-private mounts visible):
```
$ proot-distro login alpine -- /bin/sh -c 'cat /proc/mounts | head -20'
/dev/block/dm-10 / erofs ro,seclabel,relatime,user_xattr,acl,cache_strategy=readaround 0 0
tmpfs /dev tmpfs rw,seclabel,nosuid,relatime,size=5766260k,nr_inodes=1441565,mode=755 0 0
devpts /dev/pts devpts rw,seclabel,relatime,mode=600,ptmxmode=000 0 0
none /dev/blkio cgroup rw,nosuid,nodev,noexec,relatime,blkio 0 0
none /dev/cpuctl cgroup rw,nosuid,nodev,noexec,relatime,cpu 0 0
none /dev/cpuset cgroup rw,nosuid,nodev,noexec,relatime,cpuset,noprefix,cpuset_v2_mode 0 0
none /dev/memcg cgroup rw,nosuid,nodev,noexec,relatime,memory 0 0
binder /dev/binderfs binder rw,relatime,max=1048576,stats=global 0 0
adb /dev/usb-ffs/adb functionfs rw,seclabel,relatime 0 0
mtp /dev/usb-ffs/mtp functionfs rw,seclabel,relatime 0 0
ptp /dev/usb-ffs/ptp functionfs rw,seclabel,relatime 0 0
proc /proc proc rw,relatime,gid=3009,hidepid=invisible 0 0
sysfs /sys sysfs rw,seclabel,relatime 0 0
selinuxfs /sys/fs/selinux selinuxfs rw,relatime 0 0
none /sys/fs/cgroup cgroup2 rw,nosuid,nodev,noexec,relatime,memory_recursiveprot 0 0
tracefs /sys/kernel/tracing tracefs rw,seclabel,relatime,gid=3012 0 0
none /sys/fs/fuse/connections fusectl rw,relatime 0 0
bpf /sys/fs/bpf bpf rw,nosuid,nodev,noexec,relatime 0 0
pstore /sys/fs/pstore pstore rw,seclabel,nosuid,nodev,noexec,relatime 0 0
tmpfs /mnt tmpfs rw,seclabel,nosuid,nodev,noexec,relatime,size=5766260k,nr_inodes=1441565,mode=755,gid=1000 0 0
```
`[PDN-literal]` produced **no output** (wrap bug: `cat` with the path eaten). **PASS / parity** via the working form; no PDN fake binds surfaced in the first 20 entries.

---

## N. Symlink resolution

```
$ proot-distro login alpine -- /bin/sh -c 'readlink -f /bin/sh; readlink -f /bin/busybox'
/bin/busybox
/bin/busybox
```
`[PDN-literal]` (wrap bug: `readlink` got no operand):
```
$ podnext login alpine -- /bin/sh -c 'readlink -f /bin/sh; readlink -f /bin/busybox'
readlink: missing operand
Try 'readlink --help' for more information.
/bin/busybox
```
`[PDN-single]`:
```
$ podnext login alpine -- 'readlink -f /bin/sh; readlink -f /bin/busybox'
/bin/busybox
/bin/busybox
```
**PASS** via the working form — `/bin/sh` → `/bin/busybox` in both.

---

## Summary table

`PASS` = matches proot-distro / expected. `FAIL` = broken output. `DIFF` = works but behaves differently.

| Test | proot-distro | PDN | delta |
|---|---|---|---|
| A1 `id` | uid=0(root) | uid=10396(u0_a396) + SELinux ctx | **DIFF** |
| A2 `uname -a` | faked `6.17.0-PRoot-Distro` | real `6.1.157-android14… Android` | **DIFF** |
| A3 `alpine-release` | 3.24.3 | 3.24.3 | PASS |
| A4 `hostname` | localhost | localhost | PASS |
| B1 `ls /etc/` isolation | alpine, no android | FAIL literal (wrap bug) / PASS single | PASS* |
| B2 `alpine-release` | 3.24.3 | 3.24.3 | PASS |
| B3 `ls /` | 27 entries | identical | PASS |
| C fs write | `hi` | FAIL literal / PASS single | PASS* |
| D `ps` | host procs (procps) | host procs (busybox) | PASS (fmt DIFF) |
| E1 `/etc/resolv.conf` | `8.8.8.8`/`8.8.4.4` | No such file (exit 1) | **FAIL** |
| E2 `/etc/hosts` | alpine default | 2-line reduced | **DIFF** |
| E3 DNS (`nslookup`/`getent`) | resolves example.com | `nslookup: not found`; `getent` ENOENT | **FAIL** |
| F1 `apk --version` | 3.0.8-r0 | relocation errors, exit 127 | **FAIL** |
| F2 `apk update` | OK 28571 pkgs | relocation errors, exit 127 | **FAIL** |
| G PATH / `which` | container paths/binaries | host PATH + host binaries | **DIFF** |
| H `/proc`,`/sys` | `/proc/version`=faked kernel | `/proc/version` Permission denied | **DIFF** |
| I `/dev` | Permission denied | Permission denied | PASS |
| J `env` | clean container env | full host env incl. secrets+LDPRELOAD | **DIFF** |
| K shell emulation | `/` `OK` | `/` `OK` | PASS |
| L exit codes | 1 / 0 | 1 / 0 | PASS |
| M `/proc/mounts` | host mount list | identical | PASS |
| N symlinks | `/bin/busybox` x2 | FAIL literal / PASS single | PASS* |

`PASS*` = passes when the command is supplied in a form PDN does not re-split; the literal task form fails purely due to finding #1.

---

## PDN-vs-proot-distro deltas: bug vs intentional

| Delta | Assessment |
|---|---|
| **Command re-wrap breaks quoting** (`/bin/sh -c '<script>'` → inner `-c` eats one word) | **BUG.** Unintended. Any multi-word / operator-bearing argv is corrupted. |
| **Host `LD_PRELOAD` / `LD_LIBRARY_PATH` inherited** → container musl binaries (`apk`, `busybox`, `getent`, `nslookup`) fail to load (`exit 127`, relocation errors, `ENOENT`) | **BUG.** Unintended. Proven by `env -u LD_PRELOAD -u LD_LIBRARY_PATH apk --version` succeeding. Makes `apk` and DNS unusable. |
| **Host env (incl. `*_API_KEY` secrets) passed wholesale into the container** | **Likely unintended leak** (security). If env pass-through is intended, it should be an allow-list, not the full host env. |
| **No `/etc/resolv.conf` provisioning** | **GAP/omission** vs proot-distro (which writes one). Not necessarily a bug, but DNS is broken without it. |
| **Runs as host uid, keeps host `HOME`, host `PATH`**; `pwd=/` vs `/root` | **Intentionally different** — matches the "PDN binds host HOME by design" design note; but the collateral host-`PATH` resolution to host binaries compounds the env-leak bug. |
| **Real kernel string exposed; `/proc/version` unreadable** | **Intentionally different** (PDN does not fake `uname`); the `/proc/version` permission denial is a side effect of running as the real (non-root) uid. |
| **`/etc/hosts` reduced 2-line file** | **Minor divergence** — cosmetic/benign; neither host- nor Alpine-default. |
| **`ps`/`id` report real uid** | **Intentionally different** — consequence of not faking uid 0. |
| **`/dev` denied, `/sys/class` denied, `/proc/mounts` = host view, no PID namespace** | **Parity** — proot (both implementations) does not namespace these. |

---

## Reproduction

Harness (POSIX `sh`) used for each test:

```
proot-distro login alpine -- "$@"
~/workspace/PDN/target/release/podnext login alpine -- "$@"
```

Raw captured transcript: `~/.hermes/cache/scratch/raw/matrix.log` (683 lines).
NOTE: `podnext` DEBUG lines removed from all verbatim blocks above; every other byte is as returned by the tools.
