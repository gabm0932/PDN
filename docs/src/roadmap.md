# Analisis Komprehensif Arsitektur & Roadmap Pengembangan PDN (PRoot Distro Next)

**Tanggal:** November 2026  
**Status Basis:** v0.1.7 (Stable master commit `82901c5`, 100% Green CI/CD)  
**Target Platform:** Termux di Android 16+ (aarch64) & Linux Rootless Environment (x86_64, aarch64)  
**Dokumen Terkait:** `research/INTERNAL_CAPABILITY_TESTS_RUN2.md`, `research/DOCKER_RUNTIME_INTEGRATION_PLAN.md`, `docs/src/migration-guide.md`

---

## 1. Executive Summary & Status Quo

### 1.1 Fondasi yang Telah Tuntas & Tervalidasi
PDN telah berevolusi dari sekadar eksperimen porting Rust menjadi platform container dan supervisi runtime yang fungsional dan solid:
1. **Integritas CI/CD**: 15 job di GitHub Actions (Host Build, 133 Unit/Integration Tests, Cross-Compile Matrix aarch64/x86_64 dengan NDK r27c & r30, Docs mdBook, Fuzzing, Security Audit) **100% HIJAU**.
2. **Pembersihan Cacat Eksekusi & Sekuriti**:
   - *Child Exit Code*: Propagasi kode keluar anak presisi via `proot_core::ChildExit` downcasting (exit 0..127 diteruskan tanpa collapse ke 1).
   - *Guest Path Traversal*: Penutupan celah traversal pada perintah `copy` (pemeriksaan token, penolakan `..`, verifikasi batas kanonis rootfs).
   - *SELinux Hardlink Safe Unpack*: Mengatasi kegagalan unpacking layer OCI di Android (fallback copy otomatis saat `hard_link` ditolak SELinux).
   - *Shared Memory `/dev/shm` 1777*: POSIX shared memory terisolasi mandiri, memvalidasi eksekusi Chromium Headless & Playwright DOM dump tanpa crash.
   - *Environment Isolation*: Sanitasi ketat variabel lingkungan guest (hanya 6 env var esensial, memotong kebocoran 163 variabel lingkungan Termux host).
3. **Paritas Fitur Pengganti proot-distro**:
   - `install`, `login`, `run`, `list`, `remove`, `rename`, `copy`, `sync` (sha256 + `--delete`), `reset`, `search`, `info`.

### 1.2 Batasan Fundamental Kernel Android (Empirical Ground Truth)
Pengujian mendalam pada `research/INTERNAL_CAPABILITY_TESTS_RUN2.md` membuktikan secara empiris batasan arsitektur PRoot murni di Android:
- `mount(2)` diintercept dan dibunuh dengan sinyal `SIGSYS` oleh seccomp filter Android bionic.
- `unshare(CLONE_NEWUSER)` mengembalikan `EINVAL` (unprivileged user namespaces dinonaktifkan oleh Google di kernel GKI Android).
- `unshare(CLONE_NEWPID | CLONE_NEWNET)` mengembalikan `EPERM` karena `CapEff=0`.
- `/sys/fs/cgroup` berstatus read-only bagi user Termux.

**Konsekuensi Logis:** Menjalankan Docker daemon (`dockerd`) di atas PRoot murni secara teknis mustahil tanpa kernel Linux sejati. Solusi arsitektural yang wajib diambil adalah **Modularisasi Runtime Backend**.

---

## 2. Arah Pengembangan Infrastruktur (Infrastructure Evolution)

### 2.1 Dual-Runtime Backend (`RuntimeBackend` Trait)
PDN tidak boleh terkunci hanya pada ptrace PRoot. Arsitektur backend harus diabstraksikan menjadi trait polimorfik:

```rust
#[async_trait]
pub trait RuntimeBackend: Send + Sync {
    async fn create_container(&self, spec: &ContainerSpec) -> Result<String>;
    async fn start_container(&self, id: &str) -> Result<()>;
    async fn exec_process(&self, id: &str, exec_spec: &ExecSpec) -> Result<ChildProcess>;
    async fn stop_container(&self, id: &str, timeout_s: u32) -> Result<()>;
    async fn inspect_container(&self, id: &str) -> Result<ContainerInspection>;
}
```

1. **`ProotBackend` (Lightweight & Fast)**:
   - Digunakan untuk 80% kebutuhan pengguna: shell Debian/Ubuntu/Alpine, compiler development, Python/Node.js, Playwright, toolchain hacking.
   - Karakteristik: Cold-start 0.09s, zero memory overhead virtualisasi, eksekusi native instructions di CPU host.
