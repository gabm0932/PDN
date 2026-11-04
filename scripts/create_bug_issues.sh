#!/bin/sh
# Create PDN bug issues from the stress/security audit. POSIX sh only.
set -e
cd "$(dirname "$0")/.."

TOKEN=$(grep -o 'ghp_[A-Za-z0-9]\{20,\}' ~/.hermes/.env | head -1)
[ -n "$TOKEN" ] || { echo "token not found in ~/.hermes/.env"; exit 1; }

mkdir -p research/issues

# ---------- C1 ----------
cat > research/issues/c1.md <<'EOF'
## Summary

`podnext install <image> --name ""` accepts an **empty container name** and creates a
container whose rootfs lands at `~/.podnext/containers/rootfs` (a sibling-less directory,
no name segment). The container is listed with a blank NAME column and **cannot be removed
through the CLI**, leaving an unremovable orphan that also blocks installing a container
whose directory happens to collide.

## Reproduction

```sh
podnext install debian:13 --name ""
# Installing 'debian:13' as ''...
# [1/4] Authenticating with Docker Hub registry...
# [4/4] Configuring guest environment, DNS, and /dev/shm...
# Container '' installed successfully!
# Rootfs: /data/data/com.termux/files/home/.podnext/containers/rootfs

podnext list
# | NAME | DISTRO | VERSION | ARCH | STATUS | CREATED |
# |      | debian | 13      | aarch64 | Stopped | ... |

podnext remove -f ""
# Error: Container '' not found
```

## Evidence (observed log)

```
07:52:22 tool | -> terminal(podnext install debian:13 --name "")
07:53:26 result | terminal ok 64.2s: {"output": "Installing 'debian:13' as ''...
  Container '' installed successfully!
  Rootfs: /data/data/com.termux/files/home/.podnext/containers/rootfs"}
07:53:51 tool | -> terminal(podnext remove -f "")
07:53:53 result | terminal ERROR 3.0s: {"output": "Error: Container '' not found", "exit_code": 1}
```

## Root cause

`crates/podnext/src/main.rs:1851`

```rust
async fn cmd_install(image: String, name: Option<String>, force: bool) -> Result<()> {
    let container_name = name
        .clone()
        .unwrap_or_else(|| image.split(':').next().unwrap_or(&image).to_string());
    let _rootfs: PathBuf = install_image_native(&image, &container_name, force, None).await?;
```

No guard rejects an explicitly-supplied empty `--name`. Downstream, the container
directory is derived by joining the name onto the containers root; an empty segment
collapses to the containers root itself, so `rootfs/` is created one level too high.
Lookup paths then use a non-empty name and never match.

## Impact

- Orphan container consuming disk, invisible to normal lifecycle commands.
- Blocks a later `install` at the same path with "already exists".

## Proposed fix

Validate the container name before any download: reject empty/whitespace-only values and
enforce a safe charset (`[A-Za-z0-9._-]+`, no path separators), then bail with a clear
message. Add a regression test that `install --name ""` exits non-zero and creates nothing.

Labels: bug, cli
EOF

# ---------- C2 ----------
cat > research/issues/c2.md <<'EOF'
## Summary

`backup delete` **hangs forever**. A `tokio::sync::RwLock` read guard is held in scope
across a `write()` acquisition on the same lock. Tokio's `RwLock` is not reentrant and not
upgradeable, so the write request waits on a read guard that only the caller can release —
a permanent deadlock.

## Reproduction

```sh
podnext backup create my-container      # succeeds
podnext backup list                     # shows <id>
podnext backup delete <id>              # hangs, no output
# eventually: [Command timed out after 180s]  exit=124
```

## Evidence (observed log)

```
task-3 (power-user validation) reported:
  Critical Bug (Deadlock in `backup delete`):
  `crates/pn-next/src/backup_restore/mod.rs:1114-1135`:
  `let backups = self.backups.read().await;` holds read lock in scope while calling
  `self.backups.write().await.remove(backup_id);`, deadlocking indefinitely
  (exit 124 timeout).
```

