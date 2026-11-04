#!/bin/sh
# Commit the benchmark repair + workflow consistency fixes, then push. POSIX sh.
set -e
cd "$(dirname "$0")/.."

TOKEN=$(grep -o 'ghp_[A-Za-z0-9]\{20,\}' ~/.hermes/.env | head -1)
[ -n "$TOKEN" ] || { echo "token not found"; exit 1; }

MSG="$HOME/.hermes/cache/scratch/commitmsg"
mkdir -p "$MSG"

cat > "$MSG/18.txt" <<'EOF'
fix(benches): repair 26 compile errors and stop the workflow reporting false success

The `Benchmark` workflow was a false green. `cargo bench` could not compile the
benchmark targets and the failure never reached the job, because the step piped
every command and GitHub Actions' default `bash -e` does not set `pipefail`:

    cargo bench --workspace --locked --no-run 2>&1 | tail -20
    cargo bench --workspace --locked 2>&1 | tee benchmark-output.txt

A pipeline's status is the LAST command's, so `tail`/`tee` returned 0 and the
compile failure was invisible — 15/15 green while measuring nothing. Same class
as the `|| true` gates removed earlier: a check that cannot fail.

Two independent defects had been hiding each other:

1. The bench sources had drifted from the crate APIs — 26 errors across the two
   targets (8× E0422, 6× E0560, 6× E0599, 5× E0432, 1× E0433). Constructors that
   used to return `Result` now return the value (`FailSafeManager`,
   `HealthMonitor`, `MetricsCollector`), config types were renamed or moved
   (`BulkheadConfig`, `RetryPolicyConfig`, `GuardrailsConfig`/`GuardrailsEngine`/
   `ResourceRequest`), struct fields changed (`FailSafeConfig.bulkhead`/
   `.retry_policy`, `ContainerConfig.config`/`.sandbox_id`), and imports no longer
   resolved (`pn_next::{Resources, RuntimeManager, SecurityProfile}`,
   `proot_compat::path_translation::*`). Each reference was adapted to the current
   definition read from `src/`.

   The pn-next target then compiled but panicked before listing anything:
   `ContainerManager::new()` runs `block_in_place(Handle::current().block_on(..))`
   and therefore needs an active Tokio runtime, which the criterion group
   registration does not have. The reactor-dependent setup moved inside the
   benchmark closures, where the runtime already exists.

2. The guard step added earlier only grepped for the `[[bench]]` marker and
   passed — a green tick over a target that cannot build. It now builds the
   targets (`cargo bench --workspace --locked --no-run`).

Every benchmark is preserved: `bench_function`/`bench_with_input` counts are
unchanged (12 in pn-gateway, 9 in pn-next) and both `criterion_group!` lists are
intact. Nothing was deleted to make the files compile.

Verified on device:

    $ cargo bench --workspace --locked --no-run
    exit=0   (0 errors)

    pn-gateway -> 25 benchmarks
    pn-next    -> 18 benchmarks

    $ cargo bench -p pn-gateway --bench benchmarks --locked -- --warm-up-time 1 --measurement-time 2
    Benchmarking guardrails_evaluation/evaluate_request
    Benchmarking guardrails_evaluation/evaluate_request: Warming up for 1.0000 s
                            time:   [335.47 ns 349.22 ns 369.74 ns]
    Benchmarking guardrails_batch_evaluation/batch_evaluate/1
    ... 6 measurement lines produced

`research/CI_CD_AUDIT.md` D12 is corrected in the same change: it had attributed
the misleading green to "no `[[bench]]` targets exist" — the opposite is true, the
suite exists and could not build. The entry and its P3 remediation now record the
real mechanism.

Refs #25
EOF

cat > "$MSG/19.txt" <<'EOF'
fix(ci): one security gate in every workflow; remove a step that could not run

Three workflows had drifted into three different security behaviours.

- `release.yml` ran `cargo audit` unconditionally. `cargo audit` cannot read the
  acceptance list in `deny.toml`, so it reported all 35 advisory entries and would
  have failed this job the first time a release fired. Invisible so far because
  the workflow is tag-gated. Removed; `cargo deny check advisories bans licenses
  sources` is the single gate, which is what `ci.yml` does.
- `dependency-updates.yml` had the same unconditional `cargo audit`. It is now
  explicitly an *informational* report (`|| true`, renamed to say so) alongside the
  enforcing `cargo deny` step, because the two answer different questions: audit
  is a raw inventory, deny enforces the documented policy.
- Removed a `Comment on PR with security results` step gated on
  `github.event_name == 'pull_request'`. That workflow has no `pull_request`
  trigger (only `schedule` and `workflow_dispatch`), so the condition was never
  true — the same dead-condition class as the release workflow's skip conditions.
  Its body was broken independently: `report.split('\n').map(JSON.parse)` against
  the output of `cargo audit --json`, which is a single JSON object, not
  JSON-lines, so every line but the first would have thrown.

Refs #24
EOF

cat > "$MSG/20.txt" <<'EOF'
chore(scripts,fuzz): add the remaining helper scripts and track the fuzz lockfile

- scripts/fix_cargo_lock.sh, fix_deny_ignores.sh, fix_geiger.sh,
  fix_security_gate.sh, fix_session_tmp.sh, close_fixed_issues.sh — the exact
  commands used to repair and verify each layer, so the sequence is reproducible
  rather than described. Tokens are read from ~/.hermes/.env at runtime.
- `fuzz/Cargo.lock` is now tracked, matching the root decision: `fuzz/` is its own
  workspace (excluded from the root one) and builds a binary, so its lock belongs
  in version control for the same reproducibility reason. `.gitignore` no longer
  ignores `Cargo.lock` anywhere.
EOF

git add crates/pn-gateway/benches/benchmarks.rs crates/pn-next/benches/benchmarks.rs \
        .github/workflows/benchmark.yml research/CI_CD_AUDIT.md research/issues/bench_false_green.md
git commit -F "$MSG/18.txt"

git add .github/workflows/release.yml .github/workflows/dependency-updates.yml
git commit -F "$MSG/19.txt"

git add scripts/ fuzz/Cargo.lock
git commit -F "$MSG/20.txt"

echo "=== LOG ==="
git log --oneline -6
echo "=== status ==="
git status --short

echo "=== PUSH ==="
git push "https://michaelamoetoevsky:${TOKEN}@github.com/michaelamoetoevsky/PDN.git" master
LOCAL=$(git rev-parse HEAD)
REMOTE=$(git ls-remote "https://michaelamoetoevsky:${TOKEN}@github.com/michaelamoetoevsky/PDN.git" master | cut -f1)
echo "local : $LOCAL"
echo "remote: $REMOTE"
[ "$LOCAL" = "$REMOTE" ] && echo "PUSH CONFIRMED" || echo "MISMATCH"
