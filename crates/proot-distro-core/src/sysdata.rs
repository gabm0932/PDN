//! Fake `/proc` entries for containers.
//!
//! Android restricts or outright blocks a number of `/proc` files, so anything
//! inside a container that reads them (`top`, `htop`, `free`, `uptime`,
//! `nproc`) sees a permission error or nothing at all. proot-distro works
//! around this by writing static stand-ins into `containers/<name>/sysdata/`
//! and bind-mounting them over the real paths; this module does the same.
//!
//! Two rules matter:
//!
//! * The stubs live *beside* the rootfs, never inside it. The rootfs `/proc` is
//!   masked by `--bind=/proc`, so a file written into the image is invisible —
//!   which is what the previous implementation of this module did, and why none
//!   of it ever had an effect.
//! * A stub is bound only when the host's own file cannot be read, so a device
//!   that does expose the real file keeps the real file.
//!
//! The content is static except for `stat` and `uptime`, which are regenerated
//! on every call so their `cpuN` lines agree with the CPU count of the device
//! the container is started on.

use anyhow::Result;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};

/// `<name under sysdata/>`, `<path inside the guest>`
const ENTRIES: &[(&str, &str)] = &[
    ("loadavg", "/proc/loadavg"),
    ("stat", "/proc/stat"),
    ("uptime", "/proc/uptime"),
    ("version", "/proc/version"),
    ("vmstat", "/proc/vmstat"),
    (
        "sysctl_kernel_cap_last_cap",
        "/proc/sys/kernel/cap_last_cap",
    ),
    (
        "sysctl_inotify_max_user_watches",
        "/proc/sys/fs/inotify/max_user_watches",
    ),
    ("sysctl_kernel_overflowuid", "/proc/sys/kernel/overflowuid"),
    ("sysctl_kernel_overflowgid", "/proc/sys/kernel/overflowgid"),
];

const FAKE_LOADAVG: &str = "0.12 0.07 0.02 2/165 765\n";
const FAKE_OVERFLOW_ID: &str = "65534\n";
const FAKE_CAP_LAST_CAP: &str = "40\n";
const FAKE_INOTIFY_WATCHES: &str = "4096\n";
const FAKE_UPTIME_SECONDS: &str = "124.08";
const FAKE_VMSTAT: &str = "\
nr_free_pages 1743136
nr_zone_inactive_anon 179281
nr_zone_active_anon 7183
nr_zone_inactive_file 22858
nr_zone_active_file 51328
nr_zone_unevictable 642
nr_zone_write_pending 0
nr_mlock 0
nr_bounce 0
nr_zspages 0
nr_free_cma 0
numa_hit 1259626
numa_miss 0
numa_foreign 0
numa_interleave 720
pgpgin 200232
pgpgout 180764
pswpin 0
pswpout 0
";
const FAKE_STAT_TAIL: &str = "\
intr 127541 38 290 0 0 0 0 4 0 1 0 0 25329 258 0 5777 277 0 0 0 0 0 0 0 0 0
ctxt 140223
btime 1750000000
processes 8944
procs_running 2
procs_blocked 0
";

/// Per-CPU snapshot in USER_HZ ticks, in `/proc/stat` column order:
/// user nice system idle iowait irq softirq steal guest guest_nice.
/// Cycled when the host has more than eight CPUs.
const FAKE_CPU_ROWS: [[u64; 10]; 8] = [
    [31, 0, 226, 12027, 82, 10, 4, 9, 0, 0],
    [45, 0, 664, 11144, 21, 263, 233, 12, 0, 0],
    [494, 0, 537, 11283, 27, 10, 3, 8, 0, 0],
    [359, 0, 234, 11723, 24, 26, 5, 7, 0, 0],
    [295, 0, 268, 11772, 10, 12, 2, 12, 0, 0],
    [270, 0, 251, 11833, 15, 3, 1, 10, 0, 0],
    [430, 0, 520, 11386, 30, 8, 1, 12, 0, 0],
    [30, 0, 172, 12108, 50, 8, 1, 13, 0, 0],
];

const IDLE_COLUMN: usize = 3;
const USER_HZ: u64 = 100;

/// Directory holding the stubs: a sibling of the rootfs, so it is removed
/// together with the container.
pub fn sysdata_dir(container_dir: &Path) -> PathBuf {
    container_dir.join("sysdata")
}

/// True when `path` can be opened and a byte read from it.
fn readable(path: &str) -> bool {
    match fs::File::open(path) {
        Ok(mut f) => {
            let mut byte = [0u8; 1];
            f.read(&mut byte).is_ok()
        }
        Err(_) => false,
    }
}

