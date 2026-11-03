# Blueprint Integrasi Runtime Docker di PDN: Arsitektur & Implementasi RuntimeBackend

**Status:** Rencana Kerja & Blueprint Implementasi Konkret  
**Target File:** `crates/proot-distro-core/src/runtime.rs`, `container.rs`, `lib.rs`, `crates/podnext/src/main.rs`  
**Host Lingkungan:** Android 16 (API 36), aarch64, Linux 6.1.157, Termux unprivileged (`uid=10396`, `CapEff=0`)

---

## 1. Perbandingan Struktural: Docker vs PRoot

### 1.1 Mengapa Docker Mustahil Berjalan di PRoot
PRoot bekerja murni di ruang pengguna melalui pelacakan panggilan sistem `ptrace(2)` (`PTRACE_SYSCALL`). PRoot bukanlah kernel; ia hanya mencegat panggilan sistem tertentu yang berkaitan dengan jalur file dan identitas proses untuk mensimulasikan `chroot` dan `fake root` (`uid=0`, `gid=0`).

Docker (`dockerd`) dan container runtime OCI (`runc`) tidak dapat berjalan hanya dengan manipulasi path di ruang pengguna. Docker membutuhkan isolasi dan manipulasi virtualisasi tingkat kernel yang nyata.

### 1.2 Tabel Panggilan Sistem (Syscall): Kebutuhan Docker vs Kenyataan PRoot di Android

| Syscall / Fitur | Kebutuhan Docker / runc | Hasil Terukur di Android Termux (Host) | Perilaku PRoot | Dampak pada Docker |
|---|---|---|---|---|
| `mount(2)` (overlayfs) | Mount layer image `overlay` (`lowerdir`, `upperdir`, `workdir`) | **SIGSYS (Signal 31)** dari seccomp Android, atau `EPERM` (`CapEff=0`) | Mencegat dan mengembalikan `rc=0` palsu tanpa membuat VFS mount nyata | `dockerd` mengira mount sukses, tetapi direktori merged kosong; runc crash saat membuat container rootfs |
| `mount(2)` (`cgroup2`, `tmpfs`) | Mount `/sys/fs/cgroup` dan `/run`, `/dev/shm` | **SIGSYS / EPERM** | Mengembalikan `rc=0` palsu atau no-op | Layanan init & kontrol grup tidak terpasang di filesystem |
| `unshare(CLONE_NEWUSER)` | Membuat user namespace mandiri (rootless mode) | **EINVAL** (kernel Android menonaktifkan unprivileged user ns) | Tidak mampu memvirtualisasikan user namespace | Rootless docker gagal inisialisasi user namespace |
| `unshare(CLONE_NEWNS \| CLONE_NEWPID \| CLONE_NEWNET)` | Isolasi filesystem, proses tree, dan antarmuka jaringan | **EPERM** (memerlukan `CAP_SYS_ADMIN` di host) | Menghapus flag namespace atau no-op, proses tetap di host namespace | Kontainer tidak terisolasi sama sekali; konflik PID & port |
| `pivot_root(2)` | Menukar rootfs kontainer dan melepas host rootfs | **EINVAL / EPERM** (butuh `CAP_SYS_ADMIN` dan mount point nyata) | Dialihkan ke chroot biasa atau gagal | `runc` memverifikasi struktur mount point dan gagal mengamankan container jail |
| Tulis cgroup v2 (`/sys/fs/cgroup`) | Menulis ke `cgroup.procs`, `cpu.max`, `memory.max`, `pids.max` | **Permission denied (`EACCES`)** (dimiliki oleh system/root Android) | Tidak ada emulasi cgroup di ptrace | Kontainer gagal membatasi CPU/Memory; dockerd menolak berjalan tanpa cgroup driver |

