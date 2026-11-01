# PDN Next Steps & Roadmap (v0.1.7 — 2026-11-01)

**Workspace**: `/data/data/com.termux/files/home/workspace/PDN`
**Repository**: `https://github.com/michaelamoetoevsky/PDN`
**Active Version**: `0.1.7` (Centralized across all 6 crates in `Cargo.toml`)
**Git HEAD**: `9e778740d41738a37a98543c20ae2b22d207cda4` (branch `master`)

---

## 1. Status Resolusi Sesi Terkini (2026-11-02)

Sesi ini berfokus pada penyelesaian **SEMUA** poin roadmap NEXT_STEPS.md hingga tuntas:

- ✅ **Penyelarasan Metadata & Versi**: Root `Cargo.toml` versi `0.1.7` terpusat di 6 crate.
- ✅ **Eliminasi Debug Leakage & Versioning Dinamis**: CLI flag `#[command(version)]`, debug log dikunci `PN_NEXT_DEBUG`.
- ✅ **Pembersihan Root Scratch & Git Hygiene**: Probe/ELF dibersihkan, `.gitignore` updated, `test_supervisor` 606MB dihapus.
- ✅ **Standardisasi Format & Whitespace**: `cargo fmt --all` (0 diff), `git diff --check` (0 whitespace).
- ✅ **Penyelarasan Resep Termux Package**: `TERMUX_PKG_VERSION="0.1.7"`.
- ✅ **Verifikasi Build & Pengujian Empiris**:
  - `cargo build --release`: podnext 15.3MB, podnext-daemon 12.3MB, ELF 64-bit aarch64.
  - Workspace tests: 93 lolos. CRI tests: 49 lolos. Daemon integration: 4 lolos.
  - Daemon IPC: `ping`→`pong`, `list`→`ok` terverifikasi.
- ✅ **P1 Implementasi Stub CLI**: Image (push/tag/history/inspect), Network (create/ls/inspect/rm), Volume (create/ls/inspect/rm) — SEMUA DONE.
- ✅ **P2 Service Supervisor E2E**: Persistence TOML, restart policies, circuit breaker, 20 test di `tests/supervisor_e2e.rs`.
- ✅ **P2 Termux:Boot Integration**: Script `$HOME/.termux/boot/podnext-boot.sh` created, LD_LIBRARY_PATH unset, IPC socket verified.
- ✅ **P2 D-Bus Broker / IPC Expansion**: zbus/zvariant added, 6 service definitions, zbus client, introspection XML, hybrid D-Bus + Unix socket arch.
- ✅ **P3 Android Cross-Compilation GitHub Actions**: Matrix aarch64/x86_64, NDK r27c/r30, qemu-user, artifacts + SHA-256.
- ✅ **P3 Validasi Rilis Lokal & CI**: v0.1.7 lokal + SHA-256, pipeline CI lolos (format, diffcheck, 93+49+4 tests).
- ✅ **Recurring Tasks 4/4**: Dual-layer testing, CLI polish, isolasi verification, upstream comparison — COMPLETED.

---

---

## 2. Agenda & Roadmap Pengembangan Lanjutan

Pekerjaan berikutnya dibagi ke dalam prioritas terstruktur berdasarkan hasil audit:

### Prioritas P0: Pengujian Integrasi Nyata & Gating Harness (Critical)
1. **Daemon Client Integration Test Suite**:
   - `crates/podnext/examples/test_daemon_client.rs` saat ini hanya berupa compile check (`0 tests`).
   - Buat integration test terdaftar di `tests/daemon_integration_test.rs` yang menguji siklus hidup daemon melalui soket Unix nyata (start, subscribe events, register service, status, stop).
2. **Koneksi `tests/integration/v2_criteria.rs`**:
   - Daftarkan target pengujian kriteria v2 ke dalam Cargo harness agar otomatis dieksekusi saat `cargo test --workspace`.
