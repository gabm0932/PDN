#!/bin/sh
# Commit the supervisor (CI-1) and session-registration (M1) fixes. POSIX sh.
set -e
cd "$(dirname "$0")/.."

MSG="$HOME/.hermes/cache/scratch/commitmsg"
mkdir -p "$MSG"

cat > "$MSG/6.txt" <<'EOF'
fix(supervisor): an explicit stop must pre-empt the restart policy

`test_restart_policy_unless_stopped` failed on CI (run 41744808292) with
`left: Failed, right: Stopped`: after an explicit `stop_service` the service was
left in `Failed`, so the `UnlessStopped` policy did not in fact survive a user
stop.

Root cause is a TOCTOU race in the supervision restart path. The restart decision
reads `managed.user_stopped` when the process exits, but the state mutations that
carry the decision out happen later under separate write locks, unguarded. A
service whose command is `/bin/false` therefore exits, is classified `Failed`,
and schedules a restart; a concurrent `stop_service` sets `user_stopped = true`
and `Stopped`, and the already-scheduled restart then overwrote that with
`Failed`.

Guard every place the restart machinery writes `Failed` or resurrects a service:
- classify an exit after a user stop as `Stopped`, not `Failed` (the stop is
  authoritative, the exit code is incidental);
- do not arm a restart when `user_stopped` is set, or when the service is
  `Stopping`/`Stopped`;
- skip the "no process in Restarting/Starting" revive path for a stopped service;
- re-check `user_stopped` after the backoff sleep, so a stop during backoff
  cancels the restart.

The test is unchanged: the assertion encodes a real requirement.

    RUN 1: test result: ok. 19 passed; 0 failed; 0 ignored
    RUN 2: test result: ok. 19 passed; 0 failed; 0 ignored
    RUN 3: test result: ok. 19 passed; 0 failed; 0 ignored

Single-test runs (8x) also stable:
    run1..run8: test result: ok. 1 passed; 0 failed; 18 filtered out; finished in 6.02s

Refs #19
EOF

cat > "$MSG/7.txt" <<'EOF'
fix(runtime): register a session for foreground run/login so `ps` reports it

`session::register_session` (crates/proot-distro-core/src/session.rs:41) was
re-exported from lib.rs:85 but had exactly one call site in the whole repo: its
own unit test. No production code ever registered a session, so
`~/.podnext/sessions/` stayed empty and `podnext ps` always rendered an empty
table — even with workloads running. The reader was fine; registration was dead
code.

Publish a session record from `ProotBackend::execute`, which is the single point
both foreground `run` and `login` funnel through. The returned `File` holds the
`LOCK_EX` on the session file and is inherited by the descendant (the descriptor
clears FD_CLOEXEC), so dropping it once the child exits releases the lock and the
next `active_sessions()` probe prunes the stale record.

Live proof on device:

```
===PS DURING===
| ID         | CONTAINER | USER | COMMAND | CWD | PID   | STATUS  |
| sess-21855 | alpine    | 0    | sleep 8 | -   | 21855 | Running |

===PS DURING LOGIN===
| ID         | ENTITY...  | ...
| sess-22946 | alpine    | 0    | sleep 6 | -   | 22946 | Running |

test result: ok. 33 passed; 0 failed; 0 ignored; 0 measured
```

Known limitation, marked in the source: the recorded pid is this process, not the
proot grandchild, because proot-core does not expose the child pid. Same
lifetime, so the flock liveness probe remains exact.

Refs #14
EOF

git add crates/pn-next/src/supervisor.rs tests/supervisor_e2e.rs
git commit -F "$MSG/6.txt"

git add crates/proot-distro-core/src/runtime.rs
git commit -F "$MSG/7.txt"

echo "=== LOG ==="
git log --oneline -5
echo "=== REMAINING (agent E in-flight files expected) ==="
git status --short
