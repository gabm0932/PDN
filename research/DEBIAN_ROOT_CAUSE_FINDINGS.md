# PDN — Debian: temuan akar masalah & perbaikan (3 Nov 2026)

Lingkungan: Termux / Android 16, aarch64, tanpa root, uid 10396.
Pembanding: `proot-distro 5.10.3` (debian trixie) vs `podnext 0.1.7` (`deb-pdn`, debian trixie).
Semua klaim di bawah diambil dari keluaran perintah nyata; perintahnya dicantumkan.

---

## 1. Hard link diblokir Android → install Debian gagal total

**Gejala (log install):**

```
Error: failed to unpack `.../.podnext/containers/deb-pdn/rootfs/usr/bin/perl5.40.1`
Caused by:
    Permission denied (os error 13) when hard linking
    .../rootfs/usr/bin/perl to .../rootfs/usr/bin/perl5.40.1
```

**Bukti bahwa ini batasan platform, bukan bug logika** — `ln` gagal di dalam `$HOME` sendiri:

```
$ echo x > ~/hltest/a; ln ~/hltest/a ~/hltest/b
ln: failed to create hard link '.../hltest/b' => '.../hltest/a': Permission denied
```

Filesystem `f2fs` (`stat -f`), mount `seclabel,nosuid,nodev`; `/sys/fs/selinux/enforce` tidak
terbaca. Alpine pernah lolos karena applet busybox berupa **symlink**; image Debian memakai
**hard link** (`perl` → `perl5.40.1`), jadi hanya image tertentu yang meledak.

**Perbaikan:** `crates/proot-distro-core/src/untar.rs` — pengurai tar sendiri: `hard_link`
dicoba lebih dulu, bila ditolak **file disalin**. Symlink tetap symlink; device node/FIFO
dilewati; mode diterapkan setelah isi ditulis; path yang keluar dari tujuan ditolak.
Satu implementasi dipakai bersama oleh: install (`native.rs`), `compress.rs` (4 jalur),
dan restore (`backup_restore/mod.rs`).

**Verifikasi:** `podnext install debian --name deb-pdn` → `Container 'deb-pdn' installed
successfully!` + `metadata.json` tertulis (rootfs 19 MB → terisi penuh).

---

## 2. `--change-id` tidak pernah dikirim → `apt install` mustahil (akar terpenting)

**Gejala:**

```
$ podnext login deb-pdn -- apt-get install -y --no-install-recommends curl
dpkg: error: requested operation requires superuser privilege
E: Sub-process /usr/bin/dpkg returned an error code (2)
/bin/sh: 1: curl: not found
```

Pembanding, perintah sama di proot-distro: `Setting up curl (8.14.1-2+deb13u7)` +
`curl 8.14.1 (aarch64-unknown-linux-gnu) ...`.

**Akar masalah (bukti dari sumber):**

| | argv proot |
|---|---|
| PDN `crates/proot-core/src/lib.rs:200-204` | `--kill-on-exit --link2symlink --sysvipc -L` — **tanpa `--change-id`** |
| proot-distro `login/proot_cmd.py:105` | `--change-id={login_uid}:{login_gid}` (default `0:0`) |

Tanpa `--change-id`, guest tetap memakai uid nyata Android (10396), sehingga setiap operasi
berhak-root langsung ditolak `dpkg`. Ini yang memblokir node, playwright, browser, dan
pemasangan paket apa pun di container PDN.

**Perbaikan:** `--change-id=0:0` ditambahkan pada **kedua** pembangun argv
(`crates/proot-core/src/lib.rs`, dua tempat), persis seperti proot-distro untuk container
non-termux. Ini tetap ilusi ruang-pengguna: proot hanya memetakan id untuk proses yang
di-trace, tidak menambah hak nyata.

**Verifikasi (satu rangkaian, binary hasil build ulang):**

