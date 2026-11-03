use super::decompress_zlib_or_deflate;
use crate::image_builder::RootfsTree;
use std::collections::BTreeMap;

pub const UBIFS_NODE_MAGIC: u32 = 0x0610_1831;

pub(crate) const UBIFS_INO_NODE: u8 = 0;
pub(crate) const UBIFS_DATA_NODE: u8 = 1;
pub(crate) const UBIFS_DENT_NODE: u8 = 2;

pub(crate) const UBIFS_COMPR_NONE: u16 = 0;
const UBIFS_COMPR_ZLIB: u16 = 2;

const S_IFMT: u16 = 0xF000;
pub(crate) const S_IFDIR: u16 = 0x4000;
pub(crate) const S_IFREG: u16 = 0x8000;
pub(crate) const S_IFLNK: u16 = 0xA000;

#[derive(Clone, Debug)]
struct UbifsIno {
    sqnum: u64,
    size: u64,
    nlink: u32,
    uid: u16,
    gid: u16,
    mode: u16,
    data: Vec<u8>,
}

#[derive(Clone, Debug)]
struct UbifsDent {
    sqnum: u64,
    target_inum: u32,
    entry_type: u8,
    name: String,
}

#[derive(Clone, Debug)]
struct UbifsDataBlock {
    sqnum: u64,
    bytes: Vec<u8>,
}

