# Unix Socket vs D-Bus for Inter-Container IPC Comparison

## Executive Summary

This document compares Unix domain sockets and D-Bus as IPC mechanisms for inter-container communication in the PN Next architecture on Android/Termux. Both approaches have distinct advantages depending on the use case.

---

## Comparison Matrix

| Aspect | Unix Domain Sockets | D-Bus |
|--------|---------------------|-------|
| **Protocol Complexity** | Low (raw byte streams) | High (structured message protocol) |
| **Message Framing** | Manual (length-prefixed, custom) | Built-in (header + body + validation) |
| **Type System** | None (application-defined) | Rich (integers, strings, arrays, dicts, variants, structs) |
| **Service Discovery** | Manual (well-known paths) | Built-in (name registration, introspection) |
| **Security** | File permissions only | PolicyKit, SELinux, AppArmor, bus policies |
| **Broadcast/Signals** | Manual implementation | Native signal emission & match rules |
| **Language Bindings** | Universal (every language) | Excellent (C, Python, Rust, Go, JS, etc.) |
| **Tooling** | `socat`, `nc`, custom | `dbus-monitor`, `d-feet`, `busctl`, `gdbus` |
| **Performance (Latency)** | Lowest (~1-2 μs) | Low (~5-10 μs) |
| **Performance (Throughput)** | Highest (~1-2 GB/s) | High (~500 MB/s - 1 GB/s) |
| **Container Isolation** | Via socket file permissions | Via bus policies, container-specific buses |
| **Android/Termux Support** | Native, no dependencies | Requires `dbus-daemon` or `zbus` broker |
| **Debugging** | Raw bytes, hard to inspect | Structured, human-readable |
| **Learning Curve** | Low | Medium |
| **Standard Compliance** | POSIX | freedesktop.org D-Bus Specification |

---

## Detailed Analysis

### Unix Domain Sockets

#### Advantages

1. **Minimal Overhead**: Direct kernel-mediated communication with no userspace daemon
2. **Universal Availability**: Present on all Unix-like systems including Android
3. **Simple Mental Model**: File descriptor + read/write = communication
4. **Maximum Performance**: Lowest latency, highest throughput
5. **No Dependencies**: Zero external dependencies
6. **Flexible Serialization**: Use any format (JSON, MessagePack, Protobuf, bincode, raw bytes)

#### Disadvantages

1. **No Built-in Protocol**: Must implement framing, serialization, error handling
2. **No Service Discovery**: Need external registry or well-known paths
3. **No Introspection**: Cannot query API schema at runtime
4. **Manual Security**: Only filesystem permissions (chmod/chown)
5. **No Broadcasting**: Must implement pub/sub manually
6. **Fragile Evolution**: Schema changes break compatibility silently
7. **Debugging Difficulty**: Binary protocols hard to inspect without custom tools

#### Best For

- High-performance data planes (log streaming, metrics, file transfer)
- Simple request-response between known endpoints
- When dependencies must be minimized
- Legacy system integration

---

### D-Bus

#### Advantages

1. **Standardized Protocol**: Well-defined wire format, message types, errors
2. **Rich Type System**: Native support for complex types (dicts, arrays, variants, structs)
3. **Service Discovery**: Well-known names, activation, name ownership tracking
4. **Introspection**: Runtime API discovery via `Introspect` method
5. **Built-in Security**: Fine-grained policies, SELinux integration, container isolation
6. **Signals & Match Rules**: Efficient pub/sub with filtering
6. **Excellent Tooling**: `dbus-monitor`, `d-feet`, `busctl`, `gdbus`, `zbus` CLI
7. **Language Bindings**: First-class support in Rust (`zbus`), Python (`dbus-python`), C (`libdbus`), Go (`godbus`), JavaScript (`dbus-js`)
8. **Daemon-Managed**: Connection lifecycle, authentication, routing handled by broker
9. **Cross-Process**: Works across user sessions, containers, network (with tcp transport)

#### Disadvantages

1. **Higher Overhead**: Daemon hop, serialization, validation (~5-10 μs vs ~1-2 μs)
2. **Complexity**: More concepts to learn (bus types, interfaces, objects, paths, signals)
3. **External Dependency**: Requires `dbus-daemon` or embedded broker (`zbus`)
4. **Android Limitations**: No system `dbus-daemon` by default; session bus may not exist
5. **Verbosity**: More boilerplate for simple use cases

#### Best For

- Control planes (service management, configuration, lifecycle)
- Multi-party communication (broadcast, pub/sub)
- When API stability and introspection matter
- Integration with desktop/linux ecosystem tools
- Complex structured data exchange

---

## PN Next Architecture Recommendation

### Hybrid Approach (Recommended)

Use **both** mechanisms for different layers:

| Layer | Mechanism | Rationale |
|-------|-----------|-----------|
| **Control Plane** | D-Bus (zbus broker) | Service discovery, introspection, signals, structured APIs |
| **Data Plane** | Unix Sockets | High-throughput streaming (logs, metrics, file sync) |
| **Container ↔ Host** | D-Bus | Supervisor API, container lifecycle, network/storage mgmt |
| **Container ↔ Container** | D-Bus (isolated buses) | Service-to-service calls, event bus |
| **Emergency/Debug** | Unix Sockets | Raw access when D-Bus unavailable |

### Implementation Strategy

1. **Embedded zbus Broker**: Run in-process broker in supervisor (no external `dbus-daemon` needed)
2. **Per-Container Bus Isolation**: Each container gets isolated bus namespace
3. **Unix Socket Fallback**: Raw sockets for high-volume data paths
4. **Shared Schema**: Define D-Bus interfaces in `dbus/services.rs` (single source of truth)

---

