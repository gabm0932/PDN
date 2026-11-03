# PDN (podnext) v0.1.7 — User-Facing Capability Inventory

Evidence-by-log inventory. Every claim cites observed command output, file path, or line number.
Repo: `/data/data/com.termux/files/home/workspace/PDN`. Binary: `target/release/podnext`.
Investigation time: 2026-11-03 ~02:30–02:50 WIB.
Anything not directly observed is marked **UNVERIFIED**.

---

## 1. Binary identity

| property | value |
|---|---|
| SHA-256 | `7eed257523cbefe03b02f015cdfa01f127807d4632e3d7e2cf3cec343e3cc5c0` |
| size | `15734184` bytes (16M) |
| ELF | `ELF 64-bit LSB pie executable, ARM aarch64, version 1 (SYSV), dynamically linked, interpreter /system/bin/linker64, for Android 24, built by NDK r30 (16248370), not stripped` |
| mtime | `2026-11-03 02:08:46` |
| workspace version | `0.1.7` (`Cargo.toml` `[workspace.package] version = "0.1.7"`) |
| `podnext --version` | `podnext 0.1.7` |
| `podnext version` | `podnext 0.1.0`  ← MISMATCH |

```
$ sha256sum target/release/podnext
7eed257523cbefe03b02f015cdfa01f127807d4632e3d7e2cf3cec343e3cc5c0  target/release/podnext

$ file target/release/podnext
target/release/podnext: ELF 64-bit LSB pie executable, ARM aarch64, version 1 (SYSV),
dynamically linked, interpreter /system/bin/linker64, for Android 24, built by NDK r30
(16248370), not stripped

$ ./target/release/podnext --version
podnext 0.1.7

$ ./target/release/podnext version
podnext 0.1.0
```

`crates` workspace members (6): `proot-core`, `proot-distro-core`, `proot-compat`, `pn-gateway`, `pn-next`, `podnext`.

**EVIDENCE:** `sha256sum`, `file`, `ls -l` on `target/release/podnext`; `./target/release/podnext --version` / `version`; `Cargo.toml`.

### 1a. The version-string mismatch (root cause) — IMPORTANT, concurrent edit

The release **binary** was built `02:08:46`, before the source fix (below). It emits `0.1.0`
because it was compiled from source that hardcoded the string. During this investigation the
source was edited live (mtimes `02:41`/`02:45`), removing the hardcodes.

Observed hardcoded `"0.1.0"` at session start (grep of `crates/podnext/src`):

| file:line | content (at ~02:35) | function |
|---|---|---|
| `crates/podnext/src/main.rs:1853` | `version: "0.1.0".to_string(),` | `cmd_info` `SystemInfo` |
| `crates/podnext/src/main.rs:1889` | `println!("podnext 0.1.0 - PRoot Distro Next for Termux/Android")` | `cmd_info` Text branch |
| `crates/podnext/src/main.rs:2221` | `version: "0.1.0".to_string(),` | `cmd_version` `VersionInfo` |
| `crates/podnext/src/main.rs:2231` | `println!("podnext 0.1.0")` ← the `podnext version` output | `cmd_version` Text branch |
| `crates/podnext/src/native.rs:48` | `.header("User-Agent", "podnext/0.1.0")` | Docker Hub search UA |

Current source state (~02:41–02:45, `main.rs` mtime `2026-11-03 02:41:04`, `native.rs` `02:45:16`) — all five now use `env!("CARGO_PKG_VERSION")`:

```
1853:        version: env!("CARGO_PKG_VERSION").to_string(),
1889:        OutputFormat::Text => println!("podnext {} - PRoot Distro Next for Termux/Android", env!("CARGO_PKG_VERSION")),
2221:        version: env!("CARGO_PKG_VERSION").to_string(),
2231:        OutputFormat::Text => println!("podnext {}", env!("CARGO_PKG_VERSION")),
native.rs:48:        .header("User-Agent", format!("podnext/{}", env!("CARGO_PKG_VERSION")))
```

`grep -rn '0\.1\.0' crates/podnext/src/` now returns **no matches**.

Remaining hardcoded `"0.1.0"` elsewhere (NOT the CLI version command; may surface in API/dashboard):

```
crates/pn-gateway/src/config.rs:29:            version: "0.1.0".to_string(),
crates/pn-next/src/lib.rs:147:            version: "0.1.0".to_string(),
crates/pn-next/src/plugin.rs:455:        "0.1.0"
crates/pn-next/src/web_dashboard/mod.rs:351:        "version": "0.1.0"
crates/pn-next/src/cluster/mod.rs:448:            version: "0.1.0".to_string(),
```

**Conclusion:** the mismatch is a **stale binary**, not a live source hardcode. Source was fixed
mid-investigation; the release binary was not rebuilt. `cargo build --release` should make
`version`/`info` print `0.1.7`. **UNVERIFIED** whether rebuild is complete (binary mtime still `02:08`).

**EVIDENCE:** `grep -n '0\.1\.0'` / `grep -n 'CARGO_PKG_VERSION' crates/podnext/src/main.rs`; `sed -n` of lines 1853/1889/2221/2231 and native.rs:44–52; `ls -l --time-style=full-iso`; live runs of `version`/`info`/`--version`.

---

## 2. Full command tree (top-level)

`podnext --help` (verbatim, full):