```
### 1. FAKE ROOT ###        id -u / id -g / id -un   ->  0 / 0 / root
### 2. APT INSTALL ###       Setting up curl (8.14.1-2+deb13u7)
                            curl 8.14.1 (aarch64-unknown-linux-gnu) libcurl/8.14.1 ...
### 3. DPKG AS ROOT ###      dpkg -l | wc -l -> 106   (dari 83 sebelum install)
### 4. ISOLATION REGRESSION ###  ls /etc | wc -l -> 60 ; alpine-release -> 0 ; PRETTY_NAME="Debian GNU/Linux 13 (trixie)"
### 5. ENV HYGIENE ###       LD_PRELOAD|LD_LIBRARY_PATH|API_KEY|termux -> 0
                            PATH=/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin
### 6. PROC PARITY ###       124.08 932.76 / cpu  1954 0 2872 93276 259 340 250 83 0 0 / 0.12 0.07 0.02 2/165 765
### 7. ALPINE STILL OK ###   3.24.3 / apk-tools 3.0.8-r0
```

Tidak ada regresi: isolasi tetap utuh, env tetap bersih, stub `/proc` tetap identik dengan
proot-distro, dan container alpine tidak terpengaruh.

---

## 3. `restore` mengembalikan UUID acak → tidak pernah membuat container

**Akar masalah:** `restore_container` hanya `Ok(Uuid::new_v4().to_string())` dan isi arsip
(`rootfs/...`) tidak pernah dipindahkan ke `~/.podnext/containers/<nama>/`. Tanpa
`metadata.json`, container hasil restore tidak terlihat oleh perintah lain.

**Perbaikan:** `crates/pn-next/src/backup_restore/mod.rs`

1. target = `--target` atau `<container>-restored`;
2. tolak bila sudah ada kecuali `--overwrite`;
3. pindahkan `extract_dir/rootfs` → `containers_dir/<target>/rootfs` (fallback `copy_tree`
   rekursif yang mempertahankan symlink);
4. tulis `metadata.json` (ini yang mendaftarkan container);
5. siapkan stub `sysdata` seperti install baru.

**Verifikasi:**

```
$ podnext backup create deb-pdn           -> Backup created with ID: 928d887d-2eed-49f2-8ce3-e55e913d673f
$ podnext backup restore 928d887d-... --target deb-restored
                                          -> Restored to container: deb-restored
$ podnext list                            -> deb-restored | deb-pdn | restored | aarch64 | Stopped
$ podnext login deb-restored -- sh -c 'cat /etc/os-release|grep PRETTY; dpkg -l|wc -l; cat /proc/uptime'
                                          -> PRETTY_NAME="Debian GNU/Linux 13 (trixie)" / 83 / 124.08 932.76
```

---

## 4. Divergensi lain yang terukur

| Aspek | PDN | proot-distro | Catatan |
|---|---|---|---|
| uid di guest | 0 (setelah perbaikan) | 0 | sebelum perbaikan: 10396 → apt mustahil |
| `uname -r` | kernel asli `6.1.157-android14-…` | `6.17.0-PRoot-Distro` | pilihan desain PDN: tidak memalsukan identitas |
| PATH di guest | bersih (`/usr/local/sbin:…:/bin`) | **bocor** `/data/data/com.termux/files/usr/bin` | `python3` di proot-distro = Python **host** (`sys.prefix=/data/data/com.termux/files/usr`) |
| `getent hosts deb.debian.org` | resolves | resolves | IPv4/IPv6 berbeda urutan, keduanya benar |
| `apt-get update` | 10.1 MB / 13 s | 10.1 MB / 13 s | identik |
| `/proc/uptime`, `/proc/stat`, `/proc/loadavg` | identik byte | identik byte | stub sysdata |

Kebocoran PATH pada proot-distro berarti biner host (ABI Termux) dapat dipanggil dari dalam
guest — kelas masalah yang justru sudah ditutup di PDN.

---

## 5. Perintah stub yang kini bekerja

| Perintah | Sebelum | Perbaikan |
|---|---|---|
| `network connect/disconnect` | `not yet implemented` | `NetworkConfig` tidak punya field `containers` (tidak dapat dipersist) → ditambahkan; `connect` menulis DNS jaringan ke `/etc/resolv.conf` guest, `disconnect` mengembalikan default |
| `volume prune` | `not yet implemented` | menghitung volume terpakai dari `bind_mounts` seluruh container; tanya konfirmasi kecuali `--force` |
| `log tail` | `not yet implemented` | memakai `StructuredLogging::query` yang sama dengan `log query` |
| `dashboard start/stop/status` | hanya mencetak | `WebDashboard::start()` dipanggil sungguhan; pid file `~/.podnext/dashboard.pid`; stop SIGTERM→SIGKILL; status memeriksa liveness + port |

