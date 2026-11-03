use super::decompress_zlib_or_deflate;
use crate::image_builder::RootfsTree;

pub const SQUASHFS_MAGIC: u32 = 0x7371_7368; // "hsqs" LE

fn read_squashfs_meta_block(raw: &[u8], pos: usize) -> Result<(Vec<u8>, usize), String> {
    if pos + 2 > raw.len() {
        return Err("squashfs metadata block out of bounds".to_string());
    }
    let hdr = u16::from_le_bytes([raw[pos], raw[pos + 1]]);
    let comp_len = (hdr & 0x7FFF) as usize;
    let is_uncompressed = (hdr & 0x8000) != 0;
    if comp_len == 0 || pos + 2 + comp_len > raw.len() {
        return Err("squashfs metadata payload out of bounds".to_string());
    }
    let slice = &raw[pos + 2..pos + 2 + comp_len];
    let data = if is_uncompressed {
        slice.to_vec()
    } else {
        decompress_zlib_or_deflate(slice)?
    };
    Ok((data, pos + 2 + comp_len))
}

fn read_squashfs_meta_stream(
    raw: &[u8],
    table_start: u64,
    block_rel: u64,
    inner_off: usize,
    min_len: usize,
) -> Result<Vec<u8>, String> {
    let mut cur_pos = (table_start + block_rel) as usize;
    let mut stream = Vec::new();
    while stream.len() < inner_off + min_len && cur_pos < raw.len() {
        let Ok((chunk, next_pos)) = read_squashfs_meta_block(raw, cur_pos) else {
            break;
        };
        stream.extend_from_slice(&chunk);
        cur_pos = next_pos;
    }
    if stream.len() < inner_off {
        return Err("squashfs metadata inner_off exceeds stream".to_string());
    }
    Ok(stream[inner_off..].to_vec())
}

fn decompress_squashfs_data_block(
    raw: &[u8],
    offset: usize,
    size_hdr: u32,
    block_size: usize,
) -> Result<Vec<u8>, String> {
    if size_hdr == 0 {
        return Ok(vec![0u8; block_size]);
    }
    let comp_len = (size_hdr & 0x00FF_FFFF) as usize;
    let is_uncompressed = (size_hdr & 0x0100_0000) != 0;
    if offset + comp_len > raw.len() {
        return Err("squashfs data block out of bounds".to_string());
    }
    let slice = &raw[offset..offset + comp_len];
    if is_uncompressed {
        Ok(slice.to_vec())
    } else {
        decompress_zlib_or_deflate(slice)
    }
}

