# Arsitektur: Docker di dalam PDN

Status: **desain**, belum ada kode. Setiap klaim tentang batasan perangkat dan tentang
`uml4termux` di bawah berasal dari pengukuran/sumber yang dikutip, bukan asumsi.

---

## 1. Batasan yang menentukan segalanya (terukur)

PDN hari ini menjalankan container lewat **proot**: `ptrace` chroot. Itu cukup untuk
filesystem dan exec, dan tidak akan pernah cukup untuk Docker. Diukur langsung pada
perangkat ini (uid 10396, tanpa root):

| Yang dibutuhkan Docker | Hasil terukur di Android | Bukti |
|---|---|---|
| `unshare(CLONE_NEWUSER)` | `EINVAL` | syscall langsung |
| `unshare(CLONE_NEWNS/NEWPID/NEWNET)` | `EPERM` | syscall langsung |
| `mount(2)` untuk overlayfs | **proses dibunuh SIGSYS (31)** oleh seccomp Android | syscall langsung |
| `CAP_SYS_ADMIN` / `CAP_NET_ADMIN` | `CapEff = CapPrm = CapBnd = 0` | `/proc/self/status` |
| `/sys/fs/cgroup` ditulis | ada, tapi `touch` → `Permission denied` | uji tulis |
| `dockerd` di host | tidak ada; hanya klien `docker` v24.0.6 | `command -v dockerd` |

proot **memalsukan** `mount`/`unshare` dengan rc=0 tanpa melakukan apa pun. Konsekuensinya
berbahaya: `dockerd` akan tampak hidup di dalam container proot, lalu menghasilkan container
yang tidak bisa dipakai. Karena itu "Docker di dalam proot" bukan target yang sulit — itu
target yang **salah**.

Satu-satunya jalur nyata di perangkat ini adalah kernel sungguhan di ruang pengguna:
`uml4termux` (user-mode Linux, arm64). Proyek itu sudah memverifikasi di perangkat yang sama
(`README.md`, tabel capability): `dockerd` 20.10.24 + containerd + runc, storage `overlay2`
(`Native Overlay Diff: true`, `Backing Filesystem: extfs`), cgroup v2 dengan controller
`cpu io memory pids`, `docker pull/run/exec/logs`, networking antar-container di `docker0`,
port publish sampai ke HP lewat `gvproxy`, Compose v2.29.7, dan BuildKit.

---

## 2. Keputusan arsitektur

**PDN tidak mengimplementasikan Docker. PDN menambahkan backend runtime kedua.**

```
                    ┌──────────────────────────────┐
   podnext CLI ────► │  RuntimeBackend (trait)      │
   podnext-daemon   │                              │
                    ├──────────────┬───────────────┤
                    │ ProotBackend │  UmlBackend   │
                    └──────┬───────┴───────┬───────┘
                           │               │
                   ptrace chroot     uml4termuxd (UNIX socket)
                   (tanpa namespace)  └─ kernel UML → dockerd → container
```

Alasan memilih ini, bukan yang lain:

- **Bukan** mengimplementasikan ulang Docker: mustahil tanpa namespace; dan menulis
  builder/overlay sendiri di atas proot akan tetap gagal saat container pertama dijalankan.
- **Bukan** memanggil `buildctl`/`podman` di host: keduanya butuh namespace juga, jadi sama
  mustahilnya. Di host ini tidak ada, dan tidak akan bisa dipasang.
- **Bukan** menghapus `build` dari CLI: pengguna memang ingin membangun image. Yang benar
  adalah membuat `build` punya backend yang bisa melakukannya.

Backend dipilih per container (field di `metadata.json`) dan bisa dipaksa per perintah
(`--runtime uml`). Default tetap `proot` karena murah dan cepat; `uml` dipakai ketika
container membutuhkan hal yang hanya bisa diberikan kernel sungguhan.

---

## 3. Antarmuka yang dipakai (nyata, bukan karangan)

`uml4termuxd` berbicara **teks berbaris** di UNIX socket dalam direktori
`$UML4TERMUX_RUNTIME`; daemon menjawab sekali lalu menutup (`src/uml4termuxd.c:1010-1075`,
klien Rust `rust/src/client.rs:40-90`):

