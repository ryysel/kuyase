mod squashfs;
mod ubifs;

pub use squashfs::{extract_squashfs_into_tree, SQUASHFS_MAGIC};
pub use ubifs::{extract_ubifs_into_tree, UBIFS_NODE_MAGIC};

use crate::ext_fs::{read_ext2_image, EXT2_SUPER_MAGIC};
use crate::image_builder::{read_tar_from_reader, RootfsTree};
use flate2::read::{DeflateDecoder, ZlibDecoder};
use std::collections::BTreeMap;
use std::fs;
use std::io::{Cursor, Read};

pub const UBI_EC_MAGIC: &[u8; 4] = b"UBI#";
pub const UBI_VID_MAGIC: &[u8; 4] = b"UBI!";

pub(crate) fn decompress_zlib_or_deflate(input: &[u8]) -> Result<Vec<u8>, String> {
    let mut out = Vec::new();
    if ZlibDecoder::new(input).read_to_end(&mut out).is_ok() && !out.is_empty() {
        return Ok(out);
    }
    out.clear();
    DeflateDecoder::new(input)
        .read_to_end(&mut out)
        .map_err(|e| format!("deflate decompression failed: {}", e))?;
    Ok(out)
}

fn detect_ubi_peb_size(raw: &[u8], first_data_offset: usize) -> usize {
    let min_step = first_data_offset.max(512);
    let mut off = min_step;
    while off + 64 <= raw.len() {
        if &raw[off..off + 4] == UBI_EC_MAGIC && raw[off + 4] == 1 {
            let vid_off =
                u32::from_be_bytes([raw[off + 16], raw[off + 17], raw[off + 18], raw[off + 19]])
                    as usize;
            let data_off =
                u32::from_be_bytes([raw[off + 20], raw[off + 21], raw[off + 22], raw[off + 23]])
                    as usize;
            if vid_off >= 64 && data_off > vid_off && off + data_off <= raw.len() {
                return off;
            }
        }
        off += 64;
    }
    raw.len()
}

pub fn extract_payload_bytes_into_tree(payload: &[u8]) -> Result<RootfsTree, String> {
    if payload.len() >= 4 {
        let magic_le = u32::from_le_bytes([payload[0], payload[1], payload[2], payload[3]]);
        if magic_le == SQUASHFS_MAGIC {
            return extract_squashfs_into_tree(payload);
        }
        if magic_le == UBIFS_NODE_MAGIC {
            return extract_ubifs_into_tree(payload);
        }
    }
    if payload.len() >= 2048
        && u16::from_le_bytes([payload[1080], payload[1081]]) == EXT2_SUPER_MAGIC
    {
        let tmp = std::env::temp_dir().join(format!(
            "kuyase-ubi-ext-{}-{}.img",
            std::process::id(),
            payload.len()
        ));
        fs::write(&tmp, payload).map_err(|e| e.to_string())?;
        let res = read_ext2_image(&tmp);
        let _ = fs::remove_file(&tmp);
        return res;
    }
    if payload.windows(4).any(|w| w == [0x31, 0x18, 0x10, 0x06]) {
        if let Ok(tree) = extract_ubifs_into_tree(payload) {
            return Ok(tree);
        }
    }
    if let Some(pos) = payload.windows(4).position(|w| w == b"hsqs") {
        if let Ok(tree) = extract_squashfs_into_tree(&payload[pos..]) {
            return Ok(tree);
        }
    }
    if let Ok(t) = read_tar_from_reader(Cursor::new(payload)) {
        if !t.entries.is_empty() {
            return Ok(t);
        }
    }
    Err("unrecognized volume payload inside UBI image".to_string())
}

