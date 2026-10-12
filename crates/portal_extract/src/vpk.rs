//! Valve's VPK archives (versions 1 and 2): a directory file listing every entry, and
//! numbered archive files holding most of the data.

use std::collections::HashMap;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

struct Entry {
    archive: u16,
    offset: u32,
    length: u32,
    preload: Vec<u8>,
}

pub struct Vpk {
    dir: PathBuf,
    /// Where data stored in the directory file itself starts (archive index 0x7fff).
    data_start: u64,
    entries: HashMap<String, Entry>,
}

fn cstr(data: &[u8], pos: &mut usize) -> Result<String, String> {
    let start = *pos;
    while *pos < data.len() && data[*pos] != 0 {
        *pos += 1;
    }
    if *pos >= data.len() {
        return Err("truncated directory".into());
    }
    let s = String::from_utf8_lossy(&data[start..*pos]).into_owned();
    *pos += 1;
    Ok(s)
}

fn u16_at(d: &[u8], p: usize) -> u16 {
    u16::from_le_bytes([d[p], d[p + 1]])
}
fn u32_at(d: &[u8], p: usize) -> u32 {
    u32::from_le_bytes([d[p], d[p + 1], d[p + 2], d[p + 3]])
}

impl Vpk {
    pub fn open(path: &Path) -> Result<Vpk, String> {
        let data = std::fs::read(path).map_err(|e| e.to_string())?;
        if data.len() < 12 || u32_at(&data, 0) != 0x55aa1234 {
            return Err("not a VPK directory".into());
        }
        let version = u32_at(&data, 4);
        let tree = u32_at(&data, 8) as usize;
        let header = match version {
            1 => 12,
            2 => 28,
            v => return Err(format!("VPK version {v}")),
        };
        let mut pos = header;
        let end = header + tree;
        let mut entries = HashMap::new();
        while pos < end {
            let ext = cstr(&data, &mut pos)?;
            if ext.is_empty() {
                break;
            }
            loop {
                let dir = cstr(&data, &mut pos)?;
                if dir.is_empty() {
                    break;
                }
                loop {
                    let name = cstr(&data, &mut pos)?;
                    if name.is_empty() {
                        break;
                    }
                    let preload_len = u16_at(&data, pos + 4) as usize;
                    let archive = u16_at(&data, pos + 6);
                    let offset = u32_at(&data, pos + 8);
                    let length = u32_at(&data, pos + 12);
                    pos += 18;
                    let preload = data[pos..pos + preload_len].to_vec();
                    pos += preload_len;
                    let full = if dir == " " { format!("{name}.{ext}") } else { format!("{dir}/{name}.{ext}") };
                    entries.insert(full.to_ascii_lowercase(), Entry { archive, offset, length, preload });
                }
            }
        }
        Ok(Vpk { dir: path.to_path_buf(), data_start: end as u64, entries })
    }

    /// Every entry's path (lower-case, forward slashes).
    pub fn paths(&self) -> impl Iterator<Item = &str> {
        self.entries.keys().map(|k| k.as_str())
    }

    /// An entry's bytes (path lower-case, forward slashes).
    pub fn read(&self, path: &str) -> Option<Vec<u8>> {
        let e = self.entries.get(path)?;
        let mut out = e.preload.clone();
        if e.length > 0 {
            let (file, offset) = if e.archive == 0x7fff {
                (self.dir.clone(), self.data_start + e.offset as u64)
            } else {
                let name = self.dir.file_name()?.to_str()?;
                let stem = name.strip_suffix("_dir.vpk")?;
                (self.dir.with_file_name(format!("{stem}_{:03}.vpk", e.archive)), e.offset as u64)
            };
            let mut f = File::open(file).ok()?;
            f.seek(SeekFrom::Start(offset)).ok()?;
            let mut buf = vec![0u8; e.length as usize];
            f.read_exact(&mut buf).ok()?;
            out.extend_from_slice(&buf);
        }
        Some(out)
    }
}
