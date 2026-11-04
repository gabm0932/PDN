#!/bin/sh
# Commit the verified fixes as separate logical commits. POSIX sh.
set -e
cd "$(dirname "$0")/.."

MSG="$HOME/.hermes/cache/scratch/commitmsg"
mkdir -p "$MSG"

# ---------------- 1: login executes the fetched container ----------------
cat > "$MSG/1.txt" <<'EOF'
fix(core): execute login against the fetched container, not a re-lookup

LoginOptions carries per-invocation config (notably `--isolated`), but the
execute path re-fetched the container by name inside `ContainerManager::execute`
and therefore discarded the caller's copy. `--isolated` was parsed, stored on the
options struct, and then silently dropped before the backend ever saw it.

Add `ContainerManager::execute_container(&Container, ...)`, which runs against a
container value the caller already holds, and route login through it. Verified:

    podnext login --isolated deb-pdn -- ls /data/data/com.termux/files/home
    # (empty: host home unmapped, as --isolated promises)

    podnext login deb-pdn -- ls /data/data/com.termux/files/home | head -3
    # api_original.rs
    # backups
    # display_config_termux.py
EOF

# ---------------- 2: FileLock ----------------
cat > "$MSG/2.txt" <<'EOF'
fix(locking): file-lock the descriptor we opened instead of fd 0

`FileLock::lock` opened the lock file into a throwaway `_file` binding, then
called `flock(2)` on `self.file` — which is always `None` at that point — falling
back through `unwrap_or(0)` to file descriptor 0, i.e. **stdin**. It then ended
with `self.file = Some(self.file.take().unwrap())`, unwrapping that same `None`
and panicking on every call.

The type therefore provided no mutual exclusion at all, and crashed when used.
Store the opened descriptor, flock that descriptor, and keep it alive for the
lock's lifetime (flock is released when the last descriptor on the file closes).

Replaces the `#[should_panic]` test that merely pinned the panic with a test
asserting real behaviour: the first lock succeeds, a second locker on the same
path is refused while it is held, and locking succeeds again after release.

    test locking::tests::test_file_lock_acquires_and_excludes_a_second_locker ... ok
    test locking::tests::test_container_lock_exclusive_contention ... ok
    test locking::tests::test_container_lock_shared_vs_exclusive ... ok
    test result: ok. 3 passed; 0 failed

Refs #20
EOF

# ---------------- 3: backup delete deadlock ----------------
cat > "$MSG/3.txt" <<'EOF'
fix(backup): release the read lock before taking the write lock

`delete_backup` held a `tokio::sync::RwLock` read guard for the whole function
body and then called `write().await` on the same lock inside that scope. Tokio's
RwLock is neither reentrant nor upgradable, so the write request waited on a read
guard only this function could drop: `podnext backup delete` hung forever and the
scheduler/daemon callers blocked a task slot with it.

Snapshot what the removal needs, drop the read guard, then take the write lock.
Disk cleanup (archive, .meta.json sidecar, staging dir) is unchanged.

Regression test added: `delete_backup` must complete rather than hang.

    test backup_restore::tests::test_delete_backup_does_not_deadlock ... ok
    test result: ok. 1 passed; 0 failed

Refs #12
EOF

# ---------------- 4: CLI name + reset ----------------
cat > "$MSG/4.txt" <<'EOF'
fix(cli): validate container names and verify the image before reset

Two CLI defects, both reachable from ordinary use.

1. `install --name ""` was accepted. The container directory is derived by joining
   the name onto the containers root, so an empty segment collapsed the path onto
   the containers root itself: `list` showed a container with a blank name and
   `remove -f ""` answered "not found", leaving an unremovable orphan that also
   blocked any later install at that path. `NameValidator::validate_container_name`
   already existed and was already applied by `rename` — it was simply never
   called on install. Apply it to both the explicit `--name` and the name derived
   from the image, and derive that name as a single path component so a registry
   host or namespace (`ghcr.io:5000/acme/web:1.2`) cannot become a directory level.

2. `reset` deleted the container directory and only then downloaded the base image.
   A `backup restore` records the *source container* as the recorded image, which
   is not a registry reference; resetting such a container removed it, then failed
   with `401 Unauthorized`, destroying the user's data with nothing to replace it.
   Add `verify_image_available` (resolve token + manifest) and call it **before**
   the destructive step, so an unfetchable image leaves the container intact.

3. A guest command containing `-r` aborted the whole invocation: the early
   direct-execution pre-scan searched all of argv for `-r`/`--rootfs` before clap
   ran, so `podnext run alpine -- rm -r /tmp/x` was misread as a direct-execution
   call. Restrict the scan to arguments before any recognised subcommand and
   before the `--` separator; everything after `--` belongs to the guest.

Verified on device:

    podnext install debian:13 --name ""        -> Error: Container name cannot be empty (exit 1)
    podnext install debian:13 --name "../escape" -> Error: Container name can only contain ... (exit 1)
    (container count before/after diff: no new directory created)

    podnext reset test-pdn-install   (metadata image set to a non-registry name)
      -> Error: Failed to fetch manifest: 401 Unauthorized (exit 1)
      -> container directory still present, rootfs intact (17 entries)

    podnext run deb-pdn -- grep -r root /etc/passwd   -> runs (no -r hijack)

Refs #11, #13, #21
EOF

git add crates/proot-distro-core/src/container.rs crates/proot-distro-core/src/lib.rs
git commit -F "$MSG/1.txt"

git add crates/proot-distro-core/src/locking.rs
git commit -F "$MSG/2.txt"

git add crates/pn-next/src/backup_restore/mod.rs
git commit -F "$MSG/3.txt"

git add crates/podnext/src/main.rs crates/podnext/src/native.rs
git commit -F "$MSG/4.txt"

echo "=== LOG ==="
git log --oneline -6
echo "=== REMAINING (must exclude what other agents are editing) ==="
git status --short
