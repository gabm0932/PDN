#!/bin/sh
# Fix the two repo-side defects that make every PR red, neither of which is the contributor's:
#   1. benchmark.yml `Compare with baseline` dies on `git stash pop` (exit 128) in a clean CI checkout
#   2. tests/supervisor_e2e.rs::test_restart_policy_on_abnormal is flaky -> issue
set -e
cd "$(dirname "$0")/.."
TOKEN=$(grep -o 'ghp_[A-Za-z0-9]\{20,\}' ~/.hermes/.env | head -1)
[ -n "$TOKEN" ] || { echo "token not found"; exit 1; }
gh() { command env -u LD_LIBRARY_PATH gh "$@"; }

echo "=== 1. step 'Compare with baseline' sebelum ==="
sed -n '/Compare with baseline/,/stash pop/p' .github/workflows/benchmark.yml

# Replace the stash dance. Evidence for why: the PR run ended with
#   "No stash entries found." / "##[error]Process completed with exit code 128."
# `git stash` finds nothing to save in a CI checkout, and `git stash pop` then exits 1 -> the step
# aborts after ~20 minutes of benchmarking. The earlier SIGTERM masked it; bounding the suite
# uncovered it.
python3 - <<'PY'
from pathlib import Path
p = Path(".github/workflows/benchmark.yml")
s = p.read_text()
old = """          git fetch origin "$BASEREF:$BASEREF"
          git stash
          git checkout "$BASEREF\""""
new = """          git fetch origin "$BASEREF:$BASEREF"
          # No stash dance. A CI checkout has no local changes to preserve, and the
          # stash/pop pair is actively harmful here: `git stash` finds nothing to save and
          # `git stash pop` then exits 1, which aborts the step (observed: "No stash entries
          # found." + "##[error]Process completed with exit code 128") AFTER the benchmarks ran.
          # This step only runs on pull_request, so master stayed green while every PR died here.
          git checkout "$BASEREF\""""
if old not in s:
    print("  [ERROR] anchor not found -- no change made")
    raise SystemExit(1)
s = s.replace(old, new, 1)
old2 = """            | tee benchmark-baseline.txt
          git checkout -
          git stash pop"""
new2 = """            | tee benchmark-baseline.txt
          git checkout -"""
if old2 not in s:
    print("  [ERROR] second anchor not found -- no change made")
    raise SystemExit(1)
s = s.replace(old2, new2, 1)
p.write_text(s)
print("  patched: stash/stash-pop removed")
PY

echo
echo "=== 2. step sesudah ==="
sed -n '/Compare with baseline/,/::notice/p' .github/workflows/benchmark.yml

echo
echo "=== 3. validasi YAML ==="
env -u LD_LIBRARY_PATH python3 -c "
import sys
try:
    import yaml
except ImportError:
    print('  (pyyaml tidak ada - lewati)'); sys.exit(0)
d=yaml.safe_load(open('.github/workflows/benchmark.yml'))
steps=[s['name'] for s in d['jobs']['benchmark']['steps']]
print('  YAML valid; step:', steps)
"

cat > research/issues/flaky_restart_policy_on_abnormal.md <<'EOF'
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
EOF

ISSUE=$(gh issue create --title "[CI-6] test_restart_policy_on_abnormal is flaky: assertions bet on wall-clock sleeps" --body-file research/issues/flaky_restart_policy_on_abnormal.md --label "ci,flaky-test" 2>&1 | tail -1)
echo "issue: $ISSUE"

git add .github/workflows/benchmark.yml research/issues/flaky_restart_policy_on_abnormal.md
git commit -q -F - <<'MSG'
fix(ci): drop the stash dance that made every PR's benchmark check fail

`Compare with baseline` ended with:

    No stash entries found.
    ##[error]Process completed with exit code 128.

`git stash` finds nothing to save in a clean CI checkout, so `git stash pop` exits 1 and aborts the
step -- after ~20 minutes of benchmarking. It was previously invisible because a SIGTERM killed the
step before it got that far; bounding the suite is what uncovered it.

This step only runs on pull_request
(`if: github.event_name == 'pull_request' && ...`), which is why master stayed green while every PR
died on it. Both stash lines are removed: there is nothing to preserve in a CI checkout, and
switching to the base branch is exactly what the step intends.

Also filed the second repo-side failure seen on the same PR: #CI-6
`test_restart_policy_on_abnormal` is flaky (18 passed / 1 failed on the merge ref, green on master).
That is the second member of the family fixed in 33a9e10, and the reason is structural -- the
restart-policy tests assert on `sleep(N); assert_eq!(state, ...)`, a bet on scheduling rather than
an observed transition.
MSG

git push "https://michaelamoetoevsky:${TOKEN}@github.com/michaelamoetoevsky/PDN.git" master 2>&1 | tail -2
L=$(git rev-parse HEAD); R=$(git ls-remote "https://michaelamoetoevsky:${TOKEN}@github.com/michaelamoetoevsky/PDN.git" master | cut -f1)
echo "local : $L"; echo "remote: $R"; [ "$L" = "$R" ] && echo "PUSH CONFIRMED" || echo "MISMATCH"
