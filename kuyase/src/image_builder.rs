use crate::archive_formats::{extract_payload_bytes_into_tree, extract_ubi_into_tree, UBI_EC_MAGIC};
use crate::ext_fs::write_ext2_image;
pub use crate::rootfs_tree::{
    normalize_rel_path, read_tar_from_reader, read_tar_gz_into_tree, write_tree_to_tar_gz,
    RootfsNode, RootfsNodeKind, RootfsTree,
};
use crate::session::{resolve_bridge_binary, EMBEDDED_HW_BRIDGE};
use flate2::read::GzDecoder;
pub use protocol::{
    detect_board_hint_from_filename, detect_board_hint_from_path, normalize_board_slug,
};
use std::fs::{self, File};
use std::io::{Cursor, Read};
use std::path::{Path, PathBuf};

pub const EMBEDDED_INIT_SH: &[u8] = include_bytes!("../../guest-init/equasar-qemu-init.sh");
pub const EMBEDDED_S99_YAM: &[u8] = include_bytes!("../../guest-init/S99kuyase-yam");
pub const DEFAULT_DATA_IMAGE_SIZE_BYTES: u64 = 512 * 1024 * 1024;

pub fn sanitize_tar_gz_file(path: &Path) -> Result<usize, String> {
    let mut tree = read_tar_gz_into_tree(path)?;
    let before = tree.entries.len();
    sanitize_tree(&mut tree);
    let removed = before.saturating_sub(tree.entries.len());
    write_tree_to_tar_gz(&tree, path)?;
    Ok(removed)
}

pub fn detect_board_from_tree(tree: &RootfsTree) -> String {
    if let Some(board_bytes) = tree.get_file_bytes("etc/quasar-board") {
        let slug = String::from_utf8_lossy(board_bytes).trim().to_string();
        if !slug.is_empty() {
            return normalize_board_slug(&slug);
        }
    }

    // 1. Hardware DTB overlay and DSP firmware files take priority over `quasar.cfg`
    //    (where Station 2 on Amlogic AXG uses `deviceType: "yandexmidi"`).
    if tree.get("boot/yandex-cucumber-i2ctop-overlay.dtb").is_some() {
        return "cucumber".to_string();
    }
    if tree.get("boot/yandex-orion-i2c_top-overlay.dtb").is_some() {
        return "orion".to_string();
    }
    if tree.get("lib/firmware/mango-acm8625.cfg").is_some() {
        return "mango".to_string();
    }
    if tree.get("lib/firmware/bergamot-acm8625.cfg").is_some() {
        return "bergamot".to_string();
    }

    // 2. Fallback to `quasar.cfg` (disambiguating NC variants by platform IDs)
    if let Some(cfg_bytes) = tree.get_file_bytes("system/vendor/quasar/quasar.cfg") {
        let text = String::from_utf8_lossy(cfg_bytes);
        if let Some(pos) = text.find("\"deviceType\"") {
            let after = &text[pos + "\"deviceType\"".len()..];
            if let Some(colon) = after.find(':') {
                let rest = after[colon + 1..].trim_start();
                if let Some(stripped) = rest.strip_prefix('"') {
                    if let Some(end_q) = stripped.find('"') {
                        let dev = stripped[..end_q].trim();
                        if !dev.is_empty() {
                            let slug = normalize_board_slug(dev);
                            if slug == "plum" && text.contains("4794654278") {
                                return "plumnc".to_string();
                            }
                            if slug == "yandexmini2" && text.contains("4794659041") {
                                return "yandexmini2nc".to_string();
                            }
                            return slug;
                        }
                    }
                }
            }
        }
    }

    if tree.get("etc/init.d/S61leds_brightness").is_some() {
        "yandexmini2".to_string()
    } else {
        "yandexmini".to_string()
    }
}