```
$ ./target/release/podnext --help
podnext (PRoot Distro Next) - Advanced container & service management for Termux/Android.

CONTAINER MANAGEMENT:
podnext install ubuntu:22.04              # Install Ubuntu 22.04
podnext login ubuntu                      # Interactive shell
podnext run ubuntu -- apt update          # Run command in container
podnext list                              # List containers
podnext remove ubuntu                     # Delete container

SERVICE ORCHESTRATION (v2.0):
podnext service start web-server          # Start managed service
podnext service stop web-server           # Stop managed service
podnext service restart web-server        # Restart service
podnext service list                      # List all services
podnext service logs web-server           # View service logs
podnext service status web-server         # Show service status
podnext service enable web-server         # Enable auto-start at boot
podnext service disable web-server        # Disable auto-start

DAEMON MANAGEMENT:
podnext daemon start                      # Start service daemon
podnext daemon stop                       # Stop service daemon
podnext daemon status                     # Show daemon status
podnext daemon reload                     # Reload configuration

WATCHDOG & MONITORING:
podnext watchdog status                   # Show watchdog status
podnext watchdog policy set web-server OnFailure 3 60
podnext watchdog reset web-server         # Reset watchdog state

RESOURCE QUOTAS:
podnext quota set web-server memory 512M  # Set memory limit
podnext quota set web-server cpu 200      # Set CPU quota (2 cores)
podnext quota get web-server              # Show quotas
podnext quota usage web-server            # Show current usage

STRUCTURED LOGGING:
podnext log query web-server --level Error --limit 50
podnext log tail web-server               # Follow logs
podnext log stats                         # Log statistics

D-BUS & IPC:
podnext dbus list                         # List D-Bus services
podnext dbus call org.pnnext.Service /org/pnnext/Service org.pnnext.Service.List

SOCKET ACTIVATION:
podnext socket register --config socket.toml
podnext socket list

TERMUX:BOOT:
podnext boot enable web-server            # Enable auto-start at boot
podnext boot disable web-server           # Disable auto-start
podnext boot status                       # Show boot configuration

WAKE LOCKS:
podnext wakelock acquire web-server Partial
podnext wakelock release web-server
podnext wakelock status

POLICY ENGINE:
podnext policy eval web-server --json
podnext policy list

DIRECT EXECUTION (like proot):
podnext -r /path/to/rootfs -- /bin/bash     # Execute in proot environment
podnext -b /host/path:/guest/path -- ls     # Bind mount and execute


Usage: podnext [OPTIONS] [COMMAND]

Commands:
  install     Install distribution from OCI image
  list        List created containers
  login       Start interactive shell inside a container
  run         Run command in container
  ps          List active sessions
  kill        Stop container
  remove      Delete a container
  search      Search images on Docker Hub
  image       Manage images
  sessions    List active sessions (better formatted)
  attach      Attach to a container session (creates new session)
  info        Show system information
  version     Show version
  service     Service management commands
  daemon      Daemon management
  watchdog    Watchdog management
  quota       Resource quota management
  log         Structured logging
  dbus        D-Bus IPC
  socket      Socket activation
  boot        Termux:Boot integration
  wakelock    Wake lock management
  policy      Policy engine
  backup      Backup/Restore management
  build       Multi-arch build service
  gitops      GitOps integration
  dashboard   Web dashboard management
  network     Network management
  volume      Volume management
  completion  Generate shell completions
  help        Print this message or the help of the given subcommand(s)

Options:
  -r, --rootfs <ROOTFS>
          Use PATH as guest root filesystem
  -b, --bind <HOST_PATH:GUEST_PATH>
          Bind mount host path into guest (repeatable)
  -w, --pwd <PWD>
          Set initial working directory
  -q, --qemu <QEMU>
          Execute through QEMU user-mode emulation (path to qemu binary)
  -0, --link2symlink
          Enable hardlink emulation via symlinks
      --no-sysvipc / --sysvipc / --kernel <KERNEL> / --hostname <HOSTNAME>
      --uid <UID> / --gid <GID> / --seccomp / --no-seccomp / --trace
      --kill-on-exit / --no-kill-on-exit / -e, --env <VAR=VALUE> / -v, --verbose...
  -h, --help / -V, --version
```

31 subcommands (30 + `help`). Below: usage line + command-specific flags for each.
The following **GLOBAL block** is repeated verbatim under *every* subcommand and is not re-quoted:

```
GLOBAL (every command --help):
  -r, --rootfs <ROOTFS>              Use PATH as guest root filesystem
  -b, --bind <HOST_PATH:GUEST_PATH>  Bind mount host path into guest (repeatable)
  -w, --pwd <PWD>                    Set initial working directory
  -q, --qemu <QEMU>                  Execute through QEMU user-mode emulation (path to qemu binary)
  -0, --link2symlink                 Enable hardlink emulation via symlinks
      --no-sysvipc                   Disable System V IPC emulation
      --sysvipc                      Enable System V IPC emulation
      --kernel <KERNEL>              Override kernel release string
      --hostname <HOSTNAME>          Set hostname
      --uid <UID>                    User ID to run as
      --gid <GID>                    Group ID to run as
      --seccomp                      Enable seccomp filtering
      --no-seccomp                   Disable seccomp filtering
      --trace                        Enable syscall tracing
      --kill-on-exit                 Kill processes on exit
      --no-kill-on-exit              Don't kill processes on exit
  -e, --env <VAR=VALUE>              Set environment variable (repeatable)
  -v, --verbose...                   Enable verbose debug output (repeatable)
  -h, --help                         Print help
```

### install
```
Usage: podnext install [OPTIONS] <IMAGE>
Arguments:
  <IMAGE>  Image reference (e.g., ubuntu:22.04, debian:12, alpine:latest)
Options:
  -n, --name <NAME>   Container name (defaults to image name without tag)
  -f, --force         Force reinstall if container exists
```

### list
```
Usage: podnext list [OPTIONS]
Options:
  -a, --all                          Show all (including stopped)
      --no-all                       Hide stopped containers
  -f, --format <FORMAT>              [default: table] [possible values: table, json, yaml, text]
```

### login
```
Usage: podnext login [OPTIONS] <CONTAINER> [-- <COMMAND>...]
Arguments:
  <CONTAINER>   Container name
  [COMMAND]...  Command to run instead of shell (after --)
Options:
  -u, --user <USER>                  User to run as
```

