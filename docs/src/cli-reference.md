# CLI Reference

## Global Options

```bash
podnext [GLOBAL_OPTIONS] <COMMAND> [COMMAND_OPTIONS] [-- <CMD_ARGS>...]
```

| Option | Short | Description |
|--------|-------|-------------|
| `--help` | `-h` | Show help |
| `--version` | `-V` | Show version |
| `--rootfs <PATH>` | `-r` | Use PATH as guest root filesystem |
| `--bind <HOST:GUEST>` | `-b` | Bind mount (repeatable) |
| `--pwd <DIR>` | `-w` | Set working directory |
| `--qemu <BIN>` | `-q` | Execute through QEMU user-mode |
| `--link2symlink` | `-0` | Enable hardlink emulation via symlinks |
| `--no-sysvipc` | | Disable System V IPC emulation |
| `--kernel <VER>` | | Override kernel release string |
| `--hostname <NAME>` | | Set hostname |
| `--env <VAR=VALUE>` | `-e` | Set environment variable (repeatable) |

## Container Commands

### `podnext install`

Install a distribution from OCI image.

```bash
podnext install <IMAGE_REF> [OPTIONS]
```

**Arguments:**
- `IMAGE_REF` - OCI image reference (e.g., `ubuntu:22.04`, `docker.io/library/alpine:latest`)

**Options:**
| Option | Description |
|--------|-------------|
| `--name <NAME>` | Custom container name |
| `--arch <ARCH>` | Target architecture (default: host) |
| `--platform <PLATFORM>` | Target platform (e.g., `linux/arm64`) |
| `--config <FILE>` | Container configuration file |
| `--security-profile <NAME>` | Security profile to apply |
| `--no-pull` | Use local image only |
| `--insecure` | Allow insecure registry |

**Examples:**
```bash
podnext install ubuntu:22.04
podnext install alpine:latest --name my-alpine
podnext install docker.io/library/debian:bookworm --arch arm64
podnext install ghcr.io/user/image:tag --config ./container.toml
```

### `podnext list`

List created containers.

```bash
podnext list [OPTIONS]
```

**Options:**
| Option | Description |
|--------|-------------|
| `--all` | Show all containers (including stopped) |
| `--format <FORMAT>` | Output format: table, json, yaml |
| `--filter <KEY=VALUE>` | Filter containers |

**Output:**
```
NAME       IMAGE              STATUS      CREATED
ubuntu     ubuntu:22.04       stopped     2024-01-15 10:30:00
alpine     alpine:latest      running     2024-01-15 11:00:00
```

### `podnext login`

Start interactive shell inside a container.

```bash
podnext login <CONTAINER> [OPTIONS] [-- <SHELL_ARGS>...]
```

**Options:**
| Option | Description |
|--------|-------------|
| `--user <USER>` | User to run as (default: root) |
| `--workdir <DIR>` | Working directory |
| `--env <VAR=VALUE>` | Environment variable |
| `--security-profile <NAME>` | Security profile |

**Examples:**
```bash
podnext login ubuntu
podnext login ubuntu -- /bin/bash
podnext login ubuntu --user 1000 --workdir /home/user
```

### `podnext run`

Run command in container.

```bash
podnext run <CONTAINER> [OPTIONS] -- <CMD> [ARGS...]
```

**Options:**
| Option | Description |
|--------|-------------|
| `--detach` | Run in background |
| `--rm` | Remove container after exit |
| `--interactive` | Keep stdin open |
| `--tty` | Allocate pseudo-TTY |
| `--user <USER>` | User to run as |
| `--workdir <DIR>` | Working directory |
| `--env <VAR=VALUE>` | Environment variable |
| `--bind <HOST:GUEST>` | Bind mount |
| `--network <NETWORK>` | Network to connect |
| `--security-profile <NAME>` | Security profile |
| `--gpu` | Enable GPU access |
| `--no-limits` | Disable resource limits |

**Examples:**
```bash
podnext run ubuntu -- apt update
podnext run ubuntu -e "DEBIAN_FRONTEND=noninteractive" -- apt install -y curl
podnext run ubuntu -b /sdcard:/mnt/sdcard -- ls /mnt/sdcard
podnext run ubuntu --detach --rm -- nginx -g "daemon off;"
```

### `podnext ps`

List active sessions.

```bash
podnext ps [OPTIONS]
```

**Options:**
| Option | Description |
|--------|-------------|
| `--all` | Show all sessions |
| `--format <FORMAT>` | Output format: table, json, yaml |

### `podnext sessions`

List active sessions (better formatted).

```bash
podnext sessions [OPTIONS]
```

### `podnext attach`

Attach to a container session.

```bash
podnext attach <SESSION_ID> [OPTIONS]
```

### `podnext kill`

Stop container.

```bash
podnext kill <CONTAINER> [OPTIONS]
```

