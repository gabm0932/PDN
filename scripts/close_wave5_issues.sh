#!/bin/sh
# - evidence comments on #16/#17/#18 (state plainly what is and is not fixed)
# - follow-up issue for the `build local` image-assembly gap
# - #15: post the verdict, add the documentation note the verdict asks for, close as not planned
set -e
cd "$(dirname "$0")/.."
TOKEN=$(grep -o 'ghp_[A-Za-z0-9]\{20,\}' ~/.hermes/.env | head -1)
[ -n "$TOKEN" ] || { echo "token not found"; exit 1; }
gh() { command env -u LD_LIBRARY_PATH gh "$@"; }

# ---------- #16 ----------
cat > "$HOME/.hermes/cache/scratch/c16.md" <<'EOF'
Fixed in `9fcf8da`.

Cause was hypothesis (a): a per-entry O(depth) ancestor re-walk through `fs::create_dir_all`.
`extract_entries` called it on every entry's parent, and again on the target for every Directory
entry, with no memory of what already existed. On an existing path `create_dir_all` does `mkdir()`
(EEXIST) then `is_dir()` (a stat), and each resolves every component -- so n entries under a d-deep
tree paid n*d path resolutions. The instrumented counter makes it exact: ancestor-component work at
d=100/200/400 was 11909/43809/167609, i.e. Theta(d^2) for a chain. Hypotheses (b) per-component
`metadata` loops and (c) an O(n) scan of prior entries were both ruled out by reading the file --
there were no such loops and no cross-entry state at all.

Fix: a `HashSet<PathBuf>` of known-existing directories; each is created exactly once.

```
depth=200 files=2000    before 395.19ms  dir_calls=2400
                        after  352.89ms  dir_calls=201     (11.9x fewer)
entry-heavy profile (depth 200, 5000 files): dir term 689ms -> 316ms (43% -> 28% of run)
```

`dir_calls` is deterministic; the wall clock on this device is noisy at +/-30%, which is why the
test asserts a **work count** (`calls <= DEPTH + 2`) rather than a duration -- it cannot flake.
Verified to fail against the un-fixed code: "700 create_dir_all calls for 400 files under 150
levels"; 150 with the fix.

Semantics unchanged: hard-link-then-copy (SELinux), symlink recreation, special-file skip, mode
after content, and both security checks are byte-identical. All existing tests green.

**Honest limit, recorded rather than glossed.** The reported "150 levels -> 50s" is not
reproducible on this hardware: a plain 400-level chain is ~330ms here, so those absolute
magnitudes came from a different shape or environment. What IS reproducible is the quadratic
directory work above, and it dominates on entry-heavy deep archives -- the shape a real 200+ level
layer has. The residual per-entry cost (`File::create`/`io::copy`/`chmod`/`is_inside`) is intrinsic
to a path-based API; removing it needs openat-relative operations, whose natural home is
`crates/proot-distro-core/src/dirfd.rs`, not this file. That is a rewrite, not a bug fix, so it is
not attempted here.
EOF
gh issue comment 16 --body-file "$HOME/.hermes/cache/scratch/c16.md"

# ---------- #17 ----------
cat > "$HOME/.hermes/cache/scratch/c17.md" <<'EOF'
Two of the three complaints are fixed in `2f93f2c`; the third is **not**, and this comment exists
so that closing this issue does not read as "all three done".

**1. ARG was rejected -- fixed.** `parse_dockerfile` bails with "Unsupported Dockerfile
instruction: 'ARG'" (`crates/proot-distro-core/src/dockerfile.rs:210` has no `Arg` variant) and
`cmd_build_local` propagated it with `?`, so the build aborted before running anything. ARG is now
consumed before the parser sees it: declared, and `$NAME`/`${NAME}` substituted into the following
instructions.

**2. RUN was parsed, printed, and silently NOT executed -- fixed.** This was the worst of the
three: with a FROM/RUN/COPY Dockerfile the build printed `Run("echo ... > marker")` and
`Successfully built image 'test:latest'`, exited 0, and created no file. A build that reports
success while doing nothing is worse than one that fails. RUN now executes via `sh -c` with the
build context as cwd, streams output, and fails the build on a non-zero exit.

```
before: Successfully built image 'test:latest'   -> no marker file
after:  Running: echo "v=1.0 g=hello" > run1.txt -> $ cat run1.txt
                                                    v=1.0 g=hello
                                                    second
```

**3. No usable image is produced -- NOT fixed.** `cmd_build_local` still computes a tar.gz layer
diff of the raw context and deletes it; there is no base-rootfs pull, no overlay/chroot assembly,
no layer caching. This is a feature, not the bug, so it is deliberately not built here. Tracked
separately: see the follow-up issue referenced below.

