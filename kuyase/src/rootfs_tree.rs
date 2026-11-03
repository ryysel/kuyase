use crate::archive_formats::extract_payload_bytes_into_tree;
use flate2::read::GzDecoder;
use flate2::write::GzEncoder;
use flate2::Compression;
use protocol::detect_board_hint_from_filename;
use std::collections::BTreeMap;
use std::fs::{self, File};
use std::io::{Cursor, Read};
use std::path::Path;
use tar::{Archive, Builder, EntryType, Header};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RootfsNodeKind {
    Dir,
    File(Vec<u8>),
    Symlink(String),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RootfsNode {
    pub kind: RootfsNodeKind,
    pub mode: u16,
    pub uid: u16,
    pub gid: u16,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RootfsTree {
    pub entries: BTreeMap<String, RootfsNode>,
}

pub fn normalize_rel_path(raw: &str) -> Option<String> {
    let replaced = raw.replace('\\', "/");
    let mut parts = Vec::new();
    for seg in replaced.split('/') {
        match seg {
            "" | "." => continue,
            ".." => {
                parts.pop();
            }
            other => parts.push(other),
        }
    }
    if parts.is_empty() {
        None
    } else {
        Some(parts.join("/"))
    }
}

impl RootfsTree {
    pub fn insert_dir(&mut self, path: &str, mode: u16) {
        self.insert_dir_with_owner(path, mode, 0, 0);
    }

    pub fn insert_dir_with_owner(&mut self, path: &str, mode: u16, uid: u16, gid: u16) {
        if let Some(norm) = normalize_rel_path(path) {
            self.entries.insert(
                norm,
                RootfsNode {
                    kind: RootfsNodeKind::Dir,
                    mode: if mode == 0 { 0o755 } else { mode & 0o7777 },
                    uid,
                    gid,
                },
            );
        }
    }

    pub fn insert_file(&mut self, path: &str, data: Vec<u8>, mode: u16) {
        self.insert_file_with_owner(path, data, mode, 0, 0);
    }

    pub fn insert_file_with_owner(
        &mut self,
        path: &str,
        data: Vec<u8>,
        mode: u16,
        uid: u16,
        gid: u16,
    ) {
        if let Some(norm) = normalize_rel_path(path) {
            self.entries.insert(
                norm,
                RootfsNode {
                    kind: RootfsNodeKind::File(data),
                    mode: if mode == 0 { 0o644 } else { mode & 0o7777 },
                    uid,
                    gid,
                },
            );
        }
    }

    pub fn insert_symlink(&mut self, path: &str, target: String) {
        self.insert_symlink_with_owner(path, target, 0, 0);
    }

    pub fn insert_symlink_with_owner(&mut self, path: &str, target: String, uid: u16, gid: u16) {
        if let Some(norm) = normalize_rel_path(path) {
            self.entries.insert(
                norm,
                RootfsNode {
                    kind: RootfsNodeKind::Symlink(target),
                    mode: 0o777,
                    uid,
                    gid,
                },
            );
        }
    }

    pub fn get(&self, path: &str) -> Option<&RootfsNode> {
        let norm = normalize_rel_path(path)?;
        self.entries.get(&norm)
    }

    pub fn get_file_bytes(&self, path: &str) -> Option<&[u8]> {
        match &self.get(path)?.kind {
            RootfsNodeKind::File(b) => Some(b.as_slice()),
            _ => None,
        }
    }
}

pub fn read_tar_from_reader<R: Read>(reader: R) -> Result<RootfsTree, String> {
    let mut archive = Archive::new(reader);
    let mut tree = RootfsTree::default();
    let entries = archive
        .entries()
        .map_err(|e| format!("tar read error: {}", e))?;
    for entry_res in entries {
        let mut entry = entry_res.map_err(|e| format!("tar entry error: {}", e))?;
        let raw_path = entry
            .path()
            .map_err(|e| format!("tar path error: {}", e))?
            .to_string_lossy()
            .to_string();
        let Some(norm) = normalize_rel_path(&raw_path) else {
            continue;
        };
        let mode = (entry.header().mode().unwrap_or(0o644) as u16) & 0o7777;
        let uid = (entry.header().uid().unwrap_or(0) & 0xFFFF) as u16;
        let gid = (entry.header().gid().unwrap_or(0) & 0xFFFF) as u16;
        match entry.header().entry_type() {
            EntryType::Directory => {
                tree.insert_dir_with_owner(&norm, if mode == 0 { 0o755 } else { mode }, uid, gid)
            }
            EntryType::Symlink => {
                if let Ok(Some(link)) = entry.link_name() {
                    tree.insert_symlink_with_owner(
                        &norm,
                        link.to_string_lossy().to_string(),
                        uid,
                        gid,
                    );
                }
            }
            EntryType::Regular | EntryType::Continuous => {
                let mut buf = Vec::new();
                entry
                    .read_to_end(&mut buf)
                    .map_err(|e| format!("tar file read error: {}", e))?;
                tree.insert_file_with_owner(
                    &norm,
                    buf,
                    if mode == 0 { 0o644 } else { mode },
                    uid,
                    gid,
                );
            }
            _ => {}
        }
    }

    if tree.entries.len() == 1 {
        let (only_name, only_node) = tree.entries.iter().next().unwrap();
        if let RootfsNodeKind::File(ref bytes) = only_node.kind {
            if bytes.len() >= 2048 {
                let inner_fname = Path::new(only_name)
                    .file_name()
                    .and_then(|s| s.to_str())
                    .unwrap_or(only_name.as_str());
                let board_hint = detect_board_hint_from_filename(inner_fname);
                if let Ok(mut inner_tree) = extract_payload_bytes_into_tree(bytes) {
                    if !inner_tree.entries.is_empty() {
                        if let Some(slug) = board_hint {
                            if inner_tree.get("etc/quasar-board").is_none() {
                                inner_tree.insert_file(
                                    "etc/quasar-board",
                                    format!("{}\n", slug).into_bytes(),
                                    0o644,
                                );
                            }
                        }
                        return Ok(inner_tree);
                    }
                }
            }
        }
    }

    Ok(tree)
}

pub fn read_tar_gz_into_tree(path: &Path) -> Result<RootfsTree, String> {
    let file =
        File::open(path).map_err(|e| format!("failed to open '{}': {}", path.display(), e))?;
    let gz = GzDecoder::new(file);
    read_tar_from_reader(gz)
}

pub fn write_tree_to_tar_gz(tree: &RootfsTree, path: &Path) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    let file =
        File::create(path).map_err(|e| format!("failed to create '{}': {}", path.display(), e))?;
    let gz = GzEncoder::new(file, Compression::fast());
    let mut builder = Builder::new(gz);

    for (rel, node) in &tree.entries {
        let mut header = Header::new_gnu();
        header.set_uid(node.uid as u64);
        header.set_gid(node.gid as u64);
        header.set_mtime(0);
        match &node.kind {
            RootfsNodeKind::Dir => {
                header.set_entry_type(EntryType::Directory);
                header.set_mode(node.mode as u32);
                header.set_size(0);
                header.set_cksum();
                builder
                    .append_data(&mut header, format!("{}/", rel), std::io::empty())
                    .map_err(|e| e.to_string())?;
            }
            RootfsNodeKind::File(bytes) => {
                header.set_entry_type(EntryType::Regular);
                header.set_mode(node.mode as u32);
                header.set_size(bytes.len() as u64);
                header.set_cksum();
                builder
                    .append_data(&mut header, rel, Cursor::new(bytes))
                    .map_err(|e| e.to_string())?;
            }
            RootfsNodeKind::Symlink(target) => {
                header.set_entry_type(EntryType::Symlink);
                header.set_mode(0o777);
                header.set_size(0);
                builder
                    .append_link(&mut header, rel, target)
                    .map_err(|e| e.to_string())?;
            }
        }
    }
    let gz = builder.into_inner().map_err(|e| e.to_string())?;
    gz.finish().map_err(|e| e.to_string())?;
    Ok(())
}
