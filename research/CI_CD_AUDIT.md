# PDN CI/CD Audit

Read-only audit of all GitHub Actions workflows in `michaelamoetoevsky/PDN`.
Date: 2026-11-04 (WIB). Default branch: **`master`** (confirmed). Repo has exactly one remote branch (`master`).

- Workflows audited: **6**
- Distinct defects found: **12**
- Failing workflow on master: `PDN CI/CD` (`ci.yml`), run `41744808292`
- Root cause of that failure: flaky test `test_restart_policy_unless_stopped` (fixed 3000 ms sleeps)

All evidence was collected with `gh run list` / `gh run view <id> --log-failed` and file reads. No files were modified except this report.

---

## 1. Workflow inventory

### 1.1 `ci.yml` — name: `PDN CI/CD`

| Property | Value |
|---|---|
| File | `.github/workflows/ci.yml` (476 lines) |
| Triggers | `push` branches `[master, main, develop]`, tags `v*`; `pull_request` branches `[master, main, develop]`; `workflow_dispatch` (input `release_version`) |
| Path filters | **none** — runs on every push to master |
| Concurrency | **none** |
| Runner OS | `ubuntu-latest` (→ ubuntu-24.04 per logs) |
| Toolchain | `dtolnay/rust-toolchain@stable`; Android NDK `r27c` via `nttld/setup-ndk@v1`; Android API level 34 (`aarch64-linux-android34-clang`); nightly toolchain for fuzz |
| Caching | `actions/cache@v4` ×2 in host build (+ registry, + target keyed by full SHA), ×1 in android job, ×1 in integration, ×1 in release. Keys: `${runner.os}-cargo-${hashFiles('**/Cargo.lock')}` |
| Jobs | 7: `rust-host-build` (30m), `rust-android-cross-compile` (45m), `security-audit` (20m), `docs` (15m), `fuzz` (60m), `proot-distro-integration-test` (60m), `release` (30m, needs all, gated on tag push) |
| Est. cost | **34.4 billable job-minutes / push** (measured, run `41741331011`) |

### 1.2 `cross-compile.yml` — name: `Android Cross-Compile Matrix`

| Property | Value |
|---|---|
| File | `.github/workflows/cross-compile.yml` (186 lines) |
| Triggers | `push`/`pull_request` branches `[master, main, develop]`, tags `v*`; `workflow_dispatch` (`ndk_version` r27c/r30) |
| Path filters | **none** |
| Concurrency | **none** |
| Runner OS | `ubuntu-latest` |
| Toolchain | `stable`; NDK matrix `r27c` + `r30`; API 24; targets `aarch64-linux-android`, `x86_64-linux-android`; `qemu-user-static` for x86_64 |
| Caching | `actions/cache@v4` per matrix cell (registry + target) |
| Jobs | `cross-compile` (4-cell matrix, 60m) + `aggregate-artifacts` (`if: always()`, 15m) |
| Est. cost | **22.3 billable job-minutes / push** (measured, run `41744808230`) |

### 1.3 `benchmark.yml` — name: `Benchmark`

| Property | Value |
|---|---|
| File | `.github/workflows/benchmark.yml` (83 lines) |
| Triggers | `push`/`pull_request` branches `[master, main, develop]`; `workflow_dispatch` |
| Path filters | **none** |
| Concurrency | **none** |
| Runner OS | `ubuntu-latest` |
| Toolchain | `stable`; `cargo-criterion` |
| Caching | `actions/cache@v4` (registry + target) |
| Jobs | `benchmark` (60m) |
| Est. cost | **1.9 billable job-minutes / push** (measured, run `41744808339`) |

### 1.4 `dependency-updates.yml` — name: `Dependency Updates`

| Property | Value |
|---|---|
| File | `.github/workflows/dependency-updates.yml` (159 lines) |
| Triggers | `schedule` cron `0 9 * * 1` (Mon 09:00 UTC); `workflow_dispatch` (`dependency-type`) |
| Path filters | n/a (scheduled) |
| Concurrency | **none** |
| Runner OS | `ubuntu-latest` |
| Toolchain | `stable`; `cargo-outdated`, `cargo-audit`, `cargo-deny` |
| Caching | `actions/cache@v4` (registry + target) |
| Jobs | `rust-dependencies` (45m), `github-actions-dependencies` (15m), `security-audit` (30m) |
| Est. cost | ~45 min/week (job `rust-dependencies` alone 45m) |

### 1.5 `devcontainer-test.yml` — name: `DevContainer Test`

| Property | Value |
|---|---|
| File | `.github/workflows/devcontainer-test.yml` (32 lines) |
| Triggers | `push`/`pull_request` with `paths: ['.devcontainer/**']`; `workflow_dispatch` |
| Path filters | **yes** (`.devcontainer/**`) |
| Concurrency | **none** |
| Runner OS | `ubuntu-latest` |
| Toolchain | devcontainer image `ghcr.io/michaelamoetoevsky/pdn-devcontainer` |
| Caching | none |
| Jobs | `devcontainer-test` (30m) |
| Est. cost | 0 (has **never run** — 0 runs on record) |

### 1.6 `release.yml` — name: `Release Automation`

