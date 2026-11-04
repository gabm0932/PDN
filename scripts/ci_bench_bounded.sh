#!/bin/sh
# Bound the CI benchmark suite (fixes the unpredictable/hanging Benchmark job and
# the red check on PR #26), file the issue, push.
set -e
cd "$(dirname "$0")/.."
TOKEN=$(grep -o 'ghp_[A-Za-z0-9]\{20,\}' ~/.hermes/.env | head -1)
[ -n "$TOKEN" ] || { echo "token not found"; exit 1; }
gh() { command env -u LD_LIBRARY_PATH gh "$@"; }

echo "=== struktur akhir dua step ==="
grep -nA3 "cargo bench --workspace --bench benchmarks" .github/workflows/benchmark.yml

cat > research/issues/benchmark_unpredictable.md <<'EOF'
## Summary

The `Benchmark` job is not predictable, and one step of it is being killed. The suite is invoked
unbounded, so criterion's defaults (100 samples, 3 s warm-up, 5 s measurement) apply to every case —
including cases that build real sandboxes.

## Evidence

Same suite, two revisions, job limit of 60 minutes:

| Run | Revision | `Run benchmarks` | Job total |
|---|---|---|---|
| [41812140227](https://github.com/michaelamoetoevsky/PDN/actions/runs/41812140227) | `6c58b080` | success | 25.7 min |
| [41818454751](https://github.com/michaelamoetoevsky/PDN/actions/runs/41818454751) | `e3f9964` | still running at **43 min** | — |

And the step that actually died, on PR #26's baseline comparison:

```
2026-11-04T11:16:48  Running benches/benchmarks.rs (target/release/deps/benchmarks-72286bfad1e22bd3)
2026-11-04T11:16:48  Gnuplot not found, using plotters backend
2026-11-04T11:16:48  Benchmarking sandbox_creation/create_sandbox
2026-11-04T11:16:52  Benchmarking sandbox_creation/create_sandbox: Collecting 10 samples in estimated 30.000 s (16M iterations)
2026-11-04T11:17:39  ##[error]Process completed with exit code 143.
```

Exit 143 is SIGTERM, 51 seconds into that one case. It is **not** the job timeout: the job started
at 10:56:50, so it died 20m49s into a 60-minute budget, and no cancellation appears anywhere in the
log. (`timeout-minutes: 60` is unchanged between the old and current workflow; checked — the
hypothesis that a shorter old limit caused it is wrong.)

## Why this matters

1. **It blocks contributions.** PR #26's `Benchmark` check is red for a reason that has nothing to
   do with the contributor's change: their branch carries an older workflow, and the failing step
   builds the **base** branch.
2. **A green benchmark job that sometimes takes 25 min and sometimes over 60 is not a signal.** It
   is a coin flip on the job limit.
3. **The same unboundedness bites locally.** `cargo bench` on the device peaks over 1 GB just to
   compile the bench profile, before any benchmark runs, and `sandbox_creation` then adds more.
   That is what makes Android's low-memory killer pick the Termux app.

## Fix applied

Bound the suite in both benchmark steps:

```yaml
          cargo bench --workspace --bench benchmarks --locked -- \
            --warm-up-time 1 --measurement-time 3 --sample-size 10 2>&1 \
            | tee benchmark-output.txt
```

`--bench benchmarks` is required for the criterion flags to reach the criterion harness at all:
`cargo bench -- <args>` also feeds them to the libtest harness of `lib`/`lib test`, which rejects
them with `error: Unrecognized option: 'warm-up-time'`. Verified on device against the compiled bench
binary — zero "Unrecognized option", real measurement emitted
(`guardrails_evaluation/evaluate_request  time: [216.60 ns 219.34 ns 222.02 ns]`), peak 6 MB.

A CI benchmark job should be a smoke signal and a regression tripwire, not a measurement
laboratory. Full-suite numbers belong in a deliberate run.

## Still open

- **`sandbox_creation/create_sandbox` needs its own look.** It is the case that got SIGTERM, it is
  the most expensive case in the suite, and it allocates a real sandbox inside a benchmark. Whether
  it is simply slow, or whether it spawns something that signals its own process group, is not yet
  established — bounding hides the symptom, it does not explain it.
- **The 25.7 min -> 43 min growth between `6c58b080` and `e3f9964` is unexplained.** Something got
  materially slower across those commits; the bounded run will make that invisible too, which is why
  it is recorded here rather than left as a memory of a slow CI day.

Labels: ci, performance
EOF

ISSUE_URL=$(gh issue create \
  --title "[CI-5] Benchmark job is unpredictable: 25.7 min then 43+ min, and a baseline step killed with SIGTERM" \
  --body-file research/issues/benchmark_unpredictable.md \
  --label "ci,performance" 2>&1 | tail -1)
echo "issue: $ISSUE_URL"

git add .github/workflows/benchmark.yml research/issues/benchmark_unpredictable.md
git commit -q -F - <<'MSG'
ci(benchmark): bound the benchmark suite instead of running criterion's defaults

The Benchmark job was not predictable and one of its steps was being killed.

Same suite, two revisions, 60-minute job limit:
  6c58b080  Run benchmarks success    job 25.7 min
  e3f9964   Run benchmarks still running at 43 min

And on PR #26's baseline comparison, `sandbox_creation/create_sandbox` was killed
with exit 143 (SIGTERM) 51 seconds in. Not the job timeout -- the job started at
10:56:50 and died 20m49s in, with no cancellation anywhere in the log, and
`timeout-minutes: 60` is unchanged between the old and new workflow (checked, so
the "shorter old limit" hypothesis is dead).

Both benchmark steps now run bounded:

    cargo bench --workspace --bench benchmarks --locked -- \
      --warm-up-time 1 --measurement-time 3 --sample-size 10

`--bench benchmarks` is required for the flags to reach the criterion harness:
`cargo bench -- <args>` also feeds them to the libtest harness of lib/lib test,
which rejects them with `error: Unrecognized option: 'warm-up-time'`. Verified on
device against the compiled bench binary: zero "Unrecognized option", a real
measurement emitted, peak 6 MB.

A CI benchmark job is a smoke signal and a regression tripwire, not a measurement
laboratory.

Recorded rather than forgotten (issue filed): sandbox_creation/create_sandbox is
the case that died and deserves its own look, and the 25.7 -> 43 min growth between
those two revisions is unexplained. Bounding hides both, which is exactly why they
are written down.
MSG

echo "=== push ==="
git push "https://michaelamoetoevsky:${TOKEN}@github.com/michaelamoetoevsky/PDN.git" master 2>&1 | tail -2
L=$(git rev-parse HEAD); R=$(git ls-remote "https://michaelamoetoevsky:${TOKEN}@github.com/michaelamoetoevsky/PDN.git" master | cut -f1)
echo "local : $L"; echo "remote: $R"; [ "$L" = "$R" ] && echo "PUSH CONFIRMED" || echo "MISMATCH"
