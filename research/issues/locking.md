## Summary

`FileLock::lock` in `crates/proot-distro-core/src/locking.rs` can never succeed. It
performs two independent mistakes: it calls `flock(2)` on file descriptor **0** (stdin)
instead of the lock file it just opened, and it then unconditionally `unwrap()`s a `None`
field, panicking. The lock this type claims to provide is therefore either a no-op on the
wrong descriptor or a crash.

## Reproduction

```rust
let mut l = FileLock::new(Path::new("/tmp/x.lock"));
l.lock();   // panics: called `Option::unwrap()` on a `None` value
```

## Evidence (source + observed panic)

```rust
// crates/proot-distro-core/src/locking.rs:78
pub fn lock(&mut self) -> Result<()> {
    let _file = std::fs::OpenOptions::new()
        .create(true).write(true).open(&self.path)?;   // opened into `_file` ...
    unsafe {
        let result = libc::flock(
            self.file.as_ref().map(|f| f.as_raw_fd()).unwrap_or(0),  // ... self.file is None -> fd 0 = STDIN
            libc::LOCK_EX | libc::LOCK_NB,
        );
        if result != 0 { return Err(anyhow::anyhow!("Failed to acquire lock")); }
    }
    self.file = Some(self.file.take().unwrap());   // self.file is None -> PANIC
    Ok(())
}
```

Recorded during the concurrency audit:

```
FileLock Panic Bug: crates/proot-distro-core/src/locking.rs:96:
  self.file = Some(self.file.take().unwrap()) executes when self.file is None.
  Causes unconditional panic `called Option::unwrap() on a None value`.
```

## Root cause

The freshly opened `File` is bound to a throwaway `_file` and never stored, so `self.file`
stays `None`. The `flock` call then falls back to `unwrap_or(0)` — file descriptor 0, i.e.
stdin — and the final assignment unwraps the still-`None` field.

## Impact

- Any caller relying on `FileLock` gets no mutual exclusion at all (it locks stdin), and
  after the first call panics.
- A crash-on-use primitive is worse than a missing one: it reads as working code.

## Proposed fix

Store the opened descriptor, flock *that* descriptor, and keep it alive for the lock's
lifetime (flock is released when the last descriptor on the file closes). Replace the
`#[should_panic]` test that currently pins this bug with a test that `lock()` succeeds and
that a second locker on the same path is excluded while the first is held.

Labels: bug
