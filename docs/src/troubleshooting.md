# Troubleshooting

## Common Issues

### Container Won't Start

#### Symptoms
- `podnext run` hangs or fails
- Container exits immediately
- Error: "failed to start container"

#### Diagnosis
```bash
# Check logs
podnext run ubuntu --security-profile unconfined -- /bin/bash -c "echo test"

# Check system info
podnext info

# Check gateway health
curl http://127.0.0.1:8080/api/v1/system/health
```

#### Common Causes & Solutions

| Cause | Solution |
|-------|----------|
| Seccomp profile too restrictive | Use `--security-profile unconfined` or `restricted` |
| Missing capabilities | Add required capabilities: `--cap-add SYS_ADMIN` |
| Insufficient resources | Increase limits: `--memory 4G --cpu 4.0` |
| Rootfs corruption | Reinstall: `podnext remove ubuntu && podnext install ubuntu:22.04` |
| Kernel missing features | Check `podnext info` for feature support |
| PRoot not installed | `pkg install proot` |

### Image Pull Fails

#### Symptoms
- `podnext install` fails
- Error: "manifest unknown" or "unauthorized"
- Timeout during pull

#### Solutions
```bash
# Check network connectivity
curl -I https://registry-1.docker.io/v2/

# Try different registry
podnext install ghcr.io/library/ubuntu:22.04

# Use insecure registry (if needed)
podnext install my-registry:5000/myimage --insecure

# Authenticate
podnext image pull my-registry:5000/myimage --auth username:password

# Check disk space
df -h ~/.cache/pn-next
```

### Network Not Working

#### Symptoms
- No internet in container
- Cannot resolve hostnames
- Port forwarding fails

#### Diagnosis
```bash
# Check CNI
ls /etc/cni/net.d/
cat /etc/cni/net.d/10-pdn-bridge.conf

# Check bridge
ip link show pdn0
ip addr show pdn0

# Check iptables/nftables
sudo nft list ruleset | grep pdn
sudo iptables -L -n | grep pdn

# Check DNS
podnext run ubuntu -- cat /etc/resolv.conf
```

#### Solutions
```bash
# Recreate default network
podnext network remove default
podnext network create default --driver bridge --subnet 10.88.0.0/16

# Use host networking
podnext run ubuntu --network host -- ping 8.8.8.8

# Fix DNS
podnext run ubuntu --dns 1.1.1.1 --dns 8.8.8.8 -- ping google.com

# Check firewall
sudo ufw status
sudo ufw allow in on pdn0
```

### Bind Mounts Not Working

#### Symptoms
- Files not visible in container
- Permission denied on mount
- Mount shows empty

#### Solutions
```bash
# Check path translation
podnext run ubuntu -b /sdcard:/mnt/sdcard -- ls -la /mnt/sdcard

# Use correct syntax (host:guest)
podnext run ubuntu -b /host/path:/guest/path -- cmd

# Check permissions
ls -la /host/path

# Use readonly if needed
podnext run ubuntu -b /host/path:/guest/path:ro -- cmd

# Recursive bind mount
podnext run ubuntu -b /host/path:/guest/path:rshared -- cmd
```

### GPU Not Working

#### Symptoms
- `vulkaninfo` shows no devices
- `glxinfo` fails
- GPU applications crash

#### Diagnosis
```bash
# Check devices
ls -la /dev/dri/

# Check Vulkan ICD
ls /usr/share/vulkan/icd.d/
cat /usr/share/vulkan/icd.d/*.json

# Check environment
podnext run ubuntu --gpu -- env | grep -i vulkan
```

#### Solutions
```bash
# Install drivers
pkg install vulkan-loader vulkan-tools mesa-vulkan-drivers

# Set ICD
export VK_ICD_FILENAMES=/system/lib64/vulkan/adreno_icd.aarch64.json

# Run with GPU
podnext run ubuntu --gpu -- vulkaninfo

# Check security profile (may block GPU syscalls)
podnext run ubuntu --gpu --security-profile unconfined -- vulkaninfo
```

