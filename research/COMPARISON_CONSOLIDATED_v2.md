# PDN vs proot-distro — Consolidated Capability Comparison (v2)

**Date**: 2026-11-03
**Host**: Termux / Android 16, aarch64
**Tools**: `proot-distro 5.10.3` (@sylirre, Python) vs `PDN podnext 0.1.7` (Rust)
**Method**: 4 parallel subagents — 2 source/posture inventories, 1 external (host-side) benchmark, 1 internal (in-container) matrix. All numbers are from live timed runs; raw logs kept in `research/`.

---

## Evidence bundles (do not delete)

| File | What it holds |
|---|---|
| `research/PROOT_DISTRO_RESEARCH.md` | proot-distro v5.10.3 inventory: every `--help`, `commands/*.py` reading, `proot_cmd.py` bind assembly, manifest fields, backup tar modes |
| `research/PDN_RESEARCH.md` | podnext 0.1.7 inventory: 31 top-level commands + all subcommand help, storage layout, per-module implementation-vs-stub audit, gaps list |
| `research/EXTERNAL_CAPABILITY_TESTS.md` | timed install/login/list/run/backup/search + feature probes (host side) |
| `research/INTERNAL_CAPABILITY_TESTS.md` | 14-test in-container matrix (identity, isolation, fs, net, pkg, env, /proc, /dev, symlinks, mounts) |

---

## 1. Performance (3 runs each, median, real seconds)

| Operation | proot-distro | PDN | Winner |
|---|---|---|---|
| install alpine (cold) | 10.109 | 8.868 | PDN (~12%) — mostly network-bound |
| `login -- echo hello` | 0.561 | 0.347 | PDN (~38%) |
| `list` | 0.677 | 0.034 | PDN (~95%) |
| `run -- echo hello` | 0.740 | 0.418 | PDN |
| backup (alpine) | 2.000 → 9,041,920 B | 3.205 → 4,631,494 B | PDN smaller; proot-distro faster |

Structural, not incidental: PDN is one static Rust binary; proot-distro is a Python package whose every command re-execs an interpreter.

---

## 2. Feature parity

| Capability | proot-distro | PDN | Note |
|---|---|---|---|
| install / list / login / run / remove / search | ✅ | ✅ | PDN `remove` was a silent no-op — **fixed** |
| ps / kill / rename / reset | ✅ | ❌ | PDN has `ps`/`kill`/`sessions`/`attach`; no `rename`/`reset` |
| copy / sync (host ↔ guest files) | ✅ | ❌ | **PDN gap** |
| backup / restore | ✅ | ⚠️ | PDN backup creates archives; `restore` does not yet rebuild a container |
| build / push (OCI) | ✅ | ⚠️ | PDN `build`/`gitops` are stubs |
| `--shared-home/--shared-tmp/--isolated/--minimal/--bind` on login | ✅ | ❌ | PDN has `-b/--bind` on the direct-exec path only |
| service supervision (restart policy, backoff, circuit breaker) | ❌ | ✅ | PDN exclusive |
| network / volume / quota (cgroups) / structured log / D-Bus / socket / boot / wakelock / policy | ❌ | ✅ | PDN exclusive (several are partial, see §4) |

---

## 3. In-container behaviour (14-test matrix)

**Parity (both correct):** `/etc/alpine-release` = 3.24.3 · `hostname` = localhost · `/etc` isolation (no Android files leak) · `ls /` identical · filesystem write · `ps` · `/dev` denied · `/proc/mounts` identical view · exit codes (1/0) · `/bin/sh → /bin/busybox` · shell emulation · **`/proc/uptime`, `/proc/loadavg`, `/proc/stat` byte-identical after the sysdata fix** (`124.08 932.76`, `0.12 0.07 0.02 2/165 765`, `cpu  1954 0 2872 93276 259 340 250 83 0 0`).

**Differences, and what they are:**

| Delta | Verdict |
|---|---|
| PDN runs as host uid (`u0_a396`), keeps host `HOME`/`PATH`; proot-distro fakes uid 0 with `HOME=/root` | **By design** (PDN does not fake identity). Consequence: guest `PATH` could resolve host binaries — the reason the env leak below mattered. |
| PDN reports the real Android kernel in `uname`; proot-distro fakes `6.17.0-PRoot-Distro` | **By design** |
| `/proc/version` unreadable in PDN | **FIXED** — PDN now fabricates the blocked `/proc` entries the same way proot-distro does (see §4 row 12) |
| `/etc/hosts` reduced to 2 lines | Minor divergence, benign |