pub fn extract_squashfs_into_tree(raw: &[u8]) -> Result<RootfsTree, String> {
    if raw.len() < 96 {
        return Err("squashfs image too small (< 96 bytes)".to_string());
    }
    let magic = u32::from_le_bytes([raw[0], raw[1], raw[2], raw[3]]);
    if magic != SQUASHFS_MAGIC {
        return Err(format!("invalid squashfs magic: 0x{:08x}", magic));
    }

    let block_size = u32::from_le_bytes([raw[12], raw[13], raw[14], raw[15]]) as usize;
    let fragments_count = u32::from_le_bytes([raw[16], raw[17], raw[18], raw[19]]) as usize;
    let no_ids = u16::from_le_bytes([raw[26], raw[27]]) as usize;
    let root_inode_ref = u64::from_le_bytes([
        raw[32], raw[33], raw[34], raw[35], raw[36], raw[37], raw[38], raw[39],
    ]);
    let id_table_start = u64::from_le_bytes([
        raw[48], raw[49], raw[50], raw[51], raw[52], raw[53], raw[54], raw[55],
    ]);
    let inode_table_start = u64::from_le_bytes([
        raw[64], raw[65], raw[66], raw[67], raw[68], raw[69], raw[70], raw[71],
    ]);
    let directory_table_start = u64::from_le_bytes([
        raw[72], raw[73], raw[74], raw[75], raw[76], raw[77], raw[78], raw[79],
    ]);
    let fragment_table_start = u64::from_le_bytes([
        raw[80], raw[81], raw[82], raw[83], raw[84], raw[85], raw[86], raw[87],
    ]);

    // 1. Parse ID table (if present)
    let mut id_table: Vec<u16> = Vec::new();
    if no_ids > 0 && id_table_start != u64::MAX && (id_table_start as usize) + 8 <= raw.len() {
        let id_blocks = no_ids.div_ceil(2048);
        for b in 0..id_blocks {
            let ptr_off = (id_table_start as usize) + b * 8;
            if ptr_off + 8 > raw.len() {
                break;
            }
            let meta_off = u64::from_le_bytes([
                raw[ptr_off],
                raw[ptr_off + 1],
                raw[ptr_off + 2],
                raw[ptr_off + 3],
                raw[ptr_off + 4],
                raw[ptr_off + 5],
                raw[ptr_off + 6],
                raw[ptr_off + 7],
            ]) as usize;
            if let Ok((meta_bytes, _)) = read_squashfs_meta_block(raw, meta_off) {
                for chunk in meta_bytes.chunks_exact(4) {
                    if id_table.len() < no_ids {
                        let id =
                            u32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]) as u16;
                        id_table.push(id);
                    }
                }
            }
        }
    }

    // 2. Parse Fragment table (if present)
    let mut fragments: Vec<(u64, u32)> = Vec::new();
    if fragments_count > 0
        && fragment_table_start != u64::MAX
        && (fragment_table_start as usize) + 8 <= raw.len()
    {
        let frag_blocks = fragments_count.div_ceil(512);
        for b in 0..frag_blocks {
            let ptr_off = (fragment_table_start as usize) + b * 8;
            if ptr_off + 8 > raw.len() {
                break;
            }
            let meta_off = u64::from_le_bytes([
                raw[ptr_off],
                raw[ptr_off + 1],
                raw[ptr_off + 2],
                raw[ptr_off + 3],
                raw[ptr_off + 4],
                raw[ptr_off + 5],
                raw[ptr_off + 6],
                raw[ptr_off + 7],
            ]) as usize;
            if let Ok((meta_bytes, _)) = read_squashfs_meta_block(raw, meta_off) {
                for chunk in meta_bytes.chunks_exact(16) {
                    if fragments.len() < fragments_count {
                        let start_blk = u64::from_le_bytes([
                            chunk[0], chunk[1], chunk[2], chunk[3], chunk[4], chunk[5], chunk[6],
                            chunk[7],
                        ]);
                        let sz = u32::from_le_bytes([chunk[8], chunk[9], chunk[10], chunk[11]]);
                        fragments.push((start_blk, sz));
                    }
                }
            }
        }
    }

    let resolve_id = |idx: u16| -> u16 { id_table.get(idx as usize).copied().unwrap_or(0) };

    let mut tree = RootfsTree::default();
    let mut stack = vec![(root_inode_ref, String::new())];

    while let Some((inode_ref, prefix)) = stack.pop() {
        let block_rel = inode_ref >> 16;
        let inner_off = (inode_ref & 0xFFFF) as usize;
        let ino_buf = read_squashfs_meta_stream(raw, inode_table_start, block_rel, inner_off, 64)?;
        if ino_buf.len() < 16 {
            continue;
        }
        let inode_type = u16::from_le_bytes([ino_buf[0], ino_buf[1]]);
        let mode = u16::from_le_bytes([ino_buf[2], ino_buf[3]]) & 0o7777;
        let uid = resolve_id(u16::from_le_bytes([ino_buf[4], ino_buf[5]]));
        let gid = resolve_id(u16::from_le_bytes([ino_buf[6], ino_buf[7]]));

        match inode_type {
            // Basic Directory (1) or Extended Directory (8)
            1 | 8 => {
                let (dir_block_start, dir_offset, file_size) = if inode_type == 1 {
                    if ino_buf.len() < 32 {
                        continue;
                    }
                    let dbs =
                        u32::from_le_bytes([ino_buf[16], ino_buf[17], ino_buf[18], ino_buf[19]])
                            as u64;
                    let fsz = u16::from_le_bytes([ino_buf[24], ino_buf[25]]) as usize;
                    let doff = u16::from_le_bytes([ino_buf[26], ino_buf[27]]) as usize;
                    (dbs, doff, fsz)
                } else {
                    if ino_buf.len() < 40 {
                        continue;
                    }
                    let fsz =
                        u32::from_le_bytes([ino_buf[20], ino_buf[21], ino_buf[22], ino_buf[23]])
                            as usize;
                    let dbs =
                        u32::from_le_bytes([ino_buf[24], ino_buf[25], ino_buf[26], ino_buf[27]])
                            as u64;
                    let doff = u16::from_le_bytes([ino_buf[34], ino_buf[35]]) as usize;
                    (dbs, doff, fsz)
                };

                if !prefix.is_empty() {
                    tree.insert_dir_with_owner(&prefix, mode, uid, gid);
                }
                let dir_bytes_len = file_size.saturating_sub(3);
                if dir_bytes_len == 0 {
                    continue;
                }
                let dir_stream = read_squashfs_meta_stream(
                    raw,
                    directory_table_start,
                    dir_block_start,
                    dir_offset,
                    dir_bytes_len,
                )?;
                let dir_slice = &dir_stream[..dir_bytes_len.min(dir_stream.len())];
                let mut pos = 0usize;
                while pos + 12 <= dir_slice.len() {
                    let count = u32::from_le_bytes([
                        dir_slice[pos],
                        dir_slice[pos + 1],
                        dir_slice[pos + 2],
                        dir_slice[pos + 3],
                    ]) as usize
                        + 1;
                    let start_blk = u32::from_le_bytes([
                        dir_slice[pos + 4],
                        dir_slice[pos + 5],
                        dir_slice[pos + 6],
                        dir_slice[pos + 7],
                    ]) as u64;
                    pos += 12;
                    for _ in 0..count {
                        if pos + 8 > dir_slice.len() {
                            break;
                        }
                        let entry_off =
                            u16::from_le_bytes([dir_slice[pos], dir_slice[pos + 1]]) as u64;
                        let name_size =
                            u16::from_le_bytes([dir_slice[pos + 6], dir_slice[pos + 7]]) as usize
                                + 1;
                        pos += 8;
                        if pos + name_size > dir_slice.len() {
                            break;
                        }
                        let name =
                            String::from_utf8_lossy(&dir_slice[pos..pos + name_size]).to_string();
                        pos += name_size;
                        let child_path = if prefix.is_empty() {
                            name
                        } else {
                            format!("{}/{}", prefix, name)
                        };
                        let child_ref = (start_blk << 16) | entry_off;
                        stack.push((child_ref, child_path));
                    }
                }
            }
            // Basic Regular File (2) or Extended Regular File (9)
            2 | 9 => {
                let (blocks_start, frag_idx, frag_off, file_size, hdr_len) = if inode_type == 2 {
                    if ino_buf.len() < 32 {
                        continue;
                    }
                    let bs =
                        u32::from_le_bytes([ino_buf[16], ino_buf[17], ino_buf[18], ino_buf[19]])
                            as usize;
                    let fi =
                        u32::from_le_bytes([ino_buf[20], ino_buf[21], ino_buf[22], ino_buf[23]]);
                    let fo =
                        u32::from_le_bytes([ino_buf[24], ino_buf[25], ino_buf[26], ino_buf[27]])
                            as usize;
                    let fsz =
                        u32::from_le_bytes([ino_buf[28], ino_buf[29], ino_buf[30], ino_buf[31]])
                            as usize;
                    (bs, fi, fo, fsz, 32usize)
                } else {
                    if ino_buf.len() < 56 {
                        continue;
                    }
                    let bs = u64::from_le_bytes([
                        ino_buf[16],
                        ino_buf[17],
                        ino_buf[18],
                        ino_buf[19],
                        ino_buf[20],
                        ino_buf[21],
                        ino_buf[22],
                        ino_buf[23],
                    ]) as usize;
                    let fsz = u64::from_le_bytes([
                        ino_buf[24],
                        ino_buf[25],
                        ino_buf[26],
                        ino_buf[27],
                        ino_buf[28],
                        ino_buf[29],
                        ino_buf[30],
                        ino_buf[31],
                    ]) as usize;
                    let fi =
                        u32::from_le_bytes([ino_buf[44], ino_buf[45], ino_buf[46], ino_buf[47]]);
                    let fo =
                        u32::from_le_bytes([ino_buf[48], ino_buf[49], ino_buf[50], ino_buf[51]])
                            as usize;
                    (bs, fi, fo, fsz, 56usize)
                };

                let num_full_blocks = if frag_idx == u32::MAX {
                    file_size.div_ceil(block_size.max(1))
                } else {
                    file_size / block_size.max(1)
                };
                let full_ino_buf = read_squashfs_meta_stream(
                    raw,
                    inode_table_start,
                    block_rel,
                    inner_off,
                    hdr_len + num_full_blocks * 4,
                )?;
                let mut file_data = Vec::with_capacity(file_size);
                let mut cur_data_off = blocks_start;
                for b in 0..num_full_blocks {
                    let sz_off = hdr_len + b * 4;
                    if sz_off + 4 > full_ino_buf.len() {
                        break;
                    }
                    let b_sz = u32::from_le_bytes([
                        full_ino_buf[sz_off],
                        full_ino_buf[sz_off + 1],
                        full_ino_buf[sz_off + 2],
                        full_ino_buf[sz_off + 3],
                    ]);
                    let dec = decompress_squashfs_data_block(raw, cur_data_off, b_sz, block_size)?;
                    let take = dec.len().min(file_size.saturating_sub(file_data.len()));
                    file_data.extend_from_slice(&dec[..take]);
                    cur_data_off += (b_sz & 0x00FF_FFFF) as usize;
                }

                if frag_idx != u32::MAX && file_data.len() < file_size {
                    if let Some(&(frag_start, frag_sz)) = fragments.get(frag_idx as usize) {
                        let frag_bytes = decompress_squashfs_data_block(
                            raw,
                            frag_start as usize,
                            frag_sz,
                            block_size,
                        )?;
                        let rem = file_size - file_data.len();
                        if frag_off + rem <= frag_bytes.len() {
                            file_data.extend_from_slice(&frag_bytes[frag_off..frag_off + rem]);
                        }
                    }
                }
                tree.insert_file_with_owner(&prefix, file_data, mode, uid, gid);
            }
            // Basic Symlink (3) or Extended Symlink (10)
            3 | 10 => {
                if ino_buf.len() < 24 {
                    continue;
                }
                let target_len =
                    u32::from_le_bytes([ino_buf[20], ino_buf[21], ino_buf[22], ino_buf[23]])
                        as usize;
                let full_ino_buf = read_squashfs_meta_stream(
                    raw,
                    inode_table_start,
                    block_rel,
                    inner_off,
                    24 + target_len,
                )?;
                if 24 + target_len <= full_ino_buf.len() {
                    let target =
                        String::from_utf8_lossy(&full_ino_buf[24..24 + target_len]).to_string();
                    tree.insert_symlink_with_owner(&prefix, target, uid, gid);
                }
            }
            _ => {}
        }
    }

    Ok(tree)
}
