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
        let path = self.root.as_ref()?.join("textures").join(format!("{name}.png"));
        let bytes = std::fs::read(&path).ok()?;
        let sampler = ImageSampler::Descriptor(ImageSamplerDescriptor {
            address_mode_u: ImageAddressMode::Repeat,
            address_mode_v: ImageAddressMode::Repeat,
            anisotropy_clamp: 8,
            ..ImageSamplerDescriptor::linear()
        });
        match Image::from_buffer(&bytes, ImageType::Extension("png"), CompressedImageFormats::NONE, true, sampler, RenderAssetUsages::default()) {
            Ok(img) => Some(img),
            Err(e) => {
                warn!("{}: {e}", path.display());
                None
            }
        }
    }

    /// A model's path for the asset server (`extracted/models/<name>.glb`, relative to the
    /// assets folder), if it was extracted.
    pub fn model(&self, name: &str) -> Option<String> {
        let p = self.root.as_ref()?.join("models").join(format!("{name}.glb"));
        p.is_file().then(|| format!("extracted/models/{name}.glb"))
    }

    pub fn sound(&self, name: &str) -> Option<Handle<AudioSource>> {
        self.sounds.get(name).cloned()
    }

    /// Every sound whose name starts with `prefix` (its numbered variations).
    pub fn sounds_like(&self, prefix: &str) -> Vec<Handle<AudioSource>> {
        let mut v: Vec<(&String, &Handle<AudioSource>)> = self.sounds.iter().filter(|(k, _)| k.starts_with(prefix)).collect();
        v.sort_by(|a, b| a.0.cmp(b.0));
        v.into_iter().map(|(_, h)| h.clone()).collect()
    }
}

pub fn setup(mut commands: Commands, mut sources: ResMut<Assets<AudioSource>>) {
    let root = find_root();
    let mut sounds = HashMap::new();
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
                        sounds.insert(stem.to_string(), sources.add(AudioSource { bytes: Arc::from(bytes.into_boxed_slice()) }));
                    }
                }
            }
        }
        None => info!("No extracted Portal assets (run `cargo run --release -p portal_extract`): using stand-ins"),
    }
    commands.insert_resource(Extracted { root, sounds });
}