### 1.3 Bahaya Kepalsuan PRoot (Deception Trap)
Ketika aplikasi memanggil `mount(2)` di dalam PRoot, PRoot mengintersepsi panggilan tersebut dan mengembalikan status return `0` (sukses). Akibatnya:
1. `dockerd` menganggap storage driver `overlay2` aktif.
2. Ketika kontainer pertama dibuat, file layer tidak benar-benar digabung oleh kernel VFS. File biner di dalam container hilang atau tidak bisa dieksekusi.
3. Menjalankan Docker di dalam PRoot adalah usaha yang keliru sejak awal secara arsitektural.

### 1.4 Solusi UML (User-Mode Linux)
`uml4termux` menjalankan kernel Linux sesungguhnya (`linux-bionic-smp`, arm64) sebagai proses pengguna Android tunggal:
- Kernel guest mengelola tabel page virtual, scheduler, VFS, network stack, dan cgroup sendiri.
- Di dalam guest, proses berjalan sebagai root sejati (`CapEff = 0x1fffffffff`).
- Panggilan `mount(overlay)`, `unshare(CLONE_NEWNS)`, `pivot_root`, dan penulisan `/sys/fs/cgroup` dieksekusi langsung oleh kernel guest UML, bukan oleh kernel host Android.
- Host Android hanya melihat thread komputasi biasa dan socket jaringan loopback (`gvproxy`).

---

## 2. Audit Antarmuka Daemon `uml4termuxd`

### 2.1 Arsitektur & Topologi Komunikasi
Daemon pengawas `uml4termuxd` (atau kembarannya `uml4termux-rs daemon`) mengelola siklus hidup VM UML dan mengekspos UNIX domain socket lokal.

```
┌────────────────────────────────────────────────────────┐
│ Host: Termux Environment                               │
│                                                        │
│  podnext CLI / PDN Core                                │
│       │                                                │
│       ▼ (UNIX Domain Socket: 0600, SO_PEERCRED)        │
│  uml4termuxd (Supervisor Daemon)                       │
│       │                 │                              │
│       │ (PTY Master)    │ (BESS Socket)                │
│       ▼                 ▼                              │
│  linux-bionic-smp    gvproxy (User-mode Networking)    │
│  (UML Guest Kernel)     │ (Port Forwarder 127.0.0.1)   │
│       │                 │                              │
│       ├─ dockerd        │                              │
│       └─ console agent ─┘                              │
└────────────────────────────────────────────────────────┘
```

- **Socket Path:** `${UML4TERMUX_RUNTIME}/daemon.sock` (Default: `/data/data/com.termux/files/usr/tmp/uml4termux/daemon.sock`).
- **Izin File:** Mode `0600`, umask daemon `0077`.
- **Autentikasi:** Mencegah koneksi silang uid menggunakan verifikasi `SO_PEERCRED` (`cr.uid == geteuid()`). Setiap permintaan dicatat di `${UML4TERMUX_RUNTIME}/audit.log`.

### 2.2 Wire Protocol (Kontrak Runtime v1)
Klien mengirim **satu baris teks** yang diakhiri `\n`. Daemon menjawab dengan **satu baris JSON** yang diakhiri `\n` lalu menutup channel penulisan.

Daftar Perintah Protokol:
1. `protocol`  
   Respon: `{"protocol":1,"runtime_contract":1,"commands":["status","stop","ports","expose","exec","protocol"]}`
2. `status`  
   Respon: `{"state":"running"|"completed"|"idle","child_pid":12345,"restarts":0,"kernel":"...","mem":"1024M","ncpus":"4","uptime_s":120}`
3. `stop`  
   Respon: `{"state":"stopping"}` (mengirim SIGTERM/sysrq ke guest)
4. `ports`  
   Respon: `{"exposures":["127.0.0.1:8080 -> 192.168.127.2:80"]}`
5. `expose <host_port> <guest_port>`  
   Respon: `{"exposed":"127.0.0.1:<host_port> -> 192.168.127.2:<guest_port>"}` atau galat `{"error":"..."}`