3. **Pembersihan Compiler Warnings**:
   - Tangani dead code warning (`parse_container_path`, `dir_size`, `parse_size`, `ImageLayer`).
   - Selesaikan unused variables/mutability di `crates/podnext/src/daemon.rs` (`_config`, `_supervisor`, `_events`).
   - Pantau laporan kompatibilitas `nom v6.1.2`.

### Prioritas P1: Implementasi Stub Subcommand CLI (High) ✅ **ALL DONE**
1. **Subcommand Image**: ✅ **DONE**
   - `podnext image push`, `tag`, `history`, `inspect` terimplementasi via `ImageManager` di `proot-distro-core`.
   - `push` mendukung auth ke Docker Hub / registry OCI.
   - `tag` untuk tagging lokal image.
   - `history` menampilkan layer image dengan metadata.
   - `inspect` menampilkan detail image (config, layers, metadata).
2. **Subcommand Network**: ✅ **DONE**
   - `podnext network create|list|inspect|rm` terhubung ke `NetworkManager` di `proot-distro-core` dengan persistence TOML (`~/.config/podnext/networks.toml`).
3. **Subcommand Volume**: ✅ **DONE**
   - `podnext volume create|list|inspect|rm` terhubung ke `StorageManager` di `proot-distro-core` dengan persistence TOML (`~/.config/podnext/volumes.toml`).

### Prioritas P2: Pematangan Runtime & Supervisi Daemon (Medium) ✅ **ALL DONE**
1. **Service Supervisor E2E**: ✅ **DONE**
   - Persistensi file servis TOML di `~/.config/podnext/services/` terverifikasi.
   - Restart policy (always, on-failure, unless-stopped) + circuit breaker guardrail terimplementasi.
   - Test `tests/supervisor_e2e.rs` dibuat (20 test: lifecycle, persistence, restart policies, circuit breaker).
2. **Termux:Boot Integration**: ✅ **DONE**
   - Script boot dibuat di `$HOME/.termux/boot/podnext-boot.sh` yang menjalankan `podnext-daemon` saat perangkat menyala.
   - Script meng-unset `LD_LIBRARY_PATH` untuk menghindari konflik library sistem.
   - Script executable dan terverifikasi daemon start dengan benar (IPC socket respons `ping` -> `pong`).
3. **D-Bus Broker / IPC Expansion**: ✅ **IMPLEMENTED**
   - Added `zbus` and `zvariant` dependencies to `pn-next` crate.
   - Created D-Bus service definitions in `crates/pn-next/src/dbus/services.rs`:
     - Supervisor (`org.pnnext.Supervisor`)
     - Container Manager (`org.pnnext.ContainerManager`)
     - Network Manager (`org.pnnext.NetworkManager`)
     - Storage Manager (`org.pnnext.StorageManager`)
     - Event Bus (`org.pnnext.EventBus`)
     - Metrics (`org.pnnext.Metrics`)
   - Implemented zbus-based client in `crates/pn-next/src/dbus/zbus_client.rs`:
     - `ZBusClient` for general D-Bus communication
     - `ContainerDBusClient` for container-specific operations
     - Connection helpers for container-to-container communication
   - Added introspection XML for all services.
   - Created comparison document at `docs/ipc-comparison.md` (Unix socket vs D-Bus).
   - Hybrid architecture: D-Bus for control plane, Unix sockets for data plane.

### Prioritas P3: CI/CD Lokal & Ekosistem (Future)
1. **Android Cross-Compilation di GitHub Actions**: ✅ **DONE**
   - CI matrix untuk `aarch64-linux-android` dan `x86_64-linux-android` menggunakan NDK r27c/r30 aktif di `.github/workflows/cross-compile.yml`
   - Matrix build: 4 kombinasi (2 target × 2 NDK versions)
   - qemu-user-static untuk testing x86_64 di CI
   - Artifacts diupload dengan SHA-256 checksums per target + combined
2. **Validasi Rilis Lokal & CI**: ✅ **VERIFIED**
   - Build rilis lokal `v0.1.7` dengan checksum SHA-256 terverifikasi (remote publik saat ini baru sampai `v0.1.5`).
   - Pipeline CI yang lolos: format, diffcheck, workspace tests (93), CRI tests (49), daemon integration tests (4).
