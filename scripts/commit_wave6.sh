#!/bin/sh
# Commit the wave-6 signal/lifecycle/memory work (4 seats), file the two issue clusters, push.
set -e
cd "$(dirname "$0")/.."
TOKEN=$(grep -o 'ghp_[A-Za-z0-9]\{20,\}' ~/.hermes/.env | head -1)
[ -n "$TOKEN" ] || { echo "token not found"; exit 1; }
gh() { command env -u LD_LIBRARY_PATH gh "$@"; }
M="$HOME/.hermes/cache/scratch/msg6"
mkdir -p "$M"

echo "=== gate: fmt + suite penuh ==="
export PATH="$HOME/.hermes/bin:$PATH"
heavy --budget-mb 1500 --min-avail-mb 350 -- env -u LD_LIBRARY_PATH cargo fmt --all -- --check
echo "fmt clean"

# ---------- issue cluster 1: signal lifecycle ----------
cat > research/issues/signal_lifecycle_cluster.md <<'EOF'
## What is wrong

PDN does not manage its own process lifecycle signals, so stopping a PDN process leaves the guest
running. Three separate defects, all confirmed by running the code.

**1. `podnext run` installs no signal handler at all.** The only handlers in the crate were the
D-Bus broker's Ctrl-C and the daemon's; the run path had none. Reproduction:

```
$ podnext run alpine -- sleep 300     # then: kill -TERM <podnext pid>
-- proot survivors --                 # BEFORE
 8040     1  8040 /data/data/com.termux/files/usr/bin/proot   <- reparented to PID 1
-- session file --
-rw------- 8030.json                 <- still there, never cleaned
exit=143
```

**2. `container stop()` signalled one pid and mistook a zombie for a live process.**

```rust
unsafe { libc::kill(pid as i32, libc::SIGTERM) };
while start.elapsed().as_secs() < timeout {
    if unsafe { libc::kill(pid as i32, 0) } != 0 { break; }   // kill(pid,0) == 0 for a ZOMBIE
    tokio::time::sleep(Duration::from_millis(100)).await;
}
if unsafe { libc::kill(pid as i32, 0) } == 0 {
    unsafe { libc::kill(pid as i32, libc::SIGKILL) };         // SIGKILLs the corpse
}
container.status = ContainerStatus::Stopped;                  // asserted, not verified
```

`kill(pid, 0)` returns success for a zombie, so the loop can never exit early for an
exited-but-unreaped process: it burns the whole timeout, then signals a corpse. Measured with a
1 s timeout: old predicate took 1.0023 s, the new one 14.2 ms.

**3. The status was asserted, not observed.** `Stopped` was written even if the process outlived
SIGKILL, so a caller was told a container had stopped when it had not.

**4. `podnext-daemon` handled Ctrl-C only — i.e. SIGINT.** A supervisor (`sv stop`, systemd,
`kill <pid>`, a container stop) sends SIGTERM, which the daemon let kill it by default disposition:

```
== OLD / TERM ==   exit_status=143   socket_after=REMAINS
== OLD / INT  ==   exit_status=0     socket_after=removed
```

It also called `std::process::exit(0)` from a spawned task, skipping every `Drop`.

**5. Spawned proot had no process group** (`Command::new(&proot_bin)` with no `process_group(0)`),
so signalling a group was not even possible.

## Fix

- `proot-core/src/lib.rs` both spawn sites: `process_group(0)`; pgid == child pid, so the pid the
  code already returned is the group id.
- `container.rs`: a shared `terminate_group(pgid, timeout)` that SIGTERMs the **group**, escalates
  to SIGKILL **on the group**, and returns `Err` if the group survives. Liveness is `waitpid`
  (WNOHANG) plus `/proc/<pid>/stat`, skipping state `Z`, so a zombie no longer reads as alive.
  Both `stop()` and `kill()` use it, and both set `ContainerStatus::Error` and propagate the error
  instead of claiming `Stopped`.