### run
```
Usage: podnext run [OPTIONS] <CONTAINER> [-- <COMMAND>...]
Arguments:
  <CONTAINER>   Container name
  [COMMAND]...  Command to run (after --)
Options:
  -r, --rm                           Automatically remove the container when it exits
  -d, --detach                       Run container in background and print container ID
      --name <NAME>                  Assign a name to the container
      --network <NETWORK>            Connect a container to a network
  -v, --volume <HOST_PATH:CONTAINER_PATH[:ro]>   Bind mount a volume
  -p, --publish <HOST_PORT:CONTAINER_PORT[/PROTOCOL]>  Publish port(s)
      --entrypoint <ENTRYPOINT>      Overwrite the default ENTRYPOINT of the image
  -t, --tty                          Allocate a pseudo-TTY
  -i, --interactive                  Keep STDIN open even if not attached
  -u, --user <USER>                  User to run as (format: <name|uid>[:<group|gid>])
  -w, --workdir <WORKDIR>            Working directory inside the container
```
NOTE: `-r` is declared twice in `run` (`--rm` and global `--rootfs`) — see §8.

### ps
```
Usage: podnext ps [OPTIONS]
Options:
  -f, --format <FORMAT>              [default: table] [possible values: table, json, yaml, text]
```

### kill
```
Usage: podnext kill [OPTIONS] <CONTAINER>
Arguments:
  <CONTAINER>  Container name
```

### remove
```
Usage: podnext remove [OPTIONS] <NAME>
Arguments:
  <NAME>  Container name
Options:
  -f, --force                        Force remove without confirmation
```

### search
```
Usage: podnext search [OPTIONS] <QUERY>
Arguments:
  <QUERY>  Search query
Options:
  -l, --limit <LIMIT>                [default: 25]
  -f, --format <FORMAT>              [default: table] [possible values: table, json, yaml, text]
```

### info
```
Usage: podnext info [OPTIONS]
Options:
  -f, --format <FORMAT>              [default: table] [possible values: table, json, yaml, text]
```

### version
```
Usage: podnext version [OPTIONS]
Options:
  -f, --format <FORMAT>              [default: table] [possible values: table, json, yaml, text]
```
(Implied — `version --format json` observed; `version` with no args prints text.)

### sessions
```
Usage: podnext sessions [OPTIONS]
Options:
  -f, --format <FORMAT>              [default: table] [possible values: table, json, yaml, text]
```

### attach
```
Usage: podnext attach [OPTIONS] <CONTAINER> [COMMAND]...
Arguments:
  <CONTAINER>   Container name
  [COMMAND]...  Command to run instead of shell (after --)
Options:
  -u, --user <USER>                  User to run as
```

### completion
```
Usage: podnext completion [OPTIONS] <SHELL>
Arguments:
  <SHELL>  Shell to generate completions for [possible values: bash, zsh, fish, power-shell, elvish]
```

**EVIDENCE:** `./target/release/podnext --help` and `./target/release/podnext <cmd> --help` for install, list, login, run, ps, kill, remove, search, info, version, sessions, attach, completion.

---

## 3. Subcommand families (with their own subcommands)

### image — `podnext image [OPTIONS] <COMMAND>`
```
Commands:
  pull     Pull an image from a registry
  push     Push an image to a registry
  rm       Remove one or more images
  tag      Tag an image
  history  Show image history
  inspect  Display detailed information on one or more images
  ls       List images
  help     Print this message or the help of the given subcommand(s)
```
```
image pull    Usage: podnext image pull [OPTIONS] <IMAGE>
              --platform <PLATFORM>     Platform (e.g., linux/arm64, linux/amd64)
              --alltags                 Download all tagged images
image push    Usage: podnext image push [OPTIONS] <IMAGE>     (no own flags)
image rm      Usage: podnext image rm [OPTIONS] [IMAGES]...   -f, --force
image tag     Usage: podnext image tag [OPTIONS] <SOURCE> <TARGET>   (no own flags)
image history Usage: podnext image history [OPTIONS] <IMAGE>
              --notrunc; -f, --format <FORMAT> [default: table]
image inspect Usage: podnext image inspect [OPTIONS] [IMAGES]...    -f, --format <FORMAT> [default: json]
image ls      Usage: podnext image ls [OPTIONS]   -f,--format; -a,--all
```

### service — `podnext service [OPTIONS] <COMMAND>`
```
Commands:
  start      Stop/Start/Restart/List/Status/Logs/Enable/Disable/Load/Create
  stop       ...
  restart    ...
  list       ...
  status     ...
  logs       ...
  enable     Enable auto-start at boot
  disable    Disable auto-start at boot
  load       Load service from TOML config
  create     Create service from TOML config
  help       Print this message or the help of the given subcommand(s)
```
```
service start    Usage: podnext service start [OPTIONS] <NAME>     (no own flags)
service stop     Usage: podnext service stop [OPTIONS] <NAME>      -f, --force (SIGKILL)
service restart  Usage: podnext service restart [OPTIONS] <NAME>   (no own flags)
service list     Usage: podnext service list [OPTIONS]             -f, --format
service status   Usage: podnext service status [OPTIONS] <NAME>    -f, --format
service logs     Usage: podnext service logs [OPTIONS] <NAME>      -f, --follow ; -l, --lines [default: 100]
service enable   Usage: podnext service enable [OPTIONS] <NAME>    (no own flags)
service disable  Usage: podnext service disable [OPTIONS] <NAME>   (no own flags)
service load     Usage: podnext service load [OPTIONS] <NAME>      (no own flags)
service create   Usage: podnext service create [OPTIONS] --config <CONFIG> <NAME>   -c, --config <CONFIG>
```

### daemon — `podnext daemon [OPTIONS] <COMMAND>`
```
Commands:
  start   Start daemon
  stop    Stop daemon
  status  Show daemon status
  reload  Reload daemon configuration
  logs    Show daemon logs
  help
```
```
daemon start   Usage: podnext daemon start [OPTIONS]     (no own flags)
daemon stop    Usage: podnext daemon stop [OPTIONS]      (no own flags)
daemon status  Usage: podnext daemon status [OPTIONS]    -f, --format
daemon reload  Usage: podnext daemon reload [OPTIONS]    (no own flags)
daemon logs    Usage: podnext daemon logs [OPTIONS]      -f, --follow ; -l, --lines [default: 100]
```