pub fn sanitize_tree(tree: &mut RootfsTree) {
    let empty_dirs = [
        "data",
        "bkupgrade",
        "tee",
        "nv",
        "env",
        "ota",
        "tmp",
        "run",
        "var/run",
        "var/db/dhcpcd",
        "var/lib/chrony",
    ];
    let data_is_symlink = matches!(
        tree.get("data").map(|n| &n.kind),
        Some(RootfsNodeKind::Symlink(_))
    );

    let secret_exact = [
        "etc/wpa_supplicant.conf",
        "etc/hostapd_temp.conf",
        "etc/wifi/ap_name",
        "etc/shadow",
        "etc/hosts.bak",
        "etc/dropbear",
        "etc/dhcpcd.exit-hook",
        "etc/unbound/unbound.conf.head",
        "etc/init.d/S99zdropbear",
        "etc/init.d/S99zalicebridge-ssh",
        "etc/init.d/S99z_airplay",
        "etc/init.d/S99zz_jarvis",
        "etc/init.d/S99alinad",
        "etc/init.d/S100privacy",
        "etc/init.d/S89usbgadget",
        "etc/init.d/S97adb",
        "root/.ssh",
        "root/.ash_history",
        "usr/local/bin/dropbear",
        "usr/local/bin/equasar-daemon",
        "usr/bin/adbd_backup",
    ];

    let keys: Vec<String> = tree.entries.keys().cloned().collect();
    for k in keys {
        // 1. Check empty_dirs children
        let mut drop_entry = false;
        for d in &empty_dirs {
            if *d == "data" && data_is_symlink {
                continue;
            }
            if k.starts_with(&format!("{}/", d)) {
                drop_entry = true;
                break;
            }
        }
        // 2. Check secret_exact or children of secret_exact
        if !drop_entry {
            for s in &secret_exact {
                if k == *s || k.starts_with(&format!("{}/", s)) {
                    drop_entry = true;
                    break;
                }
            }
        }
        if !drop_entry && k.starts_with("etc/init.d/S99") {
            let lc = k.to_ascii_lowercase();
            if lc.contains("dropbear") || lc.contains("ssh") || lc.contains("alicebridge") {
                drop_entry = true;
            }
        }
        // 3. Check *.db / *.sqlite* under etc/, var/, root/
        if !drop_entry
            && (k.starts_with("etc/") || k.starts_with("var/") || k.starts_with("root/"))
            && (k.ends_with(".db") || k.contains(".sqlite"))
        {
            drop_entry = true;
        }
        if drop_entry {
            tree.entries.remove(&k);
        }
    }

    // Ensure required directories exist
    for dir in [
        "proc",
        "sys",
        "dev",
        "run",
        "var",
        "var/run",
        "mnt",
        "media",
        "root",
        "ota",
        "tee",
        "bkupgrade",
        "bkupgrade/data",
        "bkupgrade/data/quasar",
        "bkupgrade/data/quasar/daemons_logs",
    ] {
        tree.insert_dir(dir, 0o755);
    }
    tree.insert_dir("tmp", 0o1777);
    if !data_is_symlink {
        tree.insert_dir("data", 0o755);
    }
}

