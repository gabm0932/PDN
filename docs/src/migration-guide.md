# Migration Guide: From PRoot-distro to PDN

This guide helps you migrate from the traditional `proot-distro` shell scripts to PDN (PRoot Distro Next).

## Why Migrate?

| Aspect | PRoot-distro | PDN |
|--------|--------------|-----|
| **Language** | Shell scripts | Rust (memory safe) |
| **Login Cold Start** | ~0.603s | **~0.097s (~5× faster)** |
| **List Performance** | ~0.160s | **~0.008s (~20× faster)** |
| **Env Sanitization** | Leaks 24-163 host vars | **Strict 6 clean vars** |
| **Root Emulation** | Basic fakeroot | **`--change-id=0:0` (`apt`/`dpkg` verified)** |
| **Shared Memory** | Host clobbered/none | **Isolated `/dev/shm` 1777 (Chromium & Playwright ready)** |
| **File Traversal Defense** | None (standard tar) | **Strict canonicalization (rejects `..`)** |
| **File Sync** | None | **Delta sync via SHA256 + `--delete`** |
| **Child Exit Code** | Truncated/collapsed | **Exact child exit code propagated** |
| **Supervision** | None (dies on exit) | **Supervisor daemon, restart policy, watchdog** |
| **API & IPC** | CLI only | **CLI + D-Bus broker + REST API** |

## Command Mapping

### Container Lifecycle

| PRoot-distro | PDN | Notes |
|--------------|-----|-------|
| `proot-distro install ubuntu` | `podnext install ubuntu:22.04` | Uses OCI images directly |
| `proot-distro login ubuntu` | `podnext login ubuntu` | Same interactive experience |
| `proot-distro run ubuntu -- cmd` | `podnext run ubuntu -- cmd` | Direct execution |
| `proot-distro remove ubuntu` | `podnext remove ubuntu` | Removes container and data |
| `proot-distro list` | `podnext list` | Shows installed containers |
| `proot-distro reset ubuntu` | `podnext reset ubuntu` | Reinstalls clean container image |
| `proot-distro backup ubuntu` | `podnext backup create ubuntu` | Full tar.gz container snapshot |
| `proot-distro restore <file>` | `podnext backup restore <id>` | Restore snapshot to new container |

### File Operations (Host <-> Container)

| PRoot-distro | PDN | Notes |
|--------------|-----|-------|
| `proot-distro cp <src> <dest>` | `podnext copy <src> <dest> [-R]` | Traversal-safe copy host <-> container |
| (manual rsync) | `podnext sync <src> <dest> [-d]` | SHA256 delta synchronization |
| (manual mv) | `podnext rename <old> <new>` | Rename container and rootfs metadata |

### Image Management

| PRoot-distro | PDN | Notes |
|--------------|-----|-------|
| (built-in) | `podnext search <query>` | Search Docker Hub/OCI registries |
| (built-in) | `podnext install <image>` | Pull and install from registry |
| (built-in) | `podnext image list` | List local images |
| (built-in) | `podnext image remove <image>` | Remove local image |

### Session Management

| PRoot-distro | PDN | Notes |
|--------------|-----|-------|
| (manual) | `podnext ps` | List active sessions |
| (manual) | `podnext sessions` | Better formatted sessions |
| (manual) | `podnext attach <session>` | Attach to running session |
| (manual) | `podnext kill <container>` | Stop container |

### Network & Storage

| PRoot-distro | PDN | Notes |
|--------------|-----|-------|
| (manual bind) | `podnext network create` | CNI-based networks |
| (manual bind) | `podnext network list` | List networks |
| (manual bind) | `podnext volume create` | Named volumes |
| (manual bind) | `podnext volume list` | List volumes |
| (manual bind) | `podnext storage list` | Storage pools |

### System Commands

| PRoot-distro | PDN | Notes |
|--------------|-----|-------|
| `proot-distro --version` | `podnext version` | Version info |
| (manual) | `podnext info` | System diagnostics |

## Configuration Migration

### PRoot-distro Config Location

```
~/.termux/proot-distro/
├── installed/           # Container rootfs directories
├── shar/                # Shared scripts
└── distros/             # Distro definitions
```

### PDN Config Location

```
~/.config/pn-next/
├── config.toml          # Main configuration
├── containers/          # Container definitions (TOML)
├── images/              # Image cache (OCI layout)
├── networks/            # Network definitions
├── storage/             # Storage pools
├── volumes/             # Volume definitions
└── security/            # Security profiles
```

### Container Config Comparison

**PRoot-distro (shell variables in distro definition):**

```bash
# ~/.termux/proot-distro/distros/ubuntu.sh
DISTRO_NAME="Ubuntu"
DISTRO_COMMENT="Ubuntu 22.04"
TARBALL_URL["aarch64"]="https://..."
TARBALL_SHA256["aarch64"]="..."
```

**PDN (TOML configuration):**

```toml
# ~/.config/pn-next/containers/ubuntu.toml
[container]
name = "ubuntu"
image = "docker.io/library/ubuntu:22.04"

[container.config]
hostname = "ubuntu"
working_dir = "/root"

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

[container.security]
profile = "default"
no_new_privileges = true
read_only_rootfs = false
```

## Data Migration