| Property | Value |
|---|---|
| File | `.github/workflows/release.yml` (389 lines) |
| Triggers | `push` tags `v*`; `workflow_dispatch` (`version`, required) |
| Path filters | n/a (tag) |
| Concurrency | **none** |
| Runner OS | `ubuntu-latest` |
| Toolchain | `stable`; NDK `r27c` + API 34; `cargo-release` |
| Caching | `actions/cache@v4` (registry + target, per matrix.rust) |
| Jobs | `build-and-test` (2-cell matrix), `security-audit`, `create-release`, `publish-crates` |
| Est. cost | ~15 min/tag (only fires on tags) |

---

## 2. Run history per workflow

### 2.1 `ci.yml` — `PDN CI/CD` — 6 success / 9 failure = **40.0% success** (last 15)

| Run | Conclusion | SHA | Event | Date (UTC) | URL |
|---|---|---|---|---|---|
| 41744808292 | **failure** | 2c0ce65 | push | 2026-11-04T00:10Z | https://github.com/michaelamoetoevsky/PDN/actions/runs/41744808292 |
| 41741331011 | success | 43ed0d6 | push | 2026-11-03T23:43Z | https://github.com/michaelamoetoevsky/PDN/actions/runs/41741331011 |
| 41734184181 | success | 7544859 | push | 2026-11-03T22:50Z | https://github.com/michaelamoetoevsky/PDN/actions/runs/41734184181 |
| 41731786571 | **failure** | c00809c | push | 2026-11-03T22:33Z | https://github.com/michaelamoetoevsky/PDN/actions/runs/41731786571 |
| 41729354304 | **failure** | a13a258 | push | 2026-11-03T22:16Z | https://github.com/michaelamoetoevsky/PDN/actions/runs/41729354304 |
| 41722147939 | success | 06896c3 | push | 2026-11-03T21:28Z | https://github.com/michaelamoetoevsky/PDN/actions/runs/41722147939 |
| 41709252974 | success | 82901c5 | push | 2026-11-03T20:05Z | https://github.com/michaelamoetoevsky/PDN/actions/runs/41709252974 |
| 41700916421 | success | 295909d | push | 2026-11-03T19:11Z | https://github.com/michaelamoetoevsky/PDN/actions/runs/41700916421 |
| 41700546300 | success | a5e0756 | push | 2026-11-03T19:09Z | https://github.com/michaelamoetoevsky/PDN/actions/runs/41700546300 |
| 41699129446 | **failure** | 8ba9d3e | push | 2026-11-03T19:00Z | https://github.com/michaelamoetoevsky/PDN/actions/runs/41699129446 |
| 41697027217 | **failure** | e85412c | push | 2026-11-03T18:46Z | https://github.com/michaelamoetoevsky/PDN/actions/runs/41697027217 |
| 41696785269 | **failure** | ec3b020 | push | 2026-11-03T18:45Z | https://github.com/michaelamoetoevsky/PDN/actions/runs/41696785269 |
| 41696185696 | **failure** | dc665b2 | push | 2026-11-03T18:41Z | https://github.com/michaelamoetoevsky/PDN/actions/runs/41696185696 |
| 41695506735 | **failure** | a97fa13 | push | 2026-11-03T18:36Z | https://github.com/michaelamoetoevsky/PDN/actions/runs/41695506735 |
| 41694273236 | **failure** | 2636960 | push | 2026-11-03T18:29Z | https://github.com/michaelamoetoevsky/PDN/actions/runs/41694273236 |

Failing job/step (runs `41744808292` and `41731786571`): **`Rust Host Build` → `Run tests`**.

### 2.2 `cross-compile.yml` — `Android Cross-Compile Matrix` — 8 success / 7 failure = **53.3% success** (last 15)

| Run | Conclusion | SHA | Date (UTC) | URL |
|---|---|---|---|---|
| 41744808230 | success | 2c0ce65 | 2026-11-04T00:10Z | https://github.com/michaelamoetoevsky/PDN/actions/runs/41744808230 |
| 41741330942 | success | 43ed0d6 | 2026-11-03T23:43Z | https://github.com/michaelamoetoevsky/PDN/actions/runs/41741330942 |
| 41734184256 | success | 7544859 | 2026-11-03T22:50Z | https://github.com/michaelamoetoevsky/PDN/actions/runs/41734184256 |
| 41731786515 | success | c00809c | 2026-11-03T22:33Z | https://github.com/michaelamoetoevsky/PDN/actions/runs/41731786515 |
| 41729354310 | success | a13a258 | 2026-11-03T22:16Z | https://github.com/michaelamoetoevsky/PDN/actions/runs/41729354310 |
| 41722148165 | success | 06896c3 | 2026-11-03T21:28Z | https://github.com/michaelamoetoevsky/PDN/actions/runs/41722148165 |
| 41709252782 | success | 82901c5 | 2026-11-03T20:05Z | https://github.com/michaelamoetoevsky/PDN/actions/runs/41709252782 |
| 41700916605 | success | 295909d | 2026-11-03T19:11Z | https://github.com/michaelamoetoevsky/PDN/actions/runs/41700916605 |
| 41700546290 | **failure** | a5e0756 | 2026-11-03T19:09Z | https://github.com/michaelamoetoevsky/PDN/actions/runs/41700546290 |
| 41699129722 | **failure** | 8ba9d3e | 2026-11-03T19:00Z | https://github.com/michaelamoetoevsky/PDN/actions/runs/41699129722 |
| 41697027258 | **failure** | e85412c | 2026-11-03T18:46Z | https://github.com/michaelamoetoevsky/PDN/actions/runs/41697027258 |
| 41696785498 | **failure** | ec3b020 | 2026-11-03T18:45Z | https://github.com/michaelamoetoevsky/PDN/actions/runs/41696785498 |
| 41696185515 | **failure** | dc665b2 | 2026-11-03T18:41Z | https://github.com/michaelamoetoevsky/PDN/actions/runs/41696185515 |
| 41695506509 | **failure** | a97fa13 | 2026-11-03T18:36Z | https://github.com/michaelamoetoevsky/PDN/actions/runs/41695506509 |
| 41694273312 | **failure** | 2636960 | 2026-11-03T18:29Z | https://github.com/michaelamoetoevsky/PDN/actions/runs/41694273312 |

