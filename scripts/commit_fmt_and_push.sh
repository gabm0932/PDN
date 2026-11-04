#!/bin/sh
# Commit formatting fixes and push. POSIX sh.
set -e
cd "$(dirname "$0")/.."

MSG="$HOME/.hermes/cache/scratch/commitmsg"
mkdir -p "$MSG"
cat > "$MSG/8.txt" <<'EOF'
style: apply rustfmt to the files touched by the bug fixes

`cargo fmt --all -- --check` failed in CI (run 41760973567, step "Run cargo
fmt check" -> exit 1), which aborted the job before the test suite could run. The
diffs were layout-only:

- crates/podnext/src/main.rs (the direct-execution pre-scan added in b73b66f)
- crates/pn-next/src/backup_restore/mod.rs (the regression test in cc47612)
- crates/proot-distro-core/src/container.rs, locking.rs (0e01e3a, daaac87)
- crates/proot-distro-core/tests/deep_fs_recursion.rs (91ed36b)

Formatting was applied per-file rather than with `cargo fmt --all` so that
in-flight edits in other files were not rewritten underneath their authors.

`cargo fmt --all -- --check` is now clean across the workspace.
EOF

git add crates/podnext/src/main.rs \
        crates/pn-next/src/backup_restore/mod.rs \
        crates/proot-distro-core/src/container.rs \
        crates/proot-distro-core/src/locking.rs \
        crates/proot-distro-core/tests/deep_fs_recursion.rs
git commit -F "$MSG/8.txt"

echo "=== whitespace check ==="
git diff --check && echo "clean"

echo "=== LOG ==="
git log --oneline -6

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
