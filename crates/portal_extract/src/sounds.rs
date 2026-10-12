//! Portal's sounds as a library (`sounds/lib/`, by their path under `sound/`): every one of
//! Portal's own, and those of Half-Life 2's that Portal's soundscripts, soundscapes and maps
//! use. With them, what the game needs to play them as Source does:
//!
//! - `sounds/scripts.txt`: the soundscripts (`Portal.elevator_chime`: which files, how loud,
//!   at what pitch, how far they carry), a line each: `name|volume|pitch lo,hi|soundlevel dB|
//!   wave;wave...` (waves by their library path).
//! - `sounds/soundscapes.txt`: the soundscapes (an area's ambience), a block each: `scape
//!   <name>`, then `loop <volume> <pitch> <wave>`, `random <every lo> <hi> <volume lo> <hi>
//!   <pitch lo> <hi> <wave;wave...>` and `sub <volume> <soundscape>` lines.
//! - `sounds/lib/index.txt`: each file, a line: `path|seconds|loops|rms dB` (`loops`: 1 if the
//!   file marks a loop, as Source's looping sounds do).
//!
//! Source's MS ADPCM waves are written as 16-bit PCM (what the game's decoder reads).

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

/// KeyValues: `"key" "value"` pairs and `"key" { ... }` blocks.
#[derive(Debug, Clone)]
pub enum Kv {
    Value(String),
    Block(Vec<(String, Kv)>),
}

impl Kv {
    pub fn get(&self, key: &str) -> Option<&Kv> {
        match self {
            Kv::Block(v) => v.iter().find(|(k, _)| k.eq_ignore_ascii_case(key)).map(|(_, v)| v),
            Kv::Value(_) => None,
        }
    }

    pub fn str(&self, key: &str) -> Option<&str> {
        match self.get(key)? {
            Kv::Value(s) => Some(s),
            Kv::Block(_) => None,
        }
    }

    pub fn entries(&self) -> &[(String, Kv)] {
        match self {
            Kv::Block(v) => v,
            Kv::Value(_) => &[],
        }
    }
}

/// Read KeyValues text (comments, `[$conditions]` and `#include`s ignored).
pub fn parse_kv(text: &str) -> Vec<(String, Kv)> {
    let mut toks: Vec<String> = Vec::new();
    let mut chars = text.chars().peekable();
    while let Some(&c) = chars.peek() {
        if c.is_whitespace() {
            chars.next();
        } else if c == '/' {
            chars.next();
            if chars.peek() == Some(&'/') {
                for c in chars.by_ref() {
                    if c == '\n' {
                        break;
                    }
                }
            }
        } else if c == '"' {
            chars.next();
            let mut s = String::new();
            for c in chars.by_ref() {
                if c == '"' {
                    break;
                }
                s.push(c);
            }
            toks.push(s);
        } else if c == '{' || c == '}' {
            chars.next();
            toks.push(c.to_string());
        } else if c == '[' {
            // A platform condition: dropped.
            for c in chars.by_ref() {
                if c == ']' {
                    break;
                }
            }
        } else {
            let mut s = String::new();
            while let Some(&c) = chars.peek() {
                if c.is_whitespace() || c == '"' || c == '{' || c == '}' {
                    break;
                }
                s.push(c);
                chars.next();
            }
            toks.push(s);
        }
    }
    fn block(toks: &[String], at: &mut usize) -> Vec<(String, Kv)> {
        let mut out = Vec::new();
        while *at < toks.len() {
            let k = &toks[*at];
            if k == "}" {
                *at += 1;
                break;
            }
            *at += 1;
            match toks.get(*at).map(String::as_str) {
                Some("{") => {
                    *at += 1;
                    out.push((k.clone(), Kv::Block(block(toks, at))));
                }
                Some(v) => {
                    out.push((k.clone(), Kv::Value(v.to_string())));
                    *at += 1;
                }
                None => break,
            }
        }
        out
    }
    let mut at = 0;
    block(&toks, &mut at)
}

/// A wave's path under `sound/` as a soundscript or map names it (without Source's prefix
/// characters: `)`, `^`, `*`, `#`...), lower case, forward slashes.
pub fn wave_path(w: &str) -> String {
    w.trim().trim_start_matches(|c: char| !c.is_ascii_alphanumeric() && c != '_').replace('\\', "/").to_ascii_lowercase()
}

