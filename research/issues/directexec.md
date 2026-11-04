## Summary

`podnext run <container> -- <cmd>` aborts whenever the **guest command itself** contains a
`-r` or `--rootfs` token, because the CLI scans the whole argv for those flags *before*
parsing subcommands, and concludes the invocation is a direct-execution call.

## Reproduction

```sh
podnext run alpine -- rm -r /tmp/dummy
# Error: -r/--rootfs is required for direct execution mode
# exit 1
```

The same happens for any guest command containing `-r`, e.g. `rm -r`, `grep -r`,
`tar -r`, or a path literally named `--rootfs`.

## Evidence (observed log)

```
CRITICAL Syntax Hijack (`-r` inside guest command):
  Command: podnext run alpine -- rm -r /tmp/dummy
  Output:  Error: -r/--rootfs is required for direct execution mode (Exit 1).
  Root cause: main.rs:1407 checks raw_args.iter().any(|a| a == "-r" || a == "--rootfs")
  across entire argv before parsing subcommands or `--`. Any guest command containing
  `-r` aborts execution.
```

## Root cause

`crates/podnext/src/main.rs` (the early direct-execution pre-scan) treated the mere
presence of `-r`/`--rootfs` **anywhere** in argv as a direct-execution invocation. It ran
before clap parsed anything, so a subcommand plus a `--`-separated guest command was
misclassified, and the guest command's own flags were read as podnext's.

## Impact

- A large class of legitimate guest commands is unrunnable: anything using `-r`.
- The failure message points at podnext's own flag rather than the guest command, so the
  cause is not obvious to the user.

## Proposed fix

Only treat `-r`/`--rootfs` as direct-execution mode when it appears **before** any
recognised subcommand and before the `--` separator. Anything after `--` belongs to the
guest command and must be passed through untouched. Add a regression test covering
`run <container> -- rm -r <path>`.

Labels: bug, cli