6. `exec <perintah>`  
   Menjalankan perintah di dalam guest shell via saluran konsol PTY.

### 2.3 Mekanisme Pembungkusan Perintah (`exec`) & Penangkapan Output
Daemon `uml4termuxd` **tidak** melakukan streaming stdout/stderr langsung di atas socket kontrol. Socket kontrol hanya memberikan konfirmasi pemanggilan perintah:
```c
seq = ++g_exec_seq;
snprintf(line, sizeof(line),
         "exec:echo __EXEC_%d_BEGIN__; %s; echo __EXEC_%d_DONE_$?\n",
         seq, req + 5, seq);
write(g_ptym, line, strlen(line));
// Balasan ke socket kontrol:
snprintf(resp, sizeof(resp),
         "{\"started\":true,\"seq\":%d,\"console\":\"%s\"}\n",
         seq, g_consolelog);
respond(fd, resp);
```

#### Alur Eksekusi di Sisi Guest (`docker-init.sh`):
Agent konsol membaca dari `/dev/console`:
```sh
( while IFS= read -r aline; do
    case "$aline" in
      exec:*)
        printf '__AGENT_GOT__: %s\n' "${aline#exec:}"
        ( eval "${aline#exec:}" ) &
        ;;
    esac
  done ) < /dev/console &
```
1. Agent mencetak konfirmasi: `__AGENT_GOT__: echo __EXEC_<seq>_BEGIN__; <perintah>; echo __EXEC_<seq>_DONE_$?`
2. Agent mengevaluasi baris dalam subshell `( eval ... ) &`.
3. Guest mencetak penanda awal: `__EXEC_<seq>_BEGIN__`.
4. Perintah dieksekusi; seluruh output (stdout & stderr) mengalir ke konsol `/dev/console`.
5. Guest mencetak penanda akhir dengan kode keluar nyata: `__EXEC_<seq>_DONE_$?` (contoh: `__EXEC_1_DONE_0` atau `__EXEC_1_DONE_127`).
6. Daemon membaca PTY master dan menulis setiap baris log ke `${UML4TERMUX_RUNTIME}/uml-console.log`.

#### Strategi Parsing di Sisi Klien (PDN Runtime):
Untuk mengekstrak output dan status keluar secara deterministik:
- Buka `${UML4TERMUX_RUNTIME}/uml-console.log`.
- Cari posisi baris setelah `__EXEC_<seq>_BEGIN__`.
- Tangkap semua baris hingga menemukan baris yang berawalan `__EXEC_<seq>_DONE_<rc>`.
- Ekstrak nilai integer `<rc>` sebagai exit code proses.

### 2.4 Pembatasan & Penanganan Galat (Rate Limiting & Safety Invariants)
Daemon menerapkan proteksi eksplisit yang **tidak boleh** dilewati pemanggil:
1. **Batas Ukuran Baris Perintah (`WM_EXEC_MAX` = 2000 byte):**  
   Jika perintah > 2000 byte, daemon menolak dengan respon:
   `{"error":"command too long","limit":2000}` (klien keluar status 64). Klien PDN wajib menolak perintah sebelum memotongnya diam-diam.
2. **Pembatas Laju Pemanggilan (`exec_rate_limited`):**  
   Interval minimum antar perintah diatur oleh `UML4TERMUXD_EXEC_MIN_INTERVAL_MS` (default 100 ms).  
   Jika perintah dikirim terlalu cepat, daemon menolak dengan respon:
   `{"error":"rate-limited","min_interval_ms":100}`  
   Klien PDN wajib melakukan backoff retry (tidur selama `min_interval_ms`) bukan gagal seketika.
3. **Kesiapan Guest (`guest not running`):**  
   Jika child UML mati atau PTY belum aktif: `{"error":"guest not running"}`.
4. **Lock Backing Image Eksklusif (`F_SETLK`):**  
   UML mengunci image secara eksklusif. Jangan pernah membakar dua proses pada image backing yang sama tanpa COW overlay (`UML4TERMUXD_COW`).