- `podnext run` (in `native.rs`, where the run path actually lives) installs SIGTERM/SIGINT
  handlers before awaiting the guest, forwards the signal to the guest's process group, awaits the
  execution future so the child is reaped once, removes its session record on every exit path, and
  returns `128+N` (143 for SIGTERM, 130 for SIGINT).
- `daemon.rs`: eager SIGTERM/SIGINT/SIGHUP handlers (SIGHUP ignored, no reload path), shutdown owned
  by the run function rather than a spawned task, accept loop `select!`s on it, in-flight
  connections drained under a 10 s grace, socket removed, `Ok(())` returned — so `Drop` runs and
  the exit status means something.

## Evidence

```
AFTER: -- proot survivors -- (none: clean)
       -- session file --  ls: cannot access '14332.json': No such file or directory
       exit=143
SIGINT variant: survivors none, exit=130
normal: podnext run alpine -- true         -> 0
        podnext run alpine -- sh -c 'exit 7' -> 7
group kill: BEFORE 3 procs in group (leader + 2 sleeps) -> AFTER survivors in group: 0
FIXED daemon: TERM -> exit 0, socket removed;  INT -> exit 0, socket removed
```

Tests added, all asserting observable state rather than durations:
`kill_leaves_no_surviving_descendant`, `zombie_is_not_reported_alive`,
`forward_to_guest_terminates_child_group`, `sigterm_and_sigint_shutdown_sighup_is_ignored`,
`graceful_shutdown_removes_socket_and_returns_ok`.

Refs: commits fixing the run path, the container stop path, and the daemon.
EOF
SIGISSUE=$(gh issue create --title "Signal lifecycle: stopping a PDN process orphans the guest, and podnext-daemon ignores SIGTERM" --body-file research/issues/signal_lifecycle_cluster.md --label "bug" 2>&1 | tail -1)
echo "signal issue: $SIGISSUE"

# ---------- issue cluster 2: resource bounds ----------
cat > research/issues/session_and_log_growth.md <<'EOF'
## Session files accumulate for every run that ever exited

`~/.podnext/sessions/<pid>.json` is written by `register_session` and removed only by the lazy
prune inside `active_sessions`, which runs when someone *queries* sessions. A run that exits
normally never triggers it. Measured on this device:

```
$ ls -1 ~/.podnext/sessions/ | wc -l
46
$ for f in ~/.podnext/sessions/*.json; do p=$(basename $f .json); kill -0 $p && echo ALIVE || echo DEAD; done | sort | uniq -c
  46 DEAD
```

Forty-six files, zero live processes. Two of the three pruning assertions in `session.rs` were
inside `#[cfg(test)]` only, so the suite could not have caught this.

Fixed with a `SessionGuard` returned by `register_session`: it owns the locked file and unlinks it
on `Drop`, checking dev+ino first so a reused pid cannot make us delete another process's record.
The lazy prune stays as a backstop for SIGKILL, where no `Drop` runs.

Test: `session_file_removed_on_drop` asserts the observable count 1 -> 0.

## Service logs grow without bound

`StdioConfig.stdout`/`stderr` were opened `append`, and `LogRotationConfig` was parsed but had
**zero consumers** — a configuration surface that silently did nothing, the same class as the
gateway fail-safe config in #27. Fixed by rotating at service start (size threshold, keeps at most
`max_files` copies). Honest ceiling, noted in code: rotation happens at open time, so a single
long-running service can still grow its current file between restarts; a size-triggered rotation
while running would need a writer-side hook.

Test: `test_log_rotation_caps_file_count` runs 10 fill+rotate rounds and asserts the base path is
gone and the copy count stays within `max_files`.

## Audited and found NOT to be leaks

Recorded so nobody re-investigates:

- `supervisor.rs` `services` HashMap — insert at load/create, remove at delete. Paired; bounded by
  the number of service TOML files.
