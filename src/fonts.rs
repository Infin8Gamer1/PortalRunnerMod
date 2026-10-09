//! The UI's typeface: Inter (or another clean sans) from the system, loaded at startup.
//! Nothing is bundled: `PORTAL_FONT_DIR` or `assets/fonts` can hold the files, and failing
//! all of those Bevy's built-in font is used.

use std::path::PathBuf;

use bevy::prelude::*;
use bevy::text::Font;

#[derive(Resource, Clone, Default)]
pub struct UiFonts {
    pub regular: Handle<Font>,
    pub bold: Handle<Font>,
}

impl UiFonts {
    pub fn regular(&self, size: f32) -> TextFont {
        TextFont { font: self.regular.clone().into(), font_size: size.into(), ..default() }
    }

    pub fn bold(&self, size: f32) -> TextFont {
        TextFont { font: self.bold.clone().into(), font_size: size.into(), ..default() }
    }
}

/// Where to look, in order: (regular, bold) file pairs.
fn candidates() -> Vec<(PathBuf, PathBuf)> {
    let mut dirs: Vec<PathBuf> = Vec::new();
    if let Some(d) = std::env::var_os("PORTAL_FONT_DIR") {
        dirs.push(d.into());
    }
    dirs.push("assets/fonts".into());
    dirs.push(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("assets/fonts"));
    let mut out = Vec::new();
    for d in &dirs {
        out.push((d.join("Inter-Regular.ttf"), d.join("Inter-SemiBold.ttf")));
    }
    for (r, b) in [
        ("/usr/share/fonts/rsms-inter-fonts/Inter-Regular.ttf", "/usr/share/fonts/rsms-inter-fonts/Inter-SemiBold.ttf"),
        ("/usr/share/fonts/inter/Inter-Regular.ttf", "/usr/share/fonts/inter/Inter-SemiBold.ttf"),
        ("/usr/share/fonts/truetype/inter/Inter-Regular.ttf", "/usr/share/fonts/truetype/inter/Inter-SemiBold.ttf"),
        ("/usr/share/fonts/google-noto/NotoSans-Regular.ttf", "/usr/share/fonts/google-noto/NotoSans-SemiBold.ttf"),
        ("/usr/share/fonts/truetype/noto/NotoSans-Regular.ttf", "/usr/share/fonts/truetype/noto/NotoSans-SemiBold.ttf"),
        ("/usr/share/fonts/dejavu-sans-fonts/DejaVuSans.ttf", "/usr/share/fonts/dejavu-sans-fonts/DejaVuSans-Bold.ttf"),
        ("/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf", "/usr/share/fonts/truetype/dejavu/DejaVuSans-Bold.ttf"),
    ] {
        out.push((r.into(), b.into()));
    }
    out
}

pub fn setup(mut commands: Commands, mut fonts: ResMut<Assets<Font>>) {
    let load = |fonts: &mut Assets<Font>, p: &PathBuf| std::fs::read(p).ok().map(|b| fonts.add(Font::from_bytes(b)));
    for (r, b) in candidates() {
        if let Some(regular) = load(&mut fonts, &r) {
            let bold = load(&mut fonts, &b).unwrap_or_else(|| regular.clone());
            info!("UI font: {}", r.display());
            commands.insert_resource(UiFonts { regular, bold });
            return;
        }
    }
    info!("No UI font found; using Bevy's default");
    commands.insert_resource(UiFonts::default());
}
