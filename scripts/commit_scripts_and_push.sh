#!/bin/sh
# Commit the repo tooling scripts and push everything. POSIX sh.
set -e
cd "$(dirname "$0")/.."

echo "=== pre-push gates ==="
env -u LD_LIBRARY_PATH cargo fmt --all -- --check && echo "fmt: clean"
git diff --check && echo "whitespace: clean"
for f in ci cross-compile benchmark devcontainer-test dependency-updates release; do
  env -u LD_LIBRARY_PATH python3 -c "import yaml;d=yaml.safe_load(open('.github/workflows/$f.yml'));print('$f: yaml ok, jobs=%d' % len(d['jobs']))"
done

MSG="$HOME/.hermes/cache/scratch/commitmsg"
mkdir -p "$MSG"
cat > "$MSG/12.txt" <<'EOF'
chore(scripts): add the reproduction, commit and verification helpers

The shell helpers used while fixing the issues and the CI, so the process is
reproducible rather than described:

- scripts/create_bug_issues.sh        files the audit findings as issues #11-#21.
- scripts/link_issue_fixes.sh         posts the fix evidence back onto each issue.
- scripts/commit_fixes.sh             commits the fixes as separate logical units.
- scripts/commit_wave1.sh             supervisor (CI-1) + session (M1) commits.
- scripts/commit_and_push.sh          artifact commit + push, then verifies the
                                      remote ref matches local HEAD via ls-remote.
- scripts/commit_fmt_and_push.sh      rustfmt pass that unblocked the CI job.
- scripts/commit_tmpdir.sh            the TMPDIR-fallback fix commit.
- scripts/commit_workflows.sh         the Actions-hardening commit.
- scripts/commit_ci2.sh               the pin-audit + deny.toml commit.

All read the GitHub token from ~/.hermes/.env at runtime; none embeds a
credential. Every script states the claim it is checking and prints the raw
evidence, so a reader can tell a passing check from an assumption.
EOF

git add scripts/
git commit -F "$MSG/12.txt"

echo "=== LOG ==="
git log --oneline -12

echo "=== PUSH ==="
TOKEN=$(grep -o 'ghp_[A-Za-z0-9]\{20,\}' ~/.hermes/.env | head -1)
[ -n "$TOKEN" ] || { echo "token not found"; exit 1; }
git push "https://michaelamoetoevsky:${TOKEN}@github.com/michaelamoetoevsky/PDN.git" master

echo "=== VERIFY ==="
LOCAL=$(git rev-parse HEAD)
REMOTE=$(git ls-remote "https://michaelamoetoevsky:${TOKEN}@github.com/michaelamoetoevsky/PDN.git" master | cut -f1)
echo "local : $LOCAL"
echo "remote: $REMOTE"
[ "$LOCAL" = "$REMOTE" ] && echo "PUSH CONFIRMED" || echo "MISMATCH"
