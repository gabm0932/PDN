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