**Options:**
| Option | Description |
|--------|-------------|
| `--signal <SIGNAL>` | Signal to send (default: SIGTERM) |
| `--timeout <SECONDS>` | Wait timeout before SIGKILL (default: 10) |

### `podnext remove`

Delete a container.

```bash
podnext remove <CONTAINER> [OPTIONS]
```

**Options:**
| Option | Description |
|--------|-------------|
| `--force` (`-f`) | Force remove container without confirmation |

### `podnext rename`

Rename an installed container in its metadata and filesystem.

```bash
podnext rename <OLD_NAME> <NEW_NAME>
```

**Examples:**
```bash
podnext rename debian deb-production
```

### `podnext copy`

Copy files or directories between the host and containers. Paths take the form `[CONTAINER:]PATH`. A path without a container prefix is treated as a host path. Path traversals (e.g., `..`) are strictly verified and rejected.

```bash
podnext copy <SOURCE> <DESTINATION> [OPTIONS]
```

**Options:**
| Option | Short | Description |
|--------|-------|-------------|
| `--recursive` | `-R` | Copy directories recursively |
| `--move` | `-m` | Remove the source after successful transfer |

**Examples:**
```bash
# Copy a file from container to host
podnext copy debian:/etc/resolv.conf ./resolv.conf

# Copy a directory recursively from host to container
podnext copy ./my-app debian:/opt/my-app -R

# Move a file from container to host
podnext copy debian:/var/log/app.log ./app.log -m
```

### `podnext sync`

Mirror files between host and containers using content hashing (SHA256). Only differing files are transferred, so subsequent runs are zero-overhead no-ops.

```bash
podnext sync <SOURCE> <DESTINATION> [OPTIONS]
```

**Options:**
| Option | Short | Description |
|--------|-------|-------------|
| `--delete` | `-d` | Remove destination entries that do not exist in source |

**Examples:**
```bash
# Mirror directory to container and delete deleted files
podnext sync ./src debian:/root/src -d
```

### `podnext reset`

Reset an installed container to a clean install of the original OCI image it came from. Deletes current rootfs modifications and reinstalls cleanly.

```bash
podnext reset <CONTAINER>
```

**Examples:**
```bash
podnext reset debian
```

## Image Commands

### `podnext search`

Search images on Docker Hub/OCI registries.

```bash
podnext search <QUERY> [OPTIONS]
```

**Options:**
| Option | Description |
|--------|-------------|
| `--limit <N>` | Maximum results (default: 25) |
| `--registry <URL>` | Registry to search |
| `--format <FORMAT>` | Output format: table, json, yaml |

**Examples:**
```bash
podnext search ubuntu
podnext search alpine --limit 10
podnext search nginx --registry ghcr.io
```

### `podnext image list`

List local images.

```bash
podnext image list [OPTIONS]
```

### `podnext image remove`

Remove local image.

```bash
podnext image remove <IMAGE> [OPTIONS]
```

### `podnext image pull`

Pull image from registry.

```bash
podnext image pull <IMAGE> [OPTIONS]
```

### `podnext image push`

Push image to registry.

```bash
podnext image push <IMAGE> [OPTIONS]
```

## Network Commands

### `podnext network create`

Create a network.

```bash
podnext network create <NAME> [OPTIONS]
```

**Options:**
| Option | Description |
|--------|-------------|
| `--driver <DRIVER>` | Network driver: bridge, host, none, macvlan |
| `--subnet <CIDR>` | Subnet for bridge network |
| `--gateway <IP>` | Gateway IP |
| `--ip-range <CIDR>` | IP range for allocation |

### `podnext network list`

List networks.

```bash
podnext network list [OPTIONS]
```

### `podnext network remove`

Remove network.

```bash
podnext network remove <NAME> [OPTIONS]
```

### `podnext network inspect`

Inspect network details.

```bash
podnext network inspect <NAME> [OPTIONS]
```

## Storage Commands

### `podnext storage create`

Create storage pool.

```bash
podnext storage create <NAME> [OPTIONS]
```

**Options:**
| Option | Description |
|--------|-------------|
| `--driver <DRIVER>` | Storage driver: local, nfs, csi |
| `--path <PATH>` | Path for local driver |

### `podnext storage list`

List storage pools.

```bash
podnext storage list [OPTIONS]
```

### `podnext storage remove`

Remove storage pool.

```bash
podnext storage remove <NAME> [OPTIONS]
```

## Volume Commands

### `podnext volume create`

Create volume.

```bash
podnext volume create <NAME> [OPTIONS]
```

**Options:**
| Option | Description |
|--------|-------------|
| `--storage <POOL>` | Storage pool to use |
| `--size <SIZE>` | Volume size (e.g., 10G) |
| `--driver <DRIVER>` | Volume driver |

### `podnext volume list`

List volumes.

```bash
podnext volume list [OPTIONS]
```

### `podnext volume remove`

Remove volume.

```bash
podnext volume remove <NAME> [OPTIONS]
```

### `podnext volume inspect`

Inspect volume details.

