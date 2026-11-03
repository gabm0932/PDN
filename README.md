# PRoot Distro Next (PDN) - Next Generation Container Platform

[![Rust](https://img.shields.io/badge/rust-1.70%2B-orange.svg)](https://www.rust-lang.org)
[![License](https://img.shields.io/badge/license-GPL--2.0--or--later-blue.svg)](LICENSE)
[![CI/CD](https://img.shields.io/badge/CI%2FCD-100%25%20passing-brightgreen.svg)](https://github.com/michaelamoetoevsky/PDN/actions)
[![Platform](https://img.shields.io/badge/platform-Android%20%7C%20Linux%20aarch64%20%2F%20x86__64-blue.svg)](https://termux.dev)

PRoot Distro Next (PDN / `podnext`) adalah platform manajemen container dan orkestrasi layanan generasi baru yang dirancang khusus untuk Termux di Android 16+ serta lingkungan Linux rootless.

Dibangun seluruhnya dalam bahasa Rust, PDN menyatukan ekosistem PRoot (C), PRoot-distro (Bash script), dan lapisan kompatibilitas Android modern ke dalam satu platform terpadu, aman dari kebocoran environment, dan berkecepatan tinggi.

---

## 1. Perbandingan: PodNext vs PRoot-Distro Termux

| Fitur / Parameter | `proot-distro` (Termux Bawaan) | `podnext` (PRoot Distro Next) | Keuntungan PDN |
|---|---|---|---|
| **Bahasa & Eksekusi** | Skrip Bash berlapis-lapis | Native Rust (single binary terkompilasi) | Zero bash-overhead, memori aman |
| **Kecepatan Cold-Start Login** | ~0.603 detik | **~0.097 detik** | **~5× lebih cepat** |
| **Kecepatan Daftar Container** | ~0.160 detik | **~0.008 detik** | **~20× lebih cepat** |
| **Isolasi Environment** | Bocor 24 s/d 163 env var host (PATH, ANDROID_ROOT, dll) | **Tersanitasi ketat (hanya 6 env var bersih)** | Container murni, tidak terkontaminasi Termux |
| **Identitas Root Guest** | Emulasi permisif sederhana | Emulasi `--change-id=0:0` murni | `apt`, `dpkg`, paket Linux berjalan lancar |
| **Shared Memory (`/dev/shm`)** | Sering menimpa host atau tidak mandiri | **/dev/shm terisolasi mandiri (mode 1777)** | **Chromium headless & Playwright berjalan mulus** |
| **Propagasi Exit Code** | Rentan terpotong atau collapse ke 1 | **Propagasi kode keluar anak akurat (ChildExit)** | Skrip automasi/CI menerima exit code asli |
| **Keamanan Pertukaran Berkas** | Standar tar pipe | **Pemeriksaan traversal ketat (tolak `..`)** | Menolak eksploitasi traversal ke rootfs host |
| **Sinkronisasi Berkas** | Tidak ada sinkronisasi sha256 bawaan | **`sync` dengan content sha256 & `--delete`** | Hemat I/O, hanya mentransfer delta perubahan |
| **Supervisi Layanan** | Tidak ada (skrip mati saat shell ditutup) | **Service supervisor, restart policy, watchdog** | Layanan tetap hidup di latar belakang |
| **Kontrol Sumber Daya** | Tidak ada | **Kuota cgroups memory & CPU** | Mencegah crash OOM pada perangkat mobile |
| **Integrasi Sistem Android** | Manual | **Termux:Boot autostart, WakeLock manager** | Layanan tetap berjalan saat layar mati |
| **IPC & Otomasi** | Tidak ada | **Broker D-Bus IPC & Manifes Deklaratif GitOps** | Standar industri enterprise di Android |
| **Dukungan Docker Kernel** | Mustahil (seccomp SIGSYS pada `mount(2)`) | **Arsitektur `RuntimeBackend` siap bridging UML** | Jalur jelas menuju Docker via UML VM |

---

## 2. Struktur Penggunaan: Dari Sederhana ke Lanjut

Penggunaan `podnext` telah dirapikan dan dibagi ke dalam 3 tingkatan yang jelas:

```
┌────────────────────────────────────────────────────────────────────────┐
│  LEVEL 1: SIMPLE & CORE DISTRO (Pengganti proot-distro sehari-hari)   │
│  install · login · run · list · copy · sync · rename · reset · remove  │
├────────────────────────────────────────────────────────────────────────┤
│  LEVEL 2: INTERMEDIATE (Multi-Container, Image OCI, Volume & Backup)  │
│  image · volume · network · backup · ps · sessions · attach · kill    │
├────────────────────────────────────────────────────────────────────────┤
│  LEVEL 3: ADVANCED (Service Supervisor, Watchdog, Quota, D-Bus, GitOps)│
│  service · daemon · watchdog · quota · log · dbus · gitops · boot      │
└────────────────────────────────────────────────────────────────────────┘
```

---

### Level 1: Penggunaan Sederhana & Sehari-hari (Core Distro)

Bagi pengguna yang ingin memakai PDN layaknya `proot-distro`, perintahnya sangat sederhana dan langsung:

#### 1. Mencari dan Memasang Distro
```bash
# Cari distro di registry OCI / Docker Hub
podnext search debian

# Pasang Debian 13 (Trixie)
podnext install debian:13

# Pasang Ubuntu dengan nama kustom
podnext install ubuntu:22.04 --name my-ubuntu
```

#### 2. Masuk Shell Interaktif atau Menjalankan Perintah
```bash
# Masuk ke dalam shell container sebagai root
podnext login debian

# Jalankan perintah langsung dari host tanpa masuk shell
podnext run debian -- apt update && apt install -y curl git
```

#### 3. Melihat Daftar dan Status Container
```bash
# Tampilkan daftar container terpasang
podnext list
```

#### 4. Pertukaran Berkas Cepat (Copy & Sync)
```bash
# Salin berkas dari container ke host (aman dari path traversal)
podnext copy debian:/etc/hosts ./hosts.bak

# Salin folder dari host ke dalam container secara rekursif
podnext copy ./my-project debian:/root/my-project -R

# Sinkronisasi folder (hanya transfer berkas yang berubah berdasarkan sha256)
podnext sync ./my-project debian:/root/my-project --delete
```

#### 5. Mengganti Nama, Reset, atau Menghapus Container
```bash
# Ganti nama container terpasang
podnext rename debian deb-prod

# Reset container ke instalasi bersih semula (mengunduh ulang layer awal)
podnext reset deb-prod

# Hapus container beserta seluruh datanya
podnext remove deb-prod --force
```

---

### Level 2: Penggunaan Menengah (Multi-Container, Storage & Backup)

Untuk manajemen container yang lebih fleksibel, snapshot cadangan, dan volume persisten:

#### 1. Manajemen Image OCI Lokal
```bash
# Lihat image yang tersimpan di cache lokal
podnext image ls

# Unduh image Alpine terbaru
podnext image pull alpine:latest
```

#### 2. Volume Penyimpanan Persisten
```bash
# Buat volume data
podnext volume create postgres-data

# Tampilkan daftar volume
podnext volume ls

# Bersihkan volume yang tidak terikat
podnext volume prune
```

#### 3. Jaringan Bridge Terisolasi
```bash
# Buat network bridge terisolasi
podnext network create internal-net

# Tampilkan jaringan aktif
podnext network ls
```

#### 4. Snapshot Backup dan Restore
```bash
# Buat arsip snapshot penuh container ke tarball terkompresi
podnext backup create deb-prod

# Tampilkan riwayat backup yang tersedia
podnext backup list

# Pulihkan snapshot ke container baru
podnext backup restore <backup-id> --target deb-restored
```

---

### Level 3: Penggunaan Tingkat Lanjut (Layanan Sistem, Daemon & IPC)

Untuk menjalankan layanan latar belakang (background services), otomatisasi saat booting Termux, kontrol kuota memori/CPU, dan GitOps:

#### 1. Daemon Supervisor PDN
```bash
# Jalankan background daemon supervisor
podnext daemon start

# Periksa status daemon
podnext daemon status
```

#### 2. Pengelolaan Layanan (Service Management)
```bash
# Jalankan managed service dengan restart policy otomatis
podnext service start web-server

# Pantau status service dan PID proses
podnext service status web-server

# Lihat log keluaran service
podnext service logs web-server
```

#### 3. Kuota Sumber Daya (Resource Quotas)
```bash
# Batasi memori maksimal container ke 512MB
podnext quota set web-server memory 512M

# Batasi penggunaan CPU maksimal (2 core = 200%)
podnext quota set web-server cpu 200

# Pantau pemakaian CPU & RAM real-time
podnext quota usage web-server
```

#### 4. Watchdog & Circuit Breaker
```bash
# Atur kebijakan restart jika service gagal: maksimal 3 kali dalam 60 detik
podnext watchdog policy set web-server OnFailure 3 60

# Periksa status circuit breaker
podnext watchdog status
```

#### 5. IPC D-Bus & Integrasi GitOps
```bash
# Jalankan D-Bus broker mandiri pada UNIX socket
podnext dbus serve --socket $TMPDIR/pdn-dbus.sock

# Panggil method service via D-Bus
podnext dbus call org.pnnext.Service /org/pnnext/Service org.pnnext.Service.List --socket $TMPDIR/pdn-dbus.sock

# Sinkronkan manifest deklaratif aplikasi (Container & Volume) via GitOps
podnext gitops sync --repo ./manifests

# Cek status drift aplikasi deklaratif
podnext gitops drift
```

#### 6. Menjaga Container Tetap Aktif di Android
```bash
# Ambil Android Wake Lock agar container tidak mati saat layar mati
podnext wakelock acquire web-server Partial

# Pasang auto-start saat Termux booting (memerlukan Termux:Boot)
podnext boot enable web-server
```

---

## 3. Kompatibilitas Khusus: Browser & Playwright

PDN telah divalidasi mampu menjalankan browser modern di dalam container rootless berkat konfigurasi mandiri `/dev/shm` (POSIX shared memory mode `1777`):

```bash
# Di dalam container debian PDN:
apt update && apt install -y chromium chromium-sandbox
chromium --headless=new --no-sandbox --disable-gpu --dump-dom "https://example.com"
```
*Hasil: DOM HTML ter-dumping sempurna tanpa error shared memory (`EACCES`/`ENOSPC`).*

---

## 4. Status Verifikasi & CI/CD Pipeline

Pipeline pengujian GitHub Actions berada dalam status **100% HIJAU (Passing)**:

| Workflow | Jobs & Matrix | Status |
|---|---|---|
| **PDN CI/CD** | Host Build, Integration Test (133 passed), Cross-Compile aarch64, Fuzzing, Docs, Security Audit | **SUCCESS** |
| **Android Cross-Compile** | Matrix: `aarch64-linux-android` & `x86_64-linux-android`, NDK `r27c` & `r30` | **SUCCESS** |
| **Benchmark Suite** | Performance & latency benchmark | **SUCCESS** |

---

## 5. Arsitektur Crates

PDN dipecah secara modular menjadi 6 crates Rust:

| Crate | Peran / Tanggung Jawab |
|---|---|
| `proot-core` | Inti PRoot: ptrace interception, penerjemahan syscall, VFS, path resolution, seccomp BPF. |
| `proot-distro-core` | Manajemen container, tar layer unpacking (SELinux-safe hardlinks), verifikasi integritas digest OCI, `/dev/shm` store, OCI manifest handling. |
| `proot-compat` | Lapisan kompatibilitas Android: shim personality, mount helper, translasi cgroup v2, stub sysfs. |
| `pn-gateway` | Fail-safe manager, guardrails batas container, health checker, Prometheus metrics. |
| `pn-next` | Runtime tingkat lanjut: scheduler, network bridge, volume drivers, D-Bus broker, supervisor service, GitOps engine. |
| `podnext` | CLI frontend terpadu dan biner supervisor daemon (`podnext`, `podnext-daemon`). |

---

## 6. Dokumentasi Lanjutan

- [Panduan Migrasi dari proot-distro](docs/src/migration-guide.md)
- [Referensi Lengkap CLI Command](docs/src/cli-reference.md)
- [Analisa Arsitektur & Roadmap Pengembangan PDN](docs/ARCHITECTURAL_ANALYSIS_AND_ROADMAP.md)
- [Riset & Pengujian Kapabilitas Internal](research/INTERNAL_CAPABILITY_TESTS_RUN2.md)
- [Blueprint Integrasi Docker Runtime via UML](research/DOCKER_RUNTIME_INTEGRATION_PLAN.md)

---

## Lisensi

GPL-2.0-or-later.
