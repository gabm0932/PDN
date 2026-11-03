# PRoot-Distro 5.10.3 — User-Facing Capability Inventory

Evidence-based. Every claim cites the exact `--help` output or source file/line observed on this host.
Read-only research; nothing outside this file was modified.

- Host: Termux / Android 16, aarch64, kernel `6.17.0-PRoot-Distro` (faked; see §6)
- Python: 3.14.6
- Tool: `proot-distro` 5.10.3, author "Termux (@sylirre)"
- Captured: Tue Nov 3 02:39:23 WIB 2026

---

## 1. Version, install path, package layout

### Version string
`proot-distro --version` is **not** a valid command; it prints `Error: unknown command '--version'.` and the help page. The version is instead printed as the last line of every help page:

```
PRoot-Distro version '5.10.3' by Termux (@sylirre).
```

Confirmed independently from package metadata:

```
$ head -3 /data/data/com.termux/files/usr/lib/python3.14/site-packages/proot_distro-5.10.3.dist-info/METADATA
Metadata-Version: 2.4
Name: proot-distro
Version: 5.10.3
```

### Console entry points
`entry_points.txt` in the dist-info defines two console scripts, both pointing at `proot_distro.cli:main`:

```
[console_scripts]
pd = proot_distro.cli:main
proot-distro = proot_distro.cli:main
```

Both launcher files exist and are byte-identical size (193 bytes):

```
-rwx------. 1 u0_a396 u0_a396 193 ... /data/data/com.termux/files/usr/bin/pd
-rwx------. 1 u0_a396 u0_a396 193 ... /data/data/com.termux/files/usr/bin/proot-distro
```

`which proot-distro` → `/data/data/com.termux/files/usr/bin/proot-distro`.

### Python package layout
Package root: `/data/data/com.termux/files/usr/lib/python3.14/site-packages/proot_distro/`

Top-level modules (from `ls -la` and `find`):

```
__init__.py        arch.py       atomic.py     cli.py        compress.py
constants.py       dirfd.py      execenv.py    guestfile.py  l2s.py
locking.py         message.py    names.py      parser.py     paths.py
progress.py        session.py    shm.py        statedir.py   sysdata.py
```

Subpackages:
- `commands/` — one module/subpackage per subcommand:
  `backup.py build.py clear_cache.py copy.py install.py install_local.py kill.py list.py ps.py push.py remove.py rename.py reset.py restore.py run.py search.py sync.py`
  - `commands/help/` → `__init__.py`, `pages.py`, `render.py`
  - `commands/login/` → `__init__.py`, `bindings.py`, `detach.py`, `env.py`, `migrate.py`, `passwd.py`, `proot_cmd.py`, `quoting.py`
- `helpers/` → `build_cache.py dockerfile.py download.py layer_diff.py oci_writer.py rootfs.py tar_extract.py`
  - `helpers/build_engine/` → `constants.py copy_step.py dockerignore.py engine.py errors.py handlers.py parsing.py run_step.py stage.py users.py`
  - `helpers/docker/` → `cache.py layers.py media.py pull.py push.py refs.py search.py transport.py`
- `completions/` → `_proot-distro` (zsh), `proot-distro.bash`, `proot-distro.fish`

Dist-info directory `proot_distro-5.10.3.dist-info/` contains `METADATA RECORD REQUESTED WHEEL direct_url.json entry_points.txt top_level.txt licenses/LICENSE INSTALLER`. RECORD lists 171 entries. RECORD also installs shell completions outside the package:
```
../../../share/bash-completion/completions/proot-distro.bash
../../../share/fish/vendor_completions.d/proot-distro.fish
../../../share/zsh/site-functions/_proot-distro
```

### Runtime data directory
```
/data/data/com.termux/files/usr/var/lib/proot-distro
```
Contents observed: `cache/  containers/  locks/  sessions/`.

EVIDENCE: `proot-distro --help` (footer line), `proot-distro --version` (error), `cat .../METADATA`, `cat .../entry_points.txt`, `find .../proot_distro/ -type f`, `ls -la .../var/lib/proot-distro/`.

---

## 2. Full command list with every flag

### Top-level `proot-distro help` / `proot-distro --help`

```
USAGE

  proot-distro [COMMAND] [ARGUMENTS]

DESCRIPTION

  PRoot-Distro is a wrapper utility for proot user-space emulator of
  chroot, mount --bind and binfmt_misc. This utility provides a
  convenient way for working with Linux containers, leveraging support
  of Docker registries to provide distributions of any kind.

COMMANDS

  help         Show this help.
  search       Search images on Docker Hub.
  install      Install distribution from OCI image or rootfs archive.
  list         List created containers or cached images.
  login        Start interactive shell inside a container.
  run          Run container entrypoint in server or distroless images.
  ps           List active container sessions.
  kill         Stop active container sessions.
  remove       Delete a container or a cached image. Destroys data!
  rename       Rename a container.
  reset        Reinstall a container from scratch. Destroys data!
  backup       Save container as a TAR archive.
  restore      Restore container from a TAR archive. Destroys data!
  clear-cache  Delete cached downloads.
  copy         Copy files from/to container.
  sync         Sync files from/to container.
  build        Build an OCI image from a Dockerfile.
  push         Push a locally built image to a registry.

GETTING HELP

  Run 'proot-distro <command> --help' for details on any command.
```

Aliases (from `parser.py` `ALIAS_TO_CANONICAL`): `add/i/in/ins`→install, `rm`→remove, `sh`→login, `li/ls`→list, `bak/bkp`→backup, `clear/cl`→clear-cache, `cp`→copy, `s/se`→search, `h/he/hel`→help.

### `install`

```
USAGE

  proot-distro install [OPTIONS] (IMAGE:TAG or URL or FILE)

  Aliases: add, i, in, ins

OPTIONS

  -h, --help                Show this help.

  -n, --name [NAME]         Set a custom name for the container. Must
                            start with alphanumeric character and then
                            may contain only latin letters, numbers and
                            special symbols dot, minus, underscore.
                            Default equals to image name without tag and
                            registry prefix.

  -a, --architecture [ARCH]
                            Override the target CPU architecture.
                            Accepts native names (aarch64, arm, i686,
                            riscv64, x86_64) or Docker platform strings
                            (linux/arm64, linux/amd64, linux/arm/v7,
                            linux/386, linux/riscv64).

  --allow-insecure          Permit insecure transport: a custom registry
                            served over plain HTTP, an HTTPS endpoint
                            (registry or download URL) whose TLS
                            certificate is untrusted, expired,
                            self-signed, or for a different hostname, or
                            a redirect from HTTPS to plain HTTP. Docker
                            Hub is always reached over verified HTTPS.
                            Use only for trusted sources on a network
                            path you control.

  -q, --quiet               Suppress non-error output.
```

Hidden alias from parser: `--override-alias` is a mutually-exclusive synonym of `-n/--name` (`dest=custom_container_name`).

### `list`