### Permission Denied

#### Symptoms
- "Permission denied" on file operations
- Cannot write to mounted volumes
- Capability errors

#### Solutions
```bash
# Check security profile
podnext run ubuntu --security-profile unconfined -- cmd

# Add capabilities
podnext run ubuntu --cap-add DAC_OVERRIDE --cap-add SYS_ADMIN -- cmd

# Check file permissions
podnext run ubuntu -- ls -la /path/to/file

# Fix ownership
podnext run ubuntu -- chown -R root:root /path

# Use user namespace
podnext run ubuntu --user-namespace -- cmd
```

### Performance Issues

#### Symptoms
- Slow container startup
- High CPU/memory usage
- Slow file I/O

#### Diagnosis
```bash
# Check resource usage
podnext info

# Profile startup
time podnext run ubuntu -- echo hello

# Check I/O
podnext run ubuntu -- dd if=/dev/zero of=/tmp/test bs=1M count=100
```

#### Solutions
```bash
# Increase resources
podnext run ubuntu --memory 4G --cpu 4.0 -- cmd

# Use tmpfs for temporary files
podnext run ubuntu --tmpfs /tmp:size=1G -- cmd

# Disable security features (for benchmarking)
podnext run ubuntu --security-profile unconfined --no-limits -- cmd

# Check for swap
podnext run ubuntu -- free -h
```

### Checkpoint/Restore Fails

#### Symptoms
- `podnext checkpoint` fails
- Restored container doesn't work
- CRIU errors

#### Solutions
```bash
# Check CRIU
criu check

# Check kernel support
grep CHECKPOINT_RESTORE /boot/config-$(uname -r)

# Use unconfined profile
podnext run ubuntu --security-profile unconfined --detach --name test -- sleep 1000
podnext checkpoint test --name cp1

# Check logs
journalctl -u pdn-gateway -f
```

## Debugging

### Enable Debug Logging

```bash
# Environment variable
export PN_LOG_LEVEL=debug
export PN_LOG_FORMAT=json
podnext run ubuntu -- cmd

# Or per-command
PN_LOG_LEVEL=trace podnext run ubuntu -- cmd
```

### Gateway Logs

```bash
# If running as service
journalctl -u pdn-gateway -f

# If running manually
pdn-gateway --log-level debug
```

### Container Logs

```bash
# View container output
podnext logs <container-id>

# Follow logs
podnext logs -f <container-id>

# Last N lines
podnext logs --tail 100 <container-id>
```

### System Call Tracing

```bash
# Trace syscalls with strace (requires CAP_SYS_PTRACE)
podnext run ubuntu --security-profile unconfined -- strace -f -e trace=network ls

# Or use PDN's built-in tracing
export PN_TRACING_ENABLED=true
export PN_TRACING_ENDPOINT=http://localhost:4317
podnext run ubuntu -- cmd
```

### Memory Analysis

```bash
# Heap profiling
heaptrack podnext run ubuntu -- cmd
heaptrack_gui heaptrack.podnext.*.gz

# Valgrind (slow)
valgrind --tool=massif podnext run ubuntu -- cmd
ms_print massif.out.*
```

### CPU Profiling

```bash
# perf (Linux)
perf record -g podnext run ubuntu -- cmd
perf report

# Flamegraph
cargo install flamegraph
cargo flamegraph --package podnext --bin podnext -- run ubuntu -- cmd
```

## Error Messages

### Gateway Errors

| Error | Cause | Solution |
|-------|-------|----------|
| `connection refused` | Gateway not running | Start gateway: `pdn-gateway` |
| `deadline exceeded` | Operation timeout | Increase timeout |
| `resource exhausted` | Quota exceeded | Check guardrails config |
| `permission denied` | Security policy | Check security profile |

### Runtime Errors