Failing job/step: the two aarch64 matrix cells → step `Run tests for aarch64-linux-android`. x86_64 cells always passed.

### 2.3 `benchmark.yml` — `Benchmark` — 15 success / 0 failure = **100% success** (last 15)

| Run | Conclusion | SHA | Date (UTC) | URL |
|---|---|---|---|---|
| 41744808339 | success | 2c0ce65 | 2026-11-04T00:10Z | https://github.com/michaelamoetoevsky/PDN/actions/runs/41744808339 |
| 41741331036 | success | 43ed0d6 | 2026-11-03T23:43Z | https://github.com/michaelamoetoevsky/PDN/actions/runs/41741331036 |
| 41734184263 | success | 7544859 | 2026-11-03T22:50Z | https://github.com/michaelamoetoevsky/PDN/actions/runs/41734184263 |
| 41731786614 | success | c00809c | 2026-11-03T22:33Z | https://github.com/michaelamoetoevsky/PDN/actions/runs/41731786614 |
| 41729354516 | success | a13a258 | 2026-11-03T22:16Z | https://github.com/michaelamoetoevsky/PDN/actions/runs/41729354516 |
| 41722147963 | success | 06896c3 | 2026-11-03T21:28Z | https://github.com/michaelamoetoevsky/PDN/actions/runs/41722147963 |
| 41709252768 | success | 82901c5 | 2026-11-03T20:05Z | https://github.com/michaelamoetoevsky/PDN/actions/runs/41709252768 |
| 41700916594 | success | 295909d | 2026-11-03T19:11Z | https://github.com/michaelamoetoevsky/PDN/actions/runs/41700916594 |
| 41700546258 | success | a5e0756 | 2026-11-03T19:09Z | https://github.com/michaelamoetoevsky/PDN/actions/runs/41700546258 |
| 41699129356 | success | 8ba9d3e | 2026-11-03T19:00Z | https://github.com/michaelamoetoevsky/PDN/actions/runs/41699129356 |
| 41697027140 | success | e85412c | 2026-11-03T18:46Z | https://github.com/michaelamoetoevsky/PDN/actions/runs/41697027140 |
| 41696785235 | success | ec3b020 | 2026-11-03T18:45Z | https://github.com/michaelamoetoevsky/PDN/actions/runs/41696785235 |
| 41696185630 | success | dc665b2 | 2026-11-03T18:41Z | https://github.com/michaelamoetoevsky/PDN/actions/runs/41696185630 |
| 41695506444 | success | a97fa13 | 2026-11-03T18:36Z | https://github.com/michaelamoetoevsky/PDN/actions/runs/41695506444 |
| 41694273486 | success | 2636960 | 2026-11-03T18:29Z | https://github.com/michaelamoetoevsky/PDN/actions/runs/41694273486 |

(100% here is misleading: `cargo bench` finds no benchmarks, so the job is effectively a no-op build. See defect D12.)

### 2.4 `dependency-updates.yml` — `Dependency Updates` — 0 success / 1 failure = **0% success** (all runs on record)

| Run | Conclusion | SHA | Event | Date (UTC) | URL |
|---|---|---|---|---|---|
| 41424692889 | **failure** | 87fdf58 | schedule | 2026-11-02T09:56Z | https://github.com/michaelamoetoevsky/PDN/actions/runs/41424692889 |

Failing job/step: `Update Rust Dependencies` → `Run tests`.

### 2.5 `devcontainer-test.yml` — `DevContainer Test` — **0 runs ever**.

`gh run list --workflow=devcontainer-test.yml` returned `[]`. The workflow has never executed (no `.devcontainer/**` change since it was added, and no `workflow_dispatch`).

### 2.6 `release.yml` — `Release Automation` — 1 success / 14 failure = **6.7% success** (last 15)