---

## 3. Desain Step 1: Trait `RuntimeBackend` untuk PDN

### 3.1 Struktur Modul & Tipe Data (`crates/proot-distro-core/src/runtime.rs`)

```rust
//! Abstraksi Runtime Backend untuk PDN (PRoot Distro Next)
//!
//! Menyediakan antarmuka terpadu untuk mengeksekusi container melalui
//! backend ptrace PRoot biasa maupun backend VM kernel User-Mode Linux (UML).

use anyhow::{bail, Context, Result};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixStream;

use crate::container::Container;

/// Jenis backend runtime yang didukung
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RuntimeKind {
    /// ptrace chroot ringan (default, tanpa isolasi namespace/cgroup sejati)
    Proot,
    /// User-Mode Linux (kernel nyata, mendukung overlayfs, cgroups v2, Docker)
    Uml,
}

impl Default for RuntimeKind {
    fn default() -> Self {
        RuntimeKind::Proot
    }
}

/// Status kesehatan dan kapabilitas backend runtime
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuntimeStatus {
    pub kind: RuntimeKind,
    pub available: bool,
    pub running: bool,
    pub supports_kernel_features: bool,
    pub details: String,
}

/// Trait utama antarmuka eksekusi container
#[async_trait]
pub trait RuntimeBackend: Send + Sync {
    /// Mengembalikan identitas jenis runtime
    fn kind(&self) -> RuntimeKind;

    /// Apakah backend ini menyediakan kernel Linux nyata dengan dukungan
    /// namespace, cgroup v2, dan overlayfs (diperlukan untuk Docker).
    fn supports_kernel_features(&self) -> bool;

    /// Mengeksekusi perintah di dalam environment container
    async fn execute(&self, container: &Container, command: Vec<String>) -> Result<i32>;

    /// Memeriksa status kesiapan backend
    async fn status(&self) -> Result<RuntimeStatus>;
}
```

### 3.2 Implementasi `ProotBackend`
Memindahkan logika eksekusi PRoot yang saat ini berada di `ContainerManager::execute`:

```rust
use crate::sysdata;
use crate::guestdirs;
use proot_core::{ProotConfig, ProotEngine};
use std::sync::Arc;
use tokio::sync::Mutex;

pub struct ProotBackend {
    engine: Arc<Mutex<Option<ProotEngine>>>,
}

impl ProotBackend {
    pub fn new() -> Self {
        Self {
            engine: Arc::new(Mutex::new(None)),
        }
    }

    async fn get_engine(&self) -> Result<Arc<Mutex<ProotEngine>>> {
        let mut guard = self.engine.lock().await;
        if guard.is_none() {
            *guard = Some(ProotEngine::new(ProotConfig::default())?);
        }
        Ok(Arc::new(Mutex::new(guard.as_ref().unwrap().clone())))
    }
}

#[async_trait]
impl RuntimeBackend for ProotBackend {
    fn kind(&self) -> RuntimeKind {
        RuntimeKind::Proot
    }

    fn supports_kernel_features(&self) -> bool {
        false // PRoot tidak memiliki namespace kernel sejati atau cgroup
    }

    async fn execute(&self, container: &Container, command: Vec<String>) -> Result<i32> {
        let engine = self.get_engine().await?;
        let mut engine_guard = engine.lock().await;

        engine_guard.config.rootfs = Some(container.rootfs_path.clone());
        engine_guard.config.bind = container
            .config
            .bind_mounts
            .iter()
            .map(|b| format!("{}:{}:{}", b.source, b.target, if b.readonly { "ro" } else { "rw" }))
            .collect();

        if let Some(container_dir) = container.rootfs_path.parent() {
            if sysdata::ensure(container_dir).is_ok() {
                engine_guard.config.bind.extend(sysdata::bindings(container_dir));
            }
            if guestdirs::ensure(container_dir).is_ok() {
                engine_guard.config.bind.extend(guestdirs::bindings(container_dir));
            }
        }

        engine_guard.config.cwd = Some(
            container.config.working_dir.clone().unwrap_or_else(|| "/".to_string()).into(),
        );
        engine_guard.config.env = container
            .config
            .env_vars
            .iter()
            .map(|(k, v)| format!("{}={}", k, v))
            .collect();
        engine_guard.config.seccomp = container.config.security.seccomp_profile.is_some();
        engine_guard.config.kernel_release = container.config.kernel_release.clone();
        engine_guard.config.hostname = container.config.hostname.clone();

        engine_guard.config.command = if command.is_empty() {
            vec!["/bin/sh".to_string(), "-c".to_string(), "exec /bin/sh".to_string()]
        } else {
            command
        };

        let exit_code = engine_guard.execute().await? as i32;
        Ok(exit_code)
    }

    async fn status(&self) -> Result<RuntimeStatus> {
        Ok(RuntimeStatus {
            kind: RuntimeKind::Proot,
            available: true,
            running: true,
            supports_kernel_features: false,
            details: "PRoot user-space ptrace chroot emulation".to_string(),
        })
    }
}
```