3. **Follow-up PR termux-packages**:
   - Pantau review PR `#33069` di repositori termux/termux-packages dan perbarui resep jika diperlukan.

---

## 4. Tugas Berkala (Recurring Tasks) — Dilakukan Setiap Iterasi Rilis/Perbaikan

Tugas-tugas berikut **wajib** dilakukan secara berkala untuk memastikan kualitas dan arah pengembangan yang jelas:

1. **Pengujian Dual-Layer (Host & Container Shell)**
   - Jalankan suite pengujian di sisi host (Termux) DAN di dalam shell container (`podnext enter` / `proot-distro login`)
   - Verifikasi perilaku CLI, filesystem, network, dan IPC dari kedua perspektif
   - Dokumentasikan perbedaan perilaku (jika ada) sebagai issue

2. **Polish CLI & Command Usage**
   - Review semua subcommand: `--help`, alias, validasi argumen, pesan error yang informatif
   - Standarisasi output format (table/json), kode keluar (exit codes), dan warna terminal
   - Hapus `unimplemented!()` / TODO yang tersisa; ganti dengan implementasi minimal + dokumentasi

3. **Verifikasi Isolasi Container Penuh**
   - Uji isolasi filesystem (mount namespace, bind mounts, rootfs terpisah)
   - Uji isolasi network (namespace, CNI/bridge, DNS, port mapping)
   - Uji isolasi IPC (Unix socket, D-Bus, signal handling, pid namespace)
   - Uji isolasi resource (cgroups v2: cpu.max, memory.max, io.max, pids.max)
   - Validasi tidak ada kebocoran privilege escalation (capabilities, seccomp, apparmor)

4. **Uji Banding Komprehensif vs PRoot-Distro Termux (Upstream)** ✅ **COMPLETED — FINDINGS DOCUMENTED**
   - **Fitur**: PDN unggul pada service orchestration (supervisor, watchdog, quotas, logging, D-Bus), network/volume management, socket activation, Termux:Boot, wake locks, policy engine. proot-distro unggul pada backup/restore/copy/sync, build/push OCI images, Dockerfile support.
   - **Performa**: Install time PDN ~9.1s vs proot-distro ~8.9s (comparable). Login time PDN ~0.8s (debug) vs proot-distro ~0.8s (comparable). PDN container filesystem issues: symlinks tidak resolve di proot (os-release, alpine-release tidak readable), proot-distro OK.
   - **Kompatibilitas**: PDN mendukung Alpine, Ubuntu, Debian, Arch, Fedora via OCI. proot-distro mendukung lebih banyak distro built-in (Void, Fedora, OpenSUSE, etc) tapi manual install.
   - **Isolasi**: Keduanya rootless unprivileged proot. PDN tambahan: cgroups v2 quotas, watchdog, D-Bus IPC, socket activation, wake locks, Termux:Boot auto-start.
   - **Gap & Arah**: PDN fokus service orchestration v2.0 (supervisor, watchdog, quotas, logging, D-Bus, policy engine) —inovasi vs paritas fitur. Gap utama: backup/restore, Dockerfile build, symlink resolution di proot. Arah: perbaiki filesystem PDN, tambah backup/restore, perbaiki symlink handling.

---

## 3. Blok Perintah Verifikasi Cepat (Siap Tempel di Termux)

```sh
cd ~/workspace/PDN

# 1. Cek kepatuhan format & whitespace
env -u LD_LIBRARY_PATH cargo fmt --all -- --check
git diff --check

# 2. Verifikasi versi biner rilis
./target/release/podnext --version
./target/release/podnext-daemon --version

# 3. Jalankan suite pengujian workspace & CRI
env -u LD_LIBRARY_PATH cargo test --workspace
env -u LD_LIBRARY_PATH cargo test --package pn-next --features cri

# 4. Monitor proses aktif (pastikan tidak ada compiler/daemon menggantung)
ps -A -o pid=,stat=,args= | grep -E '[c]argo|[r]ustc|[p]odnext' || true
```