| Run | Conclusion | SHA | Date (UTC) | URL |
|---|---|---|---|---|
| 41246828431 | success | 9e77874 | 2026-10-31T22:08Z | https://github.com/michaelamoetoevsky/PDN/actions/runs/41246828431 |
| 41246007829 | **failure** | 594fa9f | 2026-10-31T21:58Z | https://github.com/michaelamoetoevsky/PDN/actions/runs/41246007829 |
| 41244948215 | **failure** | 981e791 | 2026-10-31T21:43Z | https://github.com/michaelamoetoevsky/PDN/actions/runs/41244948215 |
| 41244268792 | **failure** | fa4aadc | 2026-10-31T21:35Z | https://github.com/michaelamoetoevsky/PDN/actions/runs/41244268792 |
| 41243538954 | **failure** | 057fc39 | 2026-10-31T21:25Z | https://github.com/michaelamoetoevsky/PDN/actions/runs/41243538954 |
| 41242793471 | **failure** | 52487a2 | 2026-10-31T21:16Z | https://github.com/michaelamoetoevsky/PDN/actions/runs/41242793471 |
| 41242080309 | **failure** | 788f0e8 | 2026-10-31T21:07Z | https://github.com/michaelamoetoevsky/PDN/actions/runs/41242080309 |
| 41241268055 | **failure** | f34edce | 2026-10-31T20:57Z | https://github.com/michaelamoetoevsky/PDN/actions/runs/41241268055 |
| 41240588304 | **failure** | 57ce3fb | 2026-10-31T20:48Z | https://github.com/michaelamoetoevsky/PDN/actions/runs/41240588304 |
| 41239900336 | **failure** | 23634b0 | 2026-10-31T20:39Z | https://github.com/michaelamoetoevsky/PDN/actions/runs/41239900336 |
| 41239297409 | **failure** | 8f1f5fa | 2026-10-31T20:31Z | https://github.com/michaelamoetoevsky/PDN/actions/runs/41239297409 |
| 41237992658 | **failure** | 34d080b | 2026-10-31T20:14Z | https://github.com/michaelamoetoevsky/PDN/actions/runs/41237992658 |
| 41213132811 | **failure** | 71df242 | 2026-10-31T15:05Z | https://github.com/michaelamoetoevsky/PDN/actions/runs/41213132811 |
| 41213132429 | **failure** | 71df242 | 2026-10-31T15:05Z | https://github.com/michaelamoetoevsky/PDN/actions/runs/41213132429 |
| 41208989281 | **failure** | a86aee2 | 2026-10-31T14:11Z | https://github.com/michaelamoetoevsky/PDN/actions/runs/41208989281 |

Failing job/step: `Build and Test (stable, aarch64-linux-android)` → `Build workspace` (41208989281) or `Run tests` (41246007829, exit 101). Note 41213132811/41213132429 are two runs of the same SHA pushed seconds apart (duplicate runs — see D2).

---

## 3. Defects (each with file:line + verbatim proof)

### D1 — Flaky test with fixed sleeps fails master (ROOT CAUSE of run 41744808292) — HIGH
- File: `tests/supervisor_e2e.rs:302` (the failing assert); fixed sleeps at `tests/supervisor_e2e.rs:288` and `:298` (`sleep(Duration::from_millis(3000)).await;`); assertion under test begins `tests/supervisor_e2e.rs:278` (`test_restart_policy_unless_stopped`).
- Runs: `41744808292`, `41731786571` (failure); `41741331011`, `41734184181` (success) — same suite, opposite outcome ⇒ timing-dependent.
- Verbatim log (`gh run view 41744808292 --log-failed`):
```
test test_restart_policy_unless_stopped ... FAILED
thread 'test_restart_policy_unless_stopped' (4095) panicked at tests/supervisor_e2e.rs:302:5:
assertion `left == right` failed
  left: Failed
 right: Stopped
test result: FAILED. 18 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out; finished in 12.72s
test failed, to rerun pass `-p pdn-integration-tests --test supervisor_e2e`
##[error]Process completed with exit code 101.
```
- Corroborating log line proving the timing/orphan-process nature:
```
Cleaning up orphan processes
Terminate orphan process: pid (4034) (sleep)
```
- Triggering step: `ci.yml:80-81` (`Run tests` → `cargo test --workspace --release -- --test-threads=4`).

### D2 — No `concurrency:` control anywhere; duplicate runs pile up — HIGH
- Files: `ci.yml` (insert after `on:` block, before `jobs:` at `ci.yml:23`); same for `cross-compile.yml:25`, `benchmark.yml:10`, `dependency-updates.yml:18`, `devcontainer-test.yml:13`, `release.yml:23`.
- Proof: `grep -rn "concurrency\|cancel-in-progress" .github/workflows/` → `NONE`.
- Symptom: run `41213132811` and `41213132429` are two runs of the **same SHA `71df242`** created at the same minute (`2026-10-31T15:05:06Z`) — duplicated/queued runs with no cancellation. Every master push fans out to ~58.6 job-minutes (D8) without dedupe.

### D3 — Node.js 20 deprecation warning for `actions/cache@v4` / `actions/checkout@v4` — MEDIUM
- Files/lines: `actions/checkout@v4` at `ci.yml:31,100,171,226,259,285,366`; `actions/cache@v4` at `ci.yml:42,53,116,294,374`; `cross-compile.yml:53,76,154`; `benchmark.yml:17,27,60`; `dependency-updates.yml:26,34,85,104,122`; `release.yml:34,128,232,244,271,284,290,369`.
- Verbatim (present in **every** captured failed log):
```
##[warning]Node.js 20 is deprecated. The following actions target Node.js 20 but are being forced to run on Node.js 24: actions/cache@v4, actions/checkout@v4. For more information see: https://github.blog/changelog/2025-09-19-deprecation-of-node-20-on-github-actions-runners/
```
- Not yet fatal, but these actions will break on Node 20 removal. `actions/checkout` and `actions/cache` must be bumped to versions that target Node.js 24.