fn load_dump_into_tree(path: &Path) -> Result<RootfsTree, String> {
    if path.is_dir() {
        let candidates = [
            path.join("rootfs.tar.gz"),
            path.join("system.img.gz"),
            path.join("system.img"),
            path.join("ubivol").join("ubi0_0_rootfs.img"),
            path.join("ubi0_0_rootfs.img"),
            path.join("mtd").join("mtd3_system.bin"),
            path.join("mtd3_system.bin"),
        ];
        for c in &candidates {
            if c.is_file() {
                return load_dump_into_tree(c);
            }
        }
        if let Ok(rd) = fs::read_dir(path) {
            for e in rd.flatten() {
                let p = e.path();
                if p.extension().and_then(|s| s.to_str()) == Some("tgz") {
                    return load_dump_into_tree(&p);
                }
            }
        }
        // Load extracted directory tree
        let mut tree = RootfsTree::default();
        load_host_dir_recursive(path, path, &mut tree)?;
        return Ok(tree);
    }

    let name = path
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();

    if name.ends_with(".tar.gz") || name.ends_with(".tgz") {
        return read_tar_gz_into_tree(path);
    }
    if name.ends_with(".tar") {
        let f = File::open(path).map_err(|e| e.to_string())?;
        return read_tar_from_reader(f);
    }
    if name.ends_with(".zip") {
        let f = File::open(path).map_err(|e| e.to_string())?;
        let mut za = zip::ZipArchive::new(f).map_err(|e| format!("zip error: {}", e))?;
        // Check if zip contains a nested rootfs.tar.gz
        for i in 0..za.len() {
            let mut zf = za.by_index(i).map_err(|e| e.to_string())?;
            if zf.name().ends_with("rootfs.tar.gz") || zf.name().ends_with(".tgz") {
                let mut bytes = Vec::new();
                zf.read_to_end(&mut bytes).map_err(|e| e.to_string())?;
                return read_tar_from_reader(GzDecoder::new(Cursor::new(bytes)));
            }
        }
        let mut tree = RootfsTree::default();
        for i in 0..za.len() {
            let mut zf = za.by_index(i).map_err(|e| e.to_string())?;
            let Some(norm) = normalize_rel_path(zf.name()) else {
                continue;
            };
            let mode = zf.unix_mode().map(|m| (m as u16) & 0o7777).unwrap_or(0o755);
            if zf.is_dir() {
                tree.insert_dir(&norm, mode);
            } else {
                let mut buf = Vec::new();
                zf.read_to_end(&mut buf).map_err(|e| e.to_string())?;
                tree.insert_file(&norm, buf, mode);
            }
        }
        return Ok(tree);
    }

    let mut raw = fs::read(path).map_err(|e| format!("failed to read '{}': {}", path.display(), e))?;
    if raw.starts_with(&[0x1f, 0x8b]) {
        let mut dec = Vec::new();
        GzDecoder::new(Cursor::new(&raw))
            .read_to_end(&mut dec)
            .map_err(|e| format!("gzip decode failed: {}", e))?;
        raw = dec;
    }

    if raw.len() >= 4 && &raw[0..4] == UBI_EC_MAGIC {
        return extract_ubi_into_tree(&raw);
    }

    extract_payload_bytes_into_tree(&raw).map_err(|_| {
        format!(
            "unsupported or unrecognized dump format: {}",
            path.display()
        )
    })
}

fn load_host_dir_recursive(base: &Path, cur: &Path, tree: &mut RootfsTree) -> Result<(), String> {
    let entries = fs::read_dir(cur).map_err(|e| e.to_string())?;
    for entry in entries.flatten() {
        let p = entry.path();
        let rel = p
            .strip_prefix(base)
            .map_err(|e| e.to_string())?
            .to_string_lossy()
            .replace('\\', "/");
        if p.is_dir() {
            tree.insert_dir(&rel, 0o755);
            load_host_dir_recursive(base, &p, tree)?;
        } else if p.is_file() {
            let bytes = fs::read(&p).map_err(|e| e.to_string())?;
            tree.insert_file(&rel, bytes, 0o755);
        }
    }
    Ok(())
}

pub fn normalize_crlf_to_lf(bytes: &[u8]) -> Vec<u8> {
    bytes.iter().copied().filter(|&b| b != b'\r').collect()
}

pub fn inject_guest_files(root: &Path, tree: &mut RootfsTree) -> Result<(), String> {
    sanitize_tree(tree);
    tree.insert_dir("usr", 0o755);
    tree.insert_dir("usr/local", 0o755);
    tree.insert_dir("usr/local/bin", 0o755);
    tree.insert_dir("etc", 0o755);
    tree.insert_dir("etc/init.d", 0o755);

    let init_path = root.join("guest-init").join("equasar-qemu-init.sh");
    let raw_init = if init_path.is_file() {
        let b = fs::read(&init_path).map_err(|e| e.to_string())?;
        normalize_crlf_to_lf(&b)
    } else {
        normalize_crlf_to_lf(EMBEDDED_INIT_SH)
    };
    tree.insert_file("quasar-qemu-init.sh", raw_init, 0o755);

    let s99_yam_path = root.join("guest-init").join("S99kuyase-yam");
    let raw_s99_yam = if s99_yam_path.is_file() {
        let b = fs::read(&s99_yam_path).map_err(|e| e.to_string())?;
        normalize_crlf_to_lf(&b)
    } else {
        normalize_crlf_to_lf(EMBEDDED_S99_YAM)
    };
    tree.insert_file("etc/init.d/S99kuyase-yam", raw_s99_yam, 0o755);

    let bridge_bytes = match resolve_bridge_binary(root) {
        Ok(p) => fs::read(&p).map_err(|e| e.to_string())?,
        Err(_) => EMBEDDED_HW_BRIDGE.to_vec(),
    };
    tree.insert_file("usr/local/bin/qemu-hw-bridge", bridge_bytes, 0o755);
    Ok(())
}

