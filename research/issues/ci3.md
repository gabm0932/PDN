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