**Genuine defects found (all reproduced live):**

1. **Host env leaked into the guest** — including live `*_API_KEY` values, plus Termux's `LD_PRELOAD` (libtermux-exec) and `LD_LIBRARY_PATH`. Those two are host-ABI, so every guest musl binary died: `apk`, `busybox`, `getent`, `nslookup` → `relocation error`, exit 127. → **FIXED**
2. **Command re-wrapping corrupted quoting** — argv was space-joined then wrapped in `/bin/sh -c` with no re-quoting, so `/bin/sh -c '<script>'` collapsed (the inner `-c` ate a word). → **FIXED**
3. **No `/etc/resolv.conf` provisioned** — the guest had no DNS at all (`apk update` → temporary error), where proot-distro writes one at install. → **FIXED**
4. **`podnext remove` was a silent no-op** — printed "removed", exit 0, container stayed on disk (and the next `install` then said "already exists"). → **FIXED**
5. **No fake `/proc` entries** — every `/proc` file Android blocks (`uptime`, `stat`, `loadavg`, `version`, `vmstat`, `cap_last_cap`) returned `Permission denied` in the guest, so `top`/`htop`/`free`/`uptime` read nothing. proot-distro solves this with a `sysdata/` dir of stubs bound over the real paths; PDN had a `SysdataManager` that wrote the stubs **into the rootfs**, where `--bind=/proc` masks them — dead code that never ran. → **FIXED** by reimplementing it to write beside the rootfs and bind, gated on the host file actually being unreadable.

---

## 4. PDN quality issues (post-fix status)

| # | Issue | Status |
|---|---|---|
| 1 | `version`/`info` printed hardcoded `0.1.0` while `--version` said `0.1.7` | **FIXED** — now `env!("CARGO_PKG_VERSION")` (5 sites incl. HTTP User-Agent) |
| 2 | `list` reported `ubuntu 22.04` for every container | **FIXED** — read `image`/`tag` from `metadata.json` |
| 3 | `backup create` dropped ~335 symlinks (tar followed them into dangling targets) | **FIXED** — `Builder::follow_symlinks(false)` |
| 4 | `backup list` always empty (`load_backups` was a no-op stub) | **FIXED** — index rebuilt from the archives (`<id>.tar.gz`) + `<id>.meta.json` sidecar written at backup time |
| 5 | ~280 kB of `DEBUG` per backup, plus a DEBUG banner on every command | **FIXED** — all gated behind `PN_DEBUG=1` |
| 5b | No fake `/proc` entries (Android blocks them; guest tools read nothing) | **FIXED** — `crates/proot-distro-core/src/sysdata.rs` rewritten: stubs written beside the rootfs and bound over the real paths, only where the host file is unreadable; wired into install, execute and start |
| 6 | volume mountpoint hardcoded `/var/lib/podnext/volumes/...` (does not exist on Android) | **FIXED** — `~/.podnext/volumes/<name>`, directory created on `volume create` |
| 7 | `restore` does not recreate a container (`restore_container` returns a random UUID) | **OPEN** — documented stub |
| 8 | `build`, `gitops`, `dashboard start/stop/status`, `daemon stop/reload/logs/status`, `log tail`, `dbus call/monitor`, `policy eval/load`, `socket list/unregister` | **OPEN** — documented stubs |
| 9 | `quota`/`watchdog`/`wakelock`/`policy` state not persistent (fresh manager per invocation) | **OPEN** |
| 10 | help text advertises `watchdog policy set`, real verb is `setpolicy`; `run` lists `-r` twice | **OPEN** — cosmetic |
| 11 | `copy`/`sync`/`rename`/`reset` have no PDN equivalent | **OPEN** — feature gap |

---

## 5. Where the fixes live

| Fix | File |
|---|---|
| proot invocation: `--rootfs=.` + `cwd(rootfs)`, Android system binds, allow-listed guest env, absolute `proot` resolution, POSIX quoting | `crates/proot-core/src/lib.rs` |
| `/etc/resolv.conf` + `/etc/hosts` + `/etc/hostname` at install; `remove` deletes the directory; version strings | `crates/podnext/src/native.rs`, `crates/podnext/src/main.rs` |
| `list` reads image identity from `metadata.json` | `crates/proot-distro-core/src/container.rs` |
| volume mountpoint under `~/.podnext/volumes` | `crates/proot-distro-core/src/storage.rs` |
| symlink-preserving archive; `load_backups` implemented | `crates/pn-next/src/backup_restore/mod.rs` |
| fake `/proc` stubs (`sysdata`) written beside the rootfs and bound over blocked paths | `crates/proot-distro-core/src/sysdata.rs`, wired in `container.rs` (execute/start) and `native.rs` (install) |
| `pn_trace!` gate replaces unconditional DEBUG | all four crates |

