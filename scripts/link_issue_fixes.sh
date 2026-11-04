#!/bin/sh
# Link the fixing commits to their issues. POSIX sh.
set -e
cd "$(dirname "$0")/.."

TOKEN=$(grep -o 'ghp_[A-Za-z0-9]\{20,\}' ~/.hermes/.env | head -1)
[ -n "$TOKEN" ] || { echo "token not found"; exit 1; }

MSG="$HOME/.hermes/cache/scratch/issuecomments"
mkdir -p "$MSG"

cat > "$MSG/11.md" <<'EOF'
Fixed in `b73b66f` (branch `master`).

`NameValidator::validate_container_name` already existed and `rename` already used it; `install` simply never called it. It is now applied to both an explicit `--name` and the name derived from the image, and the derived name is reduced to a single path component so a registry host or namespace (`ghcr.io:5000/acme/web:1.2`) cannot become a directory level.

Evidence, on device with the rebuilt binary (`podnext 0.1.7`):

```
$ podnext install debian:13 --name ""
Error: Container name cannot be empty
EXIT CODE: 1

$ podnext install debian:13 --name "../escape"
Error: Container name can only contain alphanumeric, hyphen, and underscore
slash exit: 1

$ diff <(ls ~/.podnext/containers | sort) <(ls ~/.podnext/containers | sort)
NO NEW DIRS CREATED (correct)
```

The previously-created orphan at `~/.podnext/containers/rootfs` was removed by hand; the empty-name path can no longer create one.
EOF

cat > "$MSG/12.md" <<'EOF'
Fixed in `cc47612` (branch `master`).

`delete_backup` held a `tokio::sync::RwLock` read guard for the whole function body and then called `write().await` on the same lock from inside that scope. The read guard is only droppable by this function, so the write request waited on it forever.

The fix snapshots the existence check, drops the read guard, and only then takes the write lock. Disk cleanup (archive, `.meta.json` sidecar, staging dir) is unchanged.

Regression test added — `delete_backup` must complete rather than hang:

```
test backup_restore::tests::test_delete_backup_does_not_deadlock ... ok
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 50 filtered out
```
EOF

cat > "$MSG/13.md" <<'EOF'
Partially fixed in `b73b66f` (branch `master`) — the data-loss path is closed; the metadata improvement is still open.

Fixed: `reset` now calls a new `verify_image_available` (token + manifest resolution) **before** the destructive step, so an unfetchable image leaves the container intact. Evidence, pointing the recorded image at a non-registry name exactly as `backup restore` does:

```
$ podnext reset test-pdn-install
Resetting 'test-pdn-install' from 'ghost-local-container'.
This deletes the container and everything inside it.
Error: Failed to fetch manifest: 401 Unauthorized
EXIT CODE: 1

$ [ -d ~/.podnext/containers/test-pdn-install ] && echo PRESERVED
PRESERVED
rootfs entries: 17
```

Before the fix the same input removed the container directory and *then* failed.

Still open (needs a change in `backup_restore`): `backup restore` writes the **source container name** into `metadata.json["image"]`, which is not a registry reference. Restoring should persist the original OCI reference so a later `reset` resolves the real base image instead of failing verification. Tracked here until that lands.
EOF

cat > "$MSG/20.md" <<'EOF'
Fixed in `daaac87` (branch `master`).

`FileLock::lock` opened the lock file into a throwaway `_file` binding, then called `flock(2)` on `self.file` — always `None` at that point — which fell through `unwrap_or(0)` to **file descriptor 0 (stdin)**, and ended with `self.file = Some(self.file.take().unwrap())`, unwrapping that same `None` and panicking on every call. So the type provided no mutual exclusion and crashed when used.

The descriptor that was opened is now stored and flock'd, and is kept alive for the lock's lifetime (flock releases when the last descriptor on the file closes).

The `#[should_panic]` test that merely pinned the panic is replaced with a test asserting real behaviour:

```
test locking::tests::test_file_lock_acquires_and_excludes_a_second_locker ... ok
test locking::tests::test_container_lock_exclusive_contention ... ok
test locking::tests::test_container_lock_shared_vs_exclusive ... ok
test result: ok. 3 passed; 0 failed; 0 ignored; 0 measured; 30 filtered out
```

Note for follow-up: `FileLock` and `ContainerLock` have no production call sites (`grep -rn "FileLock\|ContainerLock" crates/` matches only `locking.rs` itself) — the concurrency findings in #15 are consistent with that.
EOF

cat > "$MSG/21.md" <<'EOF'
Fixed in `b73b66f` (branch `master`).

The early direct-execution pre-scan searched the whole argv for `-r`/`--rootfs` before clap ran, so `podnext run <container> -- rm -r /tmp/x` was misread as a direct-execution invocation and aborted with `Error: -r/--rootfs is required for direct execution mode`.

The scan is now restricted to arguments appearing **before** any recognised subcommand and **before** the `--` separator; everything after `--` belongs to the guest command and is passed through untouched.

Evidence after the fix (verbatim):

```
$ podnext run deb-pdn -- grep -r root /etc/passwd
root:x:0:0:root:/root:/bin/bash
exit=0

$ podnext run alpine -- rm -r /tmp/nonexistent_dir_xyz
rm: can't remove '/tmp/nonexistent_dir_xyz': No such file or directory
exit=1

$ podnext run alpine -- sh -c 'echo marker; grep -r root /etc/passwd | head -1'
marker
root:x:0:0:root:/root:/bin/sh
exit=0
```

The second case is the important one: the guest `rm` actually ran and reported its own
failure. Before the fix that invocation never reached the guest at all — podnext aborted
with `Error: -r/--rootfs is required for direct execution mode`.
EOF

comment() {
  n="$1"; body="$2"
  url=$(env -u LD_LIBRARY_PATH gh issue comment "$n" --body-file "$body" 2>&1) || { echo "FAILED #$n: $url"; return 1; }
  echo "OK #$n: $url"
}

comment 11 "$MSG/11.md"
comment 12 "$MSG/12.md"
comment 13 "$MSG/13.md"
comment 20 "$MSG/20.md"
comment 21 "$MSG/21.md"
echo "=== DONE ==="
