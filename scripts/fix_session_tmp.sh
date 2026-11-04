#!/bin/sh
# Fix the last production /tmp fallback (session.rs); push. POSIX sh.
set -e
cd "$(dirname "$0")/.."

TOKEN=$(grep -o 'ghp_[A-Za-z0-9]\{20,\}' ~/.hermes/.env | head -1)
[ -n "$TOKEN" ] || { echo "token not found"; exit 1; }

MSG="$HOME/.hermes/cache/scratch/commitmsg"
mkdir -p "$MSG"
cat > "$MSG/17.txt" <<'EOF'
fix(session): resolve the sessions directory without a /tmp fallback

`session::sessions_dir` fell back to `/tmp` when neither `RUNTIME_DIR` nor `HOME`
was resolvable, then joined `.podnext/sessions`:

    .unwrap_or_else(|| PathBuf::from("/tmp"));

On Android `/tmp/.podnext/sessions` is not writable, and `register_session` is
called best-effort (`.ok()`) from the execution path, so the failure was **silent**:
no error anywhere, `~/.podnext/sessions/` stays empty, and `podnext ps` renders an
empty table. The same class of bug that was removed from every other production site
in b97f3c1; this one was left behind deliberately because `session.rs` belonged to
another agent at the time, and that agent finished without touching it.

Route the fallback through `crate::paths::temp_base()` (`$TMPDIR` -> `$PREFIX/tmp`
-> `std::env::temp_dir()`), which is what the rest of the workspace now uses.

Verified:
- `cargo test -p proot-distro-core --no-fail-fast` -> 35 passed, 0 failed (lib) and
  3 passed, 0 failed (deep_fs_recursion).
- `cargo fmt --all -- --check` -> clean.
- No production `/tmp` fallback remains in the workspace. Every remaining literal
  is justified: the guest-internal `/tmp` bind mount and stub in `proot-compat` and
  `pn-gateway::config`, the `no_exec` deny-list in `security.rs`, the Termux
  constant in `constants.rs`, the uml4termux socket/log paths under `$PREFIX/tmp`
  in `runtime.rs`, and test path values in `untar.rs` / `paths.rs`.

Refs #14 (the symptom this shares: `podnext ps` reporting nothing)
EOF

git add crates/proot-distro-core/src/session.rs
git commit -F "$MSG/17.txt"

echo "=== PUSH ==="
git push "https://michaelamoetoevsky:${TOKEN}@github.com/michaelamoetoevsky/PDN.git" master
LOCAL=$(git rev-parse HEAD)
REMOTE=$(git ls-remote "https://michaelamoetoevsky:${TOKEN}@github.com/michaelamoetoevsky/PDN.git" master | cut -f1)
echo "local : $LOCAL"
echo "remote: $REMOTE"
[ "$LOCAL" = "$REMOTE" ] && echo "PUSH CONFIRMED" || echo "MISMATCH"