---

## 5b. Post-fix verification (live, final binary)

Every row below was run against the rebuilt binary (`podnext 0.1.7`, 15819944 B) and the
output pasted verbatim in the command column.

| Check | Command | Result |
|---|---|---|
| version string | `podnext version` | `podnext 0.1.7` (was `0.1.0`) |
| DEBUG noise | `podnext login alpine -- echo hi` | 0 DEBUG lines (was ~6/command) |
| DEBUG on backup | `podnext backup create alpine` | 0 DEBUG lines (was ~280 kB) |
| host `LD_*` in guest | `podnext login alpine -- env` | 0 matches (was present) |
| host secrets in guest | `podnext login alpine -- env` | 0 matches `API_KEY\|HERMES` |
| musl binaries | `podnext login alpine -- apk --version` | `apk-tools 3.0.8-r0` (was exit 127) |
| DNS setup (fresh install) | `podnext install alpine --name dnstest` then `cat /etc/resolv.conf` | `nameserver 8.8.8.8` … |
| DNS end-to-end (fresh) | `podnext login dnstest -- apk update` | `OK: 28571 distinct packages available` (was "temporary error") |
| argv quoting | `podnext login alpine -- /bin/sh -c 'echo Q_OK; echo $((6*7))'` | `Q_OK` / `42` (was blank) |
| container identity in list | `podnext list` | `alpine \| alpine \| latest` (was `ubuntu 22.04` for all) |
| `remove` deletes | `podnext remove dnstest -f` then `ls ~/.podnext/containers/` | gone (was silent no-op) |
| backup keeps symlinks | `tar tvzf <archive> \| grep -c '^l'` | 335 (was 0) |
| `backup list` after restart | `podnext backup list` | 14 rows; newest shows `CONTAINER=alpine` |
| volume mountpoint | `podnext volume ls` | `~/.podnext/volumes/testvol` (was `/var/lib/podnext/...`) |
| `/proc/uptime`, `/proc/loadavg` | `podnext login alpine -- cat /proc/uptime` | `124.08 932.76` — **identical** to proot-distro (was `Permission denied`) |
| `/proc/stat` | `podnext login alpine -- head -3 /proc/stat` | `cpu  1954 0 2872 93276 259 340 250 83 0 0` — **identical** to proot-distro |
| `/proc/version` | `podnext login alpine -- cat /proc/version` | real kernel `6.1.157-android14-…` (proot-distro reports its fake `6.17.0-PRoot-Distro`) |

**Note on `[5]`:** the long-lived `alpine` container predates the install-time fix, so it has
no `/etc/resolv.conf`; a fresh install does, and `apk update` then resolves 28571 packages.
Reinstall any container created before this change to pick the file up.

---

## 5c. Post-fix performance re-measure

| Operation | proot-distro | PDN (post-fix) | Note |
|---|---|---|---|
| `login -- echo hi` | 0.561 s | 0.247–0.347 s | unchanged by fixes |
| `list` | 0.677 s | 0.03–0.034 s | unchanged |
| `backup create alpine` | 2.000 s → 9,041,920 B | ~3.2 s → 4,061,406 B (420 entries, 335 symlinks) | PDN smaller, proot-distro faster |

The fixes cost nothing measurable on the hot paths; `login`/`list` remain ~2–20× faster.

---

## 6. Recommendation

| Use case | Pick | Why |
|---|---|---|
| Throwaway shell / quick container | proot-distro | Mature, `copy`/`sync`, `--isolated`, DNS handled |
| Root-like identity in guest | proot-distro | Fakes uid 0, `HOME=/root` |
| Long-running supervised service on device | **PDN** | Restart policy, backoff, circuit breaker, watchdog, boot integration |
| Fleet of containers with networking/quotas | **PDN** | network/volume/quota namespaces (quota persistence still open) |
| Host↔guest file plumbing | proot-distro | PDN lacks `copy`/`sync` |
| Fast scripted container ops | **PDN** | ~20× faster `list`, ~1.6× faster login |

Net: after this round PDN's container *runtime* is correct (isolation, DNS, env hygiene, quoting, symlinks, remove). Its remaining weaknesses are **unfinished feature surface** (`restore`, `build`, `gitops`, `copy/sync`) and **non-persistent CLI state**, not core container behaviour.