Also not done, stated rather than implied: ARG at the parser level belongs in
`crates/proot-distro-core/src/dockerfile.rs` (an `Arg` variant + an `"ARG"` arm + substitution in
`parse_dockerfile`); the workaround in `commands/mod.rs` keeps `build local` correct without
editing another crate. ENV/WORKDIR are still only parsed and printed, because there is no assembled
rootfs to apply them to. ARG line continuations are not handled.

Four tests added, asserting parser/exec-plan behaviour so they need no root:
`arg_is_accepted_and_substituted` (fails before the fix), `arg_without_default_is_declared_empty`,
`run_instructions_execute_in_order_in_context` (real `sh -c` in a tempdir), `failing_run_propagates`.

Workspace suite after the change: `161 passed; 0 failed`.
EOF
gh issue comment 17 --body-file "$HOME/.hermes/cache/scratch/c17.md"

# ---------- #18 ----------
cat > "$HOME/.hermes/cache/scratch/c18.md" <<'EOF'
Fixed in `4ef5670`.

`podnext copy`/`sync` used `std::fs::copy`, whose `copy_file_range` fast path is unavailable on
Android/aarch64, so it degenerated to a buffered loop that transfers every logical byte and never
asks where the holes are. Measured on a 1 GiB sparse source holding 16 KiB of real data:

```
source          1073741824 bytes,     32 blocks   (du -B1: 16384)
before (copy)   1073741824 bytes, 2099216 blocks   (du -B1: 1074798592)   densified
after  (copy)   1073741824 bytes,     32 blocks    (du -B1: 16384)        preserved
```

`copy_preserving_holes` walks the source with `lseek(SEEK_DATA)`/`lseek(SEEK_HOLE)`, copies only
allocated regions, seeks the output to each data offset, and fixes the length with `set_len`. All
three call sites routed through it.

Correctness guarded rather than assumed -- non-sparse content is byte-identical (`cmp` equal, equal
`sha256`, blocks 10248 before/after); a longer destination is shrunk, a shorter one extended; an
empty file stays 0/0; permissions are carried over; a partial destination is removed on error like
`std::fs::copy` does; and an unsupported `SEEK_DATA` (EINVAL/ENOTSUP) falls back to the old dense
path instead of failing. `SEEK_DATA` support on this kernel was verified directly, not assumed.

Test `tests::copy_keeps_sparse_files_sparse` asserts apparent size, byte-identity and allocated
blocks under 1 MiB, with a skip-guard if the test filesystem lacks hole support.
EOF
gh issue comment 18 --body-file "$HOME/.hermes/cache/scratch/c18.md"

# ---------- follow-up for #17 remainder ----------
cat > research/issues/build_local_image_gap.md <<'EOF'
## What is missing