| Error | Cause | Solution |
|-------|-------|----------|
| `ptrace: operation not permitted` | Missing CAP_SYS_PTRACE | Use unconfined profile |
| `seccomp: syscall blocked` | Syscall not allowed | Adjust seccomp profile |
| `cgroup: permission denied` | Cgroup v2 not mounted | Check kernel config |
| `mount: operation not permitted` | Missing CAP_SYS_ADMIN | Add capability or use unconfined |

### Network Errors

| Error | Cause | Solution |
|-------|-------|----------|
| `network not found` | Network doesn't exist | Create network first |
| `IP allocation failed` | IPAM exhausted | Increase subnet size |
| `CNI plugin not found` | Plugin missing | Install CNI plugins |

### Storage Errors

| Error | Cause | Solution |
|-------|-------|----------|
| `volume not found` | Volume doesn't exist | Create volume first |
| `mount failed` | Filesystem error | Check disk space, permissions |
| `quota exceeded` | Storage limit reached | Clean up or increase quota |

## Performance Tuning

### Startup Time

```bash
# Pre-pull images
podnext image pull ubuntu:22.04

# Use local images only
podnext install ubuntu:22.04 --no-pull

# Disable security features (dev only)
podnext run ubuntu --security-profile unconfined --no-limits -- cmd
```

### Resource Usage

```toml
# Optimize resource limits
[container.resources]
memory_limit = "2G"        # Set appropriate limit
cpu_limit = "2.0"          # Match workload
pids_limit = 50            # Prevent fork bombs
io_max_read_bps = "100MB/s"
io_max_write_bps = "100MB/s"
```

### I/O Performance

```bash
# Use tmpfs for temporary data
podnext run ubuntu --tmpfs /tmp:size=1G -- cmd

# Use volumes for persistent data (better than bind mounts)
podnext volume create my-data
podnext run ubuntu -v my-data:/data -- cmd
```

## Getting Help

### Log Collection

```bash
# Collect all logs for bug report
mkdir -p debug-logs
podnext info > debug-logs/info.txt
podnext version > debug-logs/version.txt
journalctl -u pdn-gateway --no-pager > debug-logs/gateway.log 2>&1
dmesg -T | grep -i -e proot -e seccomp -e cgroup > debug-logs/kernel.log 2>&1
tar czf debug-logs.tar.gz debug-logs/
```

### Reporting Issues

1. Check existing issues on GitHub
2. Collect logs (see above)
3. Include:
   - PDN version (`podnext version`)
   - Android/Termux version
   - Kernel version (`uname -a`)
   - Steps to reproduce
   - Expected vs actual behavior
   - Logs and error messages

### Community Resources

- GitHub Issues: https://github.com/michaelamoetoevsky/PDN/issues
- GitHub Discussions: https://github.com/michaelamoetoevsky/PDN/discussions
- Documentation: https://michaelamoetoevsky.github.io/PDN/

## See Also

- [Security](security.md) - Security configuration
- [Configuration](configuration.md) - Configuration options
- [CLI Reference](cli-reference.md) - All commands
- [Development](development.md) - Debugging tools
## Concurrent writes to the same file

If two or more `podnext run` or `podnext login` invocations write to the **same file** at the same
time, one write wins and the others are lost. This is POSIX last-writer-wins: an `>` open truncates
the file, and there is no ordering guarantee between unsynchronised processes.

PDN does not change this, and does not add anything on the write path: no serialisation, no staging
file, no copy-up. The guest filesystem is a plain host directory, so the guest's `write()` reaches
the host kernel directly. Concurrent runs against one container share a single mutable filesystem
with no atomicity guarantee, and any synchronisation is the workload's own responsibility.

Measured, so it is not speculation: the same five-worker experiment run (a) with plain host
processes, no PDN anywhere, and (b) through `podnext run`, produced the identical result -- 4 of 5
writes clobbered, file left with exactly one line -- in every run of both. If you need
read-modify-write across concurrent invocations, use a file lock (`flock`) in your command, or give
each writer its own path and merge afterwards.