### watchdog — `podnext watchdog [OPTIONS] <COMMAND>`
```
Commands:
  status     Show watchdog status
  setpolicy  Set watchdog policy for a service
  reset      Reset watchdog state for a service
  events     Show watchdog events
  help
```
```
watchdog status     Usage: podnext watchdog status [OPTIONS]   -f, --format
watchdog setpolicy  Usage: podnext watchdog setpolicy [OPTIONS] <NAME> <POLICY> <MAX_RESTARTS> <WINDOW_SECS>
                    <POLICY> [possible values: never, on-failure, always, unless-stopped, on-failure-or-crash]
watchdog reset      Usage: podnext watchdog reset [OPTIONS] <NAME>   (no own flags)
watchdog events     Usage: podnext watchdog events [OPTIONS]  -l, --limit [default: 50]
```
NOTE: top-level long_about advertises `podnext watchdog policy set ...`; actual subcommand is `setpolicy` (§8).

### quota — `podnext quota [OPTIONS] <COMMAND>`
```
Commands:
  set    Set resource quota
  get    Get resource quotas
  usage  Show current resource usage
  list   List all quotas
  help
```
```
quota set    Usage: podnext quota set [OPTIONS] <NAME> <RESOURCE> <LIMIT>
             <RESOURCE> [possible values: cpu, memory, memory-swap, io-read-bps, io-write-bps,
                         io-read-iops, io-write-iops, pids, nofile, network]
             <LIMIT>    Limit value (e.g., 512M, 200, 1000)
quota get    Usage: podnext quota get [OPTIONS] <NAME>      -f, --format
quota usage  Usage: podnext quota usage [OPTIONS] <NAME>    -f, --format
quota list   Usage: podnext quota list [OPTIONS]            -f, --format
```

### log — `podnext log [OPTIONS] <COMMAND>`
```
Commands:
  query  Query logs
  tail   Tail logs (follow)
  stats  Show log statistics
  flush  Flush log buffer
  help
```
```
log query  Usage: podnext log query [OPTIONS]
           -s, --service <SERVICE> ; --minlevel <MIN_LEVEL> ; --maxlevel <MAX_LEVEL>
           --grep <GREP> ; -l, --limit <LIMIT> [default: 100] ; -f, --format
           <LEVEL> [possible values: emergency, alert, critical, error, warning, notice, info, debug]
log tail   Usage: podnext log tail [OPTIONS]   -s, --service <SERVICE> ; -l, --lines [default: 100]
log stats  Usage: podnext log stats [OPTIONS]             -f, --format
log flush  Usage: podnext log flush [OPTIONS]             (no own flags)
```

### dbus — `podnext dbus [OPTIONS] <COMMAND>`
```
Commands:
  list     List D-Bus services
  call     Call D-Bus method
  monitor  Monitor D-Bus signals
  help
```
```
dbus list     Usage: podnext dbus list [OPTIONS]   -f, --format
dbus call     Usage: podnext dbus call [OPTIONS] <BUS> <PATH> <INTERFACE> <METHOD> [-- <ARGS>...]
              Arguments: <BUS> <PATH> <INTERFACE> <METHOD> [ARGS]... (Arguments (JSON))
dbus monitor  Usage: podnext dbus monitor [OPTIONS] <BUS>
```

### socket — `podnext socket [OPTIONS] <COMMAND>`
```
Commands:
  register    Register socket activation
  unregister  Unregister socket
  list        List socket activations
  help
```
```
socket register    Usage: podnext socket register [OPTIONS] --config <CONFIG>   -c, --config <CONFIG>
socket unregister  Usage: podnext socket unregister [OPTIONS] <NAME>
socket list        Usage: podnext socket list [OPTIONS]   -f, --format
```

### boot — `podnext boot [OPTIONS] <COMMAND>`
```
Commands:
  enable   Enable auto-start at boot
  disable  Disable auto-start at boot
  status   Show boot configuration
  help
```
```
boot enable   Usage: podnext boot enable [OPTIONS] <NAME>
boot disable  Usage: podnext boot disable [OPTIONS] <NAME>
boot status   Usage: podnext boot status [OPTIONS]   -f, --format
```

### wakelock — `podnext wakelock [OPTIONS] <COMMAND>`
```
Commands:
  acquire  Acquire wake lock
  release  Release wake lock
  status   Show wake lock status
  help
```
```
wakelock acquire  Usage: podnext wakelock acquire [OPTIONS] <NAME> <LOCK_TYPE>
                  <LOCK_TYPE> [possible values: partial, full, doze-exemption]
wakelock release  Usage: podnext wakelock release [OPTIONS] <NAME>
wakelock status   Usage: podnext wakelock status [OPTIONS]   -f, --format
```

### policy — `podnext policy [OPTIONS] <COMMAND>`
```
Commands:
  eval     Evaluate policies for a service
  list     List policy rules
  load     Load policy from file
  presets  Show policy presets
  help
```
```
policy eval     Usage: podnext policy eval [OPTIONS] <NAME>    -f, --format
policy list     Usage: podnext policy list [OPTIONS]           --tag <TAG> ; -f, --format
policy load     Usage: podnext policy load [OPTIONS] --file <FILE>   -f, --file <FILE>
policy presets  Usage: podnext policy presets [OPTIONS]
```