```
USAGE

  proot-distro list [OPTIONS]

  Aliases: li, ls

OPTIONS

  -h, --help   Show this help.

  -i, --image  List cached OCI images instead of containers.

  -q, --quiet  Print only names, one per line.
```

### `login`

```
USAGE

  proot-distro login [OPTIONS] CONTAINER [-- COMMAND]

  Aliases: sh

OPTIONS

  -h, --help             Show this help.

  -u, --user [USER]      User identity to switch to instead of root.
                         Accepted forms: 'name' (username from
                         /etc/passwd), 'name:group' (username and group
                         name from /etc/passwd and /etc/group), 'uid'
                         (numeric UID), 'uid:gid' (numeric UID and GID).

  -P, --redirect-ports   Replace privileged port bindings with higher
                         port numbers (22 -> 2022, 80 -> 2080, etc).
                         Port shift offset is hardcoded into proot
                         executable itself and can't be configured
                         through proot-distro.

  --isolated             Enable Isolated Mode. No host file system
                         bindings created unless using QEMU user mode
                         emulation or user manually requested specific
                         directories to be bound.

  --minimal              Enable Isolated Mode with bare minimum proot
                         configuration. Only /dev, /proc and /sys are
                         bound. All proot extensions except link2symlink
                         are disabled. No /proc system data workarounds,
                         no kernel release override. Specific features
                         may only be enabled through command line
                         options. Could show higher performance than in
                         other modes.

  --shared-home          Bind host home directory into the container.
                         Takes priority over Isolated Mode. Already
                         included in default mode.

  --shared-tmp           Bind host tmp directory to /tmp. Takes priority
                         over Isolated Mode. Already included in default
                         mode.

  --shared-x11           Bind host X11 socket directory to
                         /tmp/.X11-unix. Takes priority over Isolated
                         Mode. Inherited by --shared-tmp. Already
                         included in default mode.

  -b, --bind [SRC:DEST]  Custom filesystem binding. Can be specified
                         multiple times. Takes priority over Isolated
                         Mode.

  --no-link2symlink      Disable hardlink emulation by proot.
                         Recommended only for devices with SELinux in
                         permissive mode.

  --no-sysvipc           Disable System V IPC emulation by proot.
                         Recommended only for devices where kernel has
                         this feature enabled and SELinux set to
                         permissive mode.

  --no-kill-on-exit      Hang indefinitely until all session processes
                         exit.

  --emulator [FILE]      Override the QEMU emulator binary for
                         cross-arch execution. Only QEMU user mode and
                         Blink emulators are supported. FILE must be
                         executable.

  --kernel [TEXT]        Customize the kernel release string reported by
                         uname.

  --hostname [TEXT]      Customize the system hostname.

  -w, --work-dir [PATH]  Set the initial working directory.

  -e, --env VAR=VALUE    Set an environment variable. Can be specified
                         multiple times.

  -d, --detach           Start the session in the background and return
                         immediately. Standard input and output are
                         redirected to /dev/null. The session is listed
                         by 'proot-distro ps' and can be stopped with
                         'kill PID'.

  --get-proot-cmd        Print the fully assembled proot command line
                         and exit without running it.
```

Hidden aliases from parser: `--fix-low-ports` (mutually exclusive synonym of `-P/--redirect-ports`), `--termux-home` (mutually exclusive synonym of `--shared-home`).

### `run`

```
USAGE

  proot-distro run [OPTIONS] CONTAINER [-- ARG ...]

OPTIONS

  -h, --help             Show this help.

  -u, --user [USER]      User identity to switch to instead of root.
                         Accepted forms: 'name' ... 'uid:gid'.

  -P, --redirect-ports   Replace privileged port bindings with higher
                         port numbers (22 -> 2022, 80 -> 2080, etc).
  --isolated             Enable Isolated Mode...
  --minimal              Enable Isolated Mode with bare minimum proot
                         configuration...
  --shared-home          Bind host home directory into the container...
  --shared-tmp           Bind host tmp directory to /tmp...
  --shared-x11           Bind host X11 socket directory to /tmp/.X11-unix...
  -b, --bind [SRC:DEST]  Custom filesystem binding...
  --no-link2symlink      Disable hardlink emulation by proot...
  --no-sysvipc           Disable System V IPC emulation by proot...
  --no-kill-on-exit      Hang indefinitely until all session processes
                         exit.
  --emulator [FILE]      Override the QEMU emulator binary...
  --kernel [TEXT]        Customize the kernel release string reported by
                         uname.
  --hostname [TEXT]      Customize the system hostname.
  -w, --work-dir [PATH]  Set the initial working directory.
  -e, --env VAR=VALUE    Set an environment variable...
  -d, --detach           Start the session in the background...
  --get-proot-cmd        Print the fully assembled proot command line
                         and exit without running it.
```