pub fn extract_ubi_into_tree(raw: &[u8]) -> Result<RootfsTree, String> {
    if raw.len() < 128 || &raw[0..4] != UBI_EC_MAGIC {
        return Err("not a valid UBI# image".to_string());
    }
    let first_data_off = u32::from_be_bytes([raw[20], raw[21], raw[22], raw[23]]) as usize;
    let peb_size = detect_ubi_peb_size(raw, first_data_off);
    if peb_size < 128 {
        return Err("invalid UBI PEB size".to_string());
    }

    // Map (vol_id, lnum) -> (sqnum, slice)
    let mut volumes: BTreeMap<u32, BTreeMap<u32, (u64, &[u8])>> = BTreeMap::new();
    let mut peb_off = 0usize;
    while peb_off + 64 <= raw.len() {
        let peb = &raw[peb_off..(peb_off + peb_size).min(raw.len())];
        if &peb[0..4] == UBI_EC_MAGIC {
            let vid_off = u32::from_be_bytes([peb[16], peb[17], peb[18], peb[19]]) as usize;
            let data_off = u32::from_be_bytes([peb[20], peb[21], peb[22], peb[23]]) as usize;
            if vid_off + 64 <= peb.len() && data_off <= peb.len() {
                let vid = &peb[vid_off..vid_off + 64];
                if &vid[0..4] == UBI_VID_MAGIC {
                    let vol_type = vid[5];
                    let vol_id = u32::from_be_bytes([vid[8], vid[9], vid[10], vid[11]]);
                    let lnum = u32::from_be_bytes([vid[12], vid[13], vid[14], vid[15]]);
                    let data_size =
                        u32::from_be_bytes([vid[20], vid[21], vid[22], vid[23]]) as usize;
                    let sqnum = u64::from_be_bytes([
                        vid[36], vid[37], vid[38], vid[39], vid[40], vid[41], vid[42], vid[43],
                    ]);

                    if vol_id < 0x7FFF_EFFF {
                        let leb_cap = peb.len() - data_off;
                        let take = if vol_type == 2 && data_size > 0 && data_size <= leb_cap {
                            data_size
                        } else {
                            leb_cap
                        };
                        let data_slice = &peb[data_off..data_off + take];
                        let vol_map = volumes.entry(vol_id).or_default();
                        let replace = vol_map
                            .get(&lnum)
                            .map(|(old_sq, _)| sqnum >= *old_sq)
                            .unwrap_or(true);
                        if replace {
                            vol_map.insert(lnum, (sqnum, data_slice));
                        }
                    }
                }
            }
        }
        peb_off += peb_size;
    }

    for (_vol_id, lebs) in volumes {
        let mut reassembled = Vec::new();
        for (_lnum, (_sq, bytes)) in lebs {
            reassembled.extend_from_slice(bytes);
        }
        if let Ok(tree) = extract_payload_bytes_into_tree(&reassembled) {
            if !tree.entries.is_empty() {
                return Ok(tree);
            }
        }
    }

    // Fallback: scan raw image directly
    extract_payload_bytes_into_tree(raw)
}

#[cfg(test)]
mod tests {
    use super::ubifs::{
        S_IFDIR, S_IFLNK, S_IFREG, UBIFS_COMPR_NONE, UBIFS_DATA_NODE, UBIFS_DENT_NODE,
        UBIFS_INO_NODE,
    };
    use super::*;
    use crate::image_builder::RootfsNodeKind;

    fn build_ubi_peb(
        peb_size: usize,
        vol_id: u32,
        lnum: u32,
        sqnum: u64,
        leb_payload: &[u8],
    ) -> Vec<u8> {
        let vid_off = 64u32;
        let data_off = 128u32;
        let mut peb = vec![0xFFu8; peb_size];
        // EC header
        peb[0..4].copy_from_slice(UBI_EC_MAGIC);
        peb[4] = 1;
        peb[16..20].copy_from_slice(&vid_off.to_be_bytes());
        peb[20..24].copy_from_slice(&data_off.to_be_bytes());
        // VID header
        let vid = &mut peb[vid_off as usize..vid_off as usize + 64];
        vid.fill(0);
        vid[0..4].copy_from_slice(UBI_VID_MAGIC);
        vid[4] = 1;
        vid[5] = 1; // dynamic volume
        vid[8..12].copy_from_slice(&vol_id.to_be_bytes());
        vid[12..16].copy_from_slice(&lnum.to_be_bytes());
        vid[20..24].copy_from_slice(&(leb_payload.len() as u32).to_be_bytes());
        vid[36..44].copy_from_slice(&sqnum.to_be_bytes());
        // LEB data
        peb[data_off as usize..data_off as usize + leb_payload.len()].copy_from_slice(leb_payload);
        peb
    }