pub fn persistent_data_image_path(images_dir: &Path, board: &str) -> PathBuf {
    let slug = normalize_board_slug(board);
    images_dir.join(format!("{}-data.ext4", slug))
}

pub fn ensure_persistent_data_image(
    images_dir: &Path,
    board: &str,
    force_recreate: bool,
) -> Result<PathBuf, String> {
    let _ = fs::create_dir_all(images_dir);
    let data_img = persistent_data_image_path(images_dir, board);
    if !force_recreate && data_img.is_file() {
        return Ok(data_img);
    }
    let mut tree = RootfsTree::default();
    for dir in [
        "quasar",
        "quasar/daemons_logs",
        "opt",
        "opt/bin",
        "opt/lib",
        "yam",
        "luna",
    ] {
        tree.insert_dir(dir, 0o755);
    }
    write_ext2_image(&tree, &data_img, DEFAULT_DATA_IMAGE_SIZE_BYTES)?;
    Ok(data_img)
}

fn is_ignored_backup_entry(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    lower.starts_with('.')
        || lower == "readme.md"
        || lower.starts_with("sha256sums")
        || lower.ends_with(".patch")
        || lower.ends_with(".txt")
        || lower.ends_with(".md")
        || lower.ends_with(".list.gz")
        || lower.ends_with(".tar.zst")
}

fn is_single_dump_dir(dir: &Path) -> bool {
    let single_markers = [
        dir.join("rootfs.tar.gz"),
        dir.join("system.img.gz"),
        dir.join("system.img"),
        dir.join("ubivol").join("ubi0_0_rootfs.img"),
        dir.join("ubi0_0_rootfs.img"),
        dir.join("mtd").join("mtd3_system.bin"),
        dir.join("mtd3_system.bin"),
        dir.join("system").join("vendor").join("quasar").join("quasar.cfg"),
    ];
    if single_markers.iter().any(|p| p.is_file()) {
        return true;
    }
    let mut archive_count = 0usize;
    let mut has_tgz = false;
    if let Ok(rd) = fs::read_dir(dir) {
        for e in rd.flatten() {
            let p = e.path();
            let Some(fname) = p.file_name().and_then(|s| s.to_str()) else {
                continue;
            };
            if is_ignored_backup_entry(fname) {
                continue;
            }
            let lc = fname.to_ascii_lowercase();
            if lc.ends_with(".tgz") {
                has_tgz = true;
            }
            if lc.ends_with(".tar.gz")
                || lc.ends_with(".tgz")
                || lc.ends_with(".ext4")
                || lc.ends_with(".zip")
            {
                archive_count += 1;
            }
        }
    }
    has_tgz && archive_count <= 1
}

fn collect_dumps_recursive(base: &Path, cur: &Path, out: &mut Vec<(String, PathBuf)>) {
    let Ok(rd) = fs::read_dir(cur) else {
        return;
    };
    for e in rd.flatten() {
        let p = e.path();
        let Some(name) = p.file_name().and_then(|s| s.to_str()) else {
            continue;
        };
        if is_ignored_backup_entry(name) {
            continue;
        }
        let rel = p
            .strip_prefix(base)
            .map(|r| r.to_string_lossy().replace('\\', "/"))
            .unwrap_or_else(|_| name.to_string());
        if p.is_dir() && !is_single_dump_dir(&p) {
            collect_dumps_recursive(base, &p, out);
        } else {
            out.push((rel, p));
        }
    }
}

