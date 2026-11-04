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