`podnext build local` does not produce an image. `cmd_build_local`
(`crates/podnext/src/commands/mod.rs`) parses the Dockerfile, optionally executes `RUN`
(added in `2f93f2c` for #17), then computes a **layer diff of the raw context directory** as a
tar.gz, prints its digest, and deletes the tarball. Nothing is registered as an image.

So `podnext build local -t test:latest .` reports `Successfully built image 'test:latest'` while
no image exists to run.

## What would make it real

Not a small change, hence a separate issue rather than a line in #17:

1. **Pull the base image** named by `FROM` (or resolve it locally) so there is a rootfs to build on.
2. **Assemble a real rootfs**: apply the context and each instruction's filesystem effect on top of
   that base, rather than tarring the context in isolation.
3. **Produce something runnable**: register the result so `podnext run <tag>` works, and store the
   layer/rootfs where the container machinery expects it.
4. **Stop claiming success when it is not one.** Until steps 1-3 exist, the command should say what
   it actually did (produced a digest of a layer diff) rather than "Successfully built image".

`ENV` and `WORKDIR` are parsed but not applied for the same reason: there is no assembled rootfs to
apply them to. They belong in this work.

## Related

- `ARG` at the parser level: `crates/proot-distro-core/src/dockerfile.rs` still has no `Arg`
  variant (`DockerfileInstruction` + `parse_instruction`). `commands/mod.rs` works around it; the
  clean home is the parser.

Labels: enhancement
EOF
BUILD_URL=$(gh issue create --title "podnext build local computes a layer diff but produces no image" --body-file research/issues/build_local_image_gap.md --label "enhancement" 2>&1 | tail -1)
echo "build-local follow-up: $BUILD_URL"
BUILD_NUM=$(printf '%s' "$BUILD_URL" | grep -oE '[0-9]+$')
gh issue comment 17 --body "Follow-up filed for the third complaint (no image produced): $BUILD_URL"

# ---------- #15 ----------
cat > research/issues/concurrency_verdict.md <<'EOF'
## Verdict: not a PDN defect

Concurrent unsynchronised writers to one file are last-writer-wins by definition -- each `>` open
truncates with `O_TRUNC`, so one writer's content survives and the rest is lost, with no ordering
guarantee. That is POSIX, and it is what this issue measured.

## The control that settles it

The same worker shape run with **zero PDN involved** -- plain host processes, no `podnext`, no
proot, no container anywhere in the path -- reproduces the identical signature in **5 of 5 runs**:

```
--- control run 1 (final='token_4', lines=1) ---
Worker 1: expected token_1, read 'token_4', clobbered=True
Worker 2: expected token_2, read 'token_4', clobbered=True
Worker 3: expected token_3, read 'token_4', clobbered=True
Worker 4: expected token_4, read 'token_4', clobbered=False
Worker 5: expected token_5, read 'token_4', clobbered=True
Total clobbered: 4/5
... runs 2-5 identical: 4/5 clobbered, file ends with exactly one line
```

Same shape through `podnext run` on one container, 5 runs: **also 4/5 clobbered every time**. No
worse, no different.

## No PDN-level amplification

Checked for a staging file, a lock, a copy-up, or an `O_TRUNC` performed on the guest's behalf:

1. **The guest file IS the host file.** After the run, `/shared_race.txt` is a plain file in the
   container rootfs directory (`~/.podnext/containers/<name>/rootfs/shared_race.txt`) -- an ordinary
   directory, not an overlay.
2. **The run path takes no lock.** `run_container_native` -> `ProotBackend::execute()` takes no
   `ContainerLock`; the only `flock` in the path is `register_session`, which locks a per-pid
   session JSON, unrelated to guest files.
3. **`ContainerLock` is dead code** -- referenced only from its own unit tests, no production
   caller. So the issue's premise that container-level locking exists is false, but its absence
   changes nothing here.
4. **No staging or copy-up on write.** The engine execs system proot, and the guest's
   `write()`/`open(O_TRUNC)` reach the host kernel unchanged through the translated fd.

## Consequence for the docs

`docs/src/troubleshooting.md` now states the contract: concurrent `podnext run`/`login` against one
container share a single mutable filesystem with no ordering or atomicity guarantee, and PDN
performs no write serialisation, staging, or copy-up. The control experiment is cited so this
cannot be filed as a container bug again.

Explicitly NOT doing: adding a per-container execution lock. A lock on the container is not a lock
on the file the workload uses, and serialising unrelated workloads to "fix" a data race a lock
cannot fix would be worse than documenting the semantics.

Closing as not planned, with the cleanup noted below.

## Minor cleanup left open

`crates/proot-distro-core/src/locking.rs` `ContainerLock` has no production caller. Either delete it
or comment that it guards nothing in the run path -- its mere existence is what allowed the
assumption "locking exists so we are safe".
EOF

cat >> docs/src/troubleshooting.md <<'EOF'

## Concurrent writes to the same file

If two or more `podnext run` or `podnext login` invocations write to the **same file** at the same
time, one write wins and the others are lost. This is POSIX last-writer-wins: an `>` open truncates
the file, and there is no ordering guarantee between unsynchronised processes.

PDN does not change this, and does not add anything on the write path: no serialisation, no staging
file, no copy-up. The guest filesystem is a plain host directory, so the guest's `write()` reaches
the host kernel directly. Concurrent runs against one container share a single mutable filesystem
with no atomicity guarantee, and any synchronisation is the workload's own responsibility.

Measured, so it is not speculation: the same five-worker experiment run (a) with plain host
processes, no PDN anywhere, and (b) through `podnext run`, produced the identical result -- 4 of 5
writes clobbered, file left with exactly one line -- in every run of both. If you need
read-modify-write across concurrent invocations, use a file lock (`flock`) in your command, or give
each writer its own path and merge afterwards.
EOF

git add docs/src/troubleshooting.md research/issues/concurrency_verdict.md research/issues/build_local_image_gap.md
git commit -q -m "docs: state the concurrent-write contract, and record the #15 verdict

#15 asked whether concurrent guest writes clobbering a shared file is a PDN defect.
It is not: plain host processes with no PDN anywhere reproduce the identical
4/5-clobbered-every-time signature, and there is no staging, locking or copy-up on
the write path. Documented in troubleshooting.md with the control experiment cited,
so it cannot be filed as a container bug again.

Deliberately NOT adding a per-container execution lock: a lock on the container is
not a lock on the file the workload uses, and serialising unrelated workloads would
not make the unsynchronised writes correct.

Refs #15
"
git push -q "https://michaelamoetoevsky:${TOKEN}@github.com/michaelamoetoevsky/PDN.git" master 2>&1 | tail -1

gh issue comment 15 --body-file research/issues/concurrency_verdict.md
gh issue close 15 --reason "not planned"

echo "=== sisa issue terbuka (milik saya) ==="
gh issue list --state open --limit 40 --json number,title 2>/dev/null | env -u LD_LIBRARY_PATH python3 -c "
import json,sys
d=[i for i in json.load(sys.stdin)]
for i in sorted(d,key=lambda x:x['number']):
    if i['number']>=11: print(f\"  #{i['number']:3} {i['title'][:66]}\")
print('TOTAL OPEN:',len(d))
"
echo "BUILD_NUM=$BUILD_NUM"