### backup — `podnext backup [OPTIONS] <COMMAND>`
```
Commands:
  create   Create a backup
  list     List backups
  restore  Restore a backup
  delete   Delete a backup
  status   Show backup status
  config   Show backup configuration
  help
```
```
backup create   Usage: podnext backup create [OPTIONS] <CONTAINER>
                -t, --backuptype <BACKUP_TYPE> [default: full]
                [possible values: full, incremental, differential, configonly] ; -f, --format
backup list     Usage: podnext backup list [OPTIONS]     -c, --container <CONTAINER> ; -f, --format
backup restore  Usage: podnext backup restore [OPTIONS] <BACKUP_ID>   -t, --target <TARGET>
backup delete   Usage: podnext backup delete [OPTIONS] <BACKUP_ID>
backup status   Usage: podnext backup status [OPTIONS] <BACKUP_ID>
backup config   Usage: podnext backup config [OPTIONS]
```

### build — `podnext build [OPTIONS] <COMMAND>`
```
Commands: submit / status / cancel / list / artifacts / logs / help
```
```
build submit     Usage: podnext build submit [OPTIONS] --spec <SPEC>   -s, --spec <SPEC> ; -f, --format
build status     Usage: podnext build status [OPTIONS] <BUILD_ID>      -f, --format
build cancel     Usage: podnext build cancel [OPTIONS] <BUILD_ID>
build list       Usage: podnext build list [OPTIONS]    --status <STATUS> ; -f, --format
build artifacts  Usage: podnext build artifacts [OPTIONS] <BUILD_ID>
build logs       Usage: podnext build logs [OPTIONS] <BUILD_ID>        -f, --follow
```

### gitops — `podnext gitops [OPTIONS] <COMMAND>`
```
Commands: sync / status / resources / drift / reconcile / help
```
```
gitops sync       Usage: podnext gitops sync [OPTIONS]        -f, --force
gitops status     Usage: podnext gitops status [OPTIONS]      -f, --format
gitops resources  Usage: podnext gitops resources [OPTIONS]   --kind <KIND> ; -f, --format
gitops drift      Usage: podnext gitops drift [OPTIONS]       -f, --format
gitops reconcile  Usage: podnext gitops reconcile [OPTIONS] [NAME]
```

### dashboard — `podnext dashboard [OPTIONS] <COMMAND>`
```
Commands: start / stop / status / help
```
```
dashboard start   Usage: podnext dashboard start [OPTIONS]   -p, --port [default: 8800] ; --bind [default: 0.0.0.0]
dashboard stop    Usage: podnext dashboard stop [OPTIONS]
dashboard status  Usage: podnext dashboard status [OPTIONS]
```

### network — `podnext network [OPTIONS] <COMMAND>`
```
Commands: create / ls / inspect / rm / connect / disconnect / help
```
```
network create      Usage: podnext network create [OPTIONS] <NAME>
                    -d, --driver [default: bridge] ; -s, --subnet <SUBNET> (CIDR) ; -g, --gateway <GATEWAY>
network ls          Usage: podnext network ls [OPTIONS]        -f, --format
network inspect     Usage: podnext network inspect [OPTIONS] [NETWORKS]...   -f, --format [default: json]
network rm          Usage: podnext network rm [OPTIONS] [NETWORKS]...
network connect     Usage: podnext network connect [OPTIONS] <NETWORK> <CONTAINER>   -i, --ip <IP>
network disconnect  Usage: podnext network disconnect [OPTIONS] <NETWORK> <CONTAINER>
```

### volume — `podnext volume [OPTIONS] <COMMAND>`
```
Commands: create / ls / inspect / rm / prune / help
```
```
volume create   Usage: podnext volume create [OPTIONS] <NAME>
                -d, --driver [default: local] ; -o, --opts <KEY=VALUE>
volume ls       Usage: podnext volume ls [OPTIONS]      -f, --format
volume inspect  Usage: podnext volume inspect [OPTIONS] [VOLUMES]...   -f, --format [default: json]
volume rm       Usage: podnext volume rm [OPTIONS] [VOLUMES]...       -f, --force
volume prune    Usage: podnext volume prune [OPTIONS]                 -f, --force
```

**EVIDENCE:** `./target/release/podnext <family> --help` and `<family> <sub> --help` for all families above.

---

## 4. Storage layout

`~/.podnext` exists with two subdirs:

```
$ ls -la ~/.podnext
drwx------  backups
drwx------  containers
```

Depth-2 sample:

```
$ ls -la ~/.podnext/containers/
almalinux/  alpine/  pdn-bench/  test-alpine/  test-pdn-install/

$ ls -la ~/.podnext/containers/test-alpine/
-rw-------  metadata.json
drwx------  rootfs/

$ ls ~/.podnext/containers/alpine/rootfs/
bin dev etc home lib media mnt opt proc root run sbin srv sys tmp usr var

$ ls ~/.podnext/backups | wc -l
37
# entries: <uuid>/ directories AND <uuid>.tar.gz archives, e.g.
#   02c9f1f7-6542-4e30-846a-c810e5c06b2a.tar.gz   (23178 bytes)
#   12416ccd-5c85-470c-a9f7-6fabed63ad41.tar.gz   (4632579 bytes)
```

One real `metadata.json` (`~/.podnext/containers/alpine/metadata.json`):

```json
{
  "created_at": "2026-11-02T19:13:21.662893675+00:00",
  "image": "alpine",
  "name": "alpine",
  "rootfs": "/data/data/com.termux/files/home/.podnext/containers/alpine/rootfs",
  "status": "Stopped",
  "tag": "latest"
}
```

Paths defined in `crates/proot-distro-core/src/paths.rs` (`ContainerPaths::new`):

```
root_dir      = $HOME
containers_dir= ~/.podnext/containers
images_dir    = ~/.podnext/images      (NOT present on disk)
cache_dir     = ~/.podnext/cache       (NOT present on disk)
log_dir       = ~/.podnext/logs        (NOT present on disk)
config_dir    = ~/.podnext/config      (NOT present on disk)
container_rootfs(name) = containers_dir/<name>/rootfs
container_config(name) = containers_dir/<name>/config.json   (NOT present; actual file is metadata.json)
container_state(name)  = containers_dir/<name>/state.json     (NOT present)
```