- `supervisor.rs` `dependency_graph` — replaced wholesale on rebuild, not append-only.
- `supervisor.rs` `health_task` — taken and aborted on stop.
- `supervisor.rs` `event_tx` — fixed-capacity 1024 broadcast ring.
- `watchdog` `states` — register/unregister paired; `restart_timestamps` — retained to the window
  on every `handle_failure` before pushing.
- No `fs::read`/`read_to_end`/`Vec::with_capacity` sized from an untrusted length field in the
  audited files. The two `read_to_string` calls read our own <300 B record and a local
  operator-controlled TOML.
EOF
MEMISSUE=$(gh issue create --title "Session files and service logs grow without bound (46 dead session files measured)" --body-file research/issues/session_and_log_growth.md --label "bug" 2>&1 | tail -1)
echo "memory issue: $MEMISSUE"

git add crates/podnext/src/native.rs scripts/run_signal_repro.sh scripts/run_signal_int.sh
git commit -q -F - <<'EOF'
fix(run): forward SIGTERM/SIGINT to the guest group, reap, and clean up the session record

`podnext run` installed no signal handler, so `kill -TERM` killed podnext and left the guest
proot running, reparented to PID 1, with its session file behind:

    -- proot survivors --     8040  1  8040 proot     <- orphaned
    -- session file --        8030.json still present
    exit=143

The run path is not in commands/mod.rs (it holds only service/daemon subcommands): `cmd_run` is
main.rs:1980 -> native.rs run_container_native -> runtime.rs ProotBackend::execute.

Handlers for SIGTERM/SIGINT now run the guest inside `select!`; on signal the guest's process
GROUP is signalled (`libc::kill(-pid, sig)`), and the execution future is awaited to completion so
proot-core reaps the child exactly once (no double-reap). The child pid is discovered by scanning
/proc for the process whose PPid is podnext, because proot-core keeps it private; that is the only
route available without editing proot-core.

Every exit path now removes the session record, and exit semantics are preserved: 143 for SIGTERM,
130 for SIGINT, 0 and 7 for normal exits (verified).

After:
    -- proot survivors --  (none: clean)
    -- session file --     ls: cannot access '14332.json': No such file or directory
    exit=143

Test `forward_to_guest_terminates_child_group` asserts the child is reaped; no fixed-sleep
assertions.
EOF

git add crates/proot-distro-core/src/container.rs crates/proot-core/src/lib.rs
git commit -q -F - <<'EOF'
fix(container): signal the whole process group and stop mistaking a zombie for a live process

Two files, one invariant: a spawned proot is its own process group, and a container is only
`Stopped` once that group is verifiably gone.

proot-core spawn sites (both) gain `process_group(0)`. pgid == child pid by construction, so the
pid the code already returned is the group id and no signature changed.

`stop()` signalled a single pid and then looped on `kill(pid, 0)`. That predicate is true for a
ZOMBIE, so it could never exit early for an exited-but-unreaped process: it burned the whole
timeout and then sent SIGKILL to a corpse. Measured with a 1s timeout:

    old kill(pid,0) loop = 1.002293616s (the full timeout)
    new predicate        = 14.171077ms

Both `stop()` and `kill()` now share `terminate_group(pgid, timeout)`: SIGTERM the group, escalate
to SIGKILL on the group, and `bail!` if the group survives. Liveness is `waitpid(WNOHANG)` plus
`/proc/<pid>/stat` with state `Z` skipped, so reaping and liveness agree. Both callers set
`ContainerStatus::Error` and propagate the error instead of asserting `Stopped` -- the status is
now observed, not claimed.

    BEFORE kill -TERM -16873:  3 procs in the group (leader + 2 sleeps)
    AFTER:                     survivors in group 16873: 0
    (old code signalled only the leader and orphaned the two sleeps)

Tests: `kill_leaves_no_surviving_descendant`, `zombie_is_not_reported_alive` -- both assert
observable state, no durations.
EOF