## Root cause

`crates/pn-next/src/backup_restore/mod.rs:1113`

```rust
pub async fn delete_backup(&self, backup_id: &str) -> Result<()> {
    let backups = self.backups.read().await;      // read guard lives to end of fn
    if let Some(backup) = backups.get(backup_id) {
        // ... fs::remove_file(archive) / sidecar / staging dir ...
    }
    self.backups.write().await.remove(backup_id); // <-- blocks on our own read guard
    Ok(())
}
```

## Impact

- `backup delete` is completely unusable; archives accumulate with no CLI route to prune.
- Any caller awaiting it (scheduler, daemon RPC) blocks a task slot indefinitely.

## Proposed fix

Take a snapshot of the needed metadata, **drop the read guard**, then acquire the write
lock for the removal:

```rust
let existed = self.backups.read().await.get(backup_id).cloned();
if let Some(backup) = existed { /* remove files */ }
self.backups.write().await.remove(backup_id);
```

Add a test that `delete_backup` completes within a bounded timeout.

Labels: bug
EOF

# ---------- C3 ----------
cat > research/issues/c3.md <<'EOF'
## Summary

`podnext reset <name>` **deletes the container directory first and downloads the base
image afterwards**. If the image reference cannot be fetched, the download fails *after*
the existing container has already been destroyed — permanent, unrecoverable data loss.

This is reachable through the normal lifecycle: `backup restore` records the **source
container name** in `metadata.json["image"]`, which is not a registry reference. A later
`reset` of that restored container then tries to pull a local container name from Docker
Hub, gets `401 Unauthorized`, and the user's container is already gone.

## Reproduction

```sh
podnext backup create deb-pdn
podnext backup restore <id> --name deb-copy   # writes image = "deb-pdn" into metadata
podnext reset deb-copy
# Resetting 'deb-copy' from 'deb-pdn'.
# This deletes the container and everything inside it.
# <container dir removed here>
# Error: registry returned 401 Unauthorized for 'deb-pdn'   <-- too late
```

## Evidence (observed log)

```
task-3 (power-user validation) reported:
  Critical Bug (`podnext reset` on restored containers):
  `podnext backup restore` writes the source container name into `metadata.json["image"]`.
  Calling `podnext reset` attempts to fetch that local name from Docker Hub
  (`401 Unauthorized`), destroying the container before verifying download availability.
```

## Root cause

`crates/podnext/src/main.rs:3116`

```rust
println!("Resetting '{}' from '{}'.", name, image_ref);
std::fs::remove_dir_all(&container_dir)?;                 // destroy FIRST
install_image_native(&image_ref, &name, false, None).await?;  // download SECOND
```

The destructive step runs before anything proves the image is obtainable, and the
recorded `image` may not even be a registry reference.

## Impact

- Irreversible loss of container contents on any network/registry/auth failure.
- Silent: the user is told the reset is happening, then left with nothing.

## Proposed fix

1. Persist the **original OCI reference** (`debian:13`) in metadata on restore, not the
   source container name.
2. Resolve and verify the image (manifest fetch) **before** touching the existing
   container; extract to a staging directory and swap only on success, so a failed
   download leaves the original container intact.

Labels: bug, cli
EOF

# ---------- M1 ----------
cat > research/issues/m1.md <<'EOF'
## Summary

`podnext ps` reports **no sessions** while containers are actively executing. Session
records are written only by the interactive `login` path; the non-interactive `run` path
never registers a session, so `ps` shows an empty table even with N running workloads.

## Reproduction

```sh
podnext run alpine -- sleep 10 &
podnext run alpine -- sleep 10 &
sleep 1
podnext ps
# +----+-----------+------+---------+-----+-----+--------+---------+---------+
# | ID | CONTAINER | USER | COMMAND | CWD | PID | STATUS | CREATED | STARTED |
# +----+-----------+------+---------+-----+-----+--------+---------+---------+
# (empty — two workloads are running)
```

## Evidence (observed log)