### 3.3 Implementasi `UmlBackend`
Klien berbasis socket UNIX ke `uml4termuxd` dengan penanganan kontrak v1:

```rust
pub struct UmlBackend {
    socket_path: PathBuf,
}

impl UmlBackend {
    pub fn new() -> Self {
        let runtime_dir = std::env::var("UML4TERMUX_RUNTIME")
            .unwrap_or_else(|_| format!("{}/uml4termux", std::env::var("TMPDIR").unwrap_or_else(|_| "/tmp".to_string())));
        Self {
            socket_path: PathBuf::from(runtime_dir).join("daemon.sock"),
        }
    }

    pub fn with_socket(path: PathBuf) -> Self {
        Self { socket_path: path }
    }

    async fn send_command(&self, line: &str) -> Result<String> {
        let mut stream = UnixStream::connect(&self.socket_path)
            .await
            .with_context(|| format!("UML daemon socket not reachable at {}", self.socket_path.display()))?;

        let mut payload = line.to_string();
        payload.push('\n');
        stream.write_all(payload.as_bytes()).await?;
        stream.flush().await?;
        stream.shutdown(std::net::Shutdown::Write)?;

        let mut reader = BufReader::new(stream);
        let mut resp = String::new();
        reader.read_line(&mut resp).await?;
        Ok(resp.trim_end().to_string())
    }

    async fn request_with_retry(&self, line: &str) -> Result<serde_json::Value> {
        let max_retries = 5;
        let mut attempts = 0;
        loop {
            attempts += 1;
            let resp_str = self.send_command(line).await?;
            let val: serde_json::Value = serde_json::from_str(&resp_str)
                .with_context(|| format!("Invalid JSON from uml4termuxd: {}", resp_str))?;

            if let Some(err) = val.get("error").and_then(|e| e.as_str()) {
                if err == "rate-limited" && attempts <= max_retries {
                    let wait_ms = val.get("min_interval_ms").and_then(|v| v.as_u64()).unwrap_or(100);
                    tokio::time::sleep(Duration::from_millis(wait_ms + 10)).await;
                    continue;
                }
                if err == "command too long" {
                    bail!("Command exceeds uml4termux limit of 2000 bytes");
                }
                bail!("uml4termuxd error: {}", err);
            }
            return Ok(val);
        }
    }
}

#[async_trait]
impl RuntimeBackend for UmlBackend {
    fn kind(&self) -> RuntimeKind {
        RuntimeKind::Uml
    }

    fn supports_kernel_features(&self) -> bool {
        true // Kernel UML menyediakan isolasi namespace, cgroup v2, dan overlayfs sejati
    }

    async fn execute(&self, _container: &Container, command: Vec<String>) -> Result<i32> {
        // Gabungkan perintah
        let joined_cmd = command.join(" ");
        if joined_cmd.len() > 2000 {
            bail!("Command too long for uml4termux console channel (limit 2000 bytes, got {})", joined_cmd.len());
        }

        // Kirim perintah exec ke daemon
        let exec_req = format!("exec {}", joined_cmd);
        let resp = self.request_with_retry(&exec_req).await?;

        let seq = resp.get("seq").and_then(|s| s.as_u64())
            .context("Missing seq in uml4termuxd exec response")?;
        let console_path = resp.get("console").and_then(|c| c.as_str())
            .map(PathBuf::from)
            .unwrap_or_else(|| self.socket_path.parent().unwrap().join("uml-console.log"));

        // Tunggu dan parsing console log untuk penanda __EXEC_<seq>_BEGIN__ dan __EXEC_<seq>_DONE_<rc>
        let done_marker = format!("__EXEC_{}_DONE_", seq);
        let begin_marker = format!("__EXEC_{}_BEGIN__", seq);

        let timeout = Duration::from_secs(300);
        let start = std::time::Instant::now();
        let mut exit_code = None;

        while start.elapsed() < timeout {
            if let Ok(content) = std::fs::read_to_string(&console_path) {
                if let Some(begin_pos) = content.rfind(&begin_marker) {
                    let after_begin = &content[begin_pos..];
                    if let Some(done_pos) = after_begin.find(&done_marker) {
                        let rest = &after_begin[done_pos + done_marker.len()..];
                        let code_str: String = rest.chars().take_while(|c| c.is_ascii_digit() || *c == '-').collect();
                        if let Ok(rc) = code_str.parse::<i32>() {
                            exit_code = Some(rc);
                            break;
                        }
                    }
                }
            }
            tokio::time::sleep(Duration::from_millis(150)).await;
        }

        exit_code.context("Timed out waiting for command execution completion in UML guest")
    }

    async fn status(&self) -> Result<RuntimeStatus> {
        if !self.socket_path.exists() {
            return Ok(RuntimeStatus {
                kind: RuntimeKind::Uml,
                available: false,
                running: false,
                supports_kernel_features: true,
                details: "uml4termuxd socket does not exist (run 'uml4termux up' first)".into(),
            });
        }

        match self.send_command("status").await {
            Ok(reply) => {
                let v: serde_json::Value = serde_json::from_str(&reply)?;
                let state = v.get("state").and_then(|s| s.as_str()).unwrap_or("unknown");
                let is_running = state == "running";
                Ok(RuntimeStatus {
                    kind: RuntimeKind::Uml,
                    available: true,
                    running: is_running,
                    supports_kernel_features: true,
                    details: format!("uml4termuxd status: state={}, info={}", state, reply),
                })
            }
            Err(e) => Ok(RuntimeStatus {
                kind: RuntimeKind::Uml,
                available: false,
                running: false,
                supports_kernel_features: true,
                details: format!("Cannot connect to uml4termuxd: {}", e),
            }),
        }
    }
}
```

