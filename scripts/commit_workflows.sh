#!/bin/sh
# Commit GitHub Actions hardening (agents F + G scope). POSIX sh.
set -e
cd "$(dirname "$0")/.."

MSG="$HOME/.hermes/cache/scratch/commitmsg"
mkdir -p "$MSG"
cat > "$MSG/9.txt" <<'EOF'
ci(workflows): add concurrency control, path filters, --locked and honest gates

Applies the prioritised fixes from research/CI_CD_AUDIT.md to ci.yml,
cross-compile.yml, benchmark.yml and devcontainer-test.yml. Motivated by the
measured run history: ci.yml 40% success over the last 15 runs, cross-compile.yml
53%, release.yml 6.7%.

- D2 concurrency: `group: ${{ github.workflow }}-${{ github.ref }}` with
  `cancel-in-progress: true`, added to all four. Two runs of the same SHA
  (41213132811, 41213132429) were previously allowed to pile up.
- D8 path filters on ci.yml, cross-compile.yml and benchmark.yml, so a docs-only
  push no longer runs the full pipeline. Measured cost was ~58.6 billable
  job-minutes per master push across the three workflows.
- D7 `--locked` on every cargo build/test/bench invocation in these files.
  Verified in sync first: `cargo metadata --locked` resolves cleanly.
- D4 the `main` branch is not referenced anywhere in the repo; the branch list is
  permissive but any hard-coded `main` target was latent breakage.
- D12 benchmark was a no-op (`cargo bench` finds no benchmark target and exits 0,
  which is why it showed 15/15 "success"); it now fails loudly instead of
  reporting a measurement it never took.
- D3 action pins checked against the real GitHub API (`gh api
  repos/<owner>/<repo>/releases/latest`) before being changed — no version was
  guessed. Where no Node-24 target exists yet the pin was left alone.

Verified: all four files parse as YAML with their job graphs intact
(`ci: jobs=7`, `cross-compile: jobs=2`, `benchmark: jobs=1`,
`devcontainer-test: jobs=1`, each with `concurrency` set and `paths` present).
EOF

git add .github/workflows/ci.yml .github/workflows/cross-compile.yml \
        .github/workflows/benchmark.yml .github/workflows/devcontainer-test.yml
git commit -F "$MSG/9.txt"
echo "=== LOG ==="
git log --oneline -3
echo "=== remaining ==="
git status --short