```bash
podnext volume inspect <NAME> [OPTIONS]
```

## System Commands

### `podnext info`

Show system information.

```bash
podnext info [OPTIONS]
```

**Output:**
```
PDN Version: 1.0.0
Host OS: Linux 6.1.0 (aarch64)
Termux: 0.118.0
PRoot: 5.4.0
proot-distro: N/A (native)

Containers: 3
Images: 5
Networks: 2
Volumes: 4

Resources:
  CPU: 8 cores
  Memory: 8.0 GB
  Disk: 128 GB (45 GB free)

Features:
  GPU: ✓ (Adreno 730)
  CRIU: ✓
  Seccomp: ✓
  AppArmor: ✗
  SELinux: ✗
  Cgroups v2: ✓
```

### `podnext dbus`

Manage D-Bus IPC broker and service forwarder.

```bash
# Start standalone D-Bus broker on a specific UNIX socket
podnext dbus serve --socket /tmp/pn-dbus.sock

# List registered D-Bus services
podnext dbus list --socket /tmp/pn-dbus.sock

# Call a method over D-Bus
podnext dbus call org.pnnext.Service /org/pnnext/Service org.pnnext.Service.GetStatus --socket /tmp/pn-dbus.sock
```

### `podnext gitops`

Declarative application lifecycle management and reconciliation.

```bash
# Sync applications and volumes from declarative YAML manifests
podnext gitops sync --repo ./manifests

# Show GitOps synchronization status
podnext gitops status

# Inspect configuration drift against desired state
podnext gitops drift

# Reconcile drifts and enforce state
podnext gitops reconcile
```

### `podnext version`

Show version information.

```bash
podnext version [OPTIONS]
```

**Options:**
| Option | Description |
|--------|-------------|
| `--short` | Short version only |
| `--json` | JSON output |

## Direct Execution (proot-compatible)

Run commands directly with PRoot (bypassing container management):

```bash
# Basic execution
podnext -r /path/to/rootfs -- /bin/bash

# With bind mounts
podnext -r /path/to/rootfs -b /host/path:/guest/path -- ls /guest/path

# With environment
podnext -r /path/to/rootfs -e "VAR=value" -- /bin/bash -c "echo $VAR"

# With QEMU emulation
podnext -r /path/to/rootfs -q qemu-aarch64 -- /bin/bash
```

## Exit Codes

| Code | Meaning |
|------|---------|
| 0 | Success |
| 1 | General error |
| 2 | Invalid arguments |
| 3 | Container not found |
| 4 | Image not found |
| 5 | Network error |
| 6 | Storage error |
| 7 | Permission denied |
| 8 | Resource limit exceeded |
| 9 | Security policy violation |
| 10 | Gateway error |
| 11 | Runtime error |
| 12 | Internal error |

## Environment Variables

| Variable | Description | Default |
|----------|-------------|---------|
| `PN_CONFIG_DIR` | Config directory | `~/.config/pn-next` |
| `PN_DATA_DIR` | Data directory | `~/.local/share/pn-next` |
| `PN_CACHE_DIR` | Cache directory | `~/.cache/pn-next` |
| `PN_LOG_LEVEL` | Log level (trace/debug/info/warn/error) | `info` |
| `PN_GATEWAY_ADDR` | Gateway address | `unix:///run/pn-gateway.sock` |
| `PN_RUNTIME` | Runtime to use | `auto` |
| `PN_SECURITY_PROFILE` | Default security profile | `default` |
| `PN_NO_COLOR` | Disable colored output | `false` |

## Configuration Files

### Container Config (`~/.config/pn-next/containers/<name>.toml`)

```toml
[container]
name = "ubuntu"
image = "docker.io/library/ubuntu:22.04"

[container.config]
hostname = "ubuntu"
working_dir = "/root"
user = "root"

[container.config.env]
TERM = "xterm-256color"
LANG = "en_US.UTF-8"

[container.config.mounts]
"/sdcard" = { target = "/mnt/sdcard", readonly = false }
"/data/data/com.termux/files/home" = { target = "/home/termux", readonly = false }

[container.resources]
memory_limit = "2G"
cpu_limit = "2.0"
pids_limit = 100
io_weight = 100
io_max_read = "100MB/s"
io_max_write = "100MB/s"

[container.security]
profile = "default"
no_new_privileges = true
read_only_rootfs = false
user_namespace = false

[container.network]
mode = "bridge"
networks = ["default"]

[container.gpu]
enabled = false
devices = []
api = "vulkan"
```

## Shell Completion

```bash
# Bash
podnext completion bash > /etc/bash_completion.d/podnext

# Zsh
podnext completion zsh > /usr/share/zsh/site-functions/_podnext

# Fish
podnext completion fish > ~/.config/fish/completions/podnext.fish
```

## See Also

- [Getting Started](getting-started.md)
- [Configuration](configuration.md)
- [Security](security.md)
- [Architecture](architecture.md)