Network/volume state is NOT under `~/.podnext`. It persists under `~/.config/podnext/`
(`dirs::config_dir()`), from `proot-distro-core/src/{network,storage}.rs`:

```
$ cat ~/.config/podnext/networks.toml
[persist-net]
name = "persist-net"
driver = "bridge"
subnet = "10.0.1.0/24"
gateway = "10.0.1.1"
dns_servers = [ "8.8.8.8", "8.8.4.4" ]
enable_ipv6 = false
[testnet]
name = "testnet"
driver = "bridge"
dns_servers = [ "8.8.8.8", "8.8.4.4" ]
enable_ipv6 = false

$ cat ~/.config/podnext/volumes.toml
[testvol]
name = "testvol"
driver = "local"
[testvol.labels]
[testvol.options]
[persist-vol]
name = "persist-vol"
driver = "local"
[persist-vol.labels]
[persist-vol.options]
```

Daemon socket (hardcoded, `crates/podnext/src/commands/mod.rs:53`):
`/data/data/com.termux/files/home/.local/run/pn-next/daemon.sock`.
Supervisor dirs (defaults, `commands/mod.rs:267–273`): services `.config/pn-next/services`,
runtime `.local/run/pn-next`, logs `.local/log/pn-next`.

**EVIDENCE:** `ls -la ~/.podnext*`, `find ~/.podnext -maxdepth 2`, `cat ~/.podnext/containers/alpine/metadata.json`, `cat ~/.config/podnext/{networks,volumes}.toml`, `paths.rs`, `commands/mod.rs:53,267–273`.

---

## 5. Runtime architecture (module map) and real-vs-stub

Source tree (crate → files, observed via `find`):

| capability | implementing file(s) | status |
|---|---|---|
| container manager | `crates/proot-distro-core/src/container.rs` (`ContainerManager`), `login.rs`, `install.rs`, `session.rs`; CLI glue `crates/podnext/src/native.rs` | real |
| proot exec engine | `crates/proot-core/src/lib.rs` (`ProotEngine::execute/start`) used by `proot-distro-core/src/container.rs:10` | real |
| service supervisor | `crates/pn-next/src/supervisor.rs` (`ServiceSupervisor`, `SupervisorBuilder`) + `daemon_runtime/mod.rs` | real (in-process; state per invocation) |
| network manager | `crates/proot-distro-core/src/network.rs` (`NetworkManager`, TOML-persisted); alt impl `crates/pn-next/src/network.rs` | real (CLI uses distro-core) |
| volume manager | `crates/proot-distro-core/src/storage.rs` (`StorageManager`, `volumes.toml`) | real (metadata only) |
| quota (cgroups) | `crates/pn-next/src/resource_quota/mod.rs` (writes `cgroup.procs`,`cpu.max`,`memory.max`,`io.max`,`pids.max`) | real module, **not persisted by CLI** |
| logging | `crates/pn-next/src/structured_logging/mod.rs` (writes `~/.local/share/pdn/logs/pdn.log`) | real module; `log tail` stub |
| dbus | `crates/pn-next/src/dbus/{mod.rs,services.rs,zbus_client.rs}` | module real; `call`/`monitor` stub |
| backup | `crates/pn-next/src/backup_restore/mod.rs` (`BackupManager`) | real (tar.gz), several sub-features stub |
| watchdog | `crates/pn-next/src/watchdog/mod.rs` | real module, not persisted by CLI |
| wake lock | `crates/pn-next/src/wake_lock/mod.rs` | real module, in-memory |
| policy engine | `crates/pn-next/src/policy_engine/mod.rs` | module real; `eval`/`load` stub |
| termux:boot | `crates/pn-next/src/termux_boot/mod.rs` (writes boot scripts) | real |
| socket activation | `crates/pn-next/src/socket_activation/mod.rs`, `container_socket_activation/mod.rs` | module real; `unregister`/`list` stub |
| web dashboard | `crates/pn-next/src/web_dashboard/mod.rs` | module real; CLI start/stop/status stub |
| gitops | `crates/pn-next/src/gitops/mod.rs` | **CLI entirely stub** |
| multi-arch build | `crates/pn-next/src/multiarch_build/mod.rs` | **CLI entirely stub** |

`grep -rn "todo!\|unimplemented!" crates/` → **no matches** (no panic-stubs; gaps are silent
print-stubs / in-memory managers).

### CLI handler dispatch
`crates/podnext/src/main.rs:1514–1596` (`execute_command`), families in
`crates/podnext/src/commands/mod.rs`. `execute_*_command` functions:
service:97, daemon:584, watchdog:650, quota:757, log:967, dbus:1115, socket:1188, boot:1220,
wakelock:1284, policy:1363, backup:1439, build:1618, gitops:1677, dashboard:1732.

### STUBS (handlers that only print / are no-ops) — observed in `commands/mod.rs`

| command | line | evidence |
|---|---|---|
| `daemon stop` | 603–607 | only prints; no daemon signal |
| `daemon reload` | 638–641 | only prints |
| `daemon logs` | 643–646 | only prints header, no log read |
| `daemon status` | 609–636 | hardcoded `running: true, total_services: 0` |
| `log tail` | 1058–1062 | prints `(Log tailing not yet implemented)` |
| `dbus call` | 1166–1178 | prints args, no call (args are `_args`, unused) |
| `dbus monitor` | 1180–1184 | prints `(D-Bus monitoring not yet implemented)` |
| `socket unregister` | 1208–1211 | only prints |
| `socket list` | 1213–1216 | only prints header (ignores `_format`) |
| `policy eval` | 1372–1377 | prints `(Policy evaluation not yet implemented)` |
| `policy load` | 1425–1428 | only prints |
| `backup config` | 1609–1614 | prints `(Config access not directly available via API)` |
| `build submit/status/cancel/list/artifacts/logs` | 1631–1673 | all `(… not yet implemented)` |
| `gitops sync/status/resources/drift/reconcile` | 1689–1728 | all `(… not yet implemented)` |
| `dashboard start` | 1751–1757 | prints `Dashboard would start on {bind}:{port}` (never binds) |
| `dashboard stop` | 1759–1763 | prints `(Dashboard stop not yet implemented)` |
| `dashboard status` | 1765–1768 | prints `(Dashboard status not yet implemented)` |