    fn build_ubifs_ino_node(
        sqnum: u64,
        inum: u32,
        mode: u16,
        uid: u16,
        gid: u16,
        size: u64,
        inline_data: &[u8],
    ) -> Vec<u8> {
        let len = 160 + inline_data.len();
        let mut buf = vec![0u8; (len + 7) & !7];
        buf[0..4].copy_from_slice(&UBIFS_NODE_MAGIC.to_le_bytes());
        buf[8..16].copy_from_slice(&sqnum.to_le_bytes());
        buf[16..20].copy_from_slice(&(len as u32).to_le_bytes());
        buf[20] = UBIFS_INO_NODE;
        buf[24..28].copy_from_slice(&inum.to_le_bytes());
        buf[48..56].copy_from_slice(&size.to_le_bytes());
        buf[92..96].copy_from_slice(&1u32.to_le_bytes()); // nlink = 1
        buf[96..100].copy_from_slice(&(uid as u32).to_le_bytes());
        buf[100..104].copy_from_slice(&(gid as u32).to_le_bytes());
        buf[104..108].copy_from_slice(&(mode as u32).to_le_bytes());
        buf[112..116].copy_from_slice(&(inline_data.len() as u32).to_le_bytes());
        buf[160..160 + inline_data.len()].copy_from_slice(inline_data);
        buf
    }

    fn build_ubifs_dent_node(
        sqnum: u64,
        parent_inum: u32,
        target_inum: u32,
        entry_type: u8,
        name: &str,
    ) -> Vec<u8> {
        let nb = name.as_bytes();
        let len = 56 + nb.len() + 1;
        let mut buf = vec![0u8; (len + 7) & !7];
        buf[0..4].copy_from_slice(&UBIFS_NODE_MAGIC.to_le_bytes());
        buf[8..16].copy_from_slice(&sqnum.to_le_bytes());
        buf[16..20].copy_from_slice(&(len as u32).to_le_bytes());
        buf[20] = UBIFS_DENT_NODE;
        buf[24..28].copy_from_slice(&parent_inum.to_le_bytes());
        buf[40..48].copy_from_slice(&(target_inum as u64).to_le_bytes());
        buf[49] = entry_type;
        buf[50..52].copy_from_slice(&(nb.len() as u16).to_le_bytes());
        buf[56..56 + nb.len()].copy_from_slice(nb);
        buf
    }

    fn build_ubifs_data_node(sqnum: u64, inum: u32, block_num: u32, data: &[u8]) -> Vec<u8> {
        let len = 48 + data.len();
        let mut buf = vec![0u8; (len + 7) & !7];
        buf[0..4].copy_from_slice(&UBIFS_NODE_MAGIC.to_le_bytes());
        buf[8..16].copy_from_slice(&sqnum.to_le_bytes());
        buf[16..20].copy_from_slice(&(len as u32).to_le_bytes());
        buf[20] = UBIFS_DATA_NODE;
        buf[24..28].copy_from_slice(&inum.to_le_bytes());
        let k1 = (1u32 << 29) | (block_num & 0x1FFF_FFFF);
        buf[28..32].copy_from_slice(&k1.to_le_bytes());
        buf[40..44].copy_from_slice(&(data.len() as u32).to_le_bytes());
        buf[44..46].copy_from_slice(&UBIFS_COMPR_NONE.to_le_bytes());
        buf[48..48 + data.len()].copy_from_slice(data);
        buf
    }