2. **`UmlBackend` (Kernel & Docker Full-Fidelity)**:
   - Menghubungkan PDN ke daemon User-Mode Linux (`uml4termuxd`) melalui UNIX domain socket protocol.
   - Digunakan saat pengguna menjalankan perintah `podnext docker` atau container yang membutuhkan systemd, iptables, wireguard, overlayfs, dan cgroup controllers.
   - Menyediakan isolasi kernel independen tanpa membutuhkan akses root Android.

### 2.2 Kemasan Termux & Standarisasi Ekosistem (`termux-packages`)
- **Single Static vs Dynamic Linking**: Memisahkan biner menjadi `podnext` (CLI) dan `podnext-daemon` (supervisor service).
- **Termux Build Recipe**: Memperbarui `termux-package/recipe/build.sh` agar sinkron dengan NDK modern dan dapat langsung diajukan ke upstream Termux official repo atau Termux user repository.
- **Runit Service Scripts**: Menyediakan template runit service bawaan (`$PREFIX/var/service/podnextd`) agar supervisor dapat dikelola langsung via `sv up podnextd` / `sv down podnextd`.

### 2.3 Mitigasi Android Sleep & Android Phantom Process Killer
Android 12+ memiliki *Phantom Process Killer* (membatasi maksimal 32 child processes background) dan *Doze Mode*:
- **Android WakeLock Engine**: Otomatisasi akuisisi `PARTIAL_WAKE_LOCK` saat container berstatus `supervised` atau `service` berjalan.
- **Foreground Service Integration**: Integrasi opsional dengan Termux:API atau Termux:Boot untuk memunculkan notifikasi persisten agar kernel Android tidak mematikan supervisor saat RAM menipis.

---

## 3. Arah Pengembangan Arsitektur (Architectural Design)

### 3.1 Client-Daemon IPC Architecture (Decoupled Client/Server)
Saat ini beberapa subperintah mengeksekusi operasi lokal langsung ke disk, sementara yang lain menggunakan D-Bus atau daemon. Arsitektur harus distandarisasi:
- **Thin Client**: Biner `podnext` berfungsi sebagai lightweight client yang menerjemahkan argumen CLI menjadi panggilan RPC.
- **Robust Daemon (`podnextd`)**: Supervisor yang memegang state tunggal, lock manager, cgroup controller, dan routing network.
- **Protokol IPC**: Berbasis UNIX Domain Socket dengan JSON-RPC 2.0 atau D-Bus Wire Protocol di `$TMPDIR/podnext/podnextd.sock`. Jika daemon tidak aktif, CLI memiliki fallback *direct-execution* mode yang transparan.

### 3.2 Event-Driven Reactive State Engine
- **State Store**: Menggantikan pembacaan direktori parsial dengan database lokal terstruktur dan tahan korupsi (`sled` atau `sqlite` embedded di `$PN_DATA_DIR/state.db`).
- **Atomic State Transitions**: Setiap perubahan status (`Creating` -> `Installing` -> `Stopped` -> `Running` -> `Failed`) dicatat secara atomik melalui write-ahead journal (WAL) sehingga tidak ada container yang "terkunci" atau berstatus yatim akibat Termux ditutup paksa.
- **Event Broadcasting**: Memanfaatkan Tokio broadcast channel untuk mendistribusikan event ke CLI (`podnext log follow`), D-Bus monitor, dan Web Dashboard secara real-time.

### 3.3 Container Runtime Interface (CRI) & OCI Stabilization
- Menuntaskan modul `pn-next/src/runtime.rs` (CRI gRPC layer berbasis `tonic`).
- Mengaktifkan kompatibilitas dengan orchestration engine minimalis (seperti `k3s` atau custom agent runner) sehingga satu perangkat Android Termux dapat bertindak sebagai node worker edge-computing mandiri.

---

## 4. Arah Pengembangan Modul & Helper (Modules & Helpers)

### 4.1 Modul Penyimpanan (Storage & Layer Management)
- **OverlayFS Emulation / Native Directory Branching**:
  - Saat ini container mengandalkan direktori rootfs hasil ekstraksi penuh.
  - *Roadmap*: Implementasikan sistem layer OCI berbasis symlink/hardlink tree atau fuse-overlayfs rootless untuk menghemat storage Termux secara drastis (satu base image Debian dapat dipakai oleh 5 container berbeda tanpa duplikasi file 500MB).