```
task-2 (recursive/stress) reported:
  === PODNEXT PS DURING CONCURRENT RUNS ===   -> empty table
  === PODNEXT PS DURING CONCURRENT LOGINS === -> empty table
  CLI bug: `cmd_run` never invokes `register_session`.
```

## Root cause

`crates/proot-distro-core/src/session.rs:41` defines `register_session`, and
`crates/podnext/src/main.rs` calls it only on the interactive login path. The
`cmd_run` path executes the container without registering, so `active_sessions()` finds
nothing to probe.

## Impact

- Operational blind spot: no way to observe running non-interactive workloads.
- Compounded by the JSON/table split reported separately (table reads disk, JSON reads
  in-memory state).

## Proposed fix

Register a session record in the `run` path as well (and release it on exit), or unify
both paths behind one execution entry point that always registers. Add a test asserting
`ps` sees a `run` workload while it is alive.

Labels: bug, cli, monitoring
EOF

# ---------- M2 ----------
cat > research/issues/m2.md <<'EOF'
## Summary

Concurrent writes to the **same file inside the same container** clobber each other
(4 of 5 workers read back another worker's data). Container-level locking exists but
protects only metadata operations, not the guest filesystem, so two `podnext run`
invocations targeting one container race freely.

## Reproduction

```python
# 5 threads, same container, each writes its own token to the same shared path,
# then reads it back and compares.
# Worker 0: expected token_0, read 'token_0', clobbered=False
# Worker 1: expected token_1, read 'token_0', clobbered=True
# Worker 2: expected token_2, read 'token_0', clobbered=True
# Worker 3: expected token_3, read 'token_0', clobbered=True
# Worker 4: expected token_4, read 'token_1', clobbered=True
# Total clobbered: 4/5
```

## Evidence (observed log)

```
task-2 (race conditions) reported:
  Worker 0: expected token_0, read 'token_0', clobbered=False
  Worker 1: expected token_1, read 'token_0', clobbered=True
  ...
  Total clobbered: 4/5
```

## Root cause

`ContainerLock` serializes registry-style operations but is not acquired by `run`/`login`
for the duration of guest execution. Nothing arbitrates concurrent guest processes writing
the same path — the guest sees a normal shared filesystem.

## Impact

- Silent data corruption for any workload that assumes exclusive access.
- Users reasonably expect container-level isolation to extend to concurrent invocations.

## Proposed fix

Decide and document the contract: either (a) allow concurrent `run` but document it as
shared-filesystem (POSIX semantics, no guarantee), or (b) acquire an advisory lock per
container for the execution window. Option (a) is cheaper and matches container semantics;
either way, remove the dead "locking exists, so we are safe" assumption and state the real
behavior in the docs. Add the clobber test to the suite to pin the chosen contract.

Labels: bug, documentation
EOF

# ---------- M3 ----------
cat > research/issues/m3.md <<'EOF'
## Summary

Extracting a deeply nested archive inside a container scales super-linearly with depth and
**times out past ~200 levels**, because every path operation in the guest costs a traced
syscall through PRoot's ptrace loop.

## Reproduction

```sh
podnext run alpine -- /bin/sh -c '
  cd /tmp && mkdir -p deep && cd deep
  for i in $(seq 1 200); do mkdir -p d$i && cd d$i; done
  cd /tmp && tar -cf deep.tar deep && time tar -xf deep.tar -C out'
```

Measured wall time for unpack vs depth:

| Depth | Time |
|-------|------|
| 10    | 1.07 s |
| 30    | 1.61 s |
| 60    | 4.93 s |
| 100   | 15.48 s |
| 150   | 50.43 s |
| 200   | timeout (>60 s) |

## Evidence (observed log)

```
Depth 10:  code=0, time=1.07s
Depth 60:  code=0, time=4.93s
Depth 100: code=0, time=15.48s
Depth 150: code=0, time=50.43s
Depth 200: TIMEOUT >60s
proot info: vpid 1: terminated with signal 15
```

## Root cause

Each guest `openat`/`mkdirat` is trapped and path-resolved in user space; per-syscall cost
grows with path length, and deep trees multiply the number of syscalls. This is inherent
to the ptrace backend rather than a bug in PDN's extractor.

## Impact

- Legitimately deep source trees (node_modules nesting, Java package trees) are slow or
  unusable.
- Users hitting a timeout see a truncated extraction with no clear cause.

## Proposed fix

- Document the ptrace scaling characteristic and recommend bounded archive depth.
- Raise the default command timeout for extraction-heavy operations.
- Longer term, offer the UML backend (a real guest kernel) for extraction-heavy work,
  where syscall cost is native.

Labels: performance
EOF

# ---------- L1 ----------
cat > research/issues/l1.md <<'EOF'
## Summary

`podnext build local` is a stub: it rejects `ARG`, silently skips `RUN`, never produces a
usable OCI image, and does not register the result in `podnext image ls`. The output looks
like a build but nothing runnable is created.

## Reproduction

```sh
printf 'FROM alpine:3.24\nARG FOO=bar\nRUN echo hello\nCMD ["/bin/sh"]\n' > Dockerfile
podnext build local -t demo .
# Error: Unsupported Dockerfile instruction: 'ARG'
```

## Evidence (observed log)

```
task-3 (power-user validation) reported:
  - `ARG` rejected: parser in `crates/proot-distro-core/src/dockerfile.rs` lacks `ARG`
    support (`Error: Unsupported Dockerfile instruction: 'ARG'`).
  - Builder incomplete: executes layer diff between context and an empty dummy directory,
    outputs a SHA-256 digest and size, deletes the tar.gz blob immediately
    (`crates/podnext/src/commands/mod.rs:2082`). Does not execute `RUN`, does not generate
    OCI manifests, does not register into `podnext image ls`.
```

## Root cause

`crates/proot-distro-core/src/dockerfile.rs` — instruction parser has no `ARG` token.
`crates/podnext/src/commands/mod.rs` — the build path diffs context against an empty
directory and discards the blob, with no `RUN` execution and no manifest emission.

## Impact

- The advertised OCI builder workflow cannot produce an image users can run.
- Reporting success for a build that produced nothing is worse than failing.

## Proposed fix

Either implement the missing steps (execute `RUN` in a scratch container, emit a real OCI
manifest, register the tag) or make the command **fail loudly** stating the builder is not
implemented, so no user mistakes the digest for a usable image.

Labels: bug, cli
EOF

# ---------- L2 ----------
cat > research/issues/l2.md <<'EOF'
## Summary

`podnext copy` / `podnext sync` expand sparse files to their dense size. A 10 MB sparse
file with zero allocated blocks grows to ~11 MB of real disk after a sync, because the
copy reads and writes every logical byte and never detects holes.

## Reproduction

```sh
# inside container: create a sparse file, then sync it out
truncate -s 10M /tmp/sparse.bin
stat -c '%s bytes, %b blocks' /tmp/sparse.bin    # 10485760 bytes, 0 blocks
podnext sync c1:/tmp/ /tmp/out
stat -c '%s bytes, %b blocks' /tmp/out/sparse.bin # dense allocation
```

## Evidence (observed log)

```
task-3 (power-user validation) reported:
  - Sparse files: no hole detection (`SEEK_HOLE`). 10MB sparse file (0 blocks allocated)
    expanded to 11MB dense physical allocation during sync.
```

## Root cause

`cmd_copy` and `cmd_sync` in `crates/podnext/src/main.rs` stream content without
`lseek(SEEK_DATA/SEEK_HOLE)` hole mapping (or `copy_file_range`).

## Impact

- Disk amplification for any image/workspace using sparse files (VM disk images, database
  files). On a phone this is a real capacity cost.

## Proposed fix

Detect holes with `SEEK_DATA`/`SEEK_HOLE` and re-create them with `ftruncate` + positional
writes, or use `copy_file_range` where the platform supports it.

Labels: bug, performance
EOF

# ---------- CI-1 ----------
cat > research/issues/ci1.md <<'EOF'
## Summary

The `PDN CI/CD` workflow **fails on `master`**: `test_restart_policy_unless_stopped`
leaves the service in `Failed` instead of `Stopped` after an explicit `stop_service`,
because the restart policy (`UnlessStopped`) races the explicit stop and wins.

## Reproduction

```sh
env -u LD_LIBRARY_PATH cargo test --workspace --no-fail-fast
# test test_restart_policy_unless_stopped ... FAILED
# tests/supervisor_e2e.rs:302:
#   assertion `left == right` failed
#     left: Failed
#    right: Stopped
```

CI run for `master` HEAD:

```
41744808292  failure  PDN CI/CD  master  push  2026-11-04T00:10:21Z  9m45s
  Rust Host Build: test_restart_policy_unless_stopped ... FAILED
  test result: FAILED. 18 passed; 1 failed
```

## Evidence (CI log, verbatim)

```
test test_restart_policy_unless_stopped ... FAILED
---- test_restart_policy_unless_stopped stdout ----
thread 'test_restart_policy_unless_stopped' (4095) panicked at tests/supervisor_e2e.rs:302:5:
assertion `left == right` failed
  left: Failed
 right: Stopped
error: test failed, to rerun pass `-p pdn-integration-tests --test supervisor_e2e`
```

## Root cause

Test body (`tests/supervisor_e2e.rs:275`): the service command is `/bin/false`
(always exits 1) with `RestartCondition::UnlessStopped`, `max_restarts = 3`,
`base_delay_secs = 1`. After `stop_service(name, false)` the test sleeps 3 s and expects
`Stopped`, but by then the restart loop has already marked the service `Failed` — i.e. the
supervisor does not let an explicit user stop pre-empt an in-flight restart decision, and
the exit-code classifier treats an intentional stop as an abnormal exit.

## Impact

- CI is red on `master`; every push reports a failing workflow.
- Real defect, not just a flaky test: an explicit stop can lose to the restart policy,
  which is exactly what `UnlessStopped` is supposed to prevent.

## Proposed fix

Set/check the `user_stopped` flag *before* the restart decision is evaluated so an explicit
stop always wins the race; ensure the exit-code classifier does not classify a
user-initiated stop as abnormal (`exit_code > 128` / signal) and set state to `Stopped`
rather than `Failed`. Keep the test; if a sleep is genuinely needed, poll for the expected
state instead of sleeping a fixed interval.

Labels: bug, ci
EOF

create_issue() {
  title="$1"; body="$2"; labels="$3"
  url=$(env -u LD_LIBRARY_PATH gh issue create \
    --title "$title" --body-file "$body" --label "$labels" 2>&1) || {
      echo "FAILED: $title"; echo "$url"; return 1; }
  echo "OK: $url"
}

create_issue "[BUG-C1] install accepts empty container name and creates an unremovable orphan" research/issues/c1.md "bug,cli"
create_issue "[BUG-C2] backup delete deadlocks permanently (tokio RwLock read guard held across write)" research/issues/c2.md "bug"
create_issue "[BUG-C3] reset destroys the container before verifying the image is downloadable" research/issues/c3.md "bug,cli"
create_issue "[BUG-M1] podnext ps does not report non-interactive run sessions" research/issues/m1.md "bug,cli,monitoring"
create_issue "[BUG-M2] concurrent guest writes to one file clobber each other (4/5)" research/issues/m2.md "bug,documentation"
create_issue "[BUG-M3] deep tree extraction scales super-linearly and times out past ~200 levels" research/issues/m3.md "performance"
create_issue "[BUG-L1] podnext build local rejects ARG, skips RUN, and produces no usable OCI image" research/issues/l1.md "bug,cli"
create_issue "[BUG-L2] copy/sync expand sparse files to dense allocation" research/issues/l2.md "bug,performance"
create_issue "[CI-1] test_restart_policy_unless_stopped fails: explicit stop loses to restart policy" research/issues/ci1.md "bug,ci"

echo "=== DONE ==="
