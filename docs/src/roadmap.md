# Analisis Komprehensif Arsitektur & Roadmap Pengembangan PDN (PRoot Distro Next)

**Tanggal:** November 2026  
**Status Basis:** v0.1.7 (Stable master commit `a13a258`, 100% Green CI/CD)
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
   - *Dynamic Distro Identification*: Deteksi OS riil via `/etc/os-release` dan sinkronisasi penamaan pada `podnext list`.
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

## 2. Peta Komparasi Forensik Source Code: PRoot C Engine & PRoot-distro v5.10.3 Framework

Untuk merancang arsitektur PDN yang kokoh dan dapat dieksekusi dalam satu lintasan kerja (*single-pass execution*), audit forensik mendalam telah dilakukan langsung terhadap source code PRoot C (`source/proot-src`) dan PRoot-distro Python (`proot_distro/` v5.10.3) di Termux.

### 2.1 Anatomi & Mekanisme PRoot C Engine (`proot-src`)

| Modul C PRoot | Berkas Sumber | Mekanisme & Logika Internal | Implikasi Arsitektural untuk PDN |
|---------------|---------------|-----------------------------|-----------------------------------|
| **Ptrace Event Loop & Sinyal** | `src/tracee/event.c`, `src/ptrace/wait.c` | PRoot memasang `SIG_IGN` untuk hampir semua sinyal (`SIGINT`, `SIGTERM`, `SIGHUP`). Hanya sinyal tertentu (`SIGQUIT`, `SIGSEGV`, `SIGABRT`) yang memicu handler `kill_all_tracees2()`. PRoot juga tidak memasang flag `PTRACE_O_EXITKILL`. | **Penyebab Zombie Process**: `kill -9` pada proot akan meninggalkan seluruh tracee berjalan dan ter-reparent ke init. Penjinakan harus menggunakan eskalasi terarah: kirim `SIGQUIT` terlebih dahulu ke proot. |
| **VFS Syscall Interception** | `src/syscall/enter.c`, `src/syscall/exit.c` | `enter.c` (94KB) mencegat pemanggilan path (`openat`, `stat`, `execve`, `chdir`) dan memetakan path absolut guest ke host rootfs. `exit.c` (23KB) memalsukan nilai return `stat`/`fstat`/`statx` (`uid=0`, `gid=0`, fake link size, fake mountinfo). | PDN Rust (`proot-core`) wajib menjaga akurasi parsing register `orig_x0` / `x8` (aarch64) dan memanipulasi memori tracee via `process_vm_readv/writev`. |
| **Link2Symlink Extension** | `src/extension/link2symlink/` | Mengakali SELinux Android yang menolak `link(2)` pada storage `/data`. Hardlink digantikan dengan berkas intermediate berawalan `.proot.l2s.` atau `.l2s.`, lalu mencegat `stat` untuk memalsukan inode dan link count `st_nlink > 1`. | Saat container di-rename/copy, symlink l2s yang memuat path absolut rootfs lama akan patah jika tidak di-rewrite secara in-place. |
| **SysV IPC In-Memory** | `src/extension/sysvipc/` | Mengemulasikan tabel System V IPC (`shmget`, `semget`, `msgget`) secara in-memory di dalam proses PRoot tanpa syscall kernel host. | Diperlukan oleh aplikasi lawas/database Linux yang mengandalkan SysV IPC. |
| **Ashmem / Memfd** | `src/extension/ashmem_memfd/` | Mengemulasikan `memfd_create` menggunakan `/dev/ashmem` atau file anonim temporary jika kernel host tidak mendukung `memfd_create`. | Menjamin browser engine dan runtime modern (V8/Chromium/Electron) tidak mengalami crash pada kernel lawas. |
| **Kernel Spoofing (`kompat`)** | `src/extension/kompat/` | Memalsukan string rilis kernel (`--kernel-release=\\Linux\\...`) agar GNU C library (glibc) tidak memicu error fatal `FATAL: kernel too old`. | Wajib dipertahankan di PDN untuk interoperabilitas distro modern (Ubuntu 24.04/Debian 13) di Android. |
| **Seccomp Android Filter** | `patches/proot/seccomp-android.patch` | Mem-bypass syscall modern yang diblokir oleh seccomp filter Bionic Android 16: `arch_prctl`, `rseq`, `membarrier`, `process_vm_readv/writev`, `clone3`, `openat2`, `mlock/munlock`. | Menjamin kompatibilitas toolchain Node.js, Rust stdlib, dan Chromium di Android 16. |