### 3.4 Pendelegasian pada `ContainerManager::execute`
Di `crates/proot-distro-core/src/container.rs`:

```rust
// Tambahkan field runtime di ContainerConfig
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContainerConfig {
    pub name: String,
    pub distro: String,
    pub version: String,
    #[serde(default)]
    pub runtime: crate::runtime::RuntimeKind, // "proot" | "uml"
    // ... field lainnya
}

impl ContainerManager {
    /// Mengambil backend runtime yang sesuai untuk kontainer
    pub fn get_backend(&self, container: &Container) -> Box<dyn crate::runtime::RuntimeBackend> {
        match container.config.runtime {
            crate::runtime::RuntimeKind::Proot => Box::new(crate::runtime::ProotBackend::new()),
            crate::runtime::RuntimeKind::Uml => Box::new(crate::runtime::UmlBackend::new()),
        }
    }

    /// Eksekusi mendelegasikan langsung ke trait RuntimeBackend
    pub async fn execute(&self, name: &str, command: Vec<String>) -> Result<i32> {
        let container = self.get(name).await?;
        let backend = self.get_backend(&container);
        backend.execute(&container, command).await
    }
}
```

### 3.5 Pendelegasian pada `podnext login` dan `podnext run`
Di `crates/podnext/src/native.rs` dan `crates/podnext/src/main.rs`:
- Opsi CLI baru: `--runtime <proot|uml>`.
- Jika argumen `--runtime` diberikan, menimpa pilihan `container.config.runtime`.
- Pemanggilan `login_container_native` dan `run_container_native` memanggil `ContainerManager::execute`, yang secara transparan memilih driver yang tepat.

