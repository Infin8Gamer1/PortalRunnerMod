//! Assets taken from your own copies of Portal and Portal 2 by `portal-extract`
//! (`cargo run --release -p portal_extract`): textures, sounds and models, under
//! `assets/extracted/`. Nothing from either game ships with this project; without the
//! extracted files everything falls back to procedural stand-ins.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use bevy::asset::RenderAssetUsages;
use bevy::audio::AudioSource;
use bevy::image::{CompressedImageFormats, ImageAddressMode, ImageSampler, ImageSamplerDescriptor, ImageType};
use bevy::prelude::*;

#[derive(Resource, Default)]
pub struct Extracted {
    pub root: Option<PathBuf>,
    /// Sounds by name (their files' stems), each with its variations.
    pub sounds: HashMap<String, Handle<AudioSource>>,
    /// Each sound's loudness correction: the games' files are all normalised to peak at full
    /// scale, so a hum and a click come out equally loud. This brings each one's average
    /// level down to [`TARGET_DB`] (never up).
    pub gains: HashMap<AssetId<AudioSource>, f32>,
}

/// The average (RMS) level every sound is brought down to, in dB below full scale.
pub const TARGET_DB: f32 = -20.0;

/// The RMS level of a 16-bit PCM WAV file in dBFS, if it is one.
pub fn wav_rms_db(bytes: &[u8]) -> Option<f32> {
    if bytes.len() < 12 || &bytes[0..4] != b"RIFF" || &bytes[8..12] != b"WAVE" {
        return None;
    }
    let mut at = 12;
    let mut bits = 0u16;
    while at + 8 <= bytes.len() {
        let id = &bytes[at..at + 4];
        let len = u32::from_le_bytes(bytes[at + 4..at + 8].try_into().ok()?) as usize;
        let body = bytes.get(at + 8..(at + 8 + len).min(bytes.len()))?;
        if id == b"fmt " && body.len() >= 16 {
            if u16::from_le_bytes([body[0], body[1]]) != 1 {
                return None;
            }
            bits = u16::from_le_bytes([body[14], body[15]]);
        } else if id == b"data" {
            if bits != 16 || body.len() < 2 {
                return None;
            }
            let n = body.len() / 2;
            let sum: f64 = body.chunks_exact(2).map(|c| (i16::from_le_bytes([c[0], c[1]]) as f64 / 32768.0).powi(2)).sum();
            let rms = (sum / n as f64).sqrt().max(1e-6);
            return Some(20.0 * rms.log10() as f32);
        }
        at += 8 + len + (len & 1);
    }
    None
}

/// The gain that brings a sound at `db` down to [`TARGET_DB`].
pub fn normalising_gain(db: f32) -> f32 {
    10f32.powf(((TARGET_DB - db) / 20.0).min(0.0)).max(0.1)
}

/// Where the extracted assets are: `PORTAL_RUNNER_ASSETS`, else `assets/extracted` here or
/// next to the repository the binary was built in.
pub fn find_root() -> Option<PathBuf> {
    if let Ok(p) = std::env::var("PORTAL_RUNNER_ASSETS") {
        let p = PathBuf::from(p);
        if p.is_dir() {
            return Some(p);
        }
    }
    let mut candidates = vec![PathBuf::from("assets/extracted")];
    if let Ok(exe) = std::env::current_exe() {
        for up in exe.ancestors().skip(1).take(4) {
            candidates.push(up.join("assets/extracted"));
        }
    }
    candidates.push(Path::new(env!("CARGO_MANIFEST_DIR")).join("assets/extracted"));
    candidates.into_iter().find(|p| p.is_dir())
}

impl Extracted {
    /// A texture by name (`textures/<name>.png`), tiling.
    pub fn texture(&self, name: &str) -> Option<Image> {
        self.load_texture(name, true)
    }

    /// A texture holding data rather than colour (a normal or metallic-roughness map), tiling.
    pub fn texture_data(&self, name: &str) -> Option<Image> {
        self.load_texture(name, false)
    }

    fn load_texture(&self, name: &str, srgb: bool) -> Option<Image> {
        let path = self.root.as_ref()?.join("textures").join(format!("{name}.png"));
        let bytes = std::fs::read(&path).ok()?;
        let sampler = ImageSampler::Descriptor(ImageSamplerDescriptor {
            address_mode_u: ImageAddressMode::Repeat,
            address_mode_v: ImageAddressMode::Repeat,
            anisotropy_clamp: 8,
            ..ImageSamplerDescriptor::linear()
        });
        match Image::from_buffer(&bytes, ImageType::Extension("png"), CompressedImageFormats::NONE, srgb, sampler, RenderAssetUsages::default()) {
            Ok(img) => Some(img),
            Err(e) => {
                warn!("{}: {e}", path.display());
                None
            }
        }
    }

