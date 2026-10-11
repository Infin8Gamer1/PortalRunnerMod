//! GLaDOS's lines: Portal's choreographed scenes (`scenes/scenes.image`, compiled `.vcd`s) and
//! the soundscripts that name their sounds.
//!
//! The image (`VSIF`, version 2): a header, a string pool, then an entry per scene, found by
//! the CRC32 of its file name, its data LZMA-compressed (Valve's `LZMA` header) binary VCD
//! (`bvcd`). In that, a speak event is its type (5), its name and its start and end times
//! (floats), then its sound: a soundscript name, by index into the pool. A fire-trigger event
//! (type 9) is laid out the same, its parameter the number of the scene entity's output it
//! fires (`OnTrigger1`...). Those are read here as they stand, in order; the rest of the scene
//! (faces, looks) isn't wanted.

use std::collections::HashMap;

/// A scene's lines: (start, soundscript), its length (seconds), and the triggers it fires
/// (start, n: the scene entity's `OnTriggerN`).
pub struct Scene {
    pub lines: Vec<(f32, String)>,
    pub length: f32,
    pub triggers: Vec<(f32, u32)>,
}

pub struct Image<'a> {
    data: &'a [u8],
    strings: Vec<usize>,
    entries: HashMap<u32, (usize, usize)>,
}

fn u32_at(b: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes(b.get(at..at + 4)?.try_into().ok()?))
}