### 2.2 Anatomi & Mekanisme PRoot-distro Framework (`proot_distro` Python v5.10.3)

| Modul Python | Berkas Sumber | Mekanisme & Logika Internal | Implikasi Arsitektural untuk PDN |
|--------------|---------------|-----------------------------|-----------------------------------|
| **Hardened Descriptor VFS** | `dirfd.py` (57KB) | Seluruh traversal filesystem menggunakan descriptor `openat(2)` berurutan dengan flag `O_NOFOLLOW`. Melindungi dari serangan symlink swap yang dilakukan guest di sela-sela eksekusi host. Menggunakan `O_NONBLOCK` untuk membuka berkas reguler (`open_regular_at`) guna mencegah hang akibat FIFO (named pipe) jahat. Menggunakan `_chmod_fd` karena `fchmodat` Linux tidak memiliki `AT_SYMLINK_NOFOLLOW`. | PDN wajib mengganti pembacaan path string rekursif dengan abstraksi Rust `DirFd` berbasis `nix::fcntl::openat`. |
| **Inheritable POSIX Lock Manager** | `locking.py` | Menggunakan POSIX `flock(2)` non-blocking. Terbagi menjadi 2 namespace: `ContainerLock` (Shared untuk `login`/`run`/`backup`; Exclusive untuk `install`/`remove`/`reset`/`rename`/`copy`) dan `BuildLock` (Exclusive per image-tag). Flag `O_CLOEXEC` dihapus sebelum `execvpe()` sehingga descriptor lock diwarisi oleh PRoot sepanjang sesi aktif. | Mencegah race condition fatal di mana dua proses paralel merusak rootfs yang sama. |
| **Session Registry & Process Teardown** | `session.py`, `commands/kill.py` | Registri sesi aktif disimpan di `$RUNTIME_DIR/sessions/<pid>.json`. Liveness dicek via shared flock probe (jika flock berhasil, sesi sudah mati dan file registry dibersihkan). Perintah `kill` memindai descriptor proses di `/proc/*/fd` untuk menemukan inode sesi, lalu mengirimkan eskalasi sinyal: `SIGQUIT` (agar PRoot membunuh seluruh tracee secara graceful) disusul `SIGKILL` pada proses yatim yang tersisa. | Memungkinkan perintah `podnext ps` yang akurat dan `podnext kill` yang bersih tanpa meninggalkan zombie. |
| **Dynamic Sysdata Generator** | `sysdata.py` | Mengkonstruksi `/proc/stat` secara dinamis sesuai jumlah core online di host `/proc/cpuinfo`. Memetakan `/proc/loadavg`, `/proc/version`, `/proc/uptime`, `/proc/sys/fs/overflowuid` (65534), `/proc/sys/fs/overflowgid`, `/proc/sys/kernel/cap_last_cap`. Berkas dibuat di luar guest dan dibind ke dalam container. | Menghindari crash pada `htop`, `top`, dan Go runtime akibat ketidaksinkronan topologi CPU di Android. |
| **Link2Symlink Tree Rewriter** | `l2s.py` | Memindai dan menulis ulang target symlink `.proot.l2s.` / `.l2s.` secara in-place setelah operasi pemindahan rootfs (`rename`, `copy`, migrasi). Dilengkapi penangkap sinyal `SIGINT`/`SIGQUIT` agar rewrite tidak terpotong di tengah jalan. | Mencegah kerusakan integritas tautan keras saat container diganti namanya atau disalin. |
| **OCI Dockerfile Builder & Layer Differ** | `commands/build.py`, `helpers/dockerfile.py`, `helpers/layer_diff.py`, `helpers/oci_writer.py` | Mem-parse sintaks Dockerfile (`FROM`, `RUN`, `COPY`, `ENV`), mengeksekusi instruksi `RUN` di dalam container sementara, menghitung delta filesystem (layer diff) dengan whiteout markers (`.wh.<file>`), dan menghasilkan manifest serta tarball OCI v1 standar. | Menjadikan PDN platform mandiri yang mampu membangun container image lokal tanpa bantuan Docker daemon. |

---

## 3. Evaluasi Celah Kematangan & Peluang Pengayaan di PDN

Berdasarkan perbandingan antara fondasi PDN saat ini (`proot-core`, `proot-distro-core`, `podnext`) dengan source code di atas, berikut adalah celah fungsional yang wajib ditutup:

1. **Celah Pelacakan Sesi & Pembunuhan Bersih (`session` & `kill`)**:
   - *Status Saat Ini*: PDN belum memiliki registri sesi per-PID dengan pewarisan descriptor `flock(2)` ke child process.
   - *Risiko*: Jika sesi `podnext run` diinterupsi paksa, proses guest di latar belakang dapat tertinggal sebagai proses yatim di Termux.
2. **Celah Keamanan VFS pada Host-Guest Boundary (`dirfd`)**:
   - *Status Saat Ini*: Beberapa operasi di `proot-distro-core` masih menggunakan manipulasi path `std::path::PathBuf` standar.
   - *Risiko*: Kerentanan terhadap serangan *symlink planting* atau *FIFO stall* jika user mengeksekusi copy dari container yang tidak tepercaya.
3. **Celah Integritas Link2Symlink pada Rename & Copy (`l2s`)**:
   - *Status Saat Ini*: Operasi `podnext rename` dan `podnext copy` memindahkan/menyalin folder rootfs secara utuh, namun belum memperbarui isi target symlink `.proot.l2s.` di dalamnya.
   - *Risiko*: Berkas-berkas hardlink di dalam distro (seperti `/usr/bin/perl` atau hardlink package manager) menjadi tautan putus (*dangling symlinks*) setelah rename.
4. **Celah Pembuatan Image Kustom (`build`)**:
   - *Status Saat Ini*: PDN dapat mengunduh dan memasang image dari Docker Hub OCI registry, tetapi belum memiliki subperintah `podnext build` untuk membangun image dari Dockerfile lokal.
5. **Celah Sinkronisasi Topologi CPU (`sysdata`)**:
   - *Status Saat Ini*: Generator `/proc/stat` di `sysdata.rs` masih menggunakan tabel baris statis dan belum membaca dinamis core yang offline di Android.

---

## 4. Arah Pengembangan Infrastruktur (Infrastructure Evolution)

### 4.1 Dual-Runtime Backend (`RuntimeBackend` Trait)
PDN mengadopsi arsitektur dual-runtime yang memisahkan backend eksekusi secara modular:

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

### 4.2 Kemasan Termux & Standarisasi Ekosistem (`termux-packages`)
- **Single Static vs Dynamic Linking**: Memisahkan biner menjadi `podnext` (CLI) dan `podnext-daemon` (supervisor service).
- **Termux Build Recipe**: Memperbarui `termux-package/recipe/build.sh` agar sinkron dengan NDK modern dan dapat langsung diajukan ke upstream Termux official repo atau Termux user repository.
- **Runit Service Scripts**: Menyediakan template runit service bawaan (`$PREFIX/var/service/podnextd`) agar supervisor dapat dikelola langsung via `sv up podnextd` / `sv down podnextd`.

### 4.3 Mitigasi Android Sleep & Android Phantom Process Killer
Android 12+ memiliki *Phantom Process Killer* (membatasi maksimal 32 child processes background) dan *Doze Mode*:
- **Android WakeLock Engine**: Otomatisasi akuisisi `PARTIAL_WAKE_LOCK` saat container berstatus `supervised` atau `service` berjalan.
- **Foreground Service Integration**: Integrasi opsional dengan Termux:API atau Termux:Boot untuk memunculkan notifikasi persisten agar kernel Android tidak mematikan supervisor saat RAM menipis.

---

## 5. Arah Pengembangan Arsitektur (Architectural Design)

### 5.1 Client-Daemon IPC Architecture (Decoupled Client/Server)
- **Thin Client**: Biner `podnext` berfungsi sebagai lightweight client yang menerjemahkan argumen CLI menjadi panggilan RPC.
- **Robust Daemon (`podnextd`)**: Supervisor yang memegang state tunggal, lock manager, cgroup controller, dan routing network.
- **Protokol IPC**: Berbasis UNIX Domain Socket dengan JSON-RPC 2.0 atau D-Bus Wire Protocol di `$TMPDIR/podnext/podnextd.sock`. Jika daemon tidak aktif, CLI memiliki fallback *direct-execution* mode yang transparan.

### 5.2 Event-Driven Reactive State Engine
- **State Store**: Menggantikan pembacaan direktori parsial dengan database lokal terstruktur dan tahan korupsi (`sled` atau `sqlite` embedded di `$PN_DATA_DIR/state.db`).
- **Atomic State Transitions**: Setiap perubahan status (`Creating` -> `Installing` -> `Stopped` -> `Running` -> `Failed`) dicatat secara atomik melalui write-ahead journal (WAL) sehingga tidak ada container yang "terkunci" atau berstatus yatim akibat Termux ditutup paksa.
- **Event Broadcasting**: Memanfaatkan Tokio broadcast channel untuk mendistribusikan event ke CLI (`podnext log follow`), D-Bus monitor, dan Web Dashboard secara real-time.