/// Kernel release as the running system reports it.
///
/// PDN deliberately does not fake the guest's identity, so `/proc/version`
/// describes the real kernel rather than proot-distro's synthetic
/// `6.17.0-PRoot-Distro`.
fn kernel_release() -> String {
    // SAFETY: `uname` only writes into the struct handed to it.
    unsafe {
        let mut uts: libc::utsname = std::mem::zeroed();
        if libc::uname(&mut uts) == 0 {
            let bytes: Vec<u8> = uts
                .release
                .iter()
                .take_while(|c| **c != 0)
                .map(|c| *c as u8)
                .collect();
            if let Ok(s) = String::from_utf8(bytes) {
                if !s.is_empty() {
                    return s;
                }
            }
        }
    }
    "0.0.0".to_string()
}

/// Online CPU ids, mirroring what the guest's own `/proc/cpuinfo` will list.
fn host_cpu_ids() -> Vec<u32> {
    let mut ids = Vec::new();
    if let Ok(text) = fs::read_to_string("/proc/cpuinfo") {
        for line in text.lines() {
            if let Some((key, value)) = line.split_once(':') {
                if key.trim() == "processor" {
                    if let Ok(n) = value.trim().parse::<u32>() {
                        ids.push(n);
                    }
                }
            }
        }
    }
    if ids.is_empty() {
        // SAFETY: `sysconf` is a plain query with no side effects.
        let n = unsafe { libc::sysconf(libc::_SC_NPROCESSORS_ONLN) };
        ids = (0..if n > 0 { n as u32 } else { 1 }).collect();
    }
    ids.sort_unstable();
    ids.dedup();
    ids
}

fn cpu_rows(cpus: &[u32]) -> Vec<(u32, [u64; 10])> {
    if cpus.is_empty() {
        return vec![(0, FAKE_CPU_ROWS[0])];
    }
    cpus.iter()
        .enumerate()
        .map(|(i, cpu)| (*cpu, FAKE_CPU_ROWS[i % FAKE_CPU_ROWS.len()]))
        .collect()
}

/// `/proc/stat` with one `cpuN` line per online CPU. The aggregate `cpu` line
/// is the column-wise sum of those, as the kernel's is, so a tool that
/// cross-checks the two sees them agree.
fn fake_stat(cpus: &[u32]) -> String {
    let rows = cpu_rows(cpus);
    let mut totals = [0u64; 10];
    for (_, row) in &rows {
        for (t, v) in totals.iter_mut().zip(row.iter()) {
            *t += *v;
        }
    }
    let join = |row: &[u64; 10]| row.iter().map(u64::to_string).collect::<Vec<_>>().join(" ");
    let mut out = format!("cpu  {}\n", join(&totals));
    for (cpu, row) in &rows {
        out.push_str(&format!("cpu{} {}\n", cpu, join(row)));
    }
    out.push_str(FAKE_STAT_TAIL);
    out
}

/// `/proc/uptime`, the second field being total idle ticks over all CPUs so it
/// stays consistent with the idle column of `cpu` in `fake_stat`.
fn fake_uptime(cpus: &[u32]) -> String {
    let idle: u64 = cpu_rows(cpus).iter().map(|(_, r)| r[IDLE_COLUMN]).sum();
    format!(
        "{} {:.2}\n",
        FAKE_UPTIME_SECONDS,
        idle as f64 / USER_HZ as f64
    )
}

fn fake_version() -> String {
    format!(
        "Linux version {} (proot@termux) (gcc (GCC) 13.3.0, GNU ld (GNU Binutils) 2.42) \
         #1 SMP PREEMPT_DYNAMIC Fri, 10 Oct 2025 00:00:00 +0000\n",
        kernel_release()
    )
}

/// Write the stubs under `<container_dir>/sysdata/`.
///
/// Idempotent. An entry that is not a plain file — a symlink or a directory
/// planted where a stub belongs — is replaced rather than followed, so a guest
/// cannot leave a link behind that later sessions mount into the guest.
pub fn ensure(container_dir: &Path) -> Result<PathBuf> {
    let dir = sysdata_dir(container_dir);
    fs::create_dir_all(&dir)?;
    fs::create_dir_all(dir.join("sys_empty"))?;

    let cpus = host_cpu_ids();
    let entries = [
        ("loadavg", FAKE_LOADAVG.to_string()),
        ("stat", fake_stat(&cpus)),
        ("uptime", fake_uptime(&cpus)),
        ("version", fake_version()),
        ("vmstat", FAKE_VMSTAT.to_string()),
        ("sysctl_kernel_cap_last_cap", FAKE_CAP_LAST_CAP.to_string()),
        (
            "sysctl_inotify_max_user_watches",
            FAKE_INOTIFY_WATCHES.to_string(),
        ),
        ("sysctl_kernel_overflowuid", FAKE_OVERFLOW_ID.to_string()),
        ("sysctl_kernel_overflowgid", FAKE_OVERFLOW_ID.to_string()),
    ];

    for (name, content) in entries {
        let path = dir.join(name);
        match fs::symlink_metadata(&path) {
            Ok(meta) if meta.file_type().is_file() => {
                if fs::read_to_string(&path).ok().as_deref() == Some(content.as_str()) {
                    continue;
                }
            }
            Ok(_) => {
                // Wrong type (symlink, directory): drop it, do not follow it.
                fs::remove_file(&path).or_else(|_| fs::remove_dir_all(&path))?;
            }
            Err(_) => {}
        }
        fs::write(&path, content)?;
    }

    Ok(dir)
}

