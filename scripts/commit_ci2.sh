#!/bin/sh
# File the broken-action issue and commit the remaining CI/CD work. POSIX sh.
set -e
cd "$(dirname "$0")/.."

TOKEN=$(grep -o 'ghp_[A-Za-z0-9]\{20,\}' ~/.hermes/.env | head -1)
[ -n "$TOKEN" ] || { echo "token not found"; exit 1; }

MSG="$HOME/.hermes/cache/scratch/commitmsg"
mkdir -p "$MSG"

cat > research/issues/ci2.md <<'EOF'
## Summary

`dependency-updates.yml` referenced an action that **does not exist**:
`peter-evans/update-github-actions@v1`. `gh api repos/peter-evans/update-github-actions`
returns `404 Not Found`, and the owner's repository list contains no such project. The job
`github-actions-dependencies` therefore could never have succeeded — and the broken pin went
unnoticed because the scheduled trigger skips that job (`github.event.inputs.dependency-type`
is empty on a `schedule` event), so it was never executed.

## Reproduction / evidence

```
$ env -u LD_LIBRARY_PATH gh api repos/peter-evans/update-github-actions
{"message":"Not Found","documentation_url":"https://docs.github.com/rest/repos/repos#get-a-repository","status":404}

$ env -u LD_LIBRARY_PATH gh api "users/peter-evans/repos?per_page=100" --jq '.[].name' \
    | grep -i 'action\|update'
create-or-update-comment
create-or-update-project-card
approve-pull-request-action
create-or-update-comment
gradle-auto-dependency-updates
...
# no `update-github-actions`
```

Recorded in `research/CI_CD_AUDIT.md` as an unresolvable third-party reference; confirmed
independently by querying the REST API for every pin in every workflow (14 distinct pins).

## Root cause

The pin was added when the workflow first landed (commit `950a927`) and never exercised,
because the only automatic trigger (weekly `schedule`) leaves the input that gates the job
unset, and the job is `skipped` on every scheduled run — see run `41424692889`, where
`Update GitHub Actions Dependencies` shows `skipped` while `Update Rust Dependencies` failed.

## Impact

- The job is dead code: it cannot run, so GitHub Actions versions are never updated.
- Worse, it is a *silent* failure mode: an unresolvable action in any job only surfaces when
  that job runs, which may be never.

## Fix

Replaced the third-party updater with a self-contained audit step that depends on nothing
but `gh` (already present on the runner):

- enumerates every `uses:` pin across `.github/workflows/*.yml`;
- resolves each via `gh api repos/<owner>/<repo>/releases/latest`, falling back to the tag
  list for actions that publish no releases (branch-ref pins);
- writes a summary table to `$GITHUB_STEP_SUMMARY`;
- **fails the job when any pin cannot be resolved**, so a nonexistent action is caught here
  rather than silently breaking whichever job references it.

Resolving by exit status, not by output, matters: `gh api` prints its error body on stdout,
so an emptiness check treats a 404 as a resolved version. (The first version of this step had
exactly that bug; it was caught by running it against the previous revision.)

Verified both directions before committing:

```
A) PREVIOUS revision:
  peter-evans/update-github-actions  pinned=v1  latest=-   UNRESOLVABLE
  -> unresolved=1
  -> VERDICT: FAIL (exit 1)          # the gate catches it

B) CURRENT revision:
  -> unresolved=0
  -> VERDICT: pass (exit 0)
```

All 14 remaining pins were verified to exist against the REST API; the pin bump to modern
Node-24-compatible majors was done only where a release actually exists.

Labels: bug, ci, github-actions
EOF

URL=$(env -u LD_LIBRARY_PATH gh issue create \
  --title "[CI-2] dependency-updates workflow references a nonexistent action (peter-evans/update-github-actions 404s)" \
  --body-file research/issues/ci2.md --label "bug,ci,github-actions")
echo "issue: $URL"

cat > "$MSG/11.txt" <<'EOF'
ci(deps): replace a nonexistent action with a real pin audit; make security gates honest

The `github-actions-dependencies` job called `peter-evans/update-github-actions@v1`,
which does not exist — `gh api repos/peter-evans/update-github-actions` returns 404. The
job could never have run, and nobody noticed because the weekly `schedule` trigger leaves
`github.event.inputs.dependency-type` unset, so the job is `skipped` on every scheduled
run (see run 41424692889).

Replace it with a self-contained audit step built on `gh` alone:

- enumerate every `uses:` pin across `.github/workflows/*.yml`;
- resolve each via `releases/latest`, falling back to the tag list for actions that
  publish no releases (branch-ref pins such as `dtolnay/rust-toolchain@stable`);
- write a table to `$GITHUB_STEP_SUMMARY`;
- **fail the job when a pin cannot be resolved**, turning this class of silent breakage
  into a visible CI failure.

Resolution is decided by exit status rather than output: `gh api` writes its error body to
stdout, so an emptiness check reads a 404 as success. Verified against both revisions:

    A) PREVIOUS revision -> peter-evans/update-github-actions  UNRESOLVABLE  unresolved=1  FAIL(1)
    B) CURRENT  revision -> unresolved=0                                        pass(0)

Also in this change:

- D5 security gates made real. `deny.toml` is now committed (its `[licenses] allow` list
  was derived from the resolved dependency graph — `cargo metadata --locked`, 477 packages,
  distinct license expressions counted; `oci-spec` reports no SPDX field and is covered by
  its `license-file`), so `cargo deny init` no longer fabricates a config that always
  passes. `|| true` removed from the `cargo audit` / `cargo deny check` steps: a finding
  now fails the job.
- D9 the `Run tests` / `Run clippy` / `Check formatting` steps in release.yml were gated on
  `matrix.target != 'aarch64-linux-android' && matrix.target != 'x86_64-linux-android'`
  while the matrix contained only those two targets — the condition was never true, so
  release binaries shipped untested and unlinted. A host target
  (`x86_64-unknown-linux-gnu`) was added to the matrix, so tests/clippy/fmt now execute on
  the host while the Android targets only build. Android-only steps are gated on
  `!= 'x86_64-unknown-linux-gnu'`.
- D4 `base: main` -> `base: master` (the repo has only `master`).
- D2 concurrency on both files; `cancel-in-progress: false` for release.yml so a release is
  never cancelled mid-upload.
- `--locked` on every cargo invocation.
- Action pins verified against the REST API before any bump; no version was guessed.

Refs #22
EOF

git add .github/workflows/dependency-updates.yml .github/workflows/release.yml deny.toml research/issues/ci2.md
git commit -F "$MSG/11.txt"
echo "=== LOG ==="
git log --oneline -4
echo "=== remaining ==="
git status --short