/// A soundlevel (`SNDLVL_75dB`, `SNDLVL_NORM`, `ATTN_STATIC`, a number) in dB.
fn soundlevel(s: &str) -> f32 {
    let s = s.trim().to_ascii_uppercase();
    match s.as_str() {
        "SNDLVL_NONE" => 0.0,
        "SNDLVL_NORM" | "ATTN_NORM" => 75.0,
        "SNDLVL_IDLE" | "ATTN_IDLE" => 60.0,
        "SNDLVL_STATIC" | "ATTN_STATIC" => 66.0,
        "SNDLVL_TALKING" => 80.0,
        "SNDLVL_GUNFIRE" => 140.0,
        _ => s.trim_start_matches("SNDLVL_").trim_end_matches("DB").parse().unwrap_or(75.0),
    }
}

/// A range (`0.5,0.7`) or one value, with Source's names (`VOL_NORM`, `PITCH_NORM`...).
fn range(s: &str, norm: f32) -> (f32, f32) {
    let one = |v: &str| match v.trim().to_ascii_uppercase().as_str() {
        "VOL_NORM" => 1.0,
        "PITCH_NORM" => 100.0,
        "PITCH_LOW" => 95.0,
        "PITCH_HIGH" => 120.0,
        x => x.parse().unwrap_or(norm),
    };
    match s.split_once(',') {
        Some((a, b)) => (one(a), one(b)),
        None => (one(s), one(s)),
    }
}

/// A soundscript.
pub struct Script {
    pub name: String,
    pub volume: f32,
    pub pitch: (f32, f32),
    pub soundlevel: f32,
    pub waves: Vec<String>,
}

/// Every soundscript the game's manifest lists.
pub fn soundscripts(read: &dyn Fn(&str) -> Option<Vec<u8>>) -> Vec<Script> {
    let Some(manifest) = read("scripts/game_sounds_manifest.txt") else { return Vec::new() };
    let manifest = parse_kv(&String::from_utf8_lossy(&manifest));
    let mut out: BTreeMap<String, Script> = BTreeMap::new();
    for (_, m) in &manifest {
        for (k, v) in m.entries() {
            let (true, Kv::Value(file)) = (k.to_ascii_lowercase().ends_with("_file"), v) else { continue };
            let Some(text) = read(file) else { continue };
            for (name, s) in parse_kv(&String::from_utf8_lossy(&text)) {
                let mut waves: Vec<String> = s.str("wave").map(wave_path).into_iter().collect();
                if let Some(rnd) = s.get("rndwave") {
                    waves.extend(rnd.entries().iter().filter_map(|(_, w)| if let Kv::Value(w) = w { Some(wave_path(w)) } else { None }));
                }
                if waves.is_empty() {
                    continue;
                }
                let vol = range(s.str("volume").unwrap_or("1"), 1.0);
                out.entry(name.to_ascii_lowercase()).or_insert(Script {
                    name: name.clone(),
                    volume: (vol.0 + vol.1) * 0.5,
                    pitch: range(s.str("pitch").unwrap_or("100"), 100.0),
                    soundlevel: s.str("soundlevel").or_else(|| s.str("attenuation")).map(soundlevel).unwrap_or(75.0),
                    waves,
                });
            }
        }
    }
    out.into_values().collect()
}