### In-memory / non-persistent managers (functional but ephemeral across invocations)
`watchdog`, `quota`, `wakelock`, `policy list`, `dbus list`, `log query/stats` each construct a
**fresh** manager per CLI invocation (`X::new(XConfig::default())`), so writes in one invocation do
not survive to the next except where the module writes files. Modules that DO persist to disk:
`termux_boot` (boot scripts), `resource_quota`/`supervisor` (cgroup files, only when a supervisor
run holds them), `structured_logging` (`pdn.log`), `dbus/mod.rs` (`dbus.sock`), `network`/`storage`
(TOML), `backup` (tar.gz).

Observed: `quota get demo` returns **default** values even though `demo` was never set
(`ResourceQuotas::default`, `crates/pn-next/src/resource_quota/mod.rs:81–99`: cpu 100, mem 512MB,
swap 1GB, pids 100, nofile 1024). `watchdog status` and `dbus list` return empty tables.

**EVIDENCE:** `find crates/*/src -type f`; `grep -rn 'todo!\|unimplemented!'` (empty); `grep -n "execute_\|fn cmd_" commands/mod.rs`; verbatim reads of `commands/mod.rs:97–284,584–740,757–876,967–1062,1115–1220,1284–1435,1439–1614,1618–1768`; live `quota get demo`, `watchdog status`, `dbus list`, `policy presets`, `backup config`.

---

## 6. proot invocation (`crates/proot-core/src/lib.rs`)

The engine shells out to the **system `proot` binary** (`PROOT_BIN` env, default `"proot"`),
version comment targets `proot 5.1.107.101`. Two build paths: `start()` (daemon, no shell wrap)
and `execute()` (foreground, `/bin/sh -c` wrapped). `proot-distro-core/src/container.rs:10`
imports `proot_core::{ProotConfig, ProotEngine}` and `container.rs:80–88` constructs the engine,
so `execute()` is the runtime path used by the CLI.

Flags passed, in order (`execute()`, `lib.rs:217–276`):

```
proot
  --kill-on-exit                      # lib.rs:229
  --link2symlink                      # lib.rs:230
  --sysvipc                           # lib.rs:231
  -L                                  # lib.rs:232 (fix lstat for dpkg symlinks)
  --rootfs=.                          # lib.rs:238
  # process cwd set to rootfs:  cmd.current_dir(rootfs)   lib.rs:239
  [--cwd=<cwd>]                       # lib.rs:242–244 if config.cwd set
  --bind=/dev                         # lib.rs:247
  --bind=/proc                        # lib.rs:248
  --bind=/sys                         # lib.rs:249
  # then add_android_system_binds(cmd)   lib.rs:252 / def at lib.rs:84–136:
  --bind=/apex --bind=/odm --bind=/product --bind=/system --bind=/system_ext --bind=/vendor
  --bind=/linkerconfig/ld.config.txt --bind=/linkerconfig/com.android.art/ld.config.txt
  --bind=$TERMUX_PREFIX  (fallback --bind=/data/data/com.termux/files/usr)   # lib.rs:98-102
  --bind=/data/app  --bind=/data/dalvik-cache  --bind=/data/misc/apexdata/com.android.art/dalvik-cache  (if exist)
  --bind=/storage/self/primary --bind=/sdcard --bind=/storage/emulated/0  (if exist)
  --bind=$HOME
  --bind=$TERMUX_PREFIX/tmp  (if exist)
  # config env vars applied via cmd.env(k, v)      lib.rs:254–259
  # extra --bind=<bind> from config                lib.rs:262–264
  /bin/sh -c "<command joined by spaces>"          lib.rs:272–276
```

`start()` (`lib.rs:139–214`) passes the command **directly** (no `/bin/sh -c`): `cmd.arg(&command[0])` + args (`lib.rs:191–194`).

Confirmed as required: `--rootfs=.` with `current_dir(rootfs)` ✔; `--kill-on-exit` ✔;
`--link2symlink` ✔; `--sysvipc` ✔; `-L` ✔; `--bind=/dev,/proc,/sys` ✔;
`add_android_system_binds` list ✔; `/bin/sh -c` wrapping (in `execute()`) ✔.

**EVIDENCE:** `read_file crates/proot-core/src/lib.rs` lines 55–358; `grep -n proot|--rootfs|... lib.rs`; `proot-distro-core/src/container.rs:10,80–88`.

---

## 7. Backup implementation (`crates/pn-next/src/backup_restore/mod.rs`)

Config defaults (`BackupConfig::default`, `mod.rs:72–115`):
backup_dir `~/.podnext/backups` (`PDN_BACKUP_DIR` override), containers_dir `~/.podnext/containers`
(`PDN_CONTAINERS_DIR` override), `max_backups_per_container: 10`, `max_total_size: 50 GiB`,
`schedule_interval_secs: 3600`, `compression_level: 6`, `include_volumes: true`, `include_config: true`,
`include_logs: false`, `encryption_enabled: false`.

What is copied (`run_backup`, `mod.rs:518–653`):
- **rootfs only if `include_volumes`** → copies `containers_dir/<container_id>/rootfs` (`mod.rs:563–576`).
- container config if `include_config` (`mod.rs:579–582`) — but `backup_container_config` returns
  `ContainerConfigSnapshot::default()` (**stub**, `mod.rs:762–767`).
- logs if `include_logs` (`mod.rs:585–588`) — but `backup_logs` returns an empty snapshot
  (**stub**, `mod.rs:769–775`).