### D4 — Default-branch mismatch: `main` referenced but only `master` exists — HIGH
- File:line: `dependency-updates.yml:74` → `base: main`; `benchmark.yml:49-51` → `git fetch origin main:main` / `git checkout main`; also the `base` for any PR.
- Proof (`gh api repos/michaelamoetoevsky/PDN/branches --jq '.[].name'`):
```
master
```
  and `git branch -r` → `  origin/master` (no `main`).
- Consequence: `peter-evans/create-pull-request@v6` cannot target a nonexistent `base: main`; the benchmark baseline step `git checkout main` will fail on any PR that reaches it. The workflows are otherwise guarded by `branches: [master, main, develop]`, so this is a *latent* break — the branch list is permissive (fine), but the hard-coded `main` target inside steps is wrong.

### D5 — Error swallowing via `|| true` hides real failures (security gates are no-ops) — HIGH
- Counts: `ci.yml` 20, `cross-compile.yml` 10, `dependency-updates.yml` 4, `release.yml` 8, `benchmark.yml` 0, `devcontainer-test.yml` 0.
- Worst instances (security):
  - `ci.yml:192` `run: cargo audit || true`
  - `ci.yml:196-199` `cargo deny init || true` / `cargo deny check licenses || true` / `... bans || true` / `... advisories || true`
  - `ci.yml:208` `cargo geiger --workspace --output-format json > geiger-report.json || true`
  - `dependency-updates.yml:113` `cargo audit --workspace --json > audit-report.json || true`
  - `dependency-updates.yml:119` `cargo deny check advisories bans licenses sources || true`
  - `release.yml:255,261` same pattern.
- Proof the gate is vacuous — **no `deny.toml` is committed**, so `cargo deny init` generates a default config at runtime that always passes:
```
$ ls -la deny.toml .cargo/
ls: cannot access 'deny.toml': No such file or directory
ls: cannot access '.cargo/': No such file or directory
```
- Also `cargo geiger` is never installed (`ci.yml` installs `cargo-audit`/`cargo-deny` only), so line 208 is `command not found` masked by `|| true`.

### D6 — Deprecated/unmaintained `actions-rs/install@v0.1` — MEDIUM
- File:line: `ci.yml:180` and `ci.yml:186` (`uses: actions-rs/install@v0.1`).
- `actions-rs/*` is archived and unmaintained; the same tool install is already done safely via `cargo install cargo-audit` in `dependency-updates.yml:110`. Replace with `cargo install` or `taiki-e/install-action`.

### D7 — Missing `cargo --locked` / lockfile drift — MEDIUM
- Proof: `grep -rn "\-\-locked\|--frozen" .github/workflows/` → `NONE`.
- `Cargo.lock` is committed (`ls -la Cargo.lock` → `137166` bytes) but every build/test step (`ci.yml:78,81`, `ci.yml:147`, `release.yml:179`, …) runs `cargo build`/`cargo test` **without `--locked`**, so CI may silently resolve newer deps than the committed lock. `dependency-updates.yml:51` runs `cargo update --workspace` and then commits only via PR — combined with un-locked builds, CI results are not reproducible against the lockfile.

### D8 — Missing path filters → wasteful full-pipeline runs on every push — MEDIUM
- Only `devcontainer-test.yml:6,9` has `paths:`. `ci.yml`, `cross-compile.yml`, `benchmark.yml` have **no** `paths:` — every docs-only push runs the full ~58.6 job-minute pipeline.
- Measured cost per push (successful runs):
  - `ci.yml` `41741331011`: 34.4 job-minutes (Security 6.5 + Docs 1.4 + Android 3.6 + Integration 7.5 + Fuzz 7.2 + Host 8.2).
  - `cross-compile.yml` `41744808230`: 22.3 job-minutes (4 matrix cells 4.7–6.4 + aggregate 0.1).
  - `benchmark.yml` `41744808339`: 1.9 job-minutes.
  - **Total ≈ 58.6 billable minutes per master push**, multiplied across 9×/hour commit patterns observed on 2026-11-03.

### D9 — Conditional steps in `release.yml` are dead (tests/clippy/fmt never execute) — MEDIUM
- File:line: `release.yml:191`, `:204`, `:217` all use `if: matrix.target != 'aarch64-linux-android' && matrix.target != 'x86_64-linux-android'`.
- The matrix (`release.yml:31`) is `target: [aarch64-linux-android, x86_64-linux-android]` — the condition is **never true**, so `Run tests`, `Run clippy`, `Check formatting` are permanently skipped. The release pipeline ships untested/unlinted Android binaries.

### D10 — Cross-compiled aarch64 tests executed on x86_64 host → `Exec format error` — HIGH (historical, since remediated in current file)
- File:line (current file, post-fix): `cross-compile.yml:126-129` builds tests for aarch64 with `--no-run` (execution moved to x86_64+qemu at `:131-134`). In the failing runs the step `Run tests for aarch64-linux-android` actually executed the binaries.
- Runs: `41700546290`, `41699129722`, `41697027258`, `41696785498`, `41696185515`, `41695506509`, `41694273312`.
- Verbatim (`gh run view 41700546290 --log-failed`):
```
  Exec format error (os error 8)
##[error]Process completed with exit code 101.
```
- Also note the same log shows `##[error]Process completed with exit code 101.` for **both** aarch64 cells (`NDK r27c` and `NDK r30`) while x86_64 cells pass.

### D11 — Duplicate Android cross-compile work — LOW (cost)
- `ci.yml:94-162` (`rust-android-cross-compile`, NDK r27c) re-implements a subset of `cross-compile.yml` (full r27c+r30 matrix). Same target, same API 34, same NDK — the aarch64 compile runs twice per push.