/// Every soundscape the soundscape manifest lists, as `sounds/soundscapes.txt` has them, and
/// the waves they use.
pub fn soundscapes(read: &dyn Fn(&str) -> Option<Vec<u8>>) -> (String, Vec<String>) {
    let Some(manifest) = read("scripts/soundscapes_manifest.txt") else { return (String::new(), Vec::new()) };
    let mut text = String::new();
    let mut waves = Vec::new();
    for (_, m) in parse_kv(&String::from_utf8_lossy(&manifest)) {
        for (_, v) in m.entries() {
            let Kv::Value(file) = v else { continue };
            let Some(body) = read(file) else { continue };
            for (name, scape) in parse_kv(&String::from_utf8_lossy(&body)) {
                text.push_str(&format!("scape {}\n", name.to_ascii_lowercase()));
                for (k, rule) in scape.entries() {
                    let f = |key: &str, norm: f32| range(rule.str(key).unwrap_or(""), norm);
                    match k.to_ascii_lowercase().as_str() {
                        "playlooping" => {
                            let Some(w) = rule.str("wave").map(wave_path) else { continue };
                            text.push_str(&format!("loop {} {} {w}\n", f("volume", 1.0).0, f("pitch", 100.0).0));
                            waves.push(w);
                        }
                        "playrandom" => {
                            let list: Vec<String> = rule.get("rndwave").map(|r| r.entries().iter().filter_map(|(_, w)| if let Kv::Value(w) = w { Some(wave_path(w)) } else { None }).collect()).unwrap_or_default();
                            if list.is_empty() {
                                continue;
                            }
                            let (t, v, p) = (f("time", 10.0), f("volume", 1.0), f("pitch", 100.0));
                            text.push_str(&format!("random {} {} {} {} {} {} {}\n", t.0, t.1, v.0, v.1, p.0, p.1, list.join(";")));
                            waves.extend(list);
                        }
                        "playsoundscape" => {
                            if let Some(n) = rule.str("name") {
                                text.push_str(&format!("sub {} {}\n", f("volume", 1.0).0, n.to_ascii_lowercase()));
                            }
                        }
                        _ => {}
                    }
                }
            }
        }
    }
    (text, waves)
}

/// MS ADPCM's adaptation table and its coefficients.
const ADAPT: [i32; 16] = [230, 230, 230, 230, 307, 409, 512, 614, 768, 614, 512, 409, 307, 230, 230, 230];

/// A WAVE file as 16-bit PCM, if it's MS ADPCM (else as it is).
pub fn to_pcm(bytes: &[u8]) -> Vec<u8> {
    let Some((fmt, data)) = wav_chunks(bytes) else { return bytes.to_vec() };
    if u16::from_le_bytes([fmt[0], fmt[1]]) != 2 || fmt.len() < 20 {
        return bytes.to_vec();
    }
    let ch = u16::from_le_bytes([fmt[2], fmt[3]]).clamp(1, 2) as usize;
    let rate = u32::from_le_bytes([fmt[4], fmt[5], fmt[6], fmt[7]]);
    let block = u16::from_le_bytes([fmt[12], fmt[13]]) as usize;
    let ncoef = fmt.get(20..22).map_or(7, |c| u16::from_le_bytes([c[0], c[1]]) as usize);
    let coefs: Vec<(i32, i32)> = (0..ncoef).filter_map(|k| {
        let a = fmt.get(22 + k * 4..26 + k * 4)?;
        Some((i16::from_le_bytes([a[0], a[1]]) as i32, i16::from_le_bytes([a[2], a[3]]) as i32))
    }).collect();
    if block < 7 * ch || coefs.is_empty() {
        return bytes.to_vec();
    }
    let mut out: Vec<i16> = Vec::new();
    for b in data.chunks(block) {
        if b.len() < 7 * ch {
            break;
        }
        // The block's header: each channel's predictor, delta, and two first samples.
        let mut pred = [0usize; 2];
        let mut delta = [0i32; 2];
        let mut s1 = [0i32; 2];
        let mut s2 = [0i32; 2];
        let rd = |o: usize| i16::from_le_bytes([b[o], b[o + 1]]) as i32;
        for c in 0..ch {
            pred[c] = (b[c] as usize).min(coefs.len() - 1);
            delta[c] = rd(ch + c * 2);
            s1[c] = rd(3 * ch + c * 2);
            s2[c] = rd(5 * ch + c * 2);
        }
        for c in 0..ch {
            out.push(s2[c] as i16);
        }
        for c in 0..ch {
            out.push(s1[c] as i16);
        }
        let mut c = 0;
        for &byte in &b[7 * ch..] {
            for nib in [byte >> 4, byte & 15] {
                let signed = if nib & 8 != 0 { nib as i32 - 16 } else { nib as i32 };
                let (a1, a2) = coefs[pred[c]];
                let p = ((s1[c] * a1 + s2[c] * a2) >> 8) + signed * delta[c];
                let p = p.clamp(-32768, 32767);
                out.push(p as i16);
                s2[c] = s1[c];
                s1[c] = p;
                delta[c] = ((ADAPT[nib as usize] * delta[c]) >> 8).max(16);
                c = (c + 1) % ch;
            }
        }
    }
    let mut w = Vec::with_capacity(44 + out.len() * 2);
    let data_len = (out.len() * 2) as u32;
    w.extend_from_slice(b"RIFF");
    w.extend_from_slice(&(36 + data_len).to_le_bytes());
    w.extend_from_slice(b"WAVEfmt ");
    w.extend_from_slice(&16u32.to_le_bytes());
    w.extend_from_slice(&1u16.to_le_bytes());
    w.extend_from_slice(&(ch as u16).to_le_bytes());
    w.extend_from_slice(&rate.to_le_bytes());
    w.extend_from_slice(&(rate * ch as u32 * 2).to_le_bytes());
    w.extend_from_slice(&(ch as u16 * 2).to_le_bytes());
    w.extend_from_slice(&16u16.to_le_bytes());
    w.extend_from_slice(b"data");
    w.extend_from_slice(&data_len.to_le_bytes());
    for s in out {
        w.extend_from_slice(&s.to_le_bytes());
    }
    // (The loop marker goes along: it says the sound loops.)
    if looped(bytes) {
        w.extend_from_slice(b"cue ");
        w.extend_from_slice(&4u32.to_le_bytes());
        w.extend_from_slice(&0u32.to_le_bytes());
        let len = w.len() as u32 - 8;
        w[4..8].copy_from_slice(&len.to_le_bytes());
    }
    w
}