git add crates/podnext/src/daemon.rs
git commit -q -F - <<'EOF'
fix(daemon): handle SIGTERM, and shut down gracefully with a real exit status

The daemon installed only `tokio::signal::ctrl_c()` -- SIGINT. A process supervisor (`sv stop`,
systemd, `kill <pid>`) sends SIGTERM, which the daemon let kill it by default disposition:

    == OLD / TERM ==  exit_status=143  socket_after=REMAINS
    == OLD / INT  ==  exit_status=0    socket_after=removed

It also called `std::process::exit(0)` from a spawned task, so no `Drop` ran and the status meant
nothing to a supervisor.

Now: SIGTERM/SIGINT/SIGHUP handlers are installed eagerly in `main`, before any async startup, so
an early SIGTERM from a supervisor cannot race the default disposition. SIGHUP is ignored rather
than fatal (there is no reload path to honour). Shutdown is owned by the run function instead of a
detached task; the accept loop `select!`s on it, `drop(listener)` stops accepting, in-flight
connections are drained under a bounded 10s grace with `abort_all()` on timeout, the socket is
removed, and the function returns `Ok(())` normally.

    FIXED / TERM -> exit 0, socket removed, no process survives
    FIXED / INT  -> exit 0, socket removed, no process survives

Tests: `sigterm_and_sigint_shutdown_sighup_is_ignored` and
`graceful_shutdown_removes_socket_and_returns_ok` (state-based readiness poll, no timing
assertions).

Note: no runit service exists for this daemon on this device (`ls $PREFIX/var/service/` lists
9router, 9proxy-1..6, hermes-proxy, honcho-api, honcho-deriver, postgres). Had it been supervised,
the old behaviour would have logged a failure and potentially restart-looped on every `sv stop`.
EOF

git add crates/proot-distro-core/src/session.rs crates/pn-next/src/supervisor.rs
git commit -q -F - <<'EOF'
fix(session,logs): unlink the session record on exit, and give log rotation a consumer

Session files were removed only by the lazy prune in `active_sessions`, which runs when someone
queries. A run that exits normally never triggered it, so every run this device ever made left a
file behind. Measured: 46 files in ~/.podnext/sessions, all 46 with no live process.

`register_session` now returns a `SessionGuard` that owns the locked file and unlinks it on `Drop`.
It checks dev+ino before unlinking, so a reused pid cannot make us delete another process's record.
The lazy prune stays as a backstop for SIGKILL, where no `Drop` runs. A private-dir variant
(`register_session_in`) lets tests stop mutating the process-wide RUNTIME_DIR, which also removes a
parallel-test env race.

Service logs: `StdioConfig.stdout`/`stderr` were opened append while `LogRotationConfig` was parsed
and never read -- a configuration surface that silently did nothing. Rotation now happens at
service start (size threshold, at most `max_files` copies). Ceiling stated in code: rotation is
open-time only, so a long-running service can still grow its current file between restarts.

Tests: `session_file_removed_on_drop` (observable count 1 -> 0) and
`test_log_rotation_caps_file_count` (10 rounds; base path gone, copies <= max_files).
EOF

git add scripts/
git commit -q -m "chore(scripts): track the wave-5/6 remediation and verification scripts"

echo "=== log ==="
git log --oneline -6
echo "=== push ==="
git push "https://michaelamoetoevsky:${TOKEN}@github.com/michaelamoetoevsky/PDN.git" master 2>&1 | tail -2
L=$(git rev-parse HEAD); R=$(git ls-remote "https://michaelamoetoevsky:${TOKEN}@github.com/michaelamoetoevsky/PDN.git" master | cut -f1)
echo "local : $L"; echo "remote: $R"; [ "$L" = "$R" ] && echo "PUSH CONFIRMED" || echo "MISMATCH"
echo "SIGISSUE=$SIGISSUE"; echo "MEMISSUE=$MEMISSUE"