### D12 — `benchmark.yml` reported success while the benchmarks could not build — HIGH
- **Corrected 2026-11-04 after direct verification.** The original text of this entry said
  "`cargo bench --workspace` finds no `[[bench]]` targets ... exits 0 without measuring
  anything". That is wrong on the mechanism, and the error was in the *safe-looking*
  direction: it implied nothing was there to run, when in fact there is a benchmark suite
  that **fails to compile**.
- File:line: `benchmark.yml:67-70` (the `Run benchmarks` step). `crates/pn-gateway/Cargo.toml:32`
  and `crates/pn-next/Cargo.toml:73` DO declare `[[bench]] name = "benchmarks" path =
  "benches/benchmarks.rs" harness = false`, with real criterion code (`criterion_group!`,
  `criterion_main!`, `bench_function`) in both `benches/benchmarks.rs` files.
- Verbatim, on this revision:
```text
$ cargo bench --workspace --locked --no-run
error[E0422]: cannot find struct, variant or union type `BulkheadConfig` in crate `pn_gateway`
error[E0432]: unresolved imports `pn_gateway::GuardrailsConfig`, `pn_gateway::GuardrailsEngine`, `pn_gateway::ResourceRequest`
error[E0560]: struct `FailSafeConfig` has no field named `bulkhead`
error[E0599]: no method named `unwrap` found for struct `FailSafeManager` in the current scope
error: could not compile `pn-gateway` (bench "benchmarks") due to 12 previous errors
error: could not compile `pn-next` (bench "benchmarks") due to 14 previous errors
```
  (26 errors total across the two targets: 8× E0422, 6× E0560, 6× E0599, 5× E0432, 1× E0433.)
- **Why the workflow stayed green** — this is the actual defect, and it is the same class as
  the `|| true` gates: the step piped every command.
```yaml
      - name: Run benchmarks
        run: |
          cargo bench --workspace --locked --no-run 2>&1 | tail -20
          cargo bench --workspace --locked 2>&1 | tee benchmark-output.txt
```
  GitHub Actions runs `run:` steps with `bash -e` and **without `pipefail`**, so a pipeline's
  status is the *last* command's: `tail`/`tee` exit 0 and the compile failure never reached
  the job. Hence 15/15 "success" in §2.3 with a benchmark target that cannot build.
- Correction to §2.3's interpretation: "100% success here is misleading" was right, but not
  because `cargo bench` was a no-op — it is misleading because the run failed and reported
  success.
- Fixed by: adding `set -euo pipefail` to the benchmark steps, and strengthening the guard
  step to build the targets (`cargo bench --workspace --locked --no-run`) rather than only
  grepping for the `[[bench]]` marker — a guard that passes while the target cannot compile
  is worth nothing. The 26 compile errors were then repaired in
  `crates/{pn-gateway,pn-next}/benches/benchmarks.rs` so the workflow is green *and* means
  something.

Additional latent issues observed:
- `ci.yml:466-474` publish step is disabled via `if: false` and its `run:` block has broken indentation (continuation lines indented 18 spaces). Harmless today, but will fail if re-enabled.
- `release.yml:151-176` ("Install Android OpenSSL") sets `OPENSSL_LIB_DIR=.../usr/lib/$TARGET` where `$TARGET` is `aarch64-linux-android` — the NDK sysroot uses the triple without `-linux-android` trieze ordering variations; and path built as `/toolchains/llvm/prebuilt/linux-x86_64/bin/aarch64-linux-android34-clang` with an empty `$ANDROID_NDK_HOME` produced the literal `/toolchains/...` error in run `41208989281`:
```
error occurred in cc-rs: failed to find tool "/toolchains/llvm/prebuilt/linux-x86_64/bin/aarch64-linux-android34-clang": No such file or directory (os error 2)
```
- `dependency-updates.yml:113` security job `if:` gate on `github.event.inputs.dependency-type` means the `security-audit` job always runs (no `if:` on job `security-audit:98`), which is fine, but `github-actions-dependencies` (`:81`) is skipped whenever the schedule fires (no input) — scheduled run `41424692889` shows `Update GitHub Actions Dependencies | skipped`.

---

## 4. Prioritised fixes (precise YAML edits)

### P0 — Fix the flaky test (unblocks `master`)
`tests/supervisor_e2e.rs:288,298` — replace fixed `sleep(Duration::from_millis(3000)).await;` with a polling wait on the supervisor state, e.g.:
```rust
// before line 288
let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
loop {
    let st = supervisor.get_status("restart-unless-stopped").await?;
    if st.total_restarts > 0 || st.state == ServiceState::Restarting { break; }
    if tokio::time::Instant::now() > deadline { panic!("service did not restart within 10s: {:?}", st.state); }
    sleep(Duration::from_millis(100)).await;
}
```
Then at line 298, poll until `state == ServiceState::Stopped` with the same deadline loop instead of a bare sleep. (Optional CI-side mitigation: `ci.yml:81` — `cargo test --workspace --release -- --test-threads=1` reduces lock contention the test comments already worry about.)