/// A WAVE file's `fmt ` and `data` chunks.
fn wav_chunks(bytes: &[u8]) -> Option<(&[u8], &[u8])> {
    if bytes.len() < 12 || &bytes[0..4] != b"RIFF" || &bytes[8..12] != b"WAVE" {
        return None;
    }
    let (mut fmt, mut data) = (None, None);
    let mut at = 12;
    while at + 8 <= bytes.len() {
        let id = &bytes[at..at + 4];
        let len = u32::from_le_bytes(bytes[at + 4..at + 8].try_into().ok()?) as usize;
        let body = &bytes[at + 8..(at + 8 + len).min(bytes.len())];
        match id {
            b"fmt " => fmt = Some(body),
            b"data" => data = Some(body),
            _ => {}
        }
        at += 8 + len + (len & 1);
    }
    Some((fmt?, data?))
}

/// Whether a WAVE file marks a loop (a `cue ` chunk, or a sampler chunk with loops), as
/// Source's looping sounds do.
pub fn looped(bytes: &[u8]) -> bool {
    let mut at = 12;
    while at + 8 <= bytes.len() {
        let id = &bytes[at..at + 4];
        let len = u32::from_le_bytes(bytes[at + 4..at + 8].try_into().unwrap_or([0; 4])) as usize;
        if id == b"cue " {
            return true;
        }
        if id == b"smpl" && bytes.get(at + 8 + 28..at + 8 + 32).is_some_and(|n| u32::from_le_bytes(n.try_into().unwrap()) > 0) {
            return true;
        }
        at += 8 + len + (len & 1);
    }
    false
}

/// A 16-bit PCM wave's average (RMS) level, in dB below full scale.
fn rms_db(bytes: &[u8]) -> Option<f32> {
    let (fmt, data) = wav_chunks(bytes)?;
    if fmt.len() < 16 || u16::from_le_bytes([fmt[0], fmt[1]]) != 1 || u16::from_le_bytes([fmt[14], fmt[15]]) != 16 || data.len() < 2 {
        return None;
    }
    let n = data.len() / 2;
    let sum: f64 = data.chunks_exact(2).map(|c| (i16::from_le_bytes([c[0], c[1]]) as f64 / 32768.0).powi(2)).sum();
    Some(20.0 * (sum / n as f64).sqrt().max(1e-6).log10() as f32)
}

/// How long a PCM wave is (seconds).
fn seconds(bytes: &[u8]) -> f32 {
    let Some((fmt, data)) = wav_chunks(bytes) else { return 0.0 };
    if fmt.len() < 16 {
        return 0.0;
    }
    let per_second = u32::from_le_bytes([fmt[8], fmt[9], fmt[10], fmt[11]]).max(1);
    data.len() as f32 / per_second as f32
}