---

## 6. Uji kapabilitas di dalam container Debian

Semua dijalankan `podnext login deb-pdn -- ...` setelah perbaikan di atas.

### 6.1 Chromium + Playwright — BERJALAN di PDN Debian

`/dev/shm` hilang di PDN (`--bind=/dev` menutupi `rootfs/dev`), sedangkan Chromium menulis
segmen shared-memory-nya di sana. proot-distro menyediakan direktori `shm/` saudara rootfs
lalu mem-bind-nya. Perbaikan: `crates/proot-distro-core/src/guestdirs.rs` membuat
`containers/<nama>/shm` mode **1777** dan mem-bind ke `/dev/shm` setelah `--bind=/dev`.

```
$ podnext login deb-pdn -- /bin/sh -c 'ls -ld /dev/shm; touch /dev/shm/probe && echo SHM_WRITABLE'
drwxrwxrwt. 2 root root 3452 ...  /dev/shm
SHM_WRITABLE
```

Playwright (`playwright-core` 1.64.1 di host) + Chromium host, dijalankan dari dalam Debian
milik PDN:

```
$ podnext copy ~/.hermes/cache/scratch/pwtest.js deb-pdn:/root/pwtest.js
$ podnext login deb-pdn -- /bin/sh -c \
    'NODE_PATH=/data/data/com.termux/files/usr/lib/node_modules \
     /data/data/com.termux/files/usr/bin/node /root/pwtest.js'
PLAYWRIGHT_DOM=PW_OK
UA=Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) HeadlessChrome/156.0.0.0 Safari/537.36
```

Chromium headless langsung:

```
$ podnext login deb-pdn -- /bin/sh -c '/data/data/com.termux/files/usr/lib/chromium/chrome \
    --headless=new --no-sandbox --disable-gpu --user-data-dir=/root/cud \
    --dump-dom "data:text/html,<h1>OKPDN</h1>"'
<html><head></head><body><h1>OKPDN</h1></body></html>
```

Catatan teknik dari pengujian: `--no-sandbox` diperlukan (unshare tidak nyata di bawah proot);
`--disable-dev-shm-usage` **tidak** diperlukan setelah `/dev/shm` ada; `headless_shell` milik
Termux diam-diam tidak menghasilkan output untuk `--dump-dom`, pakai `chrome`.

### 6.2 Node.js

Dua jalur, keduanya tanpa memasang apa pun di guest:
- Node host (bionic) dipanggil dengan path absolut dari dalam guest → berjalan.
- `apt-get install -y nodejs npm` tersedia di trixie (nodejs 20.19.2) — belum diunduh
  (±55–70 MB, menunggu keputusan).

### 6.3 Docker di dalam container — TIDAK MUNGKIN

Diukur langsung pada uid 10396: `CapEff=CapPrm=CapBnd=0`; `mount(2)` → **proses dibunuh
SIGSYS (31)** oleh seccomp Android; `unshare(CLONE_NEWUSER)` → `EINVAL`,
`CLONE_NEWNS/NEWPID/NEWNET` → `EPERM`; `/sys/fs/cgroup` ada tapi tidak bisa ditulis;
tidak ada `dockerd` di host (hanya klien `docker` v24.0.6). proot hanya **memalsukan**
`mount`/`unshare` dengan rc=0, jadi dockerd akan tampak hidup lalu menghasilkan container
yang tidak bisa dipakai.

Satu-satunya jalur nyata adalah kernel nyata di ruang pengguna: proyek `uml4termux`
(`~/workspace/uml4termux`) menjalankan dockerd 20.10.24 + containerd + runc dengan overlay2
dan cgroup v2 di perangkat ini tanpa root/KVM.

### 6.4 Hermes Agent sendiri di dalam container — BERJALAN

