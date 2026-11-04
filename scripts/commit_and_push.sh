#!/bin/sh
# Commit research/audit artifacts and push all local commits. POSIX sh.
set -e
cd "$(dirname "$0")/.."

echo "=== strict credential scan (must print nothing) ==="
grep -rEon 'ghp_[A-Za-z0-9]{20,}' scripts/ research/ 2>/dev/null || echo "clean: no literal token"

MSG="$HOME/.hermes/cache/scratch/commitmsg"
mkdir -p "$MSG"
cat > "$MSG/5.txt" <<'EOF'
chore(research): add the stress-test evidence, CI/CD audit and issue tooling

Adds the artifacts produced by the multi-agent stress and security testing of
podnext 0.1.7, so the findings are reproducible from the repository rather than
living only in a transcript:

- research/CI_CD_AUDIT.md            workflow-by-workflow audit of all six
                                     GitHub Actions workflows: run history with
                                     real run ids, 12 defects each with file:line
                                     and the verbatim log line that proves it,
                                     and a prioritised fix list.
- research/FILESYSTEM_FAULT_INJECTION_REPORT.md
                                     corruption / degraded-rootfs behaviour.
- research/issues/                   the exact bodies filed as issues #11-#21.
- scripts/dice_monkey_test.py        stochastic 2d6 command-roll fuzzer.
- scripts/casino_chaos_runner.py     concurrent lock/rename/copy race harness.
- scripts/create_bug_issues.sh       files the issue bodies via gh.
- crates/proot-distro-core/tests/deep_fs_recursion.rs
                                     deep-tree extraction scaling test.

No credentials are stored here: the scripts read the token from ~/.hermes/.env
at runtime.
EOF

git add research/CI_CD_AUDIT.md research/FILESYSTEM_FAULT_INJECTION_REPORT.md \
        research/issues scripts crates/proot-distro-core/tests
git commit -F "$MSG/5.txt"

echo "=== LOG ==="
git log --oneline -6

echo "=== PUSH ==="
TOKEN=$(grep -o 'ghp_[A-Za-z0-9]\{20,\}' ~/.hermes/.env | head -1)
[ -n "$TOKEN" ] || { echo "token not found"; exit 1; }
git push "https://michaelamoetoevsky:${TOKEN}@github.com/michaelamoetoevsky/PDN.git" master

echo "=== VERIFY PUSH LANDED (local HEAD vs remote master) ==="
LOCAL=$(git rev-parse HEAD)
REMOTE=$(git ls-remote "https://michaelamoetoevsky:${TOKEN}@github.com/michaelamoetoevsky/PDN.git" master | cut -f1)
echo "local : $LOCAL"
echo "remote: $REMOTE"
[ "$LOCAL" = "$REMOTE" ] && echo "PUSH CONFIRMED" || echo "MISMATCH — push did not land"