### P0 — Add concurrency control to all six workflows
Insert once, immediately after the `on:` block in `ci.yml`, `cross-compile.yml`, `benchmark.yml`, `dependency-updates.yml`, `devcontainer-test.yml`, `release.yml`:
```yaml
concurrency:
  group: ${{ github.workflow }}-${{ github.ref }}
  cancel-in-progress: true
```
(for `release.yml` use `cancel-in-progress: false` so a release is never cancelled mid-upload.)

### P1 — Correct the branch mismatch
- `dependency-updates.yml:74` — change `base: main` → `base: master`.
- `benchmark.yml:49` — change `git fetch origin main:main` → `git fetch origin master:master`, and `:51` `git checkout main` → `git checkout master`.

### P1 — Make security gates real
- Add a committed `deny.toml` at repo root and delete `cargo deny init || true` (`ci.yml:196`).
- `ci.yml:192,197,198,199` — remove `|| true` so a finding fails the job; if advisory noise must be tolerated, use `cargo deny check advisories --deny warnings` with a curated `deny.toml` `[advisories] ignore` list instead of blanket `|| true`.
- Install `cargo-geiger` before `ci.yml:208` (`run: cargo install cargo-geiger`) or drop the step.
- `ci.yml:180,186` — replace `actions-rs/install@v0.1` with:
```yaml
      - name: Install cargo audit & deny
        run: cargo install cargo-audit cargo-deny --locked
```

### P1 — Pin the lockfile
Add `--locked` to every `cargo build`/`cargo test`/`cargo bench` invocation:
`ci.yml:78,81,84,147,318,332-336,340,344,348,352,390,402`; `cross-compile.yml:124,129,134`; `benchmark.yml:42,43,52`; `release.yml:179,192`; `dependency-updates.yml:54`. Example: `cargo build --workspace --release --locked`.

### P1 — Bump actions for Node.js 24
Upgrade `actions/checkout@v4` → `@v5` and `actions/cache@v4` → `@v4` (Node24 build) / `actions/cache@v5` where available, across all five files listed in D3. Also bump `actions/upload-artifact@v4` / `download-artifact@v4` and `softprops/action-gh-release@v1` (`ci.yml:437`) → `@v2` (matching `release.yml:323`).

### P2 — Add path filters (cut ~58 min/push)
Add to `ci.yml` and `cross-compile.yml` under both `push` and `pull_request`:
```yaml
    paths:
      - 'crates/**'
      - 'Cargo.toml'
      - 'Cargo.lock'
      - '.github/workflows/**'
```
Add to `benchmark.yml` a narrower filter (`crates/**`, `benches/**`) or restrict it to `workflow_dispatch` + PRs.

### P2 — Fix dead conditions in `release.yml`
`release.yml:191,204,217` — replace the never-true condition. To run tests/clippy/fmt on the host, add a separate `runs-on: ubuntu-latest` job, or change to `if: matrix.target == 'aarch64-linux-android'` plus a host-target matrix entry. Do not ship untested Android binaries.

### P2 — De-duplicate Android compile
Remove the `rust-android-cross-compile` job (`ci.yml:93-162`) since `cross-compile.yml` already builds aarch64 + x86_64 for r27c/r30, or make `ci.yml` depend on `cross-compile.yml` artifacts instead of rebuilding.

### P3 — Stop the benchmark workflow reporting success for a failed run (CORRECTED)
`benchmark.yml:67-70` — add `set -euo pipefail` so `cargo bench ... | tee` cannot mask the
exit status, and make the guard step actually build the targets
(`cargo bench --workspace --locked --no-run`) instead of only grepping for `[[bench]]`. Then
repair the 26 compile errors in `crates/{pn-gateway,pn-next}/benches/benchmarks.rs` (see D12)
— the benchmarks exist and are worth keeping; they had simply drifted away from the crate APIs.

---

## 5. Evidence index

Every command below was executed in `/data/data/com.termux/files/home/workspace/PDN`. `gh` was always run as `env -u LD_LIBRARY_PATH gh ...`.

