## Summary

`tests/supervisor_e2e.rs::test_restart_policy_on_abnormal` is flaky. It failed on the merged ref of
PR #26 while the same suite passes on `master`, i.e. the failure is timing, not code.

## Evidence

From the `Rust Host Build` job of run
[41824324094](https://github.com/michaelamoetoevsky/PDN/actions/runs/41824324094) (PR #26 head
`71611bae`):

```
test test_restart_policy_on_abnormal ... FAILED
test result: FAILED. 18 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out; finished in 12.72s
##[error]Process completed with exit code 101.
```

Meanwhile the `PDN CI/CD` run for the same suite on `master` (`5af5b02`) is green, and it was green
on the PR's own earlier head (`6ac4101b`) too — the only variable is which registry the supervisor
loop happened to land on.

## Why this keeps happening

This is the second member of the same family. Issue #19 was
`test_restart_policy_unless_stopped`, fixed in `33a9e10` by re-checking `user_stopped` under the
same write lock that performs each restart-decision mutation. That fixed one **race**, but the tests
themselves still assert on wall-clock sleeps: a policy transition is driven by a fixed delay and the
assertion reads the state at a fixed offset from it. Under a loaded CI runner (this repo runs 8-11
jobs concurrently) that offset is not reliable.

So each fix removes one race and the suite keeps another. The structural problem is that the
assertion is `sleep(3s); assert_eq!(state, Expected)` — a bet on scheduling.

## Suggested direction

Make the assertions event-driven instead of time-driven:

1. Wait for the **state transition** rather than a duration: poll the service state until it changes
   or a generous deadline passes, then assert *which* transition occurred. A deadline that is 10x the
   expected delay turns a race into a slow test, not a red one.
2. Better, expose a way to make the transition deterministic: the supervisor already records the
   decision; asserting on the recorded decision (or on a channel/counter the loop increments) removes
   wall-clock dependence entirely.
3. Do this for the whole `restart_policy` family, not one test — `on_abnormal`, `unless_stopped`,
   `on_failure`, `always`, `never`, `backoff_delay_increases` and `max_restarts_exceeded` all drive
   the same loop.

## Impact

Every PR inherits this flake, so contributors see a red `Rust Host Build` that has nothing to do
with their change. Combined with the benchmark `Compare with baseline` defect, PR #26's two failing
checks were both repo-side.

Labels: ci, flaky-test