pub fn discover_firmware_dumps(root: &Path) -> Vec<(String, PathBuf)> {
    let mut dumps = Vec::new();
    let backups = root.join("backups");
    if !backups.is_dir() {
        return dumps;
    }
    collect_dumps_recursive(&backups, &backups, &mut dumps);
    dumps.sort_by(|a, b| a.0.cmp(&b.0));
    dumps
}

pub fn build_guest_image_from_dump(
    root: &Path,
    requested_board: Option<&str>,
    dump_path: &Path,
    out_img_override: Option<&Path>,
) -> Result<(String, PathBuf), String> {
    let mut tree = load_dump_into_tree(dump_path)?;
    let detected = detect_board_hint_from_path(dump_path)
        .unwrap_or_else(|| detect_board_from_tree(&tree));
    let board = match requested_board {
        Some(b) if !b.trim().is_empty() && !b.eq_ignore_ascii_case("auto") => {
            normalize_board_slug(b)
        }
        _ => detected,
    };

    inject_guest_files(root, &mut tree)?;
    tree.insert_file(
        "etc/quasar-board",
        format!("{}\n", board).into_bytes(),
        0o644,
    );

    let work_dir = root.join("work");
    let _ = fs::create_dir_all(&work_dir);
    let out_img = match out_img_override {
        Some(p) => p.to_path_buf(),
        None => work_dir.join(format!("{}-system.ext4", board)),
    };

    let payload_bytes: usize = tree
        .entries
        .values()
        .map(|n| match &n.kind {
            RootfsNodeKind::File(b) => b.len(),
            _ => 0,
        })
        .sum();
    let img_size = if payload_bytes > 180 * 1024 * 1024 {
        1024 * 1024 * 1024
    } else if out_img_override.is_some() && payload_bytes < 8 * 1024 * 1024 {
        32 * 1024 * 1024
    } else {
        256 * 1024 * 1024
    };

    write_ext2_image(&tree, &out_img, img_size)?;
    let _ = fs::write(work_dir.join(".last-board"), format!("{}\n", board));
    println!(
        "=== [KUYASE] Built Guest Image: {} (Board Profile: {}) ===",
        out_img.display(),
        board
    );
    Ok((board, out_img))
}