/// `--bind=<src>:<dst>` arguments for the stubs whose host counterpart cannot
/// be read. Order matters: these are appended after `--bind=/proc` so they win.
pub fn bindings(container_dir: &Path) -> Vec<String> {
    let dir = sysdata_dir(container_dir);
    if !dir.is_dir() {
        return Vec::new();
    }

    let mut out: Vec<String> = Vec::new();
    // Android keeps SELinux state out of reach; an empty directory stands in.
    let empty = dir.join("sys_empty");
    if empty.is_dir() {
        out.push(format!("{}:/sys/fs/selinux", empty.display()));
    }
    for (name, guest_path) in ENTRIES {
        if readable(guest_path) {
            continue;
        }
        let stub = dir.join(name);
        if stub.is_file() {
            out.push(format!("{}:{}", stub.display(), guest_path));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stat_aggregate_equals_column_sum() {
        let stat = fake_stat(&[0, 1, 2, 3]);
        // one `cpu` + four `cpuN`
        assert_eq!(stat.lines().filter(|l| l.starts_with("cpu")).count(), 5);

        let mut sum = [0u64; 10];
        for line in stat
            .lines()
            .filter(|l| l.starts_with("cpu") && l.split_whitespace().next() != Some("cpu"))
        {
            for (i, field) in line.split_whitespace().skip(1).enumerate() {
                sum[i] += field.parse::<u64>().unwrap();
            }
        }
        let agg: Vec<u64> = stat
            .lines()
            .next()
            .unwrap()
            .split_whitespace()
            .skip(1)
            .map(|f| f.parse().unwrap())
            .collect();
        assert_eq!(agg, sum.to_vec());
    }

    #[test]
    fn uptime_idle_matches_stat_idle_column() {
        let cpus = [0u32, 1, 2, 3];
        let idle: u64 = cpu_rows(&cpus).iter().map(|(_, r)| r[IDLE_COLUMN]).sum();
        let uptime = fake_uptime(&cpus);
        assert_eq!(
            uptime.split_whitespace().nth(1).unwrap(),
            format!("{:.2}", idle as f64 / USER_HZ as f64)
        );
    }

    #[test]
    fn ensure_writes_every_entry_then_bindings_cover_the_blocked_ones() {
        let tmp = std::env::temp_dir().join(format!("pdn-sysdata-{}", std::process::id()));
        let _ = fs::remove_dir_all(&tmp);
        fs::create_dir_all(&tmp).unwrap();

        let dir = ensure(&tmp).expect("ensure");
        for (name, _) in ENTRIES {
            assert!(dir.join(name).is_file(), "missing stub {name}");
        }
        assert!(dir.join("sys_empty").is_dir());

        let b = bindings(&tmp);
        // Every entry whose host file is blocked, plus the selinux empty dir.
        let blocked = ENTRIES.iter().filter(|(_, g)| !readable(g)).count();
        assert_eq!(b.len(), blocked + 1);
        // Probes the module's own premise: /proc/uptime is blocked on Android.
        assert!(b.iter().any(|a| a.ends_with(":/proc/uptime")));
        // A host path that IS readable is never shadowed.
        assert!(!b.iter().any(|a| a.ends_with(":/etc/hostname")));

        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn ensure_replaces_a_planted_symlink() {
        let tmp = std::env::temp_dir().join(format!("pdn-sysdata-link-{}", std::process::id()));
        let _ = fs::remove_dir_all(&tmp);
        fs::create_dir_all(&tmp).unwrap();
        let dir = sysdata_dir(&tmp);
        fs::create_dir_all(&dir).unwrap();

        let victim = tmp.join("host-secret");
        fs::write(&victim, "host data").unwrap();
        #[cfg(unix)]
        std::os::unix::fs::symlink(&victim, dir.join("loadavg")).unwrap();

        ensure(&tmp).expect("ensure");
        let meta = fs::symlink_metadata(dir.join("loadavg")).unwrap();
        assert!(
            meta.file_type().is_file(),
            "symlink should have been replaced"
        );
        assert_eq!(
            fs::read_to_string(dir.join("loadavg")).unwrap(),
            FAKE_LOADAVG
        );
        assert_eq!(fs::read_to_string(&victim).unwrap(), "host data");

        let _ = fs::remove_dir_all(&tmp);
    }
}