    #[test]
    fn test_extract_synthetic_ubi_ubifs_image() {
        let mut leb0 = Vec::new();
        // Root dir (inum 1), /bin dir (inum 2), /bin/busybox file (inum 3), /bin/sh symlink (inum 4)
        leb0.extend_from_slice(&build_ubifs_ino_node(1, 1, S_IFDIR | 0o755, 0, 0, 0, b""));
        leb0.extend_from_slice(&build_ubifs_dent_node(2, 1, 2, 1, "bin"));
        leb0.extend_from_slice(&build_ubifs_ino_node(3, 2, S_IFDIR | 0o755, 0, 0, 0, b""));

        let mut leb1 = Vec::new();
        let busybox_bytes = b"\x7fELF-ubifs-busybox";
        leb1.extend_from_slice(&build_ubifs_dent_node(4, 2, 3, 0, "busybox"));
        leb1.extend_from_slice(&build_ubifs_ino_node(
            5,
            3,
            S_IFREG | 0o755,
            1000,
            1000,
            busybox_bytes.len() as u64,
            b"",
        ));
        leb1.extend_from_slice(&build_ubifs_data_node(6, 3, 0, busybox_bytes));
        leb1.extend_from_slice(&build_ubifs_dent_node(7, 2, 4, 2, "sh"));
        leb1.extend_from_slice(&build_ubifs_ino_node(
            8,
            4,
            S_IFLNK | 0o777,
            0,
            0,
            7,
            b"busybox",
        ));

        // Put LEB 1 in PEB 0 and LEB 0 in PEB 1 out-of-order to verify UBI (vol_id, lnum) sorting!
        let mut ubi_img = Vec::new();
        ubi_img.extend_from_slice(&build_ubi_peb(2048, 0, 1, 20, &leb1));
        ubi_img.extend_from_slice(&build_ubi_peb(2048, 0, 0, 10, &leb0));

        let tree = extract_ubi_into_tree(&ubi_img).unwrap();
        let bb = tree.get("bin/busybox").unwrap();
        assert_eq!(bb.mode, 0o755);
        assert_eq!(bb.uid, 1000);
        assert_eq!(bb.gid, 1000);
        assert_eq!(tree.get_file_bytes("bin/busybox").unwrap(), busybox_bytes);
        assert_eq!(
            tree.get("bin/sh").unwrap().kind,
            RootfsNodeKind::Symlink("busybox".to_string())
        );
    }