## Android/Termux Specific Considerations

### Constraints

1. **No System D-Bus Daemon**: Android doesn't run `dbus-daemon --system` by default
2. **No Session Bus**: No graphical session → no `dbus-daemon --session`
3. **Filesystem Permissions**: `/run` may not be writable; use `$TMPDIR` or `$HOME/.local/run`
4. **SELinux**: May restrict socket creation in certain paths
5. **Process Isolation**: Each Termux process has limited UID/GID visibility

### Solutions

| Constraint | Solution |
|------------|----------|
| No system bus | Embedded broker in supervisor (zbus) |
| No session bus | Per-user broker at `~/.local/run/pn-next/dbus/session` |
| /run not writable | Use `$TMPDIR/pn-next/dbus` or `$HOME/.local/run/pn-next/dbus` |
| SELinux restrictions | Set socket permissions 0666, use abstract namespace (`@pn-next/dbus`) |
| UID visibility | Pass credentials via `SO_PEERCRED` (Linux) or abstract namespace |

### Abstract Namespace Sockets (Linux-only)

```rust
// Use abstract namespace (Linux-specific, no filesystem entry)
// Format: "@pn-next/dbus/session"
let socket_path = "@pn-next/dbus/session";
```

**Pros**: No filesystem permissions, survives chroot, container-friendly
**Cons**: Linux-only, not visible in filesystem, harder to debug

---

## Performance Benchmarks (Estimated)

| Operation | Unix Socket | D-Bus (zbus) | D-Bus (dbus-daemon) |
|-----------|-------------|--------------|---------------------|
| Round-trip latency (local) | ~2 μs | ~8 μs | ~15 μs |
| Throughput (1KB msgs) | ~1.5M msg/s | ~400K msg/s | ~200K msg/s |
| Memory (broker) | N/A | ~2 MB | ~5 MB |
| Startup time | Instant | ~5 ms | ~50 ms |

*Numbers are approximate for aarch64 Android; actual values vary by hardware.*

---

## Security Model Comparison

### Unix Sockets
```rust
// Only filesystem permissions
std::fs::set_permissions(socket_path, Permissions::from_mode(0o660))?;
// Owner: root:container_group
// Container processes run in group
```

### D-Bus (zbus broker)
```rust
// Policy-based
let policy = r#"
<!DOCTYPE busconfig PUBLIC "-//freedesktop//DTD D-Bus Bus Configuration 1.0//EN"
 "http://www.freedesktop.org/standards/dbus/1.0/busconfig.dtd">
<busconfig>
  <policy user="*">
    <allow own="org.pnnext.*"/>
    <allow send_destination="org.pnnext.*"/>
    <allow receive_sender="org.pnnext.*"/>
  </policy>
  <policy context="container_*">
    <deny own="org.freedesktop.*"/>
    <allow send_destination="org.pnnext.ContainerManager"/>
  </policy>
</busconfig>
"#;
```

---

## Migration Path

### Phase 1: Current State (Unix Sockets Only)
- Daemon IPC via Unix socket at `/run/pn-next/daemon.sock`
- JSON-RPC style messages
- Custom protocol

### Phase 2: Add D-Bus Broker (This Implementation)
- Embed zbus broker in supervisor
- Define service interfaces in `dbus/services.rs`
- Expose supervisor, container manager, network, storage via D-Bus
- Keep Unix socket for high-volume data paths

### Phase 3: Container Integration
- Each container gets D-Bus client (`ContainerDBusClient`)
- Containers register as `org.pnnext.Container.<name>`
- Inter-container communication via D-Bus signals/methods
- Event bus for decoupled messaging

### Phase 4: Full D-Bus Ecosystem
- Introspection-enabled tools (`d-feet`, `busctl`)
- Policy-based security
- Network-transparent D-Bus (TCP transport for remote clusters)

---

## Decision Checklist

| Question | Unix Socket | D-Bus |
|----------|-------------|-------|
| Need service discovery? | ❌ | ✅ |
| Need introspection? | ❌ | ✅ |
| Need pub/sub signals? | Manual | ✅ |
| Need structured types? | Manual | ✅ |
| Ultra-low latency critical? | ✅ | ⚠️ |
| Zero dependencies required? | ✅ | ❌ |
| Android/Termux deployment? | ✅ | ⚠️ (needs embedded broker) |
| Integration with Linux tools? | ❌ | ✅ |
| Multi-language clients? | Manual | ✅ |
| Container isolation? | Manual | ✅ |

---

## Conclusion

**Recommendation**: Implement a **hybrid architecture** with zbus-based D-Bus broker for control plane and Unix sockets for data plane. This provides:

- ✅ Structured, introspectable APIs for management operations
- ✅ Service discovery and signal-based eventing
- ✅ High-performance raw sockets for streaming data
- ✅ No external `dbus-daemon` dependency (embedded broker)
- ✅ Android/Termux compatible (abstract namespace sockets)
- ✅ Future-proof for cluster/remote communication

The implementation in `crates/pn-next/src/dbus/` provides:
- `mod.rs` - Custom lightweight broker (fallback/embedded)
- `services.rs` - Service definitions & introspection XML
- `zbus_client.rs` - Full zbus client for containers & tools

---

## References

- [D-Bus Specification](https://dbus.freedesktop.org/doc/dbus-specification.html)
- [zbus Documentation](https://dbus.pages.freedesktop.org/zbus/)
- [Unix Domain Sockets - man 7 unix](https://man7.org/linux/man-pages/man7/unix.7.html)
- [Android Binder vs D-Bus](https://source.android.com/docs/core/architecture/binder)
- [freedesktop.org D-Bus Best Practices](https://dbus.freedesktop.org/doc/dbus-faq.html)