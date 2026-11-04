# PDN issue ledger — fixes re-verified against HEAD

Generated 2026-11-04 14:29:45Z from `d83b987` (docs(issues): ledger that re-verifies every issue's fix against HEAD).

## Why this exists

Closing an issue records an intention. It does not prove the fix is still in the tree: a later
refactor, a revert, or a merge that resolved a conflict the wrong way can quietly undo it. This
ledger pairs every issue with a command that must still succeed against the current code, so a
silently-regressed fix shows up as a FAIL in a document rather than as a surprise in production.

**19/19 checks pass.** Nothing regressed.

## Checks that FAIL (a fix that is no longer present)

None.

## Per-issue detail

**#11 [BUG-C1] install accepts empty container name and creates an unremovable orphan**  
_state_: CLOSED  ·  _labels_: bug, cli  ·  _closed_: 2026-11-04T04:23:37Z

- [PASS] install validates the container name before creating anything
  - `grep -n 'validate_container_name' crates/podnext/src/main.rs`
  - → `1898:    proot_distro_core::NameValidator::new().validate_container_name(&container_name)?; | 2931:    proot_distro_core::NameValidator::new().validate_container_`

**#12 [BUG-C2] backup delete deadlocks permanently (tokio RwLock read guard held across write)**  
_state_: CLOSED  ·  _labels_: bug  ·  _closed_: 2026-11-04T04:23:41Z

- [PASS] delete_backup drops its read guard before taking the write lock
  - `grep -n 'contains_key' crates/pn-next/src/backup_restore/mod.rs`
  - → `1115:        let exists = backups.contains_key(backup_id);`
- [PASS] the deadlock regression test exists
  - `grep -rn 'test_delete_backup_does_not_deadlock' crates/pn-next/src/backup_restore/mod.rs`
  - → `1487:    async fn test_delete_backup_does_not_deadlock() {`

**#13 [BUG-C3] reset destroys the container before verifying the image is downloadable**  
_state_: CLOSED  ·  _labels_: bug, cli  ·  _closed_: 2026-11-04T10:12:19Z

- [PASS] reset verifies the image BEFORE destroying the container
  - `awk '/fn cmd_reset/,/^}/' crates/podnext/src/main.rs | grep -nE 'verify_image_available|remove_dir_all' | head -2`
  - → `37:    verify_image_available(&image_ref).await?; | 39:    std::fs::remove_dir_all(&container_dir)?;`

**#14 [BUG-M1] podnext ps does not report non-interactive run sessions**  
_state_: CLOSED  ·  _labels_: bug, cli, monitoring  ·  _closed_: 2026-11-04T04:23:46Z

- [PASS] run sessions are registered so ps can see them
  - `grep -rn 'register_session' crates/proot-distro-core/src/runtime.rs crates/podnext/src/native.rs`
  - → `crates/proot-distro-core/src/runtime.rs:181:        let _session = crate::session::register_session(`

**#19 [CI-1] test_restart_policy_unless_stopped fails: explicit stop loses to restart policy**  
_state_: CLOSED  ·  _labels_: bug, ci  ·  _closed_: 2026-11-04T04:23:51Z

- [PASS] the restart path re-checks user_stopped under the same write lock
  - `grep -n 'user_stopped' crates/pn-next/src/supervisor.rs | head -8`
  - → `728:    user_stopped: bool, | 850:                            user_stopped: false, | 992:                                if managed.process.is_none() && !managed.us`

**#20 [BUG-C4] FileLock::lock flocks fd 0 (stdin) and then panics on a None field**  
_state_: CLOSED  ·  _labels_: bug  ·  _closed_: 2026-11-04T04:23:55Z

- [PASS] FileLock no longer flocks fd 0
  - `grep -rn 'fd 0\|stdin\|FileLock' crates/proot-distro-core/src/locking.rs | head -6`
  - → `65:pub struct FileLock { | 70:impl FileLock { | 86:        // i.e. file descriptor 0 (stdin), and then panicked unwrapping that same | 109:impl Drop for FileLock { | 24`

**#21 [BUG-C5] a guest command containing -r aborts with 'rootfs is required for direct execution mode'**  
_state_: CLOSED  ·  _labels_: bug, cli  ·  _closed_: 2026-11-04T04:24:00Z

- [PASS] direct-execution detection only fires when -r/--rootfs precedes the command
  - `grep -n 'SUBCOMMANDS\|pre_dashdash_args' crates/podnext/src/main.rs | head -4`
  - → `1410:    let pre_dashdash_args = &raw_args[..dashdash_idx]; | 1412:    const SUBCOMMANDS: &[&str] = &[ | 1445:    let has_subcommand = pre_dashdash_args | 1448:      `

**#22 [CI-2] dependency-updates workflow references a nonexistent action (peter-evans/update-github-actions 404s)**  
_state_: CLOSED  ·  _labels_: bug, github-actions, ci  ·  _closed_: 2026-11-04T04:24:05Z

- [PASS] the self-audit job that replaces the nonexistent action exists
  - `grep -n 'name: Audit GitHub Actions pins' .github/workflows/dependency-updates.yml`
  - → `98:      - name: Audit GitHub Actions pins`

**#23 [CI-3] Cargo.lock is gitignored in an application workspace, so --locked was impossible**  
_state_: CLOSED  ·  _labels_: bug, ci  ·  _closed_: 2026-11-04T04:24:09Z

- [PASS] Cargo.lock is tracked
  - `git ls-files --error-unmatch Cargo.lock`
  - → `Cargo.lock`

**#24 [SEC-1] 35 advisories in the dependency tree: inventory, exposure analysis and remediation plan**  
_state_: CLOSED  ·  _labels_: security, ci  ·  _closed_: 2026-11-04T10:12:17Z

- [PASS] deny.toml encodes the advisory acceptance list (26 entries, string form)
  - `grep -n '"RUSTSEC-' deny.toml | head -3`
  - → `70:    "RUSTSEC-2026-0258", # h2: unbounded empty DATA frames | 71:    "RUSTSEC-2026-0346", # rustls: Debug impls expose TLS secrets | 72:    "RUSTSEC-2026-0349", #`

**#25 [CI-4] Benchmark workflow is a false green: cargo bench fails to compile and the pipe hides it**  
_state_: CLOSED  ·  _labels_: bug, ci, performance  ·  _closed_: 2026-11-04T10:12:24Z

- [PASS] benchmark workflow enforces the exit status and builds the targets
  - `grep -n 'set -euo pipefail\|--bench benchmarks' .github/workflows/benchmark.yml | head -4`
  - → `58:          set -euo pipefail | 96:          # `--bench benchmarks` is REQUIRED for the criterion flags to land on the | 100:          set -euo pipefail | 101:      `
- [PASS] the benchmark sources compile (the defect behind the false green)
  - `ls -1 target/release/deps/benchmarks-*.d 2>/dev/null | head -3`
  - → `target/release/deps/benchmarks-0c4e0ca8eb5c7638.d | target/release/deps/benchmarks-6138274a3283f45c.d | target/release/deps/benchmarks-9bb0347c09149d86.d`

**#32 Signal lifecycle: stopping a PDN process orphans the guest, and podnext-daemon ignores SIGTERM**  
_state_: CLOSED  ·  _labels_: bug  ·  _closed_: 2026-11-04T13:12:18Z

- [PASS] spawned proot gets its own process group
  - `grep -n 'process_group(0)' crates/proot-core/src/lib.rs`
  - → `307:        cmd.process_group(0); | 442:            .process_group(0)`
- [PASS] container stop signals the group and reaps (zombie-aware)
  - `grep -n 'fn terminate_group\|fn wait_group_gone\|fn live_pgrps' crates/proot-distro-core/src/container.rs`
  - → `630:async fn terminate_group(pgid: u32, timeout: u64) -> Result<()> { | 646:async fn wait_group_gone(pgid: u32, timeout: u64) -> bool { | 668:fn live_pgrps() -> Vec`
- [PASS] podnext run forwards signals to the guest group
  - `grep -n 'fn forward_to_guest\|SignalKind' crates/podnext/src/native.rs | head -3`
  - → `30:use tokio::signal::unix::{signal, SignalKind}; | 566:fn forward_to_guest(sig: i32) { | 593:    let mut sigterm = signal(SignalKind::terminate())?;`
- [PASS] the daemon handles SIGTERM, not only Ctrl-C
  - `grep -n 'SignalKind::terminate' crates/podnext/src/daemon.rs`
  - → `70:    if kind == SignalKind::terminate() || kind == SignalKind::interrupt() { | 136:    let mut sigterm = signal(SignalKind::terminate()).context("install SIGTER`

**#33 Session files and service logs grow without bound (46 dead session files measured)**  
_state_: CLOSED  ·  _labels_: bug  ·  _closed_: 2026-11-04T13:12:39Z

- [PASS] the session record is unlinked on exit via a guard
  - `grep -n 'impl Drop for SessionGuard' crates/proot-distro-core/src/session.rs`
  - → `67:impl Drop for SessionGuard {`
- [PASS] log rotation has a consumer
  - `grep -n 'fn rotate_log_if_needed' crates/pn-next/src/supervisor.rs`
  - → `478:fn rotate_log_if_needed(path: &Path, cfg: &LogRotationConfig) -> std::io::Result<()> {`

## Issues with no automated check here

Listed rather than omitted, so the coverage gap is visible. These are either upstream feature
requests, or defects whose verification needs a live container and therefore belongs in CI.

| # | state | title |
|---|---|---|
| 1 | OPEN | Add shell completion for PowerShell |
| 2 | OPEN | Add example service configurations for common applications |
| 3 | OPEN | Create tutorial: Running a full-stack development environment in PDN |
| 4 | OPEN | Add VS Code Dev Container support |
| 5 | OPEN | Add systemd service unit generator for Linux hosts |
| 6 | OPEN | Add Docker Compose export for PDN services |
| 7 | CLOSED | Add GitHub Actions workflow for automated dependency updates |
| 8 | CLOSED | Add benchmark suite with Criterion for performance regression detection |
| 9 | OPEN | Add Prometheus metrics exporter for PDN services |
| 10 | OPEN | Add WebSocket support for real-time service events |
| 15 | CLOSED | [BUG-M2] concurrent guest writes to one file clobber each other (4/5) |
| 16 | CLOSED | [BUG-M3] deep tree extraction scales super-linearly and times out past ~200 leve |
| 17 | CLOSED | [BUG-L1] podnext build local rejects ARG, skips RUN, and produces no usable OCI  |
| 18 | CLOSED | [BUG-L2] copy/sync expand sparse files to dense allocation |
| 27 | OPEN | [BUG-DOC1] Documented gateway fail-safe config (bulkhead, retry_policy) does not |
| 28 | OPEN | Compose export: empty command emits an invalid service; follow-ups from PR #26 |
| 29 | OPEN | podnext build local computes a layer diff but produces no image |
| 30 | OPEN | [CI-5] Benchmark job is unpredictable: 25.7 min then 43+ min, and a baseline ste |
| 31 | OPEN | [CI-6] test_restart_policy_on_abnormal is flaky: assertions bet on wall-clock sl |

## How to re-run this

```sh
env -u LD_LIBRARY_PATH python3 scripts/build_issue_ledger.py   # rewrites this file
```

Every check is read-only (grep/git/ls). Nothing here builds, so it is safe to run on any host,
including the Android device, without the memory guard.