pub fn prepare_images(
    root: &Path,
    requested_board: Option<&str>,
    dump_path: Option<&Path>,
) -> Result<Vec<(String, PathBuf)>, String> {
    if let Some(dp) = dump_path {
        if dp.is_dir() && !is_single_dump_dir(dp) {
            let mut built = Vec::new();
            let req_slug = requested_board
                .filter(|b| !b.trim().is_empty() && !b.eq_ignore_ascii_case("auto"))
                .map(normalize_board_slug);
            let mut candidates = Vec::new();
            collect_dumps_recursive(dp, dp, &mut candidates);
            candidates.sort_by(|a, b| a.0.cmp(&b.0));
            for (_, sp) in candidates {
                if let Some(ref target_slug) = req_slug {
                    if let Some(hint) = detect_board_hint_from_path(&sp) {
                        if &hint != target_slug {
                            continue;
                        }
                    }
                }
                built.push(build_guest_image_from_dump(root, req_slug.as_deref(), &sp, None)?);
            }
            if built.is_empty() {
                return Err(format!("no matching firmware dumps found in {}", dp.display()));
            }
            return Ok(built);
        }
        let res = build_guest_image_from_dump(root, requested_board, dp, None)?;
        return Ok(vec![res]);
    }
    if let Some(b) = requested_board {
        let candidate = Path::new(b);
        if candidate.exists() {
            return prepare_images(root, None, Some(candidate));
        }
        let slug = normalize_board_slug(b);
        // Priority is given to ready Guest Images in work/<board>-system.ext4
        let existing = root.join("work").join(format!("{}-system.ext4", slug));
        if existing.is_file() {
            let res = build_guest_image_from_dump(root, Some(&slug), &existing, None)?;
            return Ok(vec![res]);
        }
        for (_, dp) in discover_firmware_dumps(root) {
            if detect_board_hint_from_path(&dp).as_deref() == Some(slug.as_str()) {
                let res = build_guest_image_from_dump(root, Some(&slug), &dp, None)?;
                return Ok(vec![res]);
            }
        }
    }

    let work_images = crate::session::discover_guest_images(root);
    let discovered = discover_firmware_dumps(root);
    if work_images.is_empty() && discovered.is_empty() {
        return Err(format!(
            "no guest images in {}/work and no firmware dumps in {}/backups (use --dump <path> or run `kuyase dump <ip>`)",
            root.display(),
            root.display()
        ));
    }
    let mut built = Vec::new();
    let mut handled_slugs = std::collections::BTreeSet::new();
    for (slug, img_path) in work_images {
        if let Some(req) = requested_board.filter(|s| !s.trim().is_empty() && !s.eq_ignore_ascii_case("auto")) {
            if normalize_board_slug(req) != slug {
                continue;
            }
        }
        handled_slugs.insert(slug.clone());
        built.push(build_guest_image_from_dump(root, Some(&slug), &img_path, None)?);
    }
    for (_, p) in discovered {
        if let Some(hint) = detect_board_hint_from_path(&p) {
            if handled_slugs.contains(&hint) {
                continue;
            }
            handled_slugs.insert(hint);
        }
        built.push(build_guest_image_from_dump(root, requested_board, &p, None)?);
    }
    Ok(built)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ext_fs::read_ext2_image;

    #[test]
    fn test_synthetic_dump_board_detection_scrubbing_crlf_and_ext4_roundtrip() {
        let temp_dir =
            std::env::temp_dir().join(format!("kuyase-img-builder-test-{}", std::process::id()));
        let _ = fs::remove_dir_all(&temp_dir);
        fs::create_dir_all(temp_dir.join("guest-init").join("bin")).unwrap();

        // Create guest-init/equasar-qemu-init.sh with CRLF line endings
        fs::write(
            temp_dir.join("guest-init").join("equasar-qemu-init.sh"),
            b"#!/bin/sh\r\necho hello\r\n",
        )
        .unwrap();
        let mut mock_elf = b"\x7fELFmockbridge".to_vec();
        mock_elf.resize(128, 0);
        fs::write(
            temp_dir
                .join("guest-init")
                .join("bin")
                .join("qemu-hw-bridge"),
            &mock_elf,
        )
        .unwrap();

        // Build synthetic firmware dump .tar.gz with secrets and mini2 quasar.cfg
        let mut dump_tree = RootfsTree::default();
        dump_tree.insert_file(
            "system/vendor/quasar/quasar.cfg",
            br#"{"deviceType": "mini2"}"#.to_vec(),
            0o644,
        );
        dump_tree.insert_file_with_owner(
            "bin/busybox",
            b"\x7fELFbusybox".to_vec(),
            0o755,
            1000,
            1005,
        );
        dump_tree.insert_symlink("bin/sh", "busybox".to_string());
        dump_tree.insert_file("etc/wpa_supplicant.conf", b"psk=secret".to_vec(), 0o600);
        dump_tree.insert_file("data/secret.txt", b"token=123".to_vec(), 0o600);
        dump_tree.insert_file("usr/local/bin/dropbear", b"dropbear-bin".to_vec(), 0o755);
        dump_tree.insert_file("var/auth.sqlite3", b"sqlite".to_vec(), 0o600);

        let dump_tar_path = temp_dir.join("rootfs.tar.gz");
        write_tree_to_tar_gz(&dump_tree, &dump_tar_path).unwrap();

        let out_img = temp_dir.join("work").join("yandexmini2-system.ext4");
        let (board, built_path) =
            build_guest_image_from_dump(&temp_dir, None, &dump_tar_path, Some(&out_img)).unwrap();

        assert_eq!(board, "yandexmini2");
        assert_eq!(built_path, out_img);
        assert!(out_img.is_file());

        // Read back the generated .ext4 image and verify invariants
        let readback = read_ext2_image(&out_img).unwrap();
        assert_eq!(detect_board_from_tree(&readback), "yandexmini2");
        assert!(readback.get("etc/wpa_supplicant.conf").is_none());
        assert!(readback.get("data/secret.txt").is_none());
        assert!(readback.get("usr/local/bin/dropbear").is_none());
        assert!(readback.get("var/auth.sqlite3").is_none());
        let bb_node = readback.get("bin/busybox").unwrap();
        assert_eq!(bb_node.uid, 1000);
        assert_eq!(bb_node.gid, 1005);

        let init_bytes = readback.get_file_bytes("quasar-qemu-init.sh").unwrap();
        assert!(!init_bytes.contains(&b'\r'));
        assert_eq!(init_bytes, b"#!/bin/sh\necho hello\n");
        assert_eq!(readback.get("quasar-qemu-init.sh").unwrap().mode, 0o755);

        let s99_yam_node = readback.get("etc/init.d/S99kuyase-yam").unwrap();
        assert_eq!(s99_yam_node.mode, 0o755);
        let s99_yam_bytes = readback.get_file_bytes("etc/init.d/S99kuyase-yam").unwrap();
        assert!(!s99_yam_bytes.contains(&b'\r'));
        assert!(String::from_utf8_lossy(s99_yam_bytes).contains("/data/yam/bin/luna"));

        let bridge_node = readback.get("usr/local/bin/qemu-hw-bridge").unwrap();
        assert_eq!(bridge_node.mode, 0o755);
        assert_eq!(
            readback.get_file_bytes("usr/local/bin/qemu-hw-bridge").unwrap(),
            mock_elf.as_slice()
        );
        assert_eq!(readback.get("tmp").unwrap().mode, 0o1777);
        assert_eq!(
            readback.get("bin/sh").unwrap().kind,
            RootfsNodeKind::Symlink("busybox".to_string())
        );
        assert_eq!(
            readback.get_file_bytes("etc/quasar-board").unwrap(),
            b"yandexmini2\n"
        );

        // Verify tar.gz wrapping a single .ext4 image (e.g. plumnc-system.tar.gz)
        let mut inner_ext4_tree = RootfsTree::default();
        inner_ext4_tree.insert_file(
            "system/vendor/quasar/quasar.cfg",
            br#"{"deviceType": "plum", "softwareVersion": "0.369.1.50.4794654278.20261021.133"}"#
                .to_vec(),
            0o644,
        );
        inner_ext4_tree.insert_file("etc/hosts.bak", b"127.0.0.1 localhost".to_vec(), 0o644);
        let raw_ext4_path = temp_dir.join("plumnc-system.ext4");
        write_ext2_image(&inner_ext4_tree, &raw_ext4_path, 4 * 1024 * 1024).unwrap();
        let raw_ext4_bytes = fs::read(&raw_ext4_path).unwrap();

        let mut wrapper_tar_tree = RootfsTree::default();
        wrapper_tar_tree.insert_file("plumnc-system.ext4", raw_ext4_bytes, 0o644);
        let wrapped_tar_gz = temp_dir.join("plumnc-system.tar.gz");
        write_tree_to_tar_gz(&wrapper_tar_tree, &wrapped_tar_gz).unwrap();

        let out_plumnc = temp_dir.join("work").join("plumnc-system.ext4");
        let (plumnc_slug, _) =
            build_guest_image_from_dump(&temp_dir, None, &wrapped_tar_gz, Some(&out_plumnc))
                .unwrap();
        assert_eq!(plumnc_slug, "plumnc");
        let readback_plumnc = read_ext2_image(&out_plumnc).unwrap();
        assert_eq!(detect_board_from_tree(&readback_plumnc), "plumnc");
        assert!(readback_plumnc.get("etc/hosts.bak").is_none());

        let _ = fs::remove_dir_all(&temp_dir);
    }
}
