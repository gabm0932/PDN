## Summary

The `Benchmark` workflow reported **success for a run that failed**. `cargo bench` could not
compile the benchmark targets (26 errors, API drift), and the failure never reached the job
because every command in the step was piped and GitHub Actions' default `bash -e` does not
set `pipefail`.

```
      - name: Run benchmarks
        run: |
          cargo bench --workspace --locked --no-run 2>&1 | tail -20
          cargo bench --workspace --locked 2>&1 | tee benchmark-output.txt
```

A pipeline's exit status is the **last** command's: `tail` and `tee` exit 0, so
`##[error]Process completed with exit code 101` never happened. The workflow has been 15/15
green while measuring nothing.

This is the same class as the `|| true` gates (D5) — a check that cannot fail. It is worth
noting that `research/CI_CD_AUDIT.md` D12 had already flagged the workflow as misleading, but
attributed it to the wrong mechanism ("no `[[bench]]` targets exist"); the opposite is true.

## Evidence

Benchmark targets exist and are real:

```
$ grep -rn '\[\[bench\]\]' crates/ Cargo.toml
crates/pn-gateway/Cargo.toml:32:[[bench]]
crates/pn-next/Cargo.toml:73:[[bench]]
# both: name = "benchmarks", path = "benches/benchmarks.rs", harness = false
# both files use criterion_group!/criterion_main!/bench_function
```

They do not compile (26 errors across the two targets — 8× E0422, 6× E0560, 6× E0599,
5× E0432, 1× E0433):

```
$ cargo bench --workspace --locked --no-run
error[E0432]: unresolved imports `pn_gateway::GuardrailsConfig`, `pn_gateway::GuardrailsEngine`, `pn_gateway::ResourceRequest`
error[E0422]: cannot find struct, variant or union type `BulkheadConfig` in crate `pn_gateway`
error[E0560]: struct `FailSafeConfig` has no field named `bulkhead`
error[E0599]: no method named `unwrap` found for struct `FailSafeManager` in the current scope
error: could not compile `pn-gateway` (bench "benchmarks") due to 12 previous errors
error: could not compile `pn-next` (bench "benchmarks") due to 14 previous errors
```

Yet every `Benchmark` run on record is green (15/15 in the audit's window).

## Root cause

Two independent defects, either of which would have hidden the other:

1. **Pipe without `pipefail`** in the benchmark step — the exit status was discarded.
2. **The bench sources drifted from the crate APIs** — constructors that used to return
   `Result` now return the value (`FailSafeManager`, `HealthMonitor`, `MetricsCollector`),
   config types were renamed or moved (`BulkheadConfig`, `RetryPolicyConfig`,
   `GuardrailsConfig`/`GuardrailsEngine`/`ResourceRequest`), struct fields changed
   (`FailSafeConfig.bulkhead`/`.retry_policy`, `ContainerConfig.config`/`.sandbox_id`), and
   several imports no longer resolve (`pn_next::{Resources, RuntimeManager, SecurityProfile}`,
   `proot_compat::path_translation::*`).

The newly added guard step made this worse rather than better: it greps for the `[[bench]]`
marker and passes, which is a green tick over a target that cannot build.

## Impact

- The benchmark suite has not run or been maintained for as long as the API has drifted.
- A green `Benchmark` check carried no information, including for performance regressions.
- Anyone trusting it would conclude the benches were healthy.

## Fix

- `set -euo pipefail` in every benchmark step, so a piped failure fails the job.
- The guard now **builds** the targets (`cargo bench --workspace --locked --no-run`) instead
  of only grepping for the marker.
- The 26 compile errors repaired in `crates/pn-gateway/benches/benchmarks.rs` and
  `crates/pn-next/benches/benchmarks.rs`, adapting each reference to the current API and
  preserving every benchmark (no benchmark deleted to make the file compile).

Labels: bug, ci, performance