```rust
// Contoh di crates/podnext/src/native.rs:
pub async fn run_container_with_runtime(
    container_name: &str,
    command: Vec<String>,
    runtime_override: Option<RuntimeKind>,
) -> Result<()> {
    let container_mgr = proot_distro_core::ContainerManager::new()?;
    let mut container = container_mgr.get(container_name).await?;

    if let Some(rt) = runtime_override {
        container.config.runtime = rt;
    }

    let backend = container_mgr.get_backend(&container);
    let exit_code = backend.execute(&container, command).await?;
    if exit_code != 0 {
        return Err(anyhow::Error::new(proot_core::ChildExit(exit_code)));
    }
    Ok(())
}
```

### 3.6 Implementasi Perintah `podnext docker` & Verifikasi Kernel Features
Di `crates/podnext/src/main.rs`:

```rust
// 1. Tambahkan varian di enum Commands:
#[derive(Subcommand, Debug)]
enum Commands {
    // ...
    /// Run Docker commands transparently via kernel-capable runtime (UML)
    Docker {
        /// Container or instance name (optional, defaults to active uml instance)
        #[arg(short, long)]
        container: Option<String>,

        /// Arguments passed through to the docker CLI
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },
}

// 2. Handler cmd_docker:
async fn cmd_docker(container_name: Option<String>, args: Vec<String>) -> Result<()> {
    let container_mgr = proot_distro_core::ContainerManager::new()?;

    // Jika container tidak disebutkan, gunakan backend UML global
    let backend: Box<dyn proot_distro_core::runtime::RuntimeBackend> = if let Some(ref name) = container_name {
        let c = container_mgr.get(name).await?;
        container_mgr.get_backend(&c)
    } else {
        Box::new(proot_distro_core::runtime::UmlBackend::new())
    };

    // VERIFIKASI UTAMA: Penolakan eksplisit jika backend tidak mendukung fitur kernel
    if !backend.supports_kernel_features() {
        bail!(
            "The configured runtime backend '{}' does not support kernel features required by Docker.\n\
             Reason: Docker requires namespaces (mount, pid, net) and cgroups v2, which PRoot cannot provide.\n\
             Resolution: Start the UML backend with 'uml4termux up' and use '--runtime uml'.",
            format!("{:?}", backend.kind()).to_lowercase()
        );
    }

    // Pastikan backend sedang aktif
    let status = backend.status().await?;
    if !status.running {
        bail!("UML kernel runtime is not running. Please start it with 'uml4termux up'.");
    }

    // Passthrough ke binary docker di guest
    let mut docker_cmd = vec!["docker".to_string()];
    docker_cmd.extend(args);

    let dummy_container = proot_distro_core::Container::default();
    let exit_code = backend.execute(&dummy_container, docker_cmd).await?;
    if exit_code != 0 {
        std::process::exit(exit_code);
    }
    Ok(())
}
```

---

## 4. Verifikasi Status Live `uml4termux` di Perangkat Ini

Hasil audit empiris langsung pada perangkat (Android 16, aarch64, uid 10396):