(`run` shares the exact option set of `login` via `_add_login_or_run_common`; the abbreviated lines above are the same verbatim text as `login`'s full block.)

### `ps`

```
USAGE

  proot-distro ps [OPTIONS]

OPTIONS

  -h, --help   Show this help.

  -q, --quiet  Print only the PID of each active session, one per line.
```

### `kill`

```
USAGE

  proot-distro kill [OPTIONS] (PID | CONTAINER | --all)

OPTIONS

  -h, --help             Show this help.

  -s, --signal [SIGNAL]  Signal to send to the guest processes instead
                         of the default SIGTERM. Accepts a name
                         (SIGTERM, KILL, HUP) or a number (15, 9, 1).

  --all                  Target all sessions system-wide.
```

### `remove`

```
USAGE

  proot-distro remove [OPTIONS] CONTAINER|IMAGE

  Aliases: rm

OPTIONS

  -h, --help                Show this help.

  -i, --image               Delete a cached OCI image instead of a
                            container. The image selected by reference
                            or id, both can be obtained by command
                            'proot-distro list --image'.

  -a, --architecture [ARCH]
                            Delete only image built for ARCH. Requires
                            --image option.

  -v, --verbose             Log each deleted file.

  -q, --quiet               Suppress non-error output. Mutually
                            exclusive with --verbose.
```

### `rename`

```
USAGE

  proot-distro rename OLDNAME NEWNAME

OPTIONS

  -h, --help   Show this help.

  -q, --quiet  Suppress non-error output.
```

### `reset`

```
USAGE

  proot-distro reset CONTAINER

OPTIONS

  -h, --help   Show this help.

  -q, --quiet  Suppress non-error output.
```

### `backup`

```
USAGE

  proot-distro backup [OPTIONS] CONTAINER

  Aliases: bak, bkp

OPTIONS

  -h, --help             Show this help.

  -c, --compress [TYPE]  Force a specific compression algorithm,
                         overriding the file extension. Supported
                         values: gzip, bzip2, xz, zstd (needs Python
                         3.14+), none.

  -o, --output [FILE]    Write the archive to FILE instead of stdout.
                         When --compress is not given, compression is
                         inferred from the file extension like tar.gz or
                         txz.

  -v, --verbose          Log each file name as it is added to the
                         archive.

  -q, --quiet            Suppress non-error output. Mutually exclusive
                         with --verbose.
```

### `restore`

```
USAGE

  proot-distro restore [OPTIONS] [BACKUP_FILE]

OPTIONS

  -h, --help     Show this help.

  -v, --verbose  Log each extracted file.

  -q, --quiet    Suppress non-error output. Mutually exclusive with
                 --verbose.
```

### `clear-cache`

```
USAGE

  proot-distro clear-cache [OPTIONS]

  Aliases: clear, cl

OPTIONS

  -h, --help     Show this help.

  --orphan       Remove only the layer blobs nothing references any
                 more, keeping the rest of the cache intact.

  --build-cache  Drop the build cache: the per-instruction index and the
                 layer blobs only it was holding on to.

  -v, --verbose  Log each removed file.

  -q, --quiet    Suppress non-error output. Mutually exclusive with
                 --verbose.
```

### `copy`

```
USAGE

  proot-distro copy [OPTIONS] [DIST:]SRC [DIST:]DEST

  Aliases: cp

OPTIONS

  -h, --help       Show this help.

  -m, --move       Delete source file after a successful copy.

  -r, --recursive  Recursive mode for copying directories.

  -v, --verbose    Log each copied file.

  -q, --quiet      Suppress non-error output. Mutually exclusive with
                   --verbose.
```

### `sync`

```
USAGE

  proot-distro sync [OPTIONS] [DIST:]SRC [DIST:]DEST

OPTIONS

  -h, --help      Show this help.

  -c, --checksum  Compare files by size and CRC32 checksum instead of
                  size and modification time. Slower but with high
                  precision.

  -d, --delete    After syncing, remove destination files and
                  directories that have no counterpart in the source.
                  Requires a directory as the source.

  -v, --verbose   Log each synced or deleted entry.

  -q, --quiet     Suppress non-error output. Mutually exclusive with
                  --verbose.
```

### `build`

```
USAGE

  proot-distro build [OPTIONS] [PATH]

OPTIONS

  -h, --help                Show this help.

  -f, --file [PATH]         Use a Dockerfile at PATH instead of
                            <PATH>/Dockerfile. Pass '-' to read the
                            Dockerfile from standard input.

  -t, --tag [REF]           Image reference to assign, without a digest.
                            Repeatable. Defaults to
                            '<basename(PATH)>:latest', adjusted to
                            Docker's name rules.

  --build-arg [K=V]         Set a build-time ARG. Only ARGs declared in
                            the Dockerfile are honoured. Repeatable.

  -a, --architecture [ARCH]
                            Target CPU architecture (default: host
                            architecture). Accepts proot-distro names
                            (aarch64, arm, i686, riscv64, x86_64) or
                            Docker platform strings (linux/arm64,
                            linux/amd64, ...).

  --target [STAGE]          Stop after the named stage of a multi-stage
                            build.

  --emulator [PATH]         Override the QEMU user-mode binary used for
                            cross-architecture builds.

  -o, --output [FILE]       Write the built image as an OCI tarball to
                            FILE. Compression is inferred from the
                            extension (.oci.tar, .oci.tar.gz,
                            .oci.tar.xz). Repeatable.

  --install-as [NAME]       Install the built image as a container named
                            NAME after the build completes.

  --no-cache                Disable build-step caching. Each instruction
                            is executed fresh.

  -v, --verbose             Echo each instruction and stream RUN output
                            to the terminal.

  -q, --quiet               Suppress non-error output. Mutually
                            exclusive with --verbose.
```

### `push`

```
USAGE

  proot-distro push [OPTIONS] IMAGE

OPTIONS

  -h, --help                Show this help.

  -a, --architecture [ARCH]
                            Push the manifest built for the given
                            architecture. Accepts proot-distro names
                            (aarch64, arm, i686, riscv64, x86_64) or
                            Docker platform strings (linux/arm64,
                            linux/amd64, ...). Default: host
                            architecture.

  --allow-insecure          Permit insecure transport to the target
                            registry: a custom registry served over
                            plain HTTP, an HTTPS registry whose TLS
                            certificate is untrusted, expired,
                            self-signed, or for a different hostname, or
                            a redirect from HTTPS to plain HTTP. Docker
                            Hub is always reached over verified HTTPS.
                            Use only for trusted registries on a network
                            path you control.

  -q, --quiet               Suppress non-error output.
```

### `search`

```
USAGE

  proot-distro search [OPTIONS] QUERY

  Aliases: se, s

OPTIONS

  -h, --help       Show this help.

  -l, --limit [N]  Maximum number of results to show. Default: 25,
                   maximum: 1000. Docker Hub serves 100 results per
                   request, so a larger limit is collected page by page.

  -q, --quiet      Print only repository names, one per line, without
                   the table. Suitable for piping into other commands.
```

EVIDENCE: `proot-distro help`; `proot-distro <cmd> --help` for each of install, list, login, run, ps, kill, remove, rename, reset, backup, restore, clear-cache, copy, sync, build, push, search; alias table in `parser.py` (`ALIAS_TO_CANONICAL`, `_install`, `_login`, `_run`).

---

## 3. Per-command behavior and defaults (from source)

Defaults and mutually-exclusive groups read from `proot_distro/parser.py`; behavior read from the module in `proot_distro/commands/`.

| Command | Module | Defaults / notes from parser.py | Behavior (from module) |
|---|---|---|---|
| `install` | `commands/install.py` | `image_ref` optional positional; `-n/--name` or `--override-alias` (mutually exclusive, dest `custom_container_name`); `-a/--architecture`; `--allow-insecure` store_true; `-q` store_true | Validates name/arch, dispatches to local-file / URL / Docker pull (`pull_image`), extracts rootfs, writes `manifest.json`, writes `/etc/resolv.conf`, `/etc/hosts`, registers Android IDs into `/etc/passwd`, creates sysdata stubs. Held under `ContainerLock(exclusive=True)`. Refuses an existing container. |
| `list` | `commands/list.py` | `-i/--image` store_true; `-q` store_true | Default lists installed containers (subdirs of `CONTAINERS_DIR` with a `rootfs/`); `--image` lists cached OCI images as IMAGE/ARCH/ID/SIZE/CREATED (degrades to stacked when narrow). `--quiet` prints bare names to stdout. |
| `login` | `commands/login/__init__.py` | `-u/--user` default `'root'`; `-P/--redirect-ports`; `--isolated`/`--minimal` (mutually exclusive, Termux only); `--shared-home`/`--termux-home`; `--shared-tmp`; `--shared-x11`; `-b/--bind` append; `--no-link2symlink/--no-sysvipc/--no-kill-on-exit`; `--emulator`; `--kernel`; `--hostname`; `-w/--work-dir`; `-e/--env` append; `-d/--detach`; `--get-proot-cmd` | Resolves user via guest `/etc/passwd`, builds env, assembles proot argv, registers session, `execvpe`s proot. Takes a **shared** container lock held for the session lifetime. |
| `run` | `commands/run.py` | Same shared block as `login`, plus `--get-proot-cmd` | Reads `image_config.config.{Entrypoint,Cmd,WorkingDir}` from `manifest.json`; builds inner argv (args after `--` replace Cmd, append to Entrypoint); sets `args._run_inner` and calls `command_login`. Refuses an image with neither Entrypoint nor Cmd and no `--` args. |
| `ps` | `commands/ps.py` | `-q` store_true | Reads the session registry (`session.active_sessions`), prints PID/CONTAINER/TYPE/USER/UPTIME/COMMAND; `--quiet` prints PIDs to stdout one per line. |
| `kill` | `commands/kill.py` | `target` positional; `-s/--signal`; `--all` store_true; default signal `SIGTERM` | Teardown of a session's process tree, escalated TERM→QUIT→KILL. Bare decimal target is always a PID; otherwise a container name. `--all` and a target are mutually exclusive. |
| `remove` | `commands/remove.py` | `target` positional; `-i/--image`; `-a/--architecture` (requires `--image`); `-v`/`-q` mutually exclusive | Default deletes a container's directory tree; `--image` deletes a cached image's manifest entry plus now-unreferenced layer blobs. No confirmation prompt. |
| `rename` | `commands/rename.py` | `orig_name`, `new_name` positionals; `-q` | Renames `containers/<old>`→`containers/<new>` and rewrites proot l2s symlink targets under the new rootfs. Refuses identical names and an existing destination. Locks both names in sorted order. |
| `reset` | `commands/reset.py` | `container_name` positional; `-q` | Removes the rootfs and `shm/` store, then re-invokes `command_install` with the stored `image_ref`+`arch`. Requires an OCI `image_ref` in the manifest. |
| `backup` | `commands/backup.py` | `container_name` positional; `-o/--output`; `-c/--compress` `choices=[gzip,bzip2,xz,zstd,none]`; `-v`/`-q` mutually exclusive | Streams a TAR (`<name>/manifest.json` + `<name>/rootfs/...`) to a file or stdout. Compression inferred from extension unless `--compress`. See §7. |
| `restore` | `commands/restore.py` | `archive` optional positional; `-v`/`-q` mutually exclusive | Extracts a single-container backup from a file or stdin; auto-detects compression by magic bytes. See §7. |
| `clear-cache` | `commands/clear_cache.py` | `--orphan`; `--build-cache`; `-v`/`-q` mutually exclusive | Default empties the whole download cache; `--orphan` removes only unreferenced layer blobs; `--build-cache` removes the build index then sweeps. Refuses to run under another command's lock. |
| `copy` | `commands/copy.py` | `source`, `destination` positionals; `-m/--move`; `-r/--recursive`; `-v`/`-q` mutually exclusive | Copies/moves between host and `container:path`, preserving symlinks; merges into an existing destination tree like `cp -a`; `--move` refuses a populated dest dir. Locks each container involved. |
| `sync` | `commands/sync.py` | `source`, `destination` positionals; `-c/--checksum`; `-d/--delete`; `-v`/`-q` mutually exclusive | Recursive rsync-like sync comparing size+mtime by default, size+CRC32 with `--checksum`; `--delete` prunes destination extras. Never changes ownership; mode/timestamp preserved. |
| `build` | `commands/build.py` | `path` default `'.'`; `-f/--file`; `-t/--tag` append default `[]`; `--build-arg` append default `[]`; `-a/--architecture`; `--target`; `--emulator`; `-o/--output` append default `[]`; `--install-as`; `--no-cache`; `-v`/`-q` mutually exclusive | Parses a Dockerfile, runs the build engine, stores the image in the manifest cache and optionally writes an OCI tarball / installs as a container. proot required only if a RUN instruction exists. |
| `push` | `commands/push.py` | `image_ref` positional; `-a/--architecture`; `--allow-insecure`; `-q` | Reads manifest+blobs from the local cache and pushes to a registry; `:latest` appended if no tag; digest references refused; pre-flight refuses when no cached manifest exists. |
| `search` | `commands/search.py` | `query` positional; `-l/--limit` (string, validated: 1..`SEARCH_LIMIT_MAX`); `-q` | Queries Docker Hub and prints a NAME/DESCRIPTION/STARS/PULLS/OFFICIAL table (or stacked); `--quiet` prints bare names. Only Docker Hub is searchable. |

Command dispatch table `_COMMAND_HANDLERS` in `cli.py` maps every canonical name (incl. `help`) to its handler.

EVIDENCE: `parser.py` (all `_add_*` builders + `REQUIRED_ARGS`); each `commands/*.py` module read in full or in the relevant command function; `cli.py` `_COMMAND_HANDLERS`.

---

## 4. Container storage layout

### Directory tree
Runtime root `DATA_LOCATION = /data/data/com.termux/files/usr/var/lib/proot-distro` (from `--help`):

```
cache/
  oci_manifests/         # per-image manifest-cache entries (e.g. 88a920ed310ae632.json)
  oci_layers/            # layer blobs named by digest (e.g. sha256_d3531b7a...)
containers/
  <name>/
    manifest.json        # OCI image metadata (absent for plain tarballs)
    rootfs/              # extracted container filesystem
    sysdata/             # fake /proc & /sys stubs (guest-writable, 0700 dir)
    shm/                 # per-container /dev/shm store (mode 1777)
locks/
  <name>.lock            # one flock file per container/build
sessions/
  <pid>.json             # one registry record per live session
```

Observed with a real installed container (`alpine`):

```
$ ls -la containers/alpine/
-rw-------. manifest.json
drwxr-xr-x. rootfs
drwxrwxrwt. shm
drwx------. sysdata
```

Path helpers from `paths.py`: `container_dir(name)=CONTAINERS_DIR/name`, `container_rootfs(name)=container_dir/rootfs`, `container_manifest(name)=container_dir/manifest.json`.

`CONTAINERS_DIR = RUNTIME_DIR/containers` (`constants.py`). Legacy layout `RUNTIME_DIR/installed-rootfs/<name>` is migrated to `containers/<name>/rootfs` on first `login` (`commands/login/migrate.py`).

### `manifest.json` fields
Written by `install._run_install` as:
```python
manifest_data = {
    "image_ref":   "<ref>",        # "" for a plain tarball
    "arch":        "<proot arch>",
    "manifest":    <OCI image manifest dict>,
    "image_config":<OCI image config dict>,
}
```

Verbatim example — `containers/alpine/manifest.json`:

```json
{
  "image_ref": "alpine",
  "arch": "aarch64",
  "manifest": {
    "schemaVersion": 2,
    "mediaType": "application/vnd.oci.image.manifest.v1+json",
    "config": {
      "mediaType": "application/vnd.oci.image.config.v1+json",
      "digest": "sha256:f7bbaec1bd0330a8dba26f03e05f421bb3c8b807bb41873df3c3b0c3d1083d8f",
      "size": 627
    },
    "layers": [
      {
        "mediaType": "application/vnd.oci.image.layer.v1.tar+gzip",
        "digest": "sha256:d3531b7a4f217bdc3426c57bcff84c170f086f456a39689d4a615c6d026c4be0",
        "size": 4190488
      }
    ],
    "annotations": {
      "com.docker.official-images.bashbrew.arch": "arm64v8",
      "org.opencontainers.image.base.name": "scratch",
      "org.opencontainers.image.created": "2026-10-21T22:51:34Z",
      "org.opencontainers.image.revision": "5aec467fb81238bae9441eea2edb000655d7044f",
      "org.opencontainers.image.source": "https://github.com/alpinelinux/docker-alpine.git#...:aarch64",
      "org.opencontainers.image.url": "https://hub.docker.com/_/alpine",
      "org.opencontainers.image.version": "3.24.3"
    },
    "_ct": "application/vnd.oci.image.manifest.v1+json"
  },
  "image_config": {
    "architecture": "arm64",
    "config": {
      "Env": ["PATH=/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin"],
      "Cmd": ["/bin/sh"],
      "WorkingDir": "/"
    },
    "created": "2026-10-21T22:51:39.084385804Z",
    "history": [ { "created_by": "ADD alpine-minirootfs-3.24.3-aarch64.tar.gz / # buildkit", ... } ],
    "os": "linux",
    "rootfs": { "type": "layers", "diff_ids": ["sha256:a278e6f39431cd0ed74238a3d377893ad0d905d68b676bf9a65529080669756b"] },
    "variant": "v8"
  }
}
```

Accessed via: `read_container_manifest` → dict; `manifest_image_config` → `image_config.config`; `container_image_config` (forgiving) → same; `container_image_origin` → `(image_ref, arch)`.

### `sysdata/` stubs (fake `/proc`, `/sys`)
Files written by `sysdata.setup_fake_sysdata` into `containers/<name>/sysdata/` (dir mode 0700). Contents observed:

```
loadavg                           0.12 0.07 0.02 2/165 765
stat                              cpu  1954 0 2872 93276 ... / cpu0 ... / cpu1 ...
uptime                            124.08 932.76
version                           Linux version 6.17.0-PRoot-Distro (proot@termux) (gcc (GCC) 13.3.0, GNU ld (GNU Binutils) 2.42) #1 SMP PREEMPT_DYNAMIC Fri, 10 Oct 2025 00:00:00 +0000
vmstat                            nr_free_pages 1743136 ...
sysctl_entry_cap_last_cap         40
sysctl_inotify_max_user_watches   4096
sysctl_kernel_overflowuid         65534
sysctl_kernel_overflowgid         65534
sys_empty/                        (empty dir, bound at /sys/fs/selinux)
```

`stat` and `uptime` are regenerated to match the host's current CPU list; the rest are written once.

### Session registry record
Verbatim `sessions/11589.json`:
```json
{"pid": 11589, "container": "alpine", "kind": "login", "command": ["/bin/sh", "-c", "/bin/sh -c 'echo PD_OK; cat /etc/alpine-release'"], "user": "root", "start_time": 1793647891.2896338, "isolated": false, "minimal": false, "detach": false}
```

### Lock file
`locks/alpine.lock` held an flock; contents are `<pid> <command>` (`"15249 login\n"` observed).

EVIDENCE: `proot-distro --help` (DATA LOCATION); `ls -la`/`find` of `/data/data/com.termux/files/usr/var/lib/proot-distro/`; `cat containers/alpine/manifest.json`; `cat containers/alpine/sysdata/*`; `cat sessions/11589.json`; `cat locks/alpine.lock`; `paths.py` (`container_dir`, `container_rootfs`, `container_manifest`); `commands/install.py` (manifest_data); `constants.py` (`CONTAINERS_DIR`).

---

## 5. proot invocation details

Source: `proot_distro/commands/login/proot_cmd.py`. The argv is assembled in a fixed order (module header comment):

```
1. proot binary + emulator flags (when cross-arch).
2. Proot extensions on Termux (--kill-on-exit, --link2symlink,
   --sysvipc, --kernel-release, -L).
3. --change-id for non-termux containers.
4. --rootfs / --cwd / baseline /dev /proc /sys binds.
5. Optional non-minimal binds: /dev/random etc., Android storage,
   Android system paths, Termux $HOME / $PREFIX bridges.
6. User-supplied --bind entries (with overlap warning), then -p when
   ports are redirected.
7. Inner command (shell or run-mode inner argv).
```

### Exact proot flags passed
From `build_proot_args`, `_add_proot_extensions`, `_add_non_minimal_binds`, `_add_termux_dev_binds`, `_add_custom_binds`:

- `-q <emulator>` — only when cross-arch emulation is needed (first, before extensions). Emulator adds extra binds (§8).
- `--kill-on-exit` — appended unless `--no-kill-on-exit` (which instead warns). Termux only.
- `--link2symlink` — unless `dist_type=="termux"` or `--no-link2symlink`.
- `--sysvipc` — unless `--no-sysvipc` (and only when not minimal).
- `"--kernel-release=\\Linux\\<hostname>\\<kernel_release>\\<DEFAULT_FAKE_KERNEL_VERSION>\\<uname_m>\\localdomain\\-1\\"` — only when not minimal (`DEFAULT_FAKE_KERNEL_VERSION = "#1 SMP PREEMPT_DYNAMIC Fri, 10 Oct 2025 00:00:00 +0000"`; `uname_m` = `ARCH_UNAME_M[target_arch]`).
- `-L` — always (fix lstat for dpkg symlink warnings).
- `--change-id=<uid>:<gid>` — appended only when `dist_type != "termux"`.
- `--rootfs=<path>` — `--rootfs=.` for a real session (chdir into the pinned fd), the real path for `--get-proot-cmd`.
- `--cwd=<login_wd>`.
- `--bind=/dev`, `--bind=/proc`, `--bind=/sys` — the baseline binds, always.
- `-p` — appended when `--redirect-ports`.

`_add_proot_extensions` returns early on non-Termux hosts (`if not IS_TERMUX: return`), so the extension block is Termux-specific.

### Full bind list logic

**(a) Baseline (always):** `--bind=/dev --bind=/proc --bind=/sys`.

**(b) Non-minimal, non-termux, Termux host** — `_add_termux_dev_binds`:
```
--bind=/dev/urandom:/dev/random
--bind=/proc/self/fd:/dev/fd                     (only if /dev/fd absent)
--bind=/proc/self/fd/0:/dev/stdin                (per fd, only if /dev/<name> absent and target exists)
--bind=/proc/self/fd/1:/dev/stdout
--bind=/proc/self/fd/2:/dev/stderr
<fake_sysdata_bindings>                          (see below)
--bind=<containers/<name>/shm>:/dev/shm          (only if shm dir validates)
```
sysdata bindings emitted when the real `/proc` entry is unreadable on the host (`fake_sysdata_bindings`):
```
--bind=<container>/sysdata/sys_empty:/sys/fs/selinux
--bind=<container>/sysdata/loadavg:/proc/loadavg
--bind=<container>/sysdata/stat:/proc/stat
--bind=<container>/sysdata/uptime:/proc/uptime
--bind=<container>/sysdata/version:/proc/version
--bind=<container>/sysdata/vmstat:/proc/vmstat
--bind=<container>/sysdata/sysctl_entry_cap_last_cap:/proc/sys/kernel/cap_last_cap
--bind=<container>/sysdata/sysctl_inotify_max_user_watches:/proc/sys/fs/inotify/max_user_watches
--bind=<container>/sysdata/sysctl_kernel_overflowuid:/proc/sys/kernel/overflowuid
--bind=<container>/sysdata/sysctl_kernel_overflowgid:/proc/sys/kernel/overflowgid
```

**(c) Non-minimal, not isolated, Termux host** — `_add_dalvik_cache_binds` + `storage_bindings()` + `_add_termux_app_binds` (app binds only when `dist_type != "termux"`):
```
--bind=/data/app                                  (dalvik dirs, only if mode last digit in 1/5/7)
--bind=/data/dalvik-cache
--bind=/data/misc/apexdata/com.android.art/dalvik-cache
<storage_bindings()>
--bind=/data/data/com.termux/files/apps           (app binds; only if that dir exists)
--bind=/data/data/com.termux/cache
--bind=/data/data/com.termux/files/home
```
`storage_bindings()` (from `bindings.py`): if `/storage` readable →
```
--bind=/storage
--bind=/storage/emulated/0:/sdcard
--bind=/storage/emulated/0:/mnt/sdcard
```
else it probes `/storage/self/primary`, `/storage/emulated/0`, `/sdcard` and, for the first readable one `p`, emits:
```
--bind=<p>:/mnt/sdcard
--bind=<p>:/sdcard
--bind=<p>:/storage/emulated/0
--bind=<p>:/storage/self/primary
```

**(d) Non-minimal, Termux host, (not isolated OR emulating)** — `system_bindings()` + (if non-termux) the Termux prefix. `system_bindings()` (from `bindings.py`) iterates `/apex`, `/odm`, `/product`, `/system`, `/system_ext`, `/vendor`, `/linkerconfig/ld.config.txt`, `/linkerconfig/com.android.art/ld.config.txt`, `/plat_property_contexts`, `/property_contexts`; dirs must carry world-execute (mode 1/5/7) and files must be readable, else skipped:
```
--bind=/apex
--bind=/odm
--bind=/product
--bind=/system
--bind=/system_ext
--bind=/vendor
--bind=/linkerconfig/ld.config.txt
--bind=/linkerconfig/com.android.art/ld.config.txt
--bind=/plat_property_contexts                   (if present/readable)
--bind=/property_contexts                        (if present/readable)
--bind=/data/data/com.termux/files/usr            (TERMUX_PREFIX; non-termux only)
```

**(e) Shared dirs (any mode that reaches here):**
```
--shared-home:
    termux-type → --bind=$TERMUX_HOME:$TERMUX_HOME
    normal root → --bind=$TERMUX_HOME:/root
    normal user → --bind=$TERMUX_HOME:<login_home>
--shared-tmp (Termux, non-termux type) → --bind=$TERMUX_PREFIX/tmp:/tmp
--shared-x11 (Termux, non-termux type) → --bind=$TERMUX_PREFIX/tmp/.X11-unix:/tmp/.X11-unix
```

**(f) User `-b/--bind` entries** appended last (with overlap warning if a destination duplicates an existing one).

**Isolated mode** skips (c) entirely and (d) unless emulating; **minimal** skips the whole non-minimal block, so only `/dev /proc /sys` remain.

### Observed assembled command (`--get-proot-cmd`)
Default `proot-distro login alpine --get-proot-cmd -- true` produced (verbatim proot segment, binds elided where repeated above):

```
/data/data/com.termux/files/usr/bin/proot \
  --kill-on-exit \
  --link2symlink \
  --sysvipc \
  "--kernel-release=\\Linux\\localhost\\6.17.0-PRoot-Distro\\#1 SMP PREEMPT_DYNAMIC Fri, 10 Oct 2025 00:00:00 +0000\\aarch64\\localdomain\\-1\\" \
  -L \
  --change-id=0:0 \
  --rootfs=/data/data/com.termux/files/usr/var/lib/proot-distro/containers/alpine/rootfs \
  --cwd=/root \
  --bind=/dev \
  --bind=/proc \
  --bind=/sys \
  --bind=/dev/urandom:/dev/random \
  ...sysdata binds... \
  --bind=.../containers/alpine/shm:/dev/shm \
  --bind=/data/app \
  --bind=/data/dalvik-cache \
  --bind=/data/misc/apexdata/com.android.art/dalvik-cache \
  --bind=/storage/self/primary:/mnt/sdcard \
  --bind=/storage/self/primary:/sdcard \
  --bind=/storage/self/primary:/storage/emulated/0 \
  --bind=/storage/self/primary:/storage/self/primary \
  --bind=/data/data/com.termux/cache \
  --bind=/data/data/com.termux/files/home \
  --bind=/apex \
  --bind=/odm \
  --bind=/product \
  --bind=/system \
  --bind=/system_ext \
  --bind=/vendor \
  --bind=/linkerconfig/ld.config.txt \
  --bind=/linkerconfig/com.android.art/ld.config.txt \
  --bind=/data/data/com.termux/files/usr \
  /bin/sh \
  -c \
  true
```

`--minimal` reduced it to exactly: `--kill-on-exit --link2symlink -L --change-id=0:0 --rootfs=... --cwd=/root --bind=/dev --bind=/proc --bind=/sys /bin/sh -c true` (no `--sysvipc`, no `--kernel-release`, no extra binds).

`--isolated` kept `--sysvipc`+`--kernel-release`+`-L` and the `/dev /proc /sys` + sysdata + shm binds, but dropped all Android/Termux app/storage/system binds.

`-u nobody` produced `--change-id=65534:65534`, `--cwd=/`, and inner `/sbin/nologin -c true` (the user's shell).

EVIDENCE: `commands/login/proot_cmd.py`; `commands/login/bindings.py`; `sysdata.py` (`fake_sysdata_bindings`); `shm.py`; `proot-distro login alpine --get-proot-cmd -- true` plus the `--isolated`, `--minimal`, `-u nobody` variants.

---

## 6. Environment variables set inside the guest

Captured from `env -i ... proot ...` in the `--get-proot-cmd` output. The child env is built by `_build_normal_env` / `_build_termux_env` (`commands/login/__init__.py`) and passed to `execvpe`.

### Default mode, normal-type container, root (from get-proot-cmd)
```
PATH=/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin:/data/data/com.termux/files/usr/bin
MOZ_FAKE_NO_SANDBOX=1
MOZ_SHM_NO_SEALS=1
PULSE_SERVER=127.0.0.1
PD_CONTAINER=alpine
container=proot-distro
PD_IMAGE=alpine
PD_IMAGE_ID=sha256:f7bbaec1bd0330a8dba26f03e05f421bb3c8b807bb41873df3c3b0c3d1083d8f
ANDROID_ART_ROOT=/apex/com.android.art
ANDROID_DATA=/data
ANDROID_I18N_ROOT=/apex/com.android.i18n
ANDROID_ROOT=/system
ANDROID_TZDATA_ROOT=/apex/com.android.tzdata
BOOTCLASSPATH=/apex/com.android.art/javalib/core-oj.jar:... (long host value)
DEX2OATBOOTCLASSPATH=/apex/com.android.art/javalib/core-oj.jar:... (host value)
EXTERNAL_STORAGE=/sdcard
HOME=/root
USER=root
TERM=xterm-256color
COLORTERM=truecolor
PROOT_L2S_DIR=/data/data/com.termux/files/usr/var/lib/proot-distro/containers/alpine/rootfs/.l2s
```

### `--isolated`
Same identity + `HOME=/root`, `USER=root`, `TERM`, `COLORTERM`, `PROOT_L2S_DIR`, `MOZ_FAKE_NO_SANDBOX/MOZ_SHM_NO_SEALS/PULSE_SERVER` — but **no** `ANDROID_*`/`BOOTCLASSPATH`/`EXTERNAL_STORAGE`, and PATH is the plain image/baseline `PATH=/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin` (no Termux bin).

### `--minimal`
Only `PATH` (plain), `PD_CONTAINER`, `container`, `PD_IMAGE`, `PD_IMAGE_ID`, `TERM`, `COLORTERM`, `PROOT_L2S_DIR`. No `HOME`, no `USER`, no Android vars, no Mozilla/Pulse hints.

### Non-root (`-u nobody`)
`HOME=/`, `USER=nobody`, PATH gains Termux bin, Android vars present (default mode).

### Env sources and rules (`commands/login/env.py`)
- **Identity vars** (`identity_env_pairs`): `PD_CONTAINER` (always = container name), `container=proot-distro` (constant `CONTAINER_MARKER`), and — read from `manifest.json` — `PD_IMAGE` (the `image_ref`) and `PD_IMAGE_ID` (the image config digest). `PD_IMAGE`/`PD_IMAGE_ID` are absent for a plain-tarball/legacy container.
- **Image Env**: `image_path_env_pairs` harvests `manifest.json` `image_config.config.Env` entries, but blocks `IMAGE_ENV_BLOCKED` = {all `ANDROID_HOST_ENV_VARS` (ANDROID_ART_ROOT, ANDROID_DATA, ANDROID_I18N_ROOT, ANDROID_ROOT, ANDROID_RUNTIME_ROOT, ANDROID_TZDATA_ROOT, BOOTCLASSPATH, DEX2OATBOOTCLASSPATH, EXTERNAL_STORAGE), the identity keys, MOZ_FAKE_NO_SANDBOX, MOZ_SHM_NO_SEALS, PULSE_SERVER, TERM, COLORTERM} plus any `LD_*`/`PROOT_*` names. An image cannot override these; user `-e/--env` can.
- **Android host vars** inherited only in default mode (not isolated, not minimal).
- **`--env VAR=VALUE`** user entries override everything except the final Termux-bin PATH append.
- **PROOT_* passthrough**: `PROOT_NO_SECCOMP`, `PROOT_VERBOSE` are forwarded if set (non-minimal only). `PROOT_L2S_DIR` is always pinned to `<rootfs>/.l2s` for non-termux Termux containers.
- **profile.d snippet**: every normal-type session writes `/etc/profile.d/termux-profile.sh` re-exporting these vars so `su - <user>` in the guest keeps them.

EVIDENCE: `proot-distro login alpine [--isolated|--minimal|-u nobody] --get-proot-cmd -- true` (env block); `commands/login/env.py` (`ANDROID_HOST_ENV_VARS`, `IMAGE_ENV_BLOCKED`, `identity_env_pairs`, `_PROFILE_SNIPPET`, `inject_termux_profile`); `commands/login/__init__.py` (`_build_normal_env`, `_build_termux_env`).

---

## 7. Backup / restore implementation

### `backup` (`commands/backup.py`)
- **Archive structure:** `<name>/manifest.json` (optional, legacy containers have none) followed by `<name>/rootfs/...`. Written by `_each_entry`.
- **Tar writers:**
  - file output: `tarfile.open(path, mode=f'w:{comp}')`
  - stdout output: `tarfile.open(fileobj=sys.stdout.buffer, mode=f'w|{comp}')`
  - zstd file output uses a special `compress.open_tar_writer(...)` (tarfile's `w|zst` refuses a level).
  - `comp` ∈ `{'gz','bz2','xz','zst',''}`.
- **Compression mapping** `_COMPRESS_EXTS` (by extension): `.tar.gz/.tgz`→gz, `.tar.bz2/.tbz2`→bz2, `.tar.xz/.txz/.tar.lzma/.tlzma`→xz, `.tar.zst/.tzst`→zst, `.tar`→none. `_UNSUPPORTED_EXTS = ('.tar.lz4', '.tar.lz')` are rejected. `_COMPRESSION_ARG_MAP`: gzip→gz, bzip2→bz2, xz→xz, zstd→zst, none→''.
- **Permission handling:** three passes over the tree, all descriptor-relative: `_fix_permissions` (chmod owner-readable/executable on the way in via `dirfd.chmod_at`, skipping regular files with `st_nlink != 1`), `_measure`, then the archive pass. Refuses to run with `--output` on an existing file or a directory, and refuses to print to a TTY without `--output`.
- **Ownership stripped:** `_strip_owner` sets `info.uid=0, gid=0, uname='', gname=''` on every member.
- **What is included:** manifest.json (regular file only) + every rootfs entry.
- **What is excluded/skipped:** `.l2s/` at the rootfs top level (its backing files are inlined into their referring symlinks); block/char devices, FIFOs and sockets (`_add_path` returns early); `.l2s` symlinks resolved to the backing file's content and packed as regular files (self-contained archive); other symlinks stored as symlinks, not followed.
- **Hard links:** same-inode files become hard-link members via `tf.gettarinfo(fileobj=fh)`; multiple l2s names sharing a backing file become independent regular files.
- **stdout discipline:** progress goes to stderr; archive bytes to stdout.

### `restore` (`commands/restore.py`)
- **Tar readers:** file input: `tarfile.open(fileobj=<ByteCounter>, mode='r|*')` (auto-detect). stdin input: magic-byte sniff (`_MAGIC_COMPRESS`) then `tarfile.open(fileobj=sys.stdin.buffer, mode=f'r|{comp}')`.
- **Magic detection** `(: gz \x1f\x8b, bz2 BZh, xz \xfd7zXZ\x00, lzma 5d00→xz, zstd ZSTD_MAGIC)`. A zstd archive on an interpreter without zstd is named as such before opening.
- **Accepted layouts:** `<name>/manifest.json`, `<name>/rootfs/...`, legacy `installed-rootfs/<name>/...` (re-rooted to `rootfs`), and very-old `<name>/<other>` treated as inside rootfs. Members with `..`/`.`/empty components are rejected (path traversal blocked); member with no subdir (bare name) rejected.
- **Single container only:** the first valid member fixes the target and takes an exclusive lock; a member naming a different container aborts.
- **Included/excluded:** only `manifest.json` (buffered, capped at `_MAX_MANIFEST_BYTES = 16 MiB`, written only on success) and rootfs content. Block/char/FIFO members skipped. Hardlink members copied (`_write_hardlink` via `dirfd.copy_data`); symlinks recreated; dirs created (`_write_directory` defers owner-rwx modes until the end); regular files written through `dirfd.open_new_at` (O_EXCL, fresh inode — never through a hardlink).
- **Permission handling:** directories whose archived mode lacks owner rwx are temporarily widened `mode | S_IRWXU` and the final mode reapplied in reverse insertion order after extraction; regular files get `fchmod(member.mode)`.
- **Destructive clear deferred:** `_clear_existing_rootfs` runs only at the first materialised rootfs member; shm store is removed too; a rootfs-less archive leaves the target untouched, and a broken (non-directory) rootfs result is removed.
- All writes are descriptor-relative (`(dir_fd, name)` via `dirfd.descend_at`); final components are not dereferenced.

EVIDENCE: `commands/backup.py`; `commands/restore.py`; `compress.py` (`open_tar_writer`, `ZSTD_AVAILABLE`); `shm.py` (`SHM_DIR_NAME`).

---

## 8. User-visible limitations and hard-coded constraints

**Nested-proot refusal** — `cli._refuse_nested_proot()` reads `/proc/<pid>/status`; if `TracerPid` points at a process whose `Name:` contains `proot`, it errors and exits 1:
```
attempted to run proot-distro in a proot session. Please check your system configuration to ensure that this program does not run under any other proot instance. Additionally check the target container to ensure it does not invoke proot-distro on a loop basis. With 100% confidence this is not a mistake. Do not send bug reports!
```

**Container-name validation** — `names.py`:
```python
_NAME_RE = re.compile(r"^[A-Za-z0-9][A-Za-z0-9_.\-]*\Z")
NAME_RULE_HINT = ("It must begin with a letter or digit and contain only letters, "
                  "digits, underscores, dots, or hyphens.")
```
Enforced by `require_valid_name` in install, remove, rename, reset, login, run, backup, restore, kill.

**Architecture detection / emulation** — `arch.py`:
- `get_device_cpu_arch()` collapses `armv7l`/`armv8l` to `arm`.
- `detect_installed_arch()` reads ELF `e_machine` from the first usable of `/usr/bin/bash, /usr/bin/sh, /usr/bin/su, /usr/bin/busybox, $PREFIX/bin/bash, /bin/bash, /bin/sh, /bin/su, /bin/busybox`; `_ELF_MACHINE_MAP` covers i686/arm/x86_64/aarch64/riscv64; else `"unknown"`.
- `_KNOWN_ARCHS = {aarch64, arm, i686, riscv64, x86_64}`; `normalize_arch` also accepts `linux/`-prefixed Docker strings and bare `arm64/amd64/386/arm/v7`.
- `get_emulator_args`: native when archs match; aarch64 host runs `arm` natively iff `supports_32bit()` (personality `PER_LINUX32` probe), x86_64 runs `i686` natively; otherwise requires `qemu-<arch>` on PATH, else errors naming the missing `qemu-user-*` package. Emulator adds binds `/apex`, `/linkerconfig/ld.config.txt`, `$PREFIX`, `/system`, `/vendor`, `/plat_property_contexts`, `/property_contexts` when present.
- Termux-type containers **cannot be emulated** (error, exit 1).
- `PD_PROOT_BIN` override is validated (must be an existing executable file).

**`build` proot requirement** — only RUN-family instructions need proot; a Dockerfile of FROM/COPY/ADD/ENV/ARG/LABEL/USER/WORKDIR/CMD/ENTRYPOINT/EXPOSE/VOLUME/STOPSIGNAL/HEALTHCHECK/SHELL/MAINTAINER/ONBUILD<non-RUN> builds in pure Python. `--no-cache` disables step caching. BuildKit-only directives (`RUN --mount/--network/--security`, `COPY --link/--parents`) are rejected. No multi-platform manifest lists.

**`restore` single-container limit** — exactly one container per archive; more than one or a rootfs-less archive is rejected. Legacy `installed-rootfs/<name>` layout is accepted.

**`search` scope** — Docker Hub only; custom registries cannot be queried. Not part of the OCI distribution protocol. Private repos never appear even with `PD_DOCKER_AUTH` configured. `--limit` range 1..1000.

**`backup` stdout** — refuses to write archive bytes to a TTY without `--output`.

**Guest-access limits (from the `login --help` NOTES)** — cannot deliver restricted `/dev`, `/proc`, `/sys` interfaces required by udev, or cgroups/namespaces required by bwrap. ARMv9 CPUs need QEMU for 32-bit programs.

**Isolated/minimal semantics** — minimal disables all proot extensions except link2symlink, no `/proc` workarounds, no kernel-release override. `--shared-home/-tmp/-x11` and `-b` take priority over isolated; isolated disables storage/app/system binds unless emulating.

**Default constants** — `constants.py`: `DEFAULT_FAKE_KERNEL_RELEASE = "6.17.0-PRoot-Distro"`; `DEFAULT_FAKE_KERNEL_VERSION = "#1 SMP PREEMPT_DYNAMIC Fri, 10 Oct 2025 00:00:00 +0000"`; `DEFAULT_PATH_ENV` (Termux) ends with `:$PREFIX/bin:/system/bin:/system/xbin`; DNS defaults `8.8.8.8` / `8.8.4.4`.

**Troubleshooting knobs** (from `--help`): `PD_FORCE_NO_COLORS=true` disables colors; `PD_DOCKER_AUTH="user:password"` supplies private-registry credentials.

EVIDENCE: `cli.py` (`_refuse_nested_proot`); `names.py` (`_NAME_RE`, `require_valid_name`); `arch.py` (`_ELF_MACHINE_MAP`, `_KNOWN_ARCHS`, `_DOCKER_TO_PROOT`, `get_emulator_args`, `get_proot_bin`); `commands/login/__init__.py` (termux-emulation refusal); `commands/build.py` header + `--help` (PROOT REQUIREMENT / LIMITATIONS); `commands/restore.py`; `commands/search.py`; `constants.py`; `proot-distro --help` (TROUBLESHOOTING).