- **Garbage Collection & Prune Engine**:
  - Modul otomatis pembersih blob layer OCI yang tidak lagi terikat (`podnext image prune`, `podnext volume prune`).

### 4.2 Modul Jaringan (Rootless Networking & DNS Helper)
- **Gvproxy / Slirp4netns Integration**:
  - Menyediakan emulasi TCP/IP user-space murni.
  - Mendukung port mapping dinamis (`podnext run -p 8080:80`) tanpa memerlukan iptables atau akses root Android.
- **Adaptive DNS Helper**:
  - Android sering mengubah nameserver saat berpindah dari Wi-Fi ke jaringan seluler.
  - Tambahkan resolver internal yang memetakan `/etc/resolv.conf` di guest ke localhost DNS forwarder di daemon PDN, mencegah putusnya koneksi internet di dalam container saat koneksi perangkat beralih.

### 4.3 Modul Supervisor, Watchdog & IPC
- **Adaptive Backoff & Flapping Detection**:
  - Peningkatan circuit breaker: mendeteksi crash loop berkelanjutan (flapping) dan memasukkan service ke status `Quarantine` alih-alih membebani CPU dengan restart terus-menerus.
- **Systemd Unit File Importer**:
  - Helper module untuk membaca file `.service` systemd sederhana dan mengonversinya secara otomatis ke format konfigurasi `podnext service`.

---

## 5. Arah Pengembangan Keamanan (Security Hardening)

### 5.1 Zero-Trust Guest Boundary
- Mengadopsi prinsip yang telah terbukti pada perbaikan path traversal `copy`: **segala input dari dalam guest adalah data tidak tepercaya**.
- Semua symlink yang mengarah ke luar rootfs (`/data/data/com.termux`, `/storage/emulated/0`) harus dicegat pada level VFS PRoot dan ditolak secara default kecuali pengguna memberikan bendera `--bind` eksplisit.

### 5.2 Seccomp Filter Profiling Berbasis Distro
- Bionic Android menolak berbagai syscall lama atau syscall kernel tertentu.
- Buat filter BPF yang memalsukan `ENOSYS` atau `EPERM` secara anggun untuk syscall seperti `sys_setxattr`, `keyctl`, atau `bpf`, sehingga program di dalam distro tidak mengalami crash `SIGSYS`.

---

## 6. Rencana Eksekusi Berfase (Phased Roadmap)

```
┌────────────────────────────────────────────────────────────────────────┐
│ FASE 1: STABILISASI CLIENT-DAEMON & DISTRO CORE (Status: SELESAI ✔)   │
│ - Perbaikan bug runtime, exit code, traversal copy, /dev/shm 1777     │
│ - Paritas fitur proot-distro (copy, sync, rename, reset)               │
│ - CI/CD 100% Hijau di master                                          │
├────────────────────────────────────────────────────────────────────────┤
│ FASE 2: DUAL-BACKEND ARCHITECTURE & DOCKER VIA UML (Prioritas Utama)   │
│ - Implementasi trait RuntimeBackend (ProotBackend & UmlBackend)        │
│ - Integrasi socket uml4termuxd untuk eksekusi kernel Linux bionic      │
│ - CLI routing: podnext docker -> forward to UML guest dockerd          │
├────────────────────────────────────────────────────────────────────────┤
│ FASE 3: USERSPACE NETWORKING (SLIRP/GVPROXY) & LAYER OVERLAYFS         │
│ - Integrasi gvproxy untuk rootless port forwarding host <-> guest      │
│ - Adaptive DNS helper untuk mobilitas jaringan Android                 │
│ - Deduplikasi layer OCI untuk efisiensi penyimpanan ponsel             │
├────────────────────────────────────────────────────────────────────────┤
│ FASE 4: CRI ORCHESTRATION & PRODUCTION PACKAGING                       │
│ - Stabilisasi CRI gRPC socket untuk worker node edge/k3s               │
│ - Termux official repository packaging submission                      │
│ - CLI Plugins via Wasm runtime                                         │
└────────────────────────────────────────────────────────────────────────┘
```

Dokumen ini menjadi cetak biru panduan formal pengembangan PDN untuk mentransformasikan ekosistem container di Android dari alat bantu scripting biasa menjadi standar platform container berkinerja tinggi, aman, dan siap produksi.
