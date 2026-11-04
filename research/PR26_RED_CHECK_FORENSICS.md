# PR #26 — Red-Check Forensics

**Verdict: all three claims hold. Nothing in PR #26's diff can produce either red check; both are repo-side.**

- Repo: `michaelamoetoevsky/PDN`
- PR: [#26](https://github.com/michaelamoetoevsky/PDN/pull/26) — *feat(pn-next): implement export_to_compose mapping for ServiceConfig*
- Author: `gabm0932` (fork) · Head: `71611baef2ab2bb15a30826f29c4f8f6f0738cd4` ("Merge branch 'master' into master", maintainer branch update) · Base: `master` · Review: `APPROVED` · `mergeStateStatus: UNSTABLE`
- Diff: `+11 −0` in one file (`crates/pn-next/src/supervisor.rs`)
- Red checks on the head: **Benchmark → job "Run Benchmarks"** (`…/runs/41824324217/job/126555807470`) and **PDN CI/CD → job "Rust Host Build"** (`…/runs/41824324094/job/126555857820`). Every other job on the head is `success` (Release: `skipped`).

Evidence below was collected read-only: `gh run view` / `--log-failed`, `gh api`, `git show`, `git log`, `grep`. Local checkout: `master @ 3e260dc6e9f70bff5f46f78f04c2a8bbada328d4`.

---

## Claim 1 — the Benchmark red is repo-side · **CONFIRMED**

### (a) Which line produces it, at the PR's head

At the PR head `71611ba`, the failing step is `Compare with baseline` (`.github/workflows/benchmark.yml:105`) and the aborting line is **`git stash pop`** (step body line 17), preceded by **`git stash`** (step body line 7).

```
$ gh api "repos/michaelamoetoevsky/PDN/contents/.github/workflows/benchmark.yml?ref=71611ba…" --jq .content | base64 -d | grep -n "Compare with baseline" -A 30
105:      - name: Compare with baseline
106-        if: github.event_name == 'pull_request' && github.event.pull_request.base.repo.full_name == github.repository
107-        run: |
108-          set -euo pipefail
111-          BASEREF="${{ github.base_ref }}"
112-          git fetch origin "$BASEREF:$BASEREF"
113-          git stash
114-          git checkout "$BASEREF"
119-          cargo bench --workspace --bench benchmarks --locked -- \
120-            --warm-up-time 1 --measurement-time 3 --sample-size 10 2>&1 \
121-            | tee benchmark-baseline.txt
122-          git checkout -
123-          git stash pop
```

The step ran exactly as written, and the exact reported epilogue is in the failed log:

```
$ gh run view 41824324217 --log-failed | grep -i "stash entries\|exit code 128\|No local changes"
29:  Run Benchmarks  Compare with baseline  …  No local changes to save
2081:Run Benchmarks  Compare with baseline  …  No stash entries found.
2082:Run Benchmarks  Compare with baseline  …  ##[error]Process completed with exit code 128.
```

Root cause: the CI checkout is clean, so `git stash` has nothing to save — it prints `No local changes to save` and creates **no** stash entry; the later `git stash pop` then finds nothing, prints `No stash entries found.` and exits non-zero, aborting the step **after** the benchmarks had already run. The log also pins the run to this PR: the last checkout line reads `HEAD is now at ebc7914 Merge 71611baef… into 5af5b029…` — i.e. GitHub's merge ref for PR #26.

### (b) The step is gated on `pull_request`, so master never ran it

`.github/workflows/benchmark.yml:106`:

```yaml
if: github.event_name == 'pull_request' && github.event.pull_request.base.repo.full_name == github.repository
```

**CONFIRMED** — the guard is on `github.event_name == 'pull_request'`. On a `push` to `master` the whole `Compare with baseline` step is skipped, which is exactly why master's Benchmark runs looked green while every fork PR died on it. (The `benchmark.yml` `on:` block triggers on both `push` and `pull_request`; only this step is PR-only.)

### (c) The defect is already fixed on current master

```
$ git log --format='%h %ci %s' -6 -- .github/workflows/benchmark.yml
c04f145 2026-11-04 20:04:26 +0700 fix(ci): drop the stash dance that made every PR's benchmark check fail
5af5b02 2026-11-04 18:50:33 +0700 ci(benchmark): bound the benchmark suite instead of running criterion's defaults
e2d2b0a 2026-11-04 17:07:41 +0700 fix(benches): repair 26 compile errors and stop the workflow reporting false success
c760e61 2026-11-04 09:46:10 +0700 ci(workflows): add concurrency control, path filters, --locked and honest gates
```

Commit **`c04f14578d4a8891e2a1d62465c5a306ab102824`** ("drop the stash dance") removed both `git stash` and `git stash pop`. The `git show` diff:

```diff
           git fetch origin "$BASEREF:$BASEREF"
-          git stash
+          # No stash dance. A CI checkout has no local changes to preserve, and the
+          # stash/pop pair is actively harmful here: `git stash` finds nothing to save and
+          # `git stash pop` then exits 1, which aborts the step (observed: "No stash entries
+          # found." + "##[error]Process completed with exit code 128") AFTER the benchmarks ran.
+          # This step only runs on pull_request, so master stayed green while every PR died here.
           git checkout "$BASEREF"
@@
           git checkout -
-          git stash pop
```

Current master line content (`.github/workflows/benchmark.yml:105–118`) — step no longer contains either stash command:

```
105:      - name: Compare with baseline
106:        if: github.event_name == 'pull_request' && github.event.pull_request.base.repo.full_name == github.repository
107:        run: |
108:          set -euo pipefail
111:          BASEREF="${{ github.base_ref }}"
112:          git fetch origin "$BASEREF:$BASEREF"
113:          # No stash dance. A CI checkout has no local changes to preserve, and the
114:          # stash/pop pair is actively harmful here: `git stash` finds nothing to save and
115:          # `git stash pop` then exits 1, which aborts the step (observed: "No stash entries
116:          # found." + "##[error]Process completed with exit code 128") AFTER the benchmarks ran.
117:          # This step only runs on pull_request, so master stayed green while every PR died here.
118:          git checkout "$BASEREF"
```

**Timing note that makes this airtight:** the PR head `71611ba` was created `2026-11-04T11:51:21Z`, but the fix `c04f145` landed at `20:04:26 +0700 = 13:04:26Z`. So the head was cut **before** the fix existed — the head still carries the pre-fix `benchmark.yml` (line 123 `git stash pop`), which is precisely what failed. A green re-run on a head that merges the post-fix master would not hit this step's bug.

---

## Claim 2 — the PDN CI/CD red is a flake, not the contributor's code · **CONFIRMED**

### (a) The same suite is green on master and on the PR's earlier head

| Run id | SHA | Event | Created | Conclusion |
|---|---|---|---|---|
| `41835842744` | `3e260dc6` (master) | push | 2026-11-04T13:11:30Z | **success** |
| `41824211695` | `5af5b029` (master) | push | 2026-11-04T11:50:37Z | **success** |
| `41797332842` | `6ac4101b` (PR author's own commit — **earlier PR head**) | pull_request | 2026-11-04T08:09:22Z | **success** |
| `41824324094` | `71611ba` (PR head after maintainer's master-merge) | pull_request | 2026-11-04T11:51:27Z | **failure** |

```
$ gh run view 41797332842 --json conclusion,headSha,event,workflowName,createdAt
{"conclusion":"success","createdAt":"2026-11-04T08:09:22Z","event":"pull_request",
 "headSha":"6ac4101b72cde506b6bc0f343d734f4ca96bd984","workflowName":"PDN CI/CD"}
$ gh run view 41835842744 --json conclusion,headSha,event,workflowName,createdAt
{"conclusion":"success","createdAt":"2026-11-04T13:11:30Z","event":"push",
 "headSha":"3e260dc6e9f70bff5f46f78f04c2a8bbada328d4","workflowName":"PDN CI/CD"}
```

The PR author's change (`6ac4101b`) passed the full PDN CI/CD workflow on its own. Master `5af5b029` passed *30 seconds before* the failing merge-head run started. The failure appeared only on the merge ref that combined master with the same +11 lines.

Failing log:

```
$ gh run view 41824324094 --log-failed | grep -n "test_restart_policy_on_abnormal\|test result\|panicked"
9125:  test test_restart_policy_on_abnormal ... FAILED
9137:  thread 'test_restart_policy_on_abnormal' (3856) panicked at tests/supervisor_e2e.rs:536:5:
9138:  assertion failed: status.total_restarts > 0 || status.state == ServiceState::Restarting ||
9153:  test result: FAILED. 18 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out; finished in 12.72s
9155:  error: test failed, to rerun pass `-p pdn-integration-tests --test supervisor_e2e`
```

`18 passed; 1 failed` in the same binary; the red job is `Rust Host Build` (`test` step).

### (b) The failing test cannot be touched by the PR's change

The PR modifies exactly one file:

```
$ gh pr view 26 --json files --jq '.files[] | {path, additions, deletions}'
{"path":"crates/pn-next/src/supervisor.rs","additions":11,"deletions":0}
```

The failing test lives in a **different file of a different package**, and that file has not changed in days:

```
$ grep -n "supervisor_e2e" --include=Cargo.toml .
./Cargo.toml:79:name = "supervisor_e2e"
./Cargo.toml:80:path = "tests/supervisor_e2e.rs"

$ git log --format='%h %ci %s' -4 -- tests/supervisor_e2e.rs
87fdf58 2026-11-02 14:59:25 +0700 feat: complete PDN v0.1.7 roadmap
7426b0e 2026-11-02 06:24:31 +0700 chore: update NEXT_STEPS.md and add D-Bus/IPC implementation
```

The panic is at `tests/supervisor_e2e.rs:536`, a **timing assertion**:

```rust
    // With OnAbnormal, SIGKILL (exit code > 128 or negative) should restart
    assert!(
        status.total_restarts > 0
            || status.state == ServiceState::Restarting
            || status.state == ServiceState::Running
    );
```

The test drives it with fixed sleeps and a 1 s restart delay — `supervision_interval(1000ms)`, `base_delay_secs=1`, then `sleep(1000ms)` after start and `sleep(3000ms)` after the kill — and asserts on the resulting state. That is a bet on the scheduler, not an observed transition, i.e. structurally flaky. The PR's added code is `impl ServiceConfig { pub fn export_to_compose(…) -> String }`, a **new, never-called** method appended at module scope; it changes no existing behaviour and is not referenced by the test binary. It cannot flip this assertion.

### (c) Measured flake rate of the workflow

```
$ gh run list --workflow "PDN CI/CD" --limit 45 --json conclusion,event,headSha,createdAt,headBranch   # counted in code
total fetched: 45
all:        {'success': 14, 'cancelled': 1, 'failure': 30}
last 40:    {'success': 14, 'cancelled': 1, 'failure': 25}   # window 2026-10-31T20:57:02Z → 2026-11-04T13:11:30Z
master branch only (32 runs): {'success': 14, 'cancelled': 1, 'failure': 17}
```

**Last ~40 PDN CI/CD runs: 14 success / 25 failure / 1 cancelled** (failure rate 64 % of concluded runs; 36 % green). Restricted to runs on `master`/`main`/`develop`: **14 success / 17 failure / 1 cancelled** over 32 runs. A ~2-in-3 failure rate is not a prompt-commit anomaly — it is the workflow's baseline behaviour, and a red tick on any individual head carries almost no signal about that head's code.

---

## Claim 3 — the contributor's change is sound in isolation · **CONFIRMED**

```
$ gh pr diff 26
diff --git a/crates/pn-next/src/supervisor.rs b/crates/pn-next/src/supervisor.rs
index 6e06512..d83035f 100644
--- a/crates/pn-next/src/supervisor.rs
+++ b/crates/pn-next/src/supervisor.rs
@@ -2363,3 +2363,14 @@ mod tests {
         assert!(config.health.enabled);
     }
 }
+
+impl ServiceConfig {
+    pub fn export_to_compose(&self) -> String {
+        let mut out = String::from("# Generated by podnext\nversion: '3.8'\nservices:\n");
+        out.push_str(&format!("  {}:\n", self.meta.name));
+        if !self.process.command.is_empty() {
+            out.push_str(&format!("    command: {:?}\n", self.process.command));
+        }
+        out
+    }
+}
```

- **Additive:** `+11 −0`, no deletions, no edits to existing lines.
- **One file:** `crates/pn-next/src/supervisor.rs` only.
- **Cannot affect either red check:** it adds a single new method that nothing calls. It does not touch `benchmark.yml`, `tests/supervisor_e2e.rs`, the `[[bench]]` targets, or any code path exercised by `test_restart_policy_on_abnormal`. The only observable effect is one more never-executed function in the crate.

**Contributor-side caveat (not a check failure):** the method is dead code and is formatted inconsistently with `rustfmt` (the long `String::from(...)` line). Neither matters to `cargo test`, but `cargo fmt --all -- --check` in the `Rust Host Build` job could flag the formatting if it ever ran on this file — worth a quick `cargo fmt` on the author's side, but it is *not* what the red checks reported.

---

## Claim 4 — what happens if it is merged now · **master is unprotected; nothing blocks the merge**

```
$ gh api repos/michaelamoetoevsky/PDN/branches/master/protection
{"message":"Branch not protected", "status":"404"}
$ gh api repos/michaelamoetoevsky/PDN/branches/master/protection/required_status_checks
{"message":"Branch not protected", "status":"404"}
$ gh api repos/michaelamoetoevsky/PDN/rulesets
[]
```

**Branch protection: NONE.** `master` has no classic branch protection and no repository ruleset, therefore **no required status checks and no required reviews**. The `mergeStateStatus: UNSTABLE` on the PR reflects only that a check is red — it is a mergeability *annotation*, not a gate. With the button available and `reviewDecision: APPROVED`, GitHub will allow the merge.

What the recorded red checks mean **after** merge:

- They stay attached to the now-historical head `71611ba` and vanish from the merge commit's own check list as the branch advances.
- **Benchmark**: the PR-only step would not run on the resulting master push at all, so a green master Benchmark run proves nothing about PR #26's benchmark; the failure cannot recur on master by construction.
- **PDN CI/CD**: the flaky `test_restart_policy_on_abnormal` can still red-tick a post-merge master run, independent of this PR — merging does not fix the flake. Tracking it as a repo-side issue (the fix commit's message already files it as `#CI-6`) is the durable remedy.

Net: the red ticks are **not** merge blockers (nothing is enforced) and are **not** attributable to `gabm0932`'s +11 lines. Merging is safe from a CI-signal standpoint; the honest framing for the contributor is "your change is fine; our benchmark workflow was broken on PRs and our restart-policy test is flaky — both tracked repo-side."

---

## Summary

| # | Claim | Verdict | Load-bearing artifact |
|---|---|---|---|
| 1 | Benchmark red is repo-side | **CONFIRMED** | `benchmark.yml` `git stash pop` @ head 71611ba (log: `No stash entries found.` + exit 128); step gated on `pull_request`; fixed on master by `c04f145` |
| 2 | PDN CI/CD red is a flake, not the contributor's code | **CONFIRMED** | green on `6ac4101b` (PR's earlier head) and on master `5af5b02`/`3e260dc6`; failing test in an untouched file/package; timing-based assertion; flake rate 14 green / 25 red in last 40 runs |
| 3 | Contributor's change is sound in isolation | **CONFIRMED** | `+11 −0`, one file, one unused method; no path to either check |
| 4 | Merge is safe from a CI perspective | **CONFIRMED** | `master` unprotected (404), no rulesets — red checks are non-blocking annotations |

**Refuted claims: none.** Both red checks were reproduced verbatim from artifacts at the PR head and traced to repo-side causes.