    #[test]
    fn test_extract_synthetic_ubi_squashfs_image() {
        // Build a valid uncompressed SquashFS 4.0 image with root dir (ino 1) -> file "hello.txt" (ino 2)
        let file_content = b"squashfs-inside-ubi-payload";
        let mut sqsh = vec![0u8; 512];

        // Data block for hello.txt at offset 96
        let data_start = 96usize;
        sqsh[data_start..data_start + file_content.len()].copy_from_slice(file_content);

        // Inode table metadata block at offset 160 (uncompressed flag 0x8000)
        // Inode 2 (basic file, 36 bytes) at inner_off 0; Inode 1 (basic dir, 32 bytes) at inner_off 36
        let inode_table_start = 160u64;
        let mut ino_meta = Vec::new();
        // Inode 2: basic file (type = 2)
        ino_meta.extend_from_slice(&2u16.to_le_bytes()); // inode_type = 2
        ino_meta.extend_from_slice(&0o644u16.to_le_bytes()); // mode
        ino_meta.extend_from_slice(&0u16.to_le_bytes()); // uid_idx = 0
        ino_meta.extend_from_slice(&0u16.to_le_bytes()); // gid_idx = 0
        ino_meta.extend_from_slice(&0u32.to_le_bytes()); // mtime
        ino_meta.extend_from_slice(&2u32.to_le_bytes()); // ino = 2
        ino_meta.extend_from_slice(&(data_start as u32).to_le_bytes()); // blocks_start
        ino_meta.extend_from_slice(&u32::MAX.to_le_bytes()); // fragment_block_index = NONE
        ino_meta.extend_from_slice(&0u32.to_le_bytes()); // block_offset
        ino_meta.extend_from_slice(&(file_content.len() as u32).to_le_bytes()); // file_size
        let blk_sz_hdr = (file_content.len() as u32) | 0x0100_0000; // uncompressed block
        ino_meta.extend_from_slice(&blk_sz_hdr.to_le_bytes());
        assert_eq!(ino_meta.len(), 36);

        // Directory table entry for "hello.txt" -> 12-byte header + 8-byte entry + 9-byte name = 29 bytes
        let mut dir_meta = Vec::new();
        dir_meta.extend_from_slice(&0u32.to_le_bytes()); // count - 1 = 0 (1 entry)
        dir_meta.extend_from_slice(&0u32.to_le_bytes()); // start_block = 0
        dir_meta.extend_from_slice(&1u32.to_le_bytes()); // inode_number = 1
        dir_meta.extend_from_slice(&0u16.to_le_bytes()); // entry_offset = 0 (points to Inode 2)
        dir_meta.extend_from_slice(&1i16.to_le_bytes()); // inode_offset = +1
        dir_meta.extend_from_slice(&2u16.to_le_bytes()); // type = file
        dir_meta.extend_from_slice(&8u16.to_le_bytes()); // name_size - 1 = 8 ("hello.txt" len 9)
        dir_meta.extend_from_slice(b"hello.txt");

        // Inode 1: basic dir (type = 1) at inner_off 36
        let root_inner_off = ino_meta.len() as u64;
        ino_meta.extend_from_slice(&1u16.to_le_bytes()); // inode_type = 1
        ino_meta.extend_from_slice(&0o755u16.to_le_bytes()); // mode
        ino_meta.extend_from_slice(&0u16.to_le_bytes());
        ino_meta.extend_from_slice(&0u16.to_le_bytes());
        ino_meta.extend_from_slice(&0u32.to_le_bytes());
        ino_meta.extend_from_slice(&1u32.to_le_bytes()); // ino = 1
        ino_meta.extend_from_slice(&0u32.to_le_bytes()); // dir_block_start = 0
        ino_meta.extend_from_slice(&2u32.to_le_bytes()); // nlink = 2
        ino_meta.extend_from_slice(&((dir_meta.len() + 3) as u16).to_le_bytes()); // file_size
        ino_meta.extend_from_slice(&0u16.to_le_bytes()); // offset = 0
        ino_meta.extend_from_slice(&1u32.to_le_bytes()); // parent_inode = 1

        let ino_hdr = (ino_meta.len() as u16) | 0x8000;
        sqsh[inode_table_start as usize..inode_table_start as usize + 2]
            .copy_from_slice(&ino_hdr.to_le_bytes());
        sqsh[inode_table_start as usize + 2..inode_table_start as usize + 2 + ino_meta.len()]
            .copy_from_slice(&ino_meta);

        let directory_table_start = (inode_table_start as usize + 2 + ino_meta.len()) as u64;
        let dir_hdr = (dir_meta.len() as u16) | 0x8000;
        sqsh[directory_table_start as usize..directory_table_start as usize + 2]
            .copy_from_slice(&dir_hdr.to_le_bytes());
        sqsh[directory_table_start as usize + 2
            ..directory_table_start as usize + 2 + dir_meta.len()]
            .copy_from_slice(&dir_meta);

        // Superblock at 0..96
        sqsh[0..4].copy_from_slice(&SQUASHFS_MAGIC.to_le_bytes());
        sqsh[4..8].copy_from_slice(&2u32.to_le_bytes()); // inodes = 2
        sqsh[12..16].copy_from_slice(&4096u32.to_le_bytes()); // block_size = 4096
        sqsh[20..22].copy_from_slice(&1u16.to_le_bytes()); // compression = 1 (gzip)
        sqsh[22..24].copy_from_slice(&12u16.to_le_bytes()); // block_log = 12
        sqsh[28..30].copy_from_slice(&4u16.to_le_bytes()); // major = 4
        sqsh[30..32].copy_from_slice(&0u16.to_le_bytes()); // minor = 0
        sqsh[32..40].copy_from_slice(&root_inner_off.to_le_bytes()); // root_inode
        sqsh[48..56].copy_from_slice(&u64::MAX.to_le_bytes()); // id_table_start
        sqsh[64..72].copy_from_slice(&inode_table_start.to_le_bytes());
        sqsh[72..80].copy_from_slice(&directory_table_start.to_le_bytes());
        sqsh[80..88].copy_from_slice(&u64::MAX.to_le_bytes()); // fragment_table_start

        let ubi_sqsh = build_ubi_peb(1024, 0, 0, 1, &sqsh);
        let tree = extract_ubi_into_tree(&ubi_sqsh).unwrap();
        assert_eq!(tree.get_file_bytes("hello.txt").unwrap(), file_content);
        assert_eq!(tree.get("hello.txt").unwrap().mode, 0o644);
    }
}