pub fn extract_ubifs_into_tree(raw: &[u8]) -> Result<RootfsTree, String> {
    let mut inodes: BTreeMap<u32, UbifsIno> = BTreeMap::new();
    let mut dents: BTreeMap<(u32, String), UbifsDent> = BTreeMap::new();
    let mut data_blocks: BTreeMap<(u32, u32), UbifsDataBlock> = BTreeMap::new();

    let mut pos = 0usize;
    while pos + 24 <= raw.len() {
        let magic = u32::from_le_bytes([raw[pos], raw[pos + 1], raw[pos + 2], raw[pos + 3]]);
        if magic != UBIFS_NODE_MAGIC {
            pos += 1;
            continue;
        }
        let sqnum = u64::from_le_bytes([
            raw[pos + 8],
            raw[pos + 9],
            raw[pos + 10],
            raw[pos + 11],
            raw[pos + 12],
            raw[pos + 13],
            raw[pos + 14],
            raw[pos + 15],
        ]);
        let node_len =
            u32::from_le_bytes([raw[pos + 16], raw[pos + 17], raw[pos + 18], raw[pos + 19]])
                as usize;
        let node_type = raw[pos + 20];

        if node_len < 24 || pos + node_len > raw.len() {
            pos += 4;
            continue;
        }
        let node = &raw[pos..pos + node_len];

        match node_type {
            UBIFS_INO_NODE if node.len() >= 160 => {
                let inum = u32::from_le_bytes([node[24], node[25], node[26], node[27]]);
                let size = u64::from_le_bytes([
                    node[48], node[49], node[50], node[51], node[52], node[53], node[54], node[55],
                ]);
                let nlink = u32::from_le_bytes([node[92], node[93], node[94], node[95]]);
                let uid = u32::from_le_bytes([node[96], node[97], node[98], node[99]]) as u16;
                let gid = u32::from_le_bytes([node[100], node[101], node[102], node[103]]) as u16;
                let mode = u32::from_le_bytes([node[104], node[105], node[106], node[107]]) as u16;
                let data_len =
                    u32::from_le_bytes([node[112], node[113], node[114], node[115]]) as usize;
                let end = (160 + data_len).min(node.len());
                let inline = node[160..end].to_vec();

                let replace = inodes.get(&inum).map(|old| sqnum >= old.sqnum).unwrap_or(true);
                if replace {
                    inodes.insert(
                        inum,
                        UbifsIno {
                            sqnum,
                            size,
                            nlink,
                            uid,
                            gid,
                            mode,
                            data: inline,
                        },
                    );
                }
            }
            UBIFS_DENT_NODE if node.len() >= 56 => {
                let parent_inum = u32::from_le_bytes([node[24], node[25], node[26], node[27]]);
                let target_inum = u64::from_le_bytes([
                    node[40], node[41], node[42], node[43], node[44], node[45], node[46], node[47],
                ]) as u32;
                let entry_type = node[49];
                let nlen = u16::from_le_bytes([node[50], node[51]]) as usize;
                if 56 + nlen <= node.len() && nlen > 0 {
                    let name = String::from_utf8_lossy(&node[56..56 + nlen])
                        .trim_end_matches('\0')
                        .to_string();
                    if !name.is_empty() && name != "." && name != ".." {
                        let key = (parent_inum, name.clone());
                        let replace = dents.get(&key).map(|old| sqnum >= old.sqnum).unwrap_or(true);
                        if replace {
                            dents.insert(
                                key,
                                UbifsDent {
                                    sqnum,
                                    target_inum,
                                    entry_type,
                                    name,
                                },
                            );
                        }
                    }
                }
            }
            UBIFS_DATA_NODE if node.len() >= 48 => {
                let inum = u32::from_le_bytes([node[24], node[25], node[26], node[27]]);
                let k1 = u32::from_le_bytes([node[28], node[29], node[30], node[31]]);
                let block_num = k1 & 0x1FFF_FFFF;
                let uncomp_len =
                    u32::from_le_bytes([node[40], node[41], node[42], node[43]]) as usize;
                let compr_type = u16::from_le_bytes([node[44], node[45]]);
                let payload = &node[48..];
                let replace = data_blocks
                    .get(&(inum, block_num))
                    .map(|old| sqnum >= old.sqnum)
                    .unwrap_or(true);
                if replace {
                    let decoded = match compr_type {
                        UBIFS_COMPR_NONE => payload[..payload.len().min(uncomp_len)].to_vec(),
                        UBIFS_COMPR_ZLIB => {
                            decompress_zlib_or_deflate(payload).unwrap_or_default()
                        }
                        _ => payload[..payload.len().min(uncomp_len)].to_vec(),
                    };
                    data_blocks.insert(
                        (inum, block_num),
                        UbifsDataBlock {
                            sqnum,
                            bytes: decoded,
                        },
                    );
                }
            }
            _ => {}
        }

        let aligned = (node_len + 7) & !7;
        pos += aligned.max(8);
    }

    if dents.is_empty() && inodes.is_empty() {
        return Err("no valid UBIFS nodes found".to_string());
    }

    let mut children_by_parent: BTreeMap<u32, Vec<UbifsDent>> = BTreeMap::new();
    for ((parent, _), dent) in dents {
        if dent.target_inum != 0 {
            children_by_parent.entry(parent).or_default().push(dent);
        }
    }

    let mut tree = RootfsTree::default();
    let mut stack = vec![(1u32, String::new())];
    while let Some((dir_inum, prefix)) = stack.pop() {
        let Some(children) = children_by_parent.get(&dir_inum) else {
            continue;
        };
        for child in children {
            let rel = if prefix.is_empty() {
                child.name.clone()
            } else {
                format!("{}/{}", prefix, child.name)
            };
            let ino_opt = inodes.get(&child.target_inum);
            if let Some(ino) = ino_opt {
                if ino.nlink == 0 {
                    continue;
                }
            }
            let (fmt, perm, uid, gid) = match ino_opt {
                Some(ino) => (
                    ino.mode & S_IFMT,
                    if (ino.mode & 0o7777) == 0 {
                        0o755
                    } else {
                        ino.mode & 0o7777
                    },
                    ino.uid,
                    ino.gid,
                ),
                None => {
                    let fmt = match child.entry_type {
                        1 => S_IFDIR,
                        2 => S_IFLNK,
                        _ => S_IFREG,
                    };
                    (fmt, 0o755, 0, 0)
                }
            };

            if fmt == S_IFDIR || child.entry_type == 1 {
                tree.insert_dir_with_owner(&rel, perm, uid, gid);
                stack.push((child.target_inum, rel));
            } else if fmt == S_IFLNK || child.entry_type == 2 {
                let target = ino_opt
                    .map(|i| String::from_utf8_lossy(&i.data).to_string())
                    .unwrap_or_default();
                tree.insert_symlink_with_owner(&rel, target, uid, gid);
            } else if fmt == S_IFREG || child.entry_type == 0 {
                let file_size = ino_opt.map(|i| i.size as usize).unwrap_or(0);
                let mut file_bytes = vec![0u8; file_size];
                let num_blks = file_size.div_ceil(4096);
                for blk_idx in 0..num_blks as u32 {
                    if let Some(blk) = data_blocks.get(&(child.target_inum, blk_idx)) {
                        let start = (blk_idx as usize) * 4096;
                        if start < file_size {
                            let take = blk.bytes.len().min(file_size - start);
                            file_bytes[start..start + take].copy_from_slice(&blk.bytes[..take]);
                        }
                    }
                }
                if file_size == 0 {
                    if let Some(blk) = data_blocks.get(&(child.target_inum, 0)) {
                        file_bytes = blk.bytes.clone();
                    }
                }
                tree.insert_file_with_owner(&rel, file_bytes, perm, uid, gid);
            }
        }
    }

    if tree.entries.is_empty() {
        return Err("UBIFS stream contained no extractable files".to_string());
    }
    Ok(tree)
}
