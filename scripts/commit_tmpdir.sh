#!/bin/sh
# Commit the TMPDIR fallback removal (agent E). POSIX sh.
set -e
cd "$(dirname "$0")/.."

MSG="$HOME/.hermes/cache/scratch/commitmsg"
mkdir -p "$MSG"
cat > "$MSG/10.txt" <<'EOF'
fix(temp): resolve temp paths through one helper instead of falling back to /tmp

Android gives an app uid no writable `/tmp`: the literal directory exists but
`touch /tmp/x` returns `Permission denied`, and `$PREFIX/tmp`
(`/data/data/com.termux/files/usr/tmp`) is the only writable temp root.
`std::env::temp_dir()` is no safer — it resolves to `/tmp` when `TMPDIR` is
unset. Several production paths hard-coded that fallback, so they worked while
`TMPDIR` happened to be set and broke the moment anything cleared the
environment (a sanitised child, `env -i`, a test harness).

Add `proot_distro_core::paths::temp_base()` (`$TMPDIR` -> `$PREFIX/tmp` ->
`std::env::temp_dir()`) and route every site through it:

- crates/proot-distro-core/src/{paths,image,shm,network,storage}.rs
- crates/pn-next/src/gitops/mod.rs
- crates/podnext/src/commands/mod.rs
- tests/daemon_client_e2e.rs
- install.sh — uses `${TMPDIR:-/tmp}` because it also runs on Linux hosts, where
  `/tmp` is the correct fallback; only the Termux default changes.

The resolution order is split into `resolve_temp_base(tmpdir, prefix)` so it is
testable without mutating process-global environment, and covered by two tests:

- `tmpdir_takes_priority` — `$TMPDIR` wins when set.
- `prefix_tmp_used_when_tmpdir_unset_and_is_not_tmp` — with `$TMPDIR` unset the
  result is `$PREFIX/tmp`, asserted `!= /tmp`.

(`std::env::temp_dir()` remains the final fallback for non-Termux platforms, and
is deliberately not covered by a unit test because asserting on it would require
mutating the process environment.)

This is why the failure mode was intermittent rather than obvious: CI runners
have `/tmp`, so a hard-coded `/tmp` passes CI and fails on-device. Agent-run
tests hit it twice during this work (`cannot create /tmp/...: Permission denied`
inside otherwise-passing commands).

Motivated by an explicit instruction to stop this recurring; the durable skill
`termux-tmpdir-hygiene` records the rule, the resolution order, and an audit
checklist.
EOF

git add crates/proot-distro-core/src/paths.rs crates/proot-distro-core/src/image.rs \
        crates/proot-distro-core/src/shm.rs crates/proot-distro-core/src/network.rs \
        crates/proot-distro-core/src/storage.rs crates/proot-distro-core/src/lib.rs \
        crates/pn-next/src/gitops/mod.rs crates/podnext/src/commands/mod.rs \
        tests/daemon_client_e2e.rs install.sh
git commit -F "$MSG/10.txt"
echo "=== LOG ==="
git log --oneline -4
echo "=== remaining uncommitted ==="
git status --short