| Request | Jawaban |
|---|---|
| `protocol` | `{"protocol":N,"runtime_contract":N,"commands":[...]}` |
| `status` | `{"state":"running\|idle\|completed","child_pid":N,"restarts":N,"kernel":"…","mem":"…","ncpus":"…","uptime_s":N}` |
| `exec <cmd>` | output perintah, dibungkus `__EXEC_<seq>_BEGIN__ … __EXEC_<seq>_DONE_$?` |
| `stop` | `{"state":"stopping"}` |

Sifat yang harus dihormati pemanggil:

- `exec` **dibatasi laju** (`exec_rate_limited`, jawaban `{"error":"rate-limited","min_interval_ms":N}`)
  dan **dibatasi panjang** (`WM_EXEC_MAX`, jawaban `{"error":"command too long"}`). Keduanya
  penolakan yang terlihat, bukan pemotongan diam-diam.
- Socket `0600`, diverifikasi `SO_PEERCRED`, setiap permintaan masuk `audit.log`.
- Sebelum dipakai, klien harus melakukan handshake `protocol` — nilai
  `UML4TERMUXD_PROTOCOL`/`RUNTIME_CONTRACT` adalah kontrak, bukan saran.

---

## 4. Pemetaan perintah PDN ke backend UML

| PDN | Di backend proot | Di backend UML |
|---|---|---|
| `login` / `run` | proot ptrace | `exec` ke shell di guest UML |
| `build` (sekarang palsu: digest = `sha256(uuid)`) | tidak didukung | `exec "DOCKER_BUILDKIT=1 docker build -t <tag> <ctx>"`, lalu `docker save` → impor layer nyata |
| `docker` (baru, passthrough) | ditolak dengan alasan | `exec "docker <args>"` |
| `network connect` | hanya menulis DNS guest | benar-benar `docker network connect` di dalam guest |
| `volume` | bind mount host | `docker volume …`; prune memakai refcount Docker |
| `quota` | state JSON + `collect_usage` | `--cpus`/`--memory` → `cpu.max`/`memory.max` (terverifikasi jalan: OOM 137 pada `--memory 64m`) |

`build` adalah kasus yang paling jelas: satu-satunya cara membuatnya **bukan palsu** adalah
menjalankannya di tempat yang punya kernel. `docker save` menghasilkan tar layer yang bisa
dimasukkan ke `ImageManager::push` yang sudah ada (`crates/proot-distro-core/src/image.rs`),
sehingga hasil build benar-benar menjadi image OCI yang bisa di-`podnext install`.

---

## 5. Yang tetap tidak bisa, bahkan lewat UML

Jangan menjanjikan ini (semua terukur di `docs/KNOWN-LIMITS.md` / tabel capability):

- `io.max` cgroup — cgroup v2 di kernel itu hanya mengekspos `io.stat`, jadi
  `--device-*-bps` tidak dapat ditegakkan.
- Swap accounting — celah kernel guest.
- Charging tmpfs/page-cache ke memory cgroup — `CONFIG_MEMCG_KMEM` mati.
- Multi-CPU di mode `ptrace` — 1 CPU; butuh `seccomp=on`.
- Inbound forwarding lewat `passt`/`umnet` — hanya outbound; jalur yang bekerja adalah
  `gvproxy` + `POST /services/forwarder/expose`.
- i386/armv7 native — `CONFIG_COMPAT` mati; hanya lewat `qemu-user-static` di dalam guest.

---

## 6. Bentuk kode yang dituju

```rust
// crates/proot-distro-core/src/runtime.rs
pub enum RuntimeKind { Proot, Uml }

pub trait RuntimeBackend: Send + Sync {
    fn kind(&self) -> RuntimeKind;
    /// Apakah backend ini bisa menjalankan perintah yang butuh namespace/kernel.
    fn supports_kernel_features(&self) -> bool;
    async fn exec(&self, container: &str, argv: &[String]) -> Result<ExitStatus>;
    fn status(&self) -> Result<RuntimeStatus>;
}
```

- `ProotBackend` — yang sekarang ada, dipindahkan apa adanya dari `container.rs::execute`.
- `UmlBackend` — klien tipis ke socket `uml4termuxd`: `protocol` sekali saat start, lalu
  `exec`. Wajib menangani `rate-limited` dengan retry berjeda (bukan dengan menurunkan
  interval secara diam-diam) dan `command too long` dengan menolak, bukan memotong.
