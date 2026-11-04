## Summary

The `PDN CI/CD` workflow **fails on `master`**: `test_restart_policy_unless_stopped`
leaves the service in `Failed` instead of `Stopped` after an explicit `stop_service`,
because the restart policy (`UnlessStopped`) races the explicit stop and wins.

## Reproduction

```sh
env -u LD_LIBRARY_PATH cargo test --workspace --no-fail-fast
# test test_restart_policy_unless_stopped ... FAILED
# tests/supervisor_e2e.rs:302:
#   assertion `left == right` failed
#     left: Failed
#    right: Stopped
```

CI run for `master` HEAD:

```
41744808292  failure  PDN CI/CD  master  push  2026-11-04T00:10:21Z  9m45s
  Rust Host Build: test_restart_policy_unless_stopped ... FAILED
  test result: FAILED. 18 passed; 1 failed
```

## Evidence (CI log, verbatim)

```
test test_restart_policy_unless_stopped ... FAILED
---- test_restart_policy_unless_stopped stdout ----
thread 'test_restart_policy_unless_stopped' (4095) panicked at tests/supervisor_e2e.rs:302:5:
assertion `left == right` failed
  left: Failed
 right: Stopped
error: test failed, to rerun pass `-p pdn-integration-tests --test supervisor_e2e`
```

## Root cause

Test body (`tests/supervisor_e2e.rs:275`): the service command is `/bin/false`
(always exits 1) with `RestartCondition::UnlessStopped`, `max_restarts = 3`,
`base_delay_secs = 1`. After `stop_service(name, false)` the test sleeps 3 s and expects
`Stopped`, but by then the restart loop has already marked the service `Failed` — i.e. the
supervisor does not let an explicit user stop pre-empt an in-flight restart decision, and
the exit-code classifier treats an intentional stop as an abnormal exit.

## Impact

- CI is red on `master`; every push reports a failing workflow.
- Real defect, not just a flaky test: an explicit stop can lose to the restart policy,
  which is exactly what `UnlessStopped` is supposed to prevent.

## Proposed fix

Set/check the `user_stopped` flag *before* the restart decision is evaluated so an explicit
stop always wins the race; ensure the exit-code classifier does not classify a
user-initiated stop as abnormal (`exit_code > 128` / signal) and set state to `Stopped`
rather than `Failed`. Keep the test; if a sleep is genuinely needed, poll for the expected
state instead of sleeping a fixed interval.

Labels: bug, ci