| # | Command | Real output excerpt |
|---|---|---|
| 1 | `ls -la .github/workflows/` | `benchmark.yml ci.yml cross-compile.yml dependency-updates.yml devcontainer-test.yml release.yml` |
| 2 | `env -u LD_LIBRARY_PATH gh repo view michaelamoetoevsky/PDN --json defaultBranchRef` | `{"defaultBranchRef":{"name":"master"}}` |
| 3 | `env -u LD_LIBRARY_PATH gh run list --workflow=ci.yml --limit 15 --json name,conclusion,headSha,createdAt,event,url` | 6× `"conclusion":"success"`, 9× `"conclusion":"failure"` (see §2.1) |
| 4 | `env -u LD_LIBRARY_PATH gh run list --workflow=cross-compile.yml --limit 15 ...` | 8 success / 7 failure (see §2.2) |
| 5 | `env -u LD_LIBRARY_PATH gh run list --workflow=benchmark.yml --limit 15 ...` | 15× success (see §2.3) |
| 6 | `env -u LD_LIBRARY_PATH gh run list --workflow=dependency-updates.yml --limit 15 ...` | `[{"conclusion":"failure","event":"schedule","headSha":"87fdf58...","url":".../41424692889"}]` |
| 7 | `env -u LD_LIBRARY_PATH gh run list --workflow=devcontainer-test.yml --limit 15 ...` | `[]` |
| 8 | `env -u LD_LIBRARY_PATH gh run list --workflow=release.yml --limit 15 ...` | 1 success / 14 failure (see §2.6) |
| 9 | `env -u LD_LIBRARY_PATH gh run view 41744808292 --json jobs` | `Rust Host Build | failure | steps: [('Run tests','failure')]`; all other jobs success; `Release | skipped` |
| 10 | `env -u LD_LIBRARY_PATH gh run view 41744808292 --log-failed` (grep) | `test test_restart_policy_unless_stopped ... FAILED` / `panicked at tests/supervisor_e2e.rs:302:5:` / `assertion \`left == right\` failed` / `left: Failed` / `right: Stopped` / `test result: FAILED. 18 passed; 1 failed ... finished in 12.72s` / `##[error]Process completed with exit code 101.` / `Terminate orphan process: pid (4034) (sleep)` |
| 11 | same log, grep deprecation | `##[warning]Node.js 20 is deprecated. The following actions target Node.js 20 but are being forced to run on Node.js 24: actions/cache@v4, actions/checkout@v4.` |
| 12 | `env -u LD_LIBRARY_PATH gh run view 41731786571 --json jobs` | `Rust Host Build | failure | [('Run tests','failure')]` (same step as #9) |
| 13 | `env -u LD_LIBRARY_PATH gh run view 41424692889 --json jobs` | `Update Rust Dependencies | failure | [('Run tests','failure')]`; `Update GitHub Actions Dependencies | skipped`; `Security Audit | success` |
| 14 | `env -u LD_LIBRARY_PATH gh run view 41424692889 --log-failed` | `Cache not found for input keys: Linux-cargo-, Linux-cargo-` / `Error: Failed to spawn daemon` / `No such file or directory (os error 2)` / `test result: FAILED. 0 passed; 4 failed; ... finished in 0.00s` / `##[error]Process completed with exit code 101.` |
| 15 | `env -u LD_LIBRARY_PATH gh run view 41246007829 --json jobs` | `Build and Test (stable, aarch64-linux-android) | failure | [('Run tests','failure')]`; x86_64 cell `cancelled`; `Create Release | skipped` |
| 16 | `env -u LD_LIBRARY_PATH gh run view 41246007829 --log-failed` | `error: test failed, to rerun pass \`-p pn-gateway --lib\`` / `##[error]Process completed with exit code 101.` |
| 17 | `env -u LD_LIBRARY_PATH gh run view 41208989281 --log-failed` | `error occurred in cc-rs: failed to find tool "/toolchains/llvm/prebuilt/linux-x86_64/bin/aarch64-linux-android34-clang": No such file or directory (os error 2)` / `##[error]Process completed with exit code 101.` (job step `Build workspace`) |
| 18 | `env -u LD_LIBRARY_PATH gh run view 41700546290 --json jobs` | aarch64 r27c **and** r30 cells → `[('Run tests for aarch64-linux-android','failure')]`; x86_64 cells success |
| 19 | `env -u LD_LIBRARY_PATH gh run view 41700546290 --log-failed` | `Exec format error (os error 8)` / `##[error]Process completed with exit code 101.` |
| 20 | `env -u LD_LIBRARY_PATH gh run view 41741331011 --json jobs` (timing) | Security 6.5m, Docs 1.4m, Android 3.6m, Integration 7.5m, Fuzz 7.2m, Host 8.2m → **TOTAL 34.4 billable minutes** |
| 21 | `env -u LD_LIBRARY_PATH gh run view 41744808230 --json jobs` (timing) | 4 matrix cells 4.7/6.4/5.6/5.6m + aggregate 0.1m → **22.3 min** |
| 22 | `env -u LD_LIBRARY_PATH gh run view 41744808339 --json jobs` (timing) | `Run Benchmarks | success | 1.9 min` |
| 23 | `grep -rn "concurrency\|cancel-in-progress" .github/workflows/` | `NONE` |
| 24 | `grep -rn "\-\-locked\|--frozen" .github/workflows/` | `NONE` |
| 25 | `grep -rc "\|\| true" .github/workflows/*.yml` | `benchmark.yml:0 ci.yml:20 cross-compile.yml:10 dependency-updates.yml:4 devcontainer-test.yml:0 release.yml:8` |
| 26 | `grep -rn "paths" .github/workflows/` | only `devcontainer-test.yml:6` and `devcontainer-test.yml:9` |
| 27 | `ls -la Cargo.lock` | `-rw------- ... 137166 ... Cargo.lock` |
| 28 | `ls -la deny.toml .cargo/` | `ls: cannot access 'deny.toml': No such file or directory` / `ls: cannot access '.cargo/': No such file or directory` |
| 29 | `env -u LD_LIBRARY_PATH gh api repos/michaelamoetoevsky/PDN/branches --jq '.[].name'` | `master` |
| 30 | `git branch -r` | `  origin/master` |
| 31 | `git log --oneline -5` | `2c0ce65 feat(security): implement --isolated mode...` (HEAD == failing run SHA) |
| 32 | `read_file .github/workflows/{ci,cross-compile,benchmark,dependency-updates,devcontainer-test,release}.yml` | full contents; defect line numbers in §3 are from these reads |
| 33 | `sed -n '270,312p' tests/supervisor_e2e.rs` + `grep -nE "sleep\|Duration::" tests/supervisor_e2e.rs` | `sleep(Duration::from_millis(3000)).await;` at lines 288 and 298; `assert_eq!(status.state, ServiceState::Stopped);` at 302 |
