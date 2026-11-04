#!/bin/sh
# Close #32/#33 with evidence, then leave CI to run.
set -e
cd "$(dirname "$0")/.."
gh() { command env -u LD_LIBRARY_PATH gh "$@"; }
C="$HOME/.hermes/cache/scratch/c6"
mkdir -p "$C"

cat > "$C/32.md" <<'EOF'
Fixed across three commits: `e4ceb65` (the run path), `7808285` (container stop/kill + process
groups), `40771fc` (the daemon).

Measured before and after on this device, not asserted:

```
# podnext run
BEFORE  -- proot survivors --   8040  1  8040 proot      <- orphaned, reparented to PID 1
        -- session file --    8030.json still present
        exit=143
AFTER   -- proot survivors --  (none: clean)
        -- session file --     ls: cannot access '14332.json': No such file or directory
        exit=143
SIGINT variant: survivors none, exit=130
normal exits preserved:  `-- true` -> 0,  `-- sh -c 'exit 7'` -> 7

# container stop -- the zombie loop
BEFORE  kill(pid,0) loop = 1.002293616s  (the full 1s timeout, then SIGKILL to a corpse)
AFTER   waitpid(WNOHANG)+/proc predicate = 14.171077ms

# group kill
BEFORE kill -TERM -16873 -> leader only; 16875/16876 orphaned
AFTER  survivors in group 16873: 0

# daemon
BEFORE TERM -> exit 143, socket REMAINS;  INT -> exit 0, socket removed
AFTER  TERM -> exit 0, socket removed;     INT -> exit 0, socket removed
```

Five tests added, all asserting observable state rather than durations:
`forward_to_guest_terminates_child_group`, `kill_leaves_no_surviving_descendant`,
`zombie_is_not_reported_alive`, `sigterm_and_sigint_shutdown_sighup_is_ignored`,
`graceful_shutdown_removes_socket_and_returns_ok`.

Two notes recorded rather than glossed:

- The run path is `main.rs:1980 -> native.rs:run_container_native`, not `commands/mod.rs`; the fix
  lives in `native.rs`.
- proot-core keeps the spawned child pid private, so the run path discovers it by scanning /proc for
  the process whose PPid is podnext. That is the only route that did not require editing proot-core
  in the same commit; exposing the pid properly is the cleaner follow-up.

Suite on the four touched crates after the change: **113 passed, 0 failed** (52 pn-next,
11 podnext, 2 daemon, 6 proot-core, 39 proot-distro-core, 3 deep_fs_recursion).
EOF
gh issue comment 32 --body-file "$C/32.md"
gh issue close 32 --reason completed

cat > "$C/33.md" <<'EOF'
Fixed in `d5dd137`.

The leak was real and measured on this device, not inferred:

```
$ ls -1 ~/.podnext/sessions/ | wc -l
46
$ for f in ~/.podnext/sessions/*.json; do p=$(basename $f .json); kill -0 $p && echo ALIVE || echo DEAD; done | sort | uniq -c
  46 DEAD
```

Forty-six records, zero live processes. `register_session` now returns a `SessionGuard` that unlinks
the file on `Drop`, checking dev+ino first so a reused pid cannot make us delete another process's
record. The lazy prune inside `active_sessions` stays as the backstop for SIGKILL, where no `Drop`
runs.

The log half of this is the more interesting one: `LogRotationConfig` was **parsed and never read**,
so a documented configuration surface silently did nothing — the same class as the gateway
fail-safe config in #27. It now has a consumer, with the ceiling stated in code: rotation happens at
service start, so a single long-running service can still grow its current file between restarts.

Tests: `session_file_removed_on_drop` (observable count 1 -> 0) and
`test_log_rotation_caps_file_count` (10 fill+rotate rounds; base path gone, copies <= max_files).

Audited and confirmed NOT to be leaks, so nobody re-investigates: `supervisor.services` (insert and
remove paired), `dependency_graph` (replaced wholesale), `health_task` (taken+aborted on stop),
`event_tx` (fixed 1024 ring), `watchdog.states` (register/unregister paired), `restart_timestamps`
(retained to the window before each push). No allocation sized from an untrusted length in the
audited files.
EOF
gh issue comment 33 --body-file "$C/33.md"
gh issue close 33 --reason completed

echo "=== sisa issue saya yang terbuka ==="
gh issue list --state open --limit 40 --json number,title 2>/dev/null | env -u LD_LIBRARY_PATH python3 -c "
import json,sys
d=[i for i in json.load(sys.stdin) if i['number']>=27]
for i in sorted(d,key=lambda x:x['number']): print(f\"  #{i['number']:3} {i['title'][:62]}\")
print('  OPEN >=27:',len(d))
"