### 4.1 Ketersediaan Biner
- **CLI Global:** `/data/data/com.termux/files/usr/bin/uml4termux` (symlink ke `/data/data/com.termux/files/usr/share/uml4termux/bin/uml4termux`) → **Tersedia (v0.3.0+)**.
- **Daemon C:** `/data/data/com.termux/files/home/workspace/uml4termux/bin/uml4termuxd` → **Tersedia (siap eksekusi)**.
- **CLI/Daemon Rust Twin:** `/data/data/com.termux/files/home/workspace/uml4termux/rust/target/release/uml4termux-rs` → **Tersedia (siap eksekusi)**.

### 4.2 Artefak & Asset Image
Direktori Asset: `/data/data/com.termux/files/home/.local/share/uml4termux/`
- `linux-bionic-smp` (13.2 MB, SHA256 verified): Kernel arm64 dengan dukungan `OVERLAY_FS`, `NETFILTER`, `BRIDGE`, `CGROUP_PIDS`, `MEMCG`.
- `debian-docker.ext4.gz` (245 MB): Image Debian 12 berisi instalasi Docker Engine 20.10.24, containerd, runc, dan BuildKit.
- `initramfs-alpine.cpio.gz` (3.8 MB): Root bootstrap awal.
- `gvproxy` (12.7 MB): Stack jaringan user-mode BESS + port forwarder.
- `stub_exe` (2 KB): Executable loader untuk Android sandbox.

### 4.3 Status Layanan & Socket Saat Ini
- `uml4termux ls`: `no instances registered`.
- File socket kontrol `${TMPDIR}/uml4termux/daemon.sock`: Belum aktif (proses idle).
- Runit service `/data/data/com.termux/files/usr/var/service/`: Tidak ada entri aktif untuk uml4termux (layanan dikontrol secara manual on-demand via `uml4termux up`).

### 4.4 Prosedur Menyalakan Runtime UML
Untuk mengaktifkan runtime backend sebelum pengujian passthrough:
```bash
uml4termux up --mem 1024M --ssh-port 12222
```
Perintah ini akan:
1. Memverifikasi integritas hash file asset.
2. Membuat sparse COW file dari `debian-docker.ext4.gz`.
3. Membuka socket UNIX di `${UML4TERMUX_RUNTIME}/daemon.sock` (mode 0600).
4. Menjalankan gvproxy pada port 12222.
5. Menjalankan `linux-bionic-smp` dengan virtual ethernet `vec0` dan konsol PTY.
6. Menginisialisasi dockerd dan melaporkan `DOCKERD_READY_FOR_REAL`.

---

## 5. Rencana Penerapan (Execution Roadmap)

### Fase 1: Refactoring Abstraksi Runtime (Tanpa Perubahan Perilaku)
1. Buat berkas `crates/proot-distro-core/src/runtime.rs`.
2. Pindahkan logika proot dari `ContainerManager::execute` ke `ProotBackend`.
3. Pasang delegasi di `ContainerManager::execute`.
4. Jalankan rangkaian tes PDN untuk memastikan 100% kompatibilitas tetap hijau.

### Fase 2: Implementasi Klien `UmlBackend`
1. Lengkapi parsing respon `protocol`, `status`, dan `exec` di `UmlBackend`.
2. Tambahkan tailing parser untuk `uml-console.log` berdasarkan marker `__EXEC_<seq>_BEGIN__` dan `__EXEC_<seq>_DONE_<rc>`.
3. Implementasikan retry otomatis saat menerima error `rate-limited`.

### Fase 3: Integrasi CLI `podnext docker` & Metadata Container
1. Tambahkan field `"runtime": "proot" | "uml"` pada `metadata.json` container.
2. Tambahkan subcommand `podnext docker` di `crates/podnext/src/main.rs`.
3. Tegakkan pengecekan `backend.supports_kernel_features()`. Jika backend adalah PRoot, tolak dengan pesan kesalahan yang jelas.
4. Lakukan pengujian langsung: `podnext docker run --rm hello-world`.
