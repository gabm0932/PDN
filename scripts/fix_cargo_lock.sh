#!/bin/sh
# Track Cargo.lock and fix the fragile aggregate step. POSIX sh.
set -e
cd "$(dirname "$0")/.."

TOKEN=$(grep -o 'ghp_[A-Za-z0-9]\{20,\}' ~/.hermes/.env | head -1)
[ -n "$TOKEN" ] || { echo "token not found"; exit 1; }

cat > research/issues/ci3.md <<'EOF'
## Summary

`Cargo.lock` is listed in `.gitignore`, so it is **not in the repository** — this is an
application workspace (`podnext`, `podnext-daemon`, `pn-gateway` are binaries), where the
lockfile must be committed for reproducible builds. Two consequences:

1. Every CI run resolved dependencies from scratch, so a green run could not be reproduced
   from its commit.
2. Adding `--locked` (the fix recommended in `research/CI_CD_AUDIT.md` D7) made **every**
   build fail on a clean checkout.

## Reproduction / evidence

```
$ git ls-files Cargo.lock
$                       # nothing — the file is not tracked

$ git check-ignore -v Cargo.lock
.gitignore:3:Cargo.lock	Cargo.lock

$ ls -la Cargo.lock
-rw-------. 1 u0_a396 u0_a396 137166 Nov  4 06:33 Cargo.lock    # exists locally only
```

CI, run 41765095295 (`Android Cross-Compile Matrix`, all four matrix cells, step
`Build workspace for <target>`):

```
Run cargo build --workspace --release --locked --target aarch64-linux-android
error: cannot create the lock file /home/runner/work/PDN/PDN/Cargo.lock because --locked was passed to prevent this
help: to generate the lock file without accessing the network, remove the --locked flag and use --offline instead.
##[error]Process completed with exit code 101.
```

The `Aggregate Artifacts & Checksums` job then failed only because there were no artifacts —
a cascade, not an independent cause.

## Root cause

`.gitignore` line 3 (`Cargo.lock`) is the stock `cargo new --lib` template. PDN is not a
library-only workspace, so the entry is wrong here. Note the audit's D7 *recommendation* was
correct while its *evidence* was not: it recorded "`Cargo.lock` is committed (`ls -la
Cargo.lock` -> 137166 bytes)", which only proves the file exists on a developer machine, not
that git tracks it. The cross-compile run is what surfaced the difference.

## Impact

- Non-reproducible CI: dependency versions were whatever the registry resolved that day.
- `--locked` — the guard against exactly that — could not be used anywhere.
- Every fresh clone resolves the full graph on first build.

## Fix

- Remove `Cargo.lock` from `.gitignore` (with a comment explaining why it is deliberate) and
  commit the lockfile.
- Keep `--locked` on every cargo invocation, which now means something.

Also fixed while here: the aggregate step's checksum command
`find all-artifacts -type f -name "*.so" -o -name "podnext"` — `-o` binds looser than the
implicit `-a`, so it read `(-type f -name "*.so") OR (-name podnext)` and could hand a
*directory* named `podnext` to `sha256sum`. Grouped the predicates, added `-print0 | xargs -0`,
`mkdir -p all-artifacts` so the redirect cannot fail, and an explicit warning instead of a
bare `cat` when there is nothing to checksum.

Labels: bug, ci
EOF

URL=$(env -u LD_LIBRARY_PATH gh issue create \
  --title "[CI-3] Cargo.lock is gitignored in an application workspace, so --locked was impossible" \
  --body-file research/issues/ci3.md --label "bug,ci")
echo "issue: $URL"

MSG="$HOME/.hermes/cache/scratch/commitmsg"
mkdir -p "$MSG"
cat > "$MSG/13.txt" <<'EOF'
fix(ci): track Cargo.lock so `--locked` can actually hold

`Cargo.lock` was in `.gitignore` (the stock `cargo new --lib` entry), so the repo
contained no lockfile at all — `git ls-files Cargo.lock` returns nothing. PDN builds
applications (`podnext`, `podnext-daemon`, `pn-gateway`), where the lock belongs in
version control.

Two consequences, in order of discovery:

1. Every CI run resolved dependencies from scratch; a passing run could not be
   reproduced from its commit.
2. Adding `--locked` (research/CI_CD_AUDIT.md D7) broke **every** build on a clean
   checkout. CI run 41765095295, all four cross-compile matrix cells:

       error: cannot create the lock file /home/runner/work/PDN/PDN/Cargo.lock
              because --locked was passed to prevent this
       ##[error]Process completed with exit code 101.

   The `Aggregate Artifacts & Checksums` job failed only as a cascade (no artifacts
   to collect).

Worth recording: the audit's D7 recommendation was right but its evidence was not — it
read `ls -la Cargo.lock` (which shows the file on a developer machine) as proof the
file was committed. Only the cross-compile run distinguished the two.

Changes:
- `.gitignore`: drop `Cargo.lock`, with a comment stating that tracking it is
  deliberate.
- commit `Cargo.lock` (137166 bytes).
- `cross-compile.yml` aggregate step: group the `find` predicates (`-o` binds looser
  than the implicit `-a`, so `-type f -name "*.so" -o -name podnext` could match a
  *directory* named `podnext` and pass it to `sha256sum`), use `-print0 | xargs -0`,
  `mkdir -p all-artifacts` so the redirect cannot fail, and emit an explicit warning
  rather than a bare `cat` when there is nothing to checksum.

Refs #23
EOF

git add .gitignore Cargo.lock .github/workflows/cross-compile.yml research/issues/ci3.md
git commit -F "$MSG/13.txt"

echo "=== confirm the lockfile is now tracked ==="
git ls-files Cargo.lock

echo "=== PUSH ==="
git push "https://michaelamoetoevsky:${TOKEN}@github.com/michaelamoetoevsky/PDN.git" master
LOCAL=$(git rev-parse HEAD)
REMOTE=$(git ls-remote "https://michaelamoetoevsky:${TOKEN}@github.com/michaelamoetoevsky/PDN.git" master | cut -f1)
echo "local : $LOCAL"
echo "remote: $REMOTE"
[ "$LOCAL" = "$REMOTE" ] && echo "PUSH CONFIRMED" || echo "MISMATCH"