impl<'a> Image<'a> {
    pub fn parse(data: &'a [u8]) -> Option<Image<'a>> {
        if data.get(..4)? != b"VSIF" {
            return None;
        }
        let (scenes, strings, entries_at) = (u32_at(data, 8)? as usize, u32_at(data, 12)? as usize, u32_at(data, 16)? as usize);
        let strings = (0..strings).map(|k| u32_at(data, 20 + k * 4).map(|o| o as usize)).collect::<Option<Vec<_>>>()?;
        let mut entries = HashMap::new();
        for k in 0..scenes {
            let a = entries_at + 16 * k;
            entries.insert(u32_at(data, a)?, (u32_at(data, a + 4)? as usize, u32_at(data, a + 8)? as usize));
        }
        Some(Image { data, strings, entries })
    }

    fn string(&self, i: usize) -> Option<&str> {
        let at = *self.strings.get(i)?;
        let end = self.data.get(at..)?.iter().position(|&c| c == 0)? + at;
        std::str::from_utf8(&self.data[at..end]).ok()
    }

    /// A scene's lines, by its file (`scenes/test_chamber/ai_00_a_entry.vcd`).
    pub fn scene(&self, file: &str) -> Option<Scene> {
        let file = file.to_ascii_lowercase().replace('/', "\\");
        let (ofs, len) = [file.clone(), file.replace('\\', "/")].iter().find_map(|n| self.entries.get(&crc32(n.as_bytes())).copied())?;
        let raw = self.data.get(ofs..ofs + len)?;
        let b = if raw.get(..4)? == b"LZMA" { unlzma(raw)? } else { raw.to_vec() };
        if b.get(..4)? != b"bvcd" {
            return None;
        }
        let mut lines = Vec::new();
        let mut triggers = Vec::new();
        let mut length = 0.0f32;
        for i in 11..b.len().saturating_sub(2) {
            let f = |at: usize| f32::from_le_bytes(b[at..at + 4].try_into().unwrap());
            if b[i - 11] == 9 {
                let (start, name, param) = (f(i - 8), u16::from_le_bytes([b[i - 10], b[i - 9]]) as usize, u16::from_le_bytes([b[i], b[i + 1]]) as usize);
                if (0.0..600.0).contains(&start) && self.string(name).is_some() {
                    if let Some(n) = self.string(param).and_then(|p| p.trim().parse::<u32>().ok()).filter(|n| (1..=16).contains(n)) {
                        triggers.push((start, n));
                        length = length.max(start);
                    }
                }
                continue;
            }
            if b[i - 11] != 5 {
                continue;
            }
            let (start, end) = (f(i - 8), f(i - 4));
            if !(start >= 0.0 && end > start && end < 600.0) {
                continue;
            }
            let name = u16::from_le_bytes([b[i - 10], b[i - 9]]) as usize;
            let sound = u16::from_le_bytes([b[i], b[i + 1]]) as usize;
            let (Some(_), Some(sound)) = (self.string(name), self.string(sound)) else { continue };
            if sound.contains('.') && !sound.contains(' ') {
                lines.push((start, sound.to_string()));
                length = length.max(end);
            }
        }
        Some(Scene { lines, length, triggers })
    }
}

/// Valve's LZMA: `LZMA`, the unpacked size, the packed size, the 5 property bytes, the stream.
fn unlzma(b: &[u8]) -> Option<Vec<u8>> {
    let real = u32_at(b, 4)? as u64;
    let props = b.get(12..17)?;
    let mut stream = props.to_vec();
    stream.extend_from_slice(&real.to_le_bytes());
    stream.extend_from_slice(b.get(17..)?);
    let mut out = Vec::new();
    lzma_rs::lzma_decompress(&mut std::io::Cursor::new(stream), &mut out).ok()?;
    Some(out)
}

fn crc32(b: &[u8]) -> u32 {
    let mut c = 0xffff_ffffu32;
    for &x in b {
        c ^= x as u32;
        for _ in 0..8 {
            c = if c & 1 != 0 { (c >> 1) ^ 0xedb8_8320 } else { c >> 1 };
        }
    }
    !c
}

/// Soundscripts' sounds: name (lower case) to its wave file (the first, if it picks one at
/// random), under `sound/`.
pub fn soundscripts(text: &str) -> HashMap<String, String> {
    let mut out = HashMap::new();
    let mut tokens = Vec::new();
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '/' if chars.peek() == Some(&'/') => {
                for c in chars.by_ref() {
                    if c == '\n' {
                        break;
                    }
                }
            }
            '"' => tokens.push(chars.by_ref().take_while(|&c| c != '"').collect::<String>()),
            '{' => tokens.push("{".into()),
            '}' => tokens.push("}".into()),
            c if c.is_whitespace() => {}
            c => {
                let mut t = c.to_string();
                while let Some(&n) = chars.peek() {
                    if n.is_whitespace() || n == '"' || n == '{' || n == '}' {
                        break;
                    }
                    t.push(n);
                    chars.next();
                }
                tokens.push(t);
            }
        }
    }
    let mut depth = 0;
    let mut current: Option<String> = None;
    for (k, t) in tokens.iter().enumerate() {
        match t.as_str() {
            "{" => {
                if depth == 0 {
                    current = k.checked_sub(1).map(|p| tokens[p].to_ascii_lowercase());
                }
                depth += 1;
            }
            "}" => depth -= 1,
            w if (w.eq_ignore_ascii_case("wave") || w.eq_ignore_ascii_case("rndwave")) && depth >= 1 => {
                let wave = if w.eq_ignore_ascii_case("wave") { tokens.get(k + 1) } else { tokens.get(k + 2).filter(|_| tokens.get(k + 1).map(String::as_str) == Some("{")).and_then(|_| tokens.get(k + 3)) };
                if let (Some(name), Some(wave)) = (&current, wave) {
                    let w = wave.trim_start_matches(['*', '#', '@', ')', '(', '^', '<', '>', '!', '?', '&', '~', '$', '`', '+', '%']);
                    out.entry(name.clone()).or_insert_with(|| w.replace('\\', "/").to_ascii_lowercase());
                }
            }
            _ => {}
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn soundscripts_name_their_waves() {
        let s = soundscripts("// GLaDOS\n\"Portal.ding_on\"\n{\n \"channel\" \"CHAN_VOICE\"\n \"wave\" \"*vo/aperture_ai/ding_on.wav\"\n}\n\"Turret.Retire\" { \"rndwave\" { \"wave\" \"npc/turret_floor/turret_retire_1.wav\" } }");
        assert_eq!(s.get("portal.ding_on").map(String::as_str), Some("vo/aperture_ai/ding_on.wav"));
        assert_eq!(s.get("turret.retire").map(String::as_str), Some("npc/turret_floor/turret_retire_1.wav"));
    }

    #[test]
    fn crc_is_crc32() {
        assert_eq!(crc32(b"123456789"), 0xcbf4_3926);
    }
}