Excluded directories (`backup_directory`, `mod.rs:676–760`): when walking, any directory whose
`file_name()` matches `"proc" | "sys" | "dev"` is skipped (`mod.rs:736–738`). Symlinks preserved
(`mod.rs:743–745`), regular files copied (`mod.rs:746–751`), other special files skipped (`mod.rs:752–754`).
Per-file manifest entries record path,size,mode,uid,gid,mtime,SHA-256 checksum,file_type (`mod.rs:709–728`).

Archive format (`create_archive`, `mod.rs:777–845`): **`.tar.gz`** — `flate2::write::GzEncoder`
(level = `compression_level` 6) + `tar::Builder`, walking with `walkdir::WalkDir`; unreadable files
are skipped (`mod.rs:818–828`). Checksum = SHA-256 of the final archive (`mod.rs:847–853`).

Layout written (`mod.rs:544–608`): temp dir `backup_dir/<uuid>/` with `manifest.json`
(`BackupManifest`, written at `mod.rs:595–596`) + `rootfs/`, then archive `backup_dir/<uuid>.tar.gz`;
temp dir removed (`mod.rs:608`). Matches disk sample (37 entries of `<uuid>/` + `<uuid>.tar.gz`).

Manifest fields (`BackupManifest`, `mod.rs:216–224`): `files: Vec<ManifestEntry>`, `total_files`,
`total_size`, `volumes: Vec<VolumeInfo>`, `config: Option<ContainerConfigSnapshot>`,
`logs: Option<LogsSnapshot>`. `ManifestEntry` (`mod.rs:226–236`): `path,size,mode,uid,gid,mtime,checksum,file_type`.
Metadata (`BackupMetadata`, `mod.rs:175–192`): `id,name,container_id,container_name,backup_type,status,
created_at,completed_at,size_bytes,compressed_size,checksum,manifest,labels,annotations,error_message`.

Restore stubs: `restore_backup` unpacks the tar (`mod.rs:904–925`) but `restore_container` returns a
random UUID without creating anything (**stub**, `mod.rs:950–959`). `load_backups` is a no-op
(**stub**, `mod.rs:1129–1149` — "Simplified - would load actual metadata files"), so `backup list`
on a fresh process sees nothing. `start_scheduled_backups` just logs a tick (**stub**,
`mod.rs:1002–1017`). `get_container_info` hardcodes `image: "alpine"` (**stub**, `mod.rs:655–674`).
`cmd_backup_create` passes `container` for both id and name (`commands/mod.rs:1485`).

**EVIDENCE:** `read_file crates/pn-next/src/backup_restore/mod.rs` (full, lines 1–1231); `ls ~/.podnext/backups`.

---

## 8. Known gaps (observed in code / CLI)

1. **Stale version binary** — release binary (`02:08`) prints `podnext 0.1.0` / info `0.1.0`
   while `--version` prints `0.1.7`. Source fixed mid-session (mtimes `02:41`/`02:45`); needs
   `cargo build --release`. See §1/§1a.
2. **`-r` short-flag collision in `run`** — `run --help` lists both `-r, --rm` and `-r, --rootfs`.
   Potential clap conflict (§2). **UNVERIFIED** whether it constructs the expected arg.
3. **Help text ≠ real command tree** — top-level long_about advertises `podnext watchdog policy set …`
   but the real subcommand is `watchdog setpolicy` (§3). long_about examples are not validated against Clap.
4. **`list` misreports distro/version** — every container shows `DISTRO=ubuntu VERSION=22.04`
   (including `alpine`, whose `metadata.json` says `image: "alpine", tag: "latest"`):
   ```
   | NAME             | DISTRO | VERSION | ARCH    | STATUS  | CREATED          |
   | pdn-bench        | ubuntu | 22.04   | aarch64 | Stopped | 2026-11-02 19:41 |
   | test-alpine      | ubuntu | 22.04   | aarch64 | Stopped | 2026-11-02 19:41 |
   | alpine           | ubuntu | 22.04   | aarch64 | Stopped | 2026-11-02 19:41 |
   ```
5. **Volume mountpoint hardcoded unwritable path** — `proot-distro-core/src/storage.rs:93,107`
   sets `mountpoint = /var/lib/podnext/volumes/<name>`; `ls -ld /var/lib/podnext` →
   `No such file or directory` (Android/Termux has no `/var/lib`, no root). `volume ls` prints it anyway.
6. **Non-persistent CLI state** — `quota`/`watchdog`/`wakelock`/`policy`/`dbus`/`log` construct a
   fresh manager per invocation; `quota set` then `quota get` do not round-trip. `quota get` shows
   defaults regardless (§5).
7. **`ContainerPaths` defines dirs that are never created** — `images/`, `cache/`, `logs/`,
   `config/` under `~/.podnext` are absent; container config path is `config.json`/`state.json`
   in code but the on-disk file is `metadata.json` (§4).
8. **Debug logging always on** — every command prints `DEBUG main: Executing command: …` and
   `DEBUG execute_command: START …`; backup prints extensive `DEBUG:` lines to stderr
   (observed in all live runs; non-toggleable via `-v`).
9. **Stub commands** — build (all 6), gitops (all 5), dashboard start/stop/status, daemon stop/reload/logs/status,
   log tail, dbus call/monitor, policy eval/load, socket unregister/list; backup restore does not
   recreate a container (§5, §7).

**EVIDENCE:** live runs of `list`, `quota get demo`, `backup config`, `version`, `info`;
`ls -ld /var/lib/podnext`; `proot-distro-core/src/storage.rs:93,107`; `paths.rs`;
`commands/mod.rs` line refs in §5; main.rs `--help` long_about vs `watchdog --help`.

---

*Generated by read-only inspection. Source files under `crates/podnext/src` were edited
concurrently by another process during this run (mtimes 02:41 / 02:45) — line contents for
§1a reflect the state at the cited timestamps; the shipped binary predates them.*