    /// A model's path for the asset server (`extracted/models/<name>.glb`, relative to the
    /// assets folder), if it was extracted.
    /// A model's file: a map's (`map/props/...`), else the props library's copy of it (one
    /// the editor's placed that no map uses).
    pub fn model(&self, name: &str) -> Option<String> {
        let root = self.root.as_ref()?;
        if root.join("models").join(format!("{name}.glb")).is_file() {
            return Some(format!("extracted/models/{name}.glb"));
        }
        let rel = name.strip_prefix("map/")?;
        root.join("props/portal").join(format!("{rel}.glb")).is_file().then(|| format!("extracted/props/portal/{rel}.glb"))
    }

    pub fn sound(&self, name: &str) -> Option<Handle<AudioSource>> {
        self.sounds.get(name).cloned()
    }

    /// A sound's loudness correction (1 for anything not measured).
    pub fn gain(&self, h: &Handle<AudioSource>) -> f32 {
        self.gains.get(&h.id()).copied().unwrap_or(1.0)
    }

    /// Every sound whose name starts with `prefix` (its numbered variations).
    pub fn sounds_like(&self, prefix: &str) -> Vec<Handle<AudioSource>> {
        let mut v: Vec<(&String, &Handle<AudioSource>)> = self.sounds.iter().filter(|(k, _)| k.starts_with(prefix)).collect();
        v.sort_by(|a, b| a.0.cmp(b.0));
        v.into_iter().map(|(_, h)| h.clone()).collect()
    }
}

/// Whether a sound file's bytes look like what its extension says (a RIFF wave; an MP3 with
/// its tag or a frame header).
fn playable(ext: &str, bytes: &[u8]) -> bool {
    match ext {
        "wav" => bytes.len() > 44 && bytes.starts_with(b"RIFF"),
        "mp3" => bytes.starts_with(b"ID3") || (bytes.len() > 4 && bytes[0] == 0xff && bytes[1] & 0xe0 == 0xe0),
        _ => false,
    }
}

pub fn setup(mut commands: Commands, mut sources: ResMut<Assets<AudioSource>>) {
    let root = find_root();
    let mut sounds = HashMap::new();
    let mut gains = HashMap::new();
    match &root {
        Some(r) => {
            info!("Extracted Portal assets: {}", r.display());
            if let Ok(dir) = std::fs::read_dir(r.join("sounds")) {
                for e in dir.flatten() {
                    let p = e.path();
                    let ext = p.extension().and_then(|x| x.to_str()).unwrap_or("");
                    if !matches!(ext, "wav" | "mp3") {
                        continue;
                    }
                    let Some(stem) = p.file_stem().and_then(|s| s.to_str()) else { continue };
                    if let Ok(bytes) = std::fs::read(&p) {
                        // Only what can be decoded: the decoder panics on anything else (an
                        // empty file left by an old extract, say).
                        if !playable(ext, &bytes) {
                            warn!("{}: not a sound, skipped (run `cargo run --release -p portal_extract` again)", p.display());
                            continue;
                        }
                        let db = wav_rms_db(&bytes);
                        let h = sources.add(AudioSource { bytes: Arc::from(bytes.into_boxed_slice()) });
                        if let Some(db) = db {
                            gains.insert(h.id(), normalising_gain(db));
                        }
                        sounds.insert(stem.to_string(), h);
                    }
                }
            }
        }
        None => info!("No extracted Portal assets (run `cargo run --release -p portal_extract`): using stand-ins"),
    }
    commands.insert_resource(Extracted { root, sounds, gains });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wav(samples: &[i16]) -> Vec<u8> {
        let data: Vec<u8> = samples.iter().flat_map(|s| s.to_le_bytes()).collect();
        let mut b = b"RIFF".to_vec();
        b.extend((36 + data.len() as u32).to_le_bytes());
        b.extend(b"WAVEfmt ");
        b.extend(16u32.to_le_bytes());
        b.extend([1, 0, 1, 0]);
        b.extend(44100u32.to_le_bytes());
        b.extend(88200u32.to_le_bytes());
        b.extend([2, 0, 16, 0]);
        b.extend(b"data");
        b.extend((data.len() as u32).to_le_bytes());
        b.extend(data);
        b
    }

    #[test]
    fn measures_and_normalises_loudness() {
        let loud = wav(&[i16::MAX, i16::MIN].repeat(500));
        let db = wav_rms_db(&loud).unwrap();
        assert!(db.abs() < 0.1, "{db}");
        assert!((normalising_gain(db) - 0.1).abs() < 0.01);
        let quiet = wav(&[1000, -1000].repeat(500));
        let db = wav_rms_db(&quiet).unwrap();
        assert!(db < TARGET_DB);
        // Quiet sounds are never boosted.
        assert_eq!(normalising_gain(db), 1.0);
        assert_eq!(wav_rms_db(b"not a wav"), None);
        // Empty or foreign files aren't loaded (the decoder would panic on them).
        assert!(playable("wav", &loud));
        assert!(!playable("wav", b""));
        assert!(!playable("mp3", b""));
    }
}
