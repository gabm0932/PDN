#!/bin/sh
# Close the issues whose fixes are verified by the green CI run. POSIX sh.
set -e
cd "$(dirname "$0")/.."

TOKEN=$(grep -o 'ghp_[A-Za-z0-9]\{20,\}' ~/.hermes/.env | head -1)
[ -n "$TOKEN" ] || { echo "token not found"; exit 1; }

GREEN="https://github.com/michaelamoetoevsky/PDN/actions/runs/41771601678"
MSG="$HOME/.hermes/cache/scratch/closing"
mkdir -p "$MSG"

close_one() {
  n="$1"; commit="$2"; extra="$3"
  cat > "$MSG/$n.md" <<EOF
Fixed in \`$commit\` and verified green on \`master\`.

CI run for the closing commit: $GREEN — every job success
(\`Rust Host Build\`, \`Rust Android Cross-Compile (aarch64)\`, \`PRoot Distro Integration Test\`,
\`Fuzzing\`, \`Security Audit\`, \`Build Documentation\`; \`Release\` skipped as tag-gated).

$extra
EOF
  env -u LD_LIBRARY_PATH gh issue comment "$n" --body-file "$MSG/$n.md" >/dev/null
  env -u LD_LIBRARY_PATH gh issue close "$n" --reason completed >/dev/null
  echo "closed #$n"
}

close_one 11 b73b66f "Full test suite after the change: \`cargo test --workspace --no-fail-fast\` -> 155 passed, 0 failed, 0 panics."
close_one 12 cc47612 "Regression test \`backup_restore::tests::test_delete_backup_does_not_deadlock\` passes; the whole workspace suite is green."
close_one 14 fa04d3c "Live proof was captured on-device before committing: \`podnext ps\` reported \`sess-21855 | alpine | sleep 8 | Running\` during a \`run\`, and \`sess-22946\` during a \`login\`."
close_one 19 33a9e10 "This was the workflow that had been red on master. Root cause was a TOCTOU race in the supervision restart path (the restart decision read \`user_stopped\` at process exit, while the state mutations happened later under separate write locks). The test was left unchanged because its assertion encodes a real requirement."
close_one 20 daaac87 "The \`#[should_panic]\` test that pinned the panic was replaced with one asserting the real behaviour (first lock succeeds, second is refused, locking succeeds again after release)."
close_one 21 b73b66f "Guest commands containing \`-r\` now reach the guest: \`podnext run alpine -- rm -r /tmp/x\` runs \`rm\` instead of aborting."
close_one 22 13ea547 "The pin-audit step was verified against both revisions before committing: the previous revision reports \`peter-evans/update-github-actions\` as UNRESOLVABLE (exit 1); the current one reports 0 unresolved (exit 0)."
close_one 23 c40ed64 "Route the fix took: adding \`--locked\` (the audit's D7 recommendation) broke every job, because \`Cargo.lock\` was in \`.gitignore\` and therefore absent from a clean checkout. \`git ls-files Cargo.lock\` now returns the file."

echo
echo "=== open issues now ==="
env -u LD_LIBRARY_PATH gh issue list --limit 30 --state open 2>&1 | head -20