/// Write the library: Portal's own sounds (`portal_sounds`: their paths under `sound/`) and
/// every other one named in `wanted`, the tables beside it. Returns (files written, kept).
pub fn library(read: &dyn Fn(&str) -> Option<Vec<u8>>, portal_sounds: &[String], wanted: &BTreeSet<String>, out: &Path, again: bool) -> (usize, usize) {
    let dir = out.join("sounds").join("lib");
    let mut paths: BTreeSet<String> = portal_sounds.iter().cloned().collect();
    paths.extend(wanted.iter().cloned());
    let (mut written, mut kept) = (0, 0);
    let mut index = String::new();
    for p in &paths {
        let file = dir.join(p);
        let bytes = match std::fs::read(&file) {
            Ok(b) if !again && !b.is_empty() => {
                kept += 1;
                b
            }
            _ => {
                let Some(raw) = read(&format!("sound/{p}")) else { continue };
                let b = if p.ends_with(".wav") { to_pcm(&raw) } else { raw };
                if std::fs::create_dir_all(file.parent().unwrap()).is_err() || std::fs::write(&file, &b).is_err() {
                    continue;
                }
                written += 1;
                b
            }
        };
        let (secs, loops, db) = if p.ends_with(".wav") { (seconds(&bytes), looped(&bytes), rms_db(&bytes).unwrap_or(-20.0)) } else { (0.0, false, -20.0) };
        index.push_str(&format!("{p}|{secs:.2}|{}|{db:.1}\n", u8::from(loops)));
    }
    let _ = std::fs::write(dir.join("index.txt"), index);
    (written, kept)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keyvalues_and_soundscripts_read() {
        let kv = parse_kv("// a comment\n\"Portal.chime\"\n{\n\t\"volume\" \"0.5,0.7\"\n\t\"soundlevel\" \"SNDLVL_90dB\" [$X360]\n\trndwave { \"wave\" \")plats/bell1.wav\" \"wave\" \"^Plats\\Bell2.wav\" }\n}\n");
        assert_eq!(kv.len(), 1);
        let s = &kv[0].1;
        assert_eq!(s.str("volume"), Some("0.5,0.7"));
        assert_eq!(soundlevel(s.str("soundlevel").unwrap()), 90.0);
        let waves: Vec<String> = s.get("rndwave").unwrap().entries().iter().map(|(_, w)| if let Kv::Value(w) = w { wave_path(w) } else { String::new() }).collect();
        assert_eq!(waves, vec!["plats/bell1.wav", "plats/bell2.wav"]);
        assert_eq!(range("PITCH_NORM", 100.0), (100.0, 100.0));
    }

    #[test]
    fn adpcm_becomes_pcm() {
        // A mono MS ADPCM block: header (predictor 0, delta 16, samples 100 and 50), then
        // zero nibbles (the prediction carries on).
        let mut fmt = vec![2, 0, 1, 0];
        fmt.extend(22050u32.to_le_bytes());
        fmt.extend(11025u32.to_le_bytes());
        fmt.extend(16u16.to_le_bytes());
        fmt.extend(4u16.to_le_bytes());
        fmt.extend(2u16.to_le_bytes());
        fmt.extend(4u16.to_le_bytes());
        fmt.extend(7u16.to_le_bytes());
        for (a, b) in [(256, 0), (512, -256), (0, 0), (192, 64), (240, 0), (460, -208), (392, -232)] {
            fmt.extend((a as i16).to_le_bytes());
            fmt.extend((b as i16).to_le_bytes());
        }
        let mut data = vec![0u8, 16, 0, 100, 0, 50, 0];
        data.extend([0u8; 9]);
        let mut w = b"RIFF".to_vec();
        w.extend(((4 + 8 + fmt.len() + 8 + data.len()) as u32).to_le_bytes());
        w.extend(b"WAVEfmt ");
        w.extend((fmt.len() as u32).to_le_bytes());
        w.extend(&fmt);
        w.extend(b"data");
        w.extend((data.len() as u32).to_le_bytes());
        w.extend(&data);
        let pcm = to_pcm(&w);
        let (f, d) = wav_chunks(&pcm).unwrap();
        assert_eq!(u16::from_le_bytes([f[0], f[1]]), 1);
        let s: Vec<i16> = d.chunks_exact(2).map(|c| i16::from_le_bytes([c[0], c[1]])).collect();
        // The two header samples (older first), then predictor 0 (256, 0) keeps the last.
        assert_eq!(&s[..4], &[50, 100, 100, 100]);
        assert_eq!(s.len(), 2 + 18);
    }
}