```
$ proot-distro login debian -- /data/data/com.termux/files/usr/bin/hermes --version
Hermes Agent v0.22.0+canary.20261014T070054Z (2026.9.24) · canary
Install directory: /data/data/com.termux/files/usr/lib/hermes-agent/app
Install method: apt
Python: 3.14.6
OpenAI SDK: 2.24.0
```

Seluruh rantai impor termuat di dalam guest, karena `$TERMUX_PREFIX` di-bind sehingga
interpreter bionic terselesaikan. Pemasangan bersih di dalam guest (Python glibc trixie
3.13.5) juga layak: `app/pyproject.toml` menetapkan `requires-python = ">=3.11,<3.15"` dan
penanda `psutil==7.2.2; sys_platform != 'android'` — di guest `sys_platform == 'linux'`,
jadi wheel PyPI biasa berlaku. Yang **tidak** boleh dipakai ulang adalah launcher
`$PREFIX/bin/hermes` (meng-hardcode `SITE=venv/lib/python3.14/site-packages`,
`tools/python/...`, dan `LD_LIBRARY_PATH` Termux) dan wheel native bionic
(`_cffi_backend.cpython-314-aarch64-linux-android.so`).

---

## 7. Perintah yang ditambahkan

| Perintah | Perilaku |
|---|---|
| `podnext rename <lama> <baru>` | direktori container *adalah* identitasnya: `rename(2)` + tulis ulang `metadata.json` (nama + path rootfs) |
| `podnext copy [--recursive] [--move] <SRC> <DEST>` | spec `[CONTAINER:]PATH`; rootfs adalah direktori host biasa, jadi tidak perlu protokol. Symlink ujung **diikuti** pada copy seperti `cp`, dan dipertahankan pada `--move`. Direktori butuh `--recursive` |

Bukti:

```
$ podnext rename deb-restored deb-renamed
Renamed container 'deb-restored' to 'deb-renamed'
$ podnext list | grep deb-
| deb-pdn          | debian  | latest   | aarch64 | Stopped | ... |
| deb-renamed      | deb-pdn | restored | aarch64 | Stopped | ... |

$ podnext copy ~/pdn-copytest.txt deb-pdn:/root/copytest.txt
$ podnext login deb-pdn -- cat /root/copytest.txt
hello-from-host
```

**Bug yang ditemukan saat mengujinya:** versi pertama `copy` mempertahankan symlink ujung,
sehingga `copy deb-pdn:/etc/os-release ~/x` menghasilkan symlink menggantung
(`-> ../usr/lib/os-release`) dan bukan isi berkas. `cp` dan proot-distro mengikuti symlink
ujung pada copy — hanya `--move` yang memindahkan link-nya. Sudah diperbaiki.

---

## 8. Bug lain yang ditemukan dan diperbaiki

**`ContainerLock::acquire` panik.** `libc::flock(self.file.as_ref().unwrap()...)` dijalankan
sebelum `self.file = Some(file)`, jadi setiap pemanggilan pertama panik pada
`Option::unwrap()`. Diperbaiki: kunci deskriptor `file` yang baru dibuka. Efek: setiap
operasi container yang memakai kunci kini benar-benar dapat mengambil kunci.

**Ekstraksi arsip tidak aman/portabel.** `Archive::unpack` dipakai di empat jalur
(`compress.rs` ×3, `backup_restore`) dan tidak menangani hard link di Android. Semua kini
memakai `untar::extract` yang sama.

---

## 9. Batasan yang tetap ada

- Ownership (uid/gid) file tidak dipulihkan saat ekstraksi — aplikasi Android tanpa hak
  tidak dapat `chown`; hanya mode yang diterapkan.
- `sync` dan `reset` belum ada (spec ada, implementasi belum).
- `build`, `gitops`, `policy eval`, `dbus monitor` masih stub; sesuai audit stub, ~18 dari 21
  stub hanya butuh *wiring* karena manajer-nya sudah nyata — yang benar-benar hilang hanya
  backend OCI nyata, daemon yang memegang manajer, dan persistensi state manajer.
- `quota set`/`get` hari ini **lossy** (state in-memory per-invokasi), bukan hanya `usage`.
- `/etc/hostname` di rootfs debian berisi `debuerreotype` (dari image); `hostname` tetap
  melaporkan `localhost` karena proot menimpanya, sama seperti proot-distro.