### Migrating Existing Containers

If you have existing `proot-distro` containers, you can migrate them:

```bash
# 1. Export the container rootfs as tar
cd ~/.termux/proot-distro/installed
tar czf ubuntu-backup.tar.gz ubuntu/

# 2. Import into PDN (requires OCI image conversion)
# Note: Direct rootfs import is planned for v1.1
# For now, reinstall with PDN and copy data manually

# 3. Copy data from old container
podnext install ubuntu:22.04 --name ubuntu-new
podnext run ubuntu-new -- mkdir -p /mnt/old
podnext run ubuntu-new -b ~/.termux/proot-distro/installed/ubuntu:/mnt/old -- \
  cp -r /mnt/old/root/* /root/ 2>/dev/null || true
```

### Migrating Bind Mounts

**PRoot-distro (in `~/.termux/proot-distro/installed/ubuntu/etc/proot-distro/binds`):**

```
/sdcard:/sdcard
/data/data/com.termux/files/home:/home/termux
```

**PDN (in container TOML or CLI flags):**

```bash
# Via CLI
podnext run ubuntu -b /sdcard:/sdcard -b /data/data/com.termux/files/home:/home/termux -- cmd

# Via config
[container.config.mounts]
"/sdcard" = { target = "/sdcard", readonly = false }
"/data/data/com.termux/files/home" = { target = "/home/termux", readonly = false }
```

## Security Migration

### PRoot-distro Security

- Basic PRoot isolation
- No seccomp filtering
- All capabilities available
- No AppArmor/SELinux

### PDN Security Profiles

```toml
# ~/.config/pn-next/security/default.toml
[profile]
name = "default"
type = "default"
enabled = true

[profile.seccomp]
default_action = "errno"
architectures = ["SCMP_ARCH_AARCH64", "SCMP_ARCH_X86_64"]

[profile.capabilities]
drop = [
  "CAP_SYS_ADMIN", "CAP_SYS_MODULE", "CAP_SYS_RAWIO",
  "CAP_DAC_OVERRIDE", "CAP_SYS_PTRACE", "CAP_SYS_RESOURCE"
]
add = []

[profile.apparmor]
profile = "pn-next-default"
enforce = true

[profile.selinux]
context = "system_u:system_r:container_t:s0:c123,c456"
enforce = true
```

### Applying Security Profiles

```bash
# Use default profile (secure)
podnext run ubuntu -- cmd

# Use restricted profile (minimal syscalls)
podnext run ubuntu --security-profile restricted -- cmd

# Use unconfined (for debugging)
podnext run ubuntu --security-profile unconfined -- cmd

# Custom profile
podnext run ubuntu --security-profile my-custom -- cmd
```

## Advanced Feature Migration

### Multi-Container Orchestration

**PRoot-distro:** Manual scripting required

**PDN:** Native pod/stack support (planned)

```toml
# pod.toml
[pod]
name = "web-stack"
containers = ["nginx", "php-fpm", "mysql"]

[pod.networks]
default = { driver = "bridge" }

[pod.volumes]
web-data = { driver = "local" }
db-data = { driver = "local" }
```

```bash
podnext pod up -f pod.toml
podnext pod down -f pod.toml
```

### Checkpoint/Restore (CRIU)

**PRoot-distro:** Not supported

**PDN:**

```bash
# Checkpoint a running container
podnext checkpoint ubuntu --name checkpoint-1

# Restore from checkpoint
podnext restore checkpoint-1 --name ubuntu-restored
```

### GPU Acceleration

**PRoot-distro:** Manual `--gpu` flag with limited support

**PDN:**

```toml
[container.gpu]
enabled = true
devices = ["/dev/dri/renderD128", "/dev/dri/card0"]
api = "vulkan"  # or "opengl"
```

```bash
podnext run ubuntu --gpu -- glxinfo | grep "OpenGL renderer"
```

## Troubleshooting Migration Issues

### Issue: Container won't start

**Check:** Security profile compatibility

```bash
# Try unconfined profile
podnext run ubuntu --security-profile unconfined -- /bin/bash
```

### Issue: Bind mounts not working

**Check:** Path translation in config

```toml
[container.config.path_translation]
host_prefix = "/data/data/com.termux/files/home/.config/pn-next/rootfs"
guest_prefix = "/"
```

### Issue: Network not working

**Check:** CNI plugin installation

```bash
# Install CNI plugins
pkg install cni-plugins

# Or use host networking
podnext run ubuntu --network host -- cmd
```

### Issue: Performance regression

**Check:** Resource limits

```bash
# Run without limits for comparison
podnext run ubuntu --no-limits -- cmd
```

## Rollback Plan

If migration fails, you can always fall back to `proot-distro`:

1. PDN doesn't modify `proot-distro` installations
2. Both can coexist
3. Use `proot-distro` commands as before

```bash
# Keep using proot-distro for existing containers
proot-distro login ubuntu

# Use PDN for new containers
podnext install debian:bookworm
```

## Getting Help

- [GitHub Issues](https://github.com/michaelamoetoevsky/PDN/issues) - Report migration issues
- [Discussions](https://github.com/michaelamoetoevsky/PDN/discussions) - Ask questions
- [CLI Help](cli-reference.md) - `podnext help <command>`