### 5.3 Container Runtime Interface (CRI) & OCI Stabilization
- Menuntaskan modul `pn-next/src/runtime.rs` (CRI gRPC layer berbasis `tonic`).
- Mengaktifkan kompatibilitas dengan orchestration engine minimalis (seperti `k3s` atau custom agent runner) sehingga satu perangkat Android Termux dapat bertindak sebagai node worker edge-computing mandiri.

---

## 6. Spesifikasi Detail Mekanisme, Modul & Helper untuk Eksekusi Terpadu (Single-Pass Plan)

Berikut adalah cetak biru teknis per modul yang dirancang secara detail agar seluruh komponen dapat diimplementasikan dalam 1 kali jalan sekaligus:

### 6.1 Modul `proot-distro-core::session` & `podnext kill` / `podnext ps`
- **Tujuan**: Registri sesi container aktif dan penjinakan proses yang andal.
- **Implementasi**:
  1. *Struktur Data*: Berkas `$RUNTIME_DIR/sessions/<pid>.json` memuat `container_name`, `pid`, `started_at`, `login_uid`, dan `cmdline`.
  2. *Inherited Lock*: Sebelum melakukan `execvpe` ke proot, buka berkas sesi dan ambil `flock(fd, LOCK_EX)`. Hapus flag `FD_CLOEXEC` pada descriptor tersebut agar proot dan seluruh tracee mewarisi descriptor ini.
  3. *Liveness Probe*: Perintah `podnext ps` membuka setiap file JSON di direktori sessions dan mencoba `flock(fd, LOCK_SH | LOCK_NB)`. Jika probe berhasil (lock didapat), artinya tidak ada proses yang memegang fd (sesi telah mati) → file dihapus secara aman. Jika probe ditolak (`EWOULDBLOCK`), sesi terverifikasi masih hidup.
  4. *Teardown Engine (`podnext kill <container> [--all]`)`:
     - Pindai `/proc/[0-9]*/fd/*` untuk mencocokkan inode berkas registry sesi.
     - Kirim sinyal bertahap (*signal escalation*):
       1. Tahap 1: Kirim `SIGQUIT` ke root PID proot (memicu handler `kill_all_tracees2` proot secara anggun).
       2. Tahap 2: Tunggu 500ms; periksa apakah proses masih aktif.
       3. Tahap 3: Jika masih ada proses guest yang tertinggal (reparented ke init), kirim `SIGTERM` lalu `SIGKILL` langsung ke PID tersebut.

### 6.2 Modul `proot-distro-core::dirfd` (Hardened Descriptor VFS)
- **Tujuan**: Mencegah race condition symlink dan DoS FIFO saat memanipulasi filesystem guest.
- **Implementasi**:
  1. Buat struct wrapper `DirFd(RawFd)` di Rust dengan penutupan otomatis via `Drop`.
  2. Implementasikan metode:
     - `open_dir_at(&self, name: &str) -> Result<DirFd>` dengan flag `O_NOFOLLOW | O_DIRECTORY | O_CLOEXEC`.
     - `open_regular_at(&self, name: &str, flags: OFlag) -> Result<File>` dengan flag `O_NOFOLLOW | O_NONBLOCK | O_CLOEXEC`, lalu verifikasi dengan `fstat` bahwa berkas adalah `S_IFREG` (menolak FIFO/socket yang bisa memicu hang).
     - `safe_chmod_at(&self, fd: RawFd, mode: u32) -> Result<()>` yang memanggil `fchmod(fd, mode)` langsung pada descriptor terverifikasi, bukan melalui path.
     - `atomic_write_at(&self, name: &str, content: &[u8]) -> Result<()>` yang menulis ke file sementara dengan `O_EXCL` lalu memindahkannya via `renameat(2)`.

### 6.3 Modul `proot-distro-core::l2s` (Link2Symlink Tree Rewriter)
- **Tujuan**: Memperbaiki symlink tautan keras setelah container dipindahkan atau diganti namanya.
- **Implementasi**:
  1. Buat fungsi `rewrite_l2s_tree(rootfs: &Path, old_rootfs: &Path, new_rootfs: &Path) -> Result<usize>`.
  2. Lakukan traversal rekursif terhadap rootfs:
     - Periksa setiap symlink: jika target membaca pola `.proot.l2s.` atau `.l2s.` yang diawali oleh `old_rootfs`, ganti prefix tersebut dengan `new_rootfs`.
     - Gunakan penulisan symlink atomik: buat symlink sementara lalu replace via `renameat`.
  3. Lindungi proses rewrite dengan signal blocker (`SIGINT`/`SIGTERM`) untuk mencegah pembatalan operasi di tengah jalan yang berisiko meninggalkan symlink korup.
  4. Panggil modul ini secara otomatis di akhir subperintah `podnext rename` dan `podnext copy`.

### 6.4 Modul `podnext build` (Native Dockerfile Builder & OCI Layer Differ)
- **Tujuan**: Memungkinkan pembuatan image kustom langsung dari Dockerfile di Termux.
- **Implementasi**:
  1. *Parser Dockerfile*: Ekstraksi instruksi `FROM`, `RUN`, `COPY`, `ADD`, `ENV`, `WORKDIR`, `EXPOSE`, `ENTRYPOINT`, `CMD`.
  2. *Build Execution Engine*:
     - Inisialisasi container temporary dari base image `FROM`.
     - Eksekusi setiap baris `RUN` di dalam environment PRoot sandbox terisolasi.
  3. *Layer Differ*:
     - Hitung checksum SHA256 dari berkas yang berubah atau baru ditambahkan.
     - Deteksi berkas yang terhapus dan sisipkan berkas whiteout OCI (format `.wh.<filename>`).
     - Kompresi delta perubahan menjadi layer `tar.gz`.
  4. *OCI Serializer*: Hasilkan manifest OCI v1 standar, image config JSON, dan catat ke image cache lokal.

### 6.5 Modul `proot-distro-core::sysdata` (Dynamic CPU Topology Generator)
- **Tujuan**: Menghindari crash pada aplikasi pemantau sistem (htop, top, node.js) akibat pembacaan core CPU yang keliru di Android.
- **Implementasi**:
  1. Baca `/proc/cpuinfo` host secara riil dan kumpulkan daftar ID core online.
  2. Hasilkan `/proc/stat` sintetis dengan jumlah baris `cpu0`, `cpu1`, ... yang persis sama dengan core aktif host.
  3. Sediakan nilai default untuk `/proc/loadavg` (`0.12 0.07 0.02 2/165 765`), `/proc/uptime` (`124.08`), `/proc/sys/fs/overflowuid` (`65534`), dan `/proc/sys/kernel/cap_last_cap`.
  4. Simpan stub sysdata ini di luar direktori guest (`$CONTAINER_DIR/sysdata/`) dan bind secara read-only ke guest.

### 6.6 Modul `proot-distro-core::locking` (Inheritable POSIX Lock Manager)
- **Tujuan**: Mencegah tabrakan konkurensi antar perintah CLI PDN.
- **Implementasi**:
  1. Struct `ContainerLock` mengelola file `$RUNTIME_DIR/locks/<container>.lock`.
     - Mode `Shared`: untuk `login`, `run`, `backup`, `copy` (source).
     - Mode `Exclusive`: untuk `install`, `remove`, `reset`, `rename`, `copy` (destination).
  2. Struct `BuildLock` mengelola file `$RUNTIME_DIR/locks/build/<image_tag>.lock` (selalu `Exclusive`).
  3. Tangani re-entrancy secara aman agar pemanggilan bersarang (misal `reset` yang memanggil `install`) tidak mengalami deadlock.

### 6.7 Modul Jaringan (Rootless Networking & Dynamic DNS Helper)
- **Gvproxy / Slirp4netns Integration**:
  - Menyediakan emulasi TCP/IP user-space murni.
  - Mendukung port mapping dinamis (`podnext run -p 8080:80`) tanpa memerlukan iptables atau akses root Android.
- **Adaptive DNS Helper**:
  - Android sering mengubah nameserver saat berpindah dari Wi-Fi ke jaringan seluler.
  - Tambahkan resolver internal yang memetakan `/etc/resolv.conf` di guest ke localhost DNS forwarder di daemon PDN, mencegah putusnya koneksi internet di dalam container saat koneksi perangkat beralih.

### 6.8 Modul Penyimpanan (Storage & Layer Deduplication)
- **OverlayFS Emulation / Reflink Deduplication**:
  - Memanfaatkan `fuse-overlayfs` rootless atau mekanisme symlink/hardlink branching untuk menghemat penyimpanan internal perangkat secara signifikan (beberapa container berbasis Debian 13 dapat berbagi base rootfs yang sama).
- **Garbage Collection & Prune Engine**:
  - Modul otomatis pembersih blob layer OCI yang tidak lagi terikat (`podnext image prune`, `podnext volume prune`).

### 6.9 Modul Supervisor, Watchdog & IPC
- **Adaptive Backoff & Flapping Detection**:
  - Peningkatan circuit breaker: mendeteksi crash loop berkelanjutan (flapping) dan memasukkan service ke status `Quarantine` alih-alih membebani CPU dengan restart terus-menerus.
- **Systemd Unit File Importer**:
  - Helper module untuk membaca file `.service` systemd sederhana dan mengonversinya secara otomatis ke format konfigurasi `podnext service`.

---

## 7. Arah Pengembangan Keamanan (Security Hardening)

### 7.1 Zero-Trust Guest Boundary
- Mengadopsi prinsip yang telah terbukti pada perbaikan path traversal `copy`: **segala input dari dalam guest adalah data tidak tepercaya**.
- Semua symlink yang mengarah ke luar rootfs (`/data/data/com.termux`, `/storage/emulated/0`) harus dicegat pada level VFS PRoot dan ditolak secara default kecuali pengguna memberikan bendera `--bind` eksplisit.

### 7.2 Seccomp Filter Profiling Berbasis Distro
- Bionic Android menolak berbagai syscall lama atau syscall kernel tertentu.
- Buat filter BPF yang memalsukan `ENOSYS` atau `EPERM` secara anggun untuk syscall seperti `sys_setxattr`, `keyctl`, atau `bpf`, sehingga program di dalam distro tidak mengalami crash `SIGSYS`.

---

## 8. Rencana Eksekusi Berfase Terpadu (Unified Single-Pass Master Plan)

Untuk mengimplementasikan seluruh perbaikan dan fitur di atas secara efisien dalam satu lintasan pengerjaan:

```
┌────────────────────────────────────────────────────────────────────────┐
│ TAHAP 1: DISTRO CORE & PROCESS LIFECYCLE (Target: PRoot Parity Plus)   │
│ - Implementasi proot-distro-core::session (Registry JSON & flock probe) │
│ - Implementasi proot-distro-core::dirfd (Hardened VFS & FIFO defense)  │
│ - Implementasi proot-distro-core::l2s (Symlink hardlink tree rewriter) │
│ - Implementasi podnext ps & podnext kill (SIGQUIT signal escalation)   │
│ - Dinamisasi sysdata.rs berdasarkan /proc/cpuinfo host                 │
├────────────────────────────────────────────────────────────────────────┤
│ TAHAP 2: CONTAINER IMAGE BUILDER & REPOSITORY (Target: Docker CLI Lite)│
│ - Implementasi parser Dockerfile (FROM, RUN, COPY, ENV, WORKDIR)       │
│ - Implementasi engine layer diffing dan whiteout (.wh.*) generator     │
│ - Implementasi CLI command podnext build -t <tag> <dir>                │
│ - Deduplikasi blob OCI dan podnext image prune                         │
├────────────────────────────────────────────────────────────────────────┤
│ TAHAP 3: DUAL-BACKEND (PROOT + UML) & ROOTLESS NETWORKING              │
│ - Abstraksi RuntimeBackend trait (ProotBackend & UmlBackend)            │
│ - Integrasi socket client ke uml4termuxd (eksekusi kernel bionic)      │
│ - Integrasi userspace TCP/IP networking (gvproxy / slirp4netns)        │
│ - Dynamic DNS helper untuk Android Wi-Fi / Cellular mobility           │
├────────────────────────────────────────────────────────────────────────┤
│ TAHAP 4: PRODUCTION PACKAGING, DAEMON & ORCHESTRATION                  │
│ - Stabilisasi CRI gRPC socket untuk node worker k3s                    │
│ - Penyerahan resep paket resmi Termux (termux-packages)                │
│ - Skrip layanan Runit bawaan ($PREFIX/var/service/podnextd)            │
│ - Integrasi WakeLock & Termux:Boot autostart supervisor                │
└────────────────────────────────────────────────────────────────────────┘
```

Dokumen ini menjadi cetak biru panduan formal pengembangan PDN untuk mentransformasikan ekosistem container di Android dari alat bantu scripting biasa menjadi standar platform container berkinerja tinggi, aman, dan siap produksi.
