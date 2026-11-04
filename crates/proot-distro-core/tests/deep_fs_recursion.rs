use std::fs;
use std::os::unix::fs::symlink;
use std::path::{Path, PathBuf};
use tempfile::tempdir;

#[test]
fn test_deep_l2s_rewrite_220_levels() {
    let td = tempdir().expect("tempdir");
    let base = td.path();
    let old_root = base.join("old_rootfs");
    let new_root = base.join("new_rootfs");
    fs::create_dir_all(&old_root).expect("create old_root");
    fs::create_dir_all(&new_root).expect("create new_root");

    // Build 220 levels of nesting inside old_root
    let mut current = old_root.clone();
    let mut symlink_count = 0;
    for i in 0..220 {
        current = current.join(format!("d{}", i));
        fs::create_dir(&current).expect("create dir");

        // Insert l2s symlinks at various depths
        if i == 50 || i == 100 || i == 150 || i == 215 {
            let l2s_link = current.join(format!("link_{}", i));
            let target = old_root.join(format!(".proot.l2s.{}", i));
            symlink(&target, &l2s_link).expect("create symlink");
            symlink_count += 1;
        }
    }

    println!(
        "[TEST] Created 220-level directory with {} l2s symlinks",
        symlink_count
    );

    // Run rewrite_l2s_tree
    let start = std::time::Instant::now();
    let res = proot_distro_core::l2s::rewrite_l2s_tree(&old_root, &old_root, &new_root);
    let dur = start.elapsed();
    println!(
        "[TEST] rewrite_l2s_tree 220 levels result: {:?}, time: {:?}",
        res, dur
    );

    assert!(
        res.is_ok(),
        "rewrite_l2s_tree failed on 220 levels: {:?}",
        res
    );
    assert_eq!(res.unwrap(), symlink_count);
}

#[test]
fn test_deep_layer_diff_220_levels() {
    let td = tempdir().expect("tempdir");
    let base = td.path().join("base_rootfs");
    let modified = td.path().join("mod_rootfs");
    let out_tar = td.path().join("diff.tar");

    fs::create_dir_all(&base).expect("create base");
    fs::create_dir_all(&modified).expect("create mod");

    // Build 220 levels in base
    let mut cur_base = base.clone();
    let mut cur_mod = modified.clone();
    for i in 0..220 {
        cur_base = cur_base.join(format!("d{}", i));
        cur_mod = cur_mod.join(format!("d{}", i));
        fs::create_dir(&cur_base).expect("create cur_base");
        fs::create_dir(&cur_mod).expect("create cur_mod");
    }

    // Add a file in modified at depth 219
    fs::write(cur_mod.join("new_file.txt"), b"payload_at_depth_220").expect("write new file");

    // Add a file in base that is deleted in modified
    let base_del_dir = base.join("d0").join("d1").join("d2");
    fs::write(base_del_dir.join("to_delete.txt"), b"delete_me").expect("write delete file");

    println!("[TEST] Running layer_diff over 220 nested levels...");
    let start = std::time::Instant::now();
    let res = proot_distro_core::layer_diff::compute_layer_diff(&base, &modified, &out_tar);
    let dur = start.elapsed();
    println!("[TEST] layer_diff result: {:?}, time: {:?}", res, dur);

    assert!(
        res.is_ok(),
        "compute_layer_diff failed on 220 levels: {:?}",
        res
    );
    assert!(out_tar.exists(), "diff tar was not created");
}

#[test]
fn test_deep_untar_220_levels() {
    let td = tempdir().expect("tempdir");
    let src = td.path().join("src");
    let dest = td.path().join("dest");
    let tar_path = td.path().join("deep.tar");

    fs::create_dir_all(&src).expect("create src");
    fs::create_dir_all(&dest).expect("create dest");

    // Build 220 levels in src
    let mut cur = src.clone();
    for i in 0..220 {
        cur = cur.join(format!("d{}", i));
        fs::create_dir(&cur).expect("create dir");
    }
    fs::write(cur.join("deepest.txt"), b"deep_content").expect("write leaf");

    // Create tar using tar crate
    let tar_file = fs::File::create(&tar_path).expect("create tar file");
    let mut builder = tar::Builder::new(tar_file);
    builder.append_dir_all(".", &src).expect("append dir all");
    builder.finish().expect("finish builder");

    println!("[TEST] Unpacking 220-level tar using untar::extract...");
    let start = std::time::Instant::now();
    let reader = fs::File::open(&tar_path).expect("open tar");
    let res = proot_distro_core::untar::extract(reader, &dest);
    let dur = start.elapsed();
    println!("[TEST] untar::extract result: {:?}, time: {:?}", res, dur);

    assert!(
        res.is_ok(),
        "untar::extract failed on 220 levels: {:?}",
        res
    );

    // Verify leaf exists
    let mut check = dest.clone();
    for i in 0..220 {
        check = check.join(format!("d{}", i));
    }
    assert!(
        check.join("deepest.txt").exists(),
        "deepest file missing in unpacked tree"
    );
}