- Pemilihan: `metadata.json` → `"runtime": "proot" | "uml"`; `--runtime` menimpa.
- `podnext docker …` menolak dengan pesan jelas bila backend-nya `proot`
  (`"requires the uml runtime: docker needs namespaces; see 'podnext runtime info'"`).

---

## 6a. `build` tanpa `RUN`: subset Dockerfile yang benar-benar nyata

Bedah alur Docker menunjukkan bahwa hanya **eksekusi** yang mustahil; penyusunan artefak
tidak. `RUN` butuh fork+exec di dalam mount namespace — `unshare` EPERM, `mount(2)` SIGSYS.
Sisanya (`FROM`, `COPY`, `ADD` dari tar/URL lokal, `ENV`, `LABEL`, `WORKDIR`, `USER`,
`EXPOSE`, `CMD`, `ENTRYPOINT`) murni tar + JSON.

Empat opsi, dengan keputusan:

| Opsi | Putusan |
|---|---|
| `buildctl`/`podman` | **ditolak** — tidak ada di Termux, dan keduanya butuh namespace juga; akan mati di dinding yang sama |
| serahkan ke daemon UML | nyata, tapi ini keputusan pengiriman (biaya boot VM), harus opt-in — bukan arsitektur milik PDN |
| hapus `build` dan katakan di `--help` | jujur tapi membuang alasan `ImageManager::push` ada |
| **subset Dockerfile → layer tar → diffID → manifest schema 2 → push** | **dipilih** |

Alasannya: setiap byte beralamat-konten dan bisa diverifikasi `docker pull` dari registry
mana pun; tidak ada digest yang dikarang. `RUN` ditolak dengan pesan eksplisit yang
menunjuk uml4termux sebagai jalur eksekusinya.

**Tiga cacat di `ImageManager` yang membuat `push` tidak nyata** (ditemukan saat bedah):

| # | Cacat | Status |
|---|---|---|
| 1 | `diff_ids` diisi digest layer **terkompresi**, bukan diffID (sha256 tar tak-terkompresi). Dua nilai berbeda; registry yang memverifikasi chain-ID menolak | **DIPERBAIKI** — `Layer.diff_id` ditambahkan, `build_config` memakai nilai itu dan menolak bila tidak ada |
| 2 | `config.size` ditulis `0` dan `config.digest` meminjam `image.digest` (digest *layer*, bukan digest config) | **DIPERBAIKI** — config diserialisasi, `size` = panjang nyata, `digest` = sha256 blob itu |
| 3 | Upload blob adalah no-op yang dicetak (`"would be pushed (skipped)"`), sementara manifest tetap dikirim lebih dulu — registry menyimpan manifest yang menunjuk blob yang tidak pernah ada, dan `docker pull` gagal setelahnya | **DIPERBAIKI** — `push` menolak sebelum menyentuh jaringan; manifest tidak lagi dikirim untuk blob yang tidak ada |

Sisa pekerjaan untuk `push` yang benar-benar berfungsi adalah **menyimpan blob layer**
(PDN membuang byte-nya setelah ekstraksi) — itu bagian dari langkah 4 di bawah.

---

## 7. Urutan pengerjaan yang masuk akal

1. **`RuntimeBackend` + `ProotBackend`** — refactor murni, tanpa perubahan perilaku.
   Gerbang: seluruh 128 test tetap lulus, perintah tidak berubah.
2. **`UmlBackend` minimal** — `protocol` + `status` + `exec`; `podnext login <c> --runtime uml`.
   Gerbang: `exec "uname -a"` mengembalikan kernel UML, bukan kernel Android.
3. **`podnext docker` passthrough** — hanya di backend UML.
   Gerbang: `podnext docker run --rm hello-world` berhasil lewat PDN.
4. **`build` sungguhan** — `docker build` di guest, `docker save` → `ImageManager::push`.
   Gerbang: image hasil build bisa `podnext install` dan `login`.
5. **`network`/`volume`/`quota`** dipetakan ke Docker asli saat backend = UML.

Setiap langkah punya gerbang yang bisa dijalankan, dan langkah 1 tidak mengubah perilaku
apa pun — jadi risikonya terkendali.