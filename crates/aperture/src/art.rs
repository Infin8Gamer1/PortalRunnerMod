//! What's drawn of one of Portal's maps besides the game's own pieces: its faces (by brush
//! model and material), props, glow sprites, grill particles, overlays and cables. A map read
//! from `portal_extract`'s files has these from them ([`MapArt::from_files`]); the game's own
//! copy of a map (a level file, [`crate::doc`]) makes them from its brushes and entities.

use glam::{Quat, Vec3};

use crate::rope::Rope;
use crate::source::{self, key, Entity};

/// Faces of one material on one brush model (0: the world; a brush entity's move with it).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct FaceGroup {
    pub model: usize,
    pub mat: String,
    pub pos: Vec<[f32; 3]>,
    pub nrm: Vec<[f32; 3]>,
    pub uv: Vec<[f32; 2]>,
    pub idx: Vec<u32>,
}

/// What a prop is.
#[derive(Clone, Debug, PartialEq)]
pub enum PropKind {
    /// A static prop.
    Static,
    /// A prop the logic can animate, hide or reskin (prop_dynamic).
    Dynamic,
    /// A physics prop (one the game only draws), its lowest point `bottom` below its origin
    /// as it's turned, if its model says.
    Physics { bottom: Option<f32> },
    /// A glow sprite (env_sprite): `size` metres across, glowing in `color` (0-1) at `alpha`;
    /// the game shows it once the logic turns it on, unless it starts on.
    Sprite { size: f32, color: Vec3, alpha: f32 },
    /// An emancipation grill's particle system (`model` is its effect).
    Particles,
}

/// One of a map's props, sprites or particle systems.
#[derive(Clone, Debug, PartialEq)]
pub struct ArtProp {
    pub kind: PropKind,
    /// Its model under the extracted `models/` (`map/props/door_01_frame_wide_reference`,
    /// `_skinN` for another skin); a sprite's material; a particle system's effect.
    pub model: String,
    pub at: Vec3,
    pub turn: Quat,
    /// What it's parented to (it rides it), and its own name (the logic's), lower case.
    pub parent: String,
    pub name: String,
}

/// An overlay (a decal: signs, indicator strips, stains): a quad just off the surface it's on.
#[derive(Clone, Debug, PartialEq)]
pub struct ArtOverlay {
    pub mat: String,
    /// Its material's flags (as the map's mesh file has them), where they're known.
    pub flags: Option<u8>,
    /// Its accessor's name, for the logic to switch its texture's frame.
    pub name: String,
    /// Its corners and their texture coordinates.
    pub corners: [(Vec3, [f32; 2]); 4],
    pub normal: Vec3,
}

/// What's drawn of a map.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct MapArt {
    /// The map whose mesh file (`maps/<name>.mesh`) has its faces, when they're read from it.
    pub mesh: Option<String>,
    /// Its faces (else).
    pub faces: Vec<FaceGroup>,
    pub props: Vec<ArtProp>,
    pub overlays: Vec<ArtOverlay>,
    pub ropes: Vec<Rope>,
}

/// Models the game draws its own way in Portal's maps (cubes, buttons).
const OWN_MODELS: &[&str] = &["models/props/metal_box.mdl", "models/props/button_top_reference.mdl", "models/props/button_base_reference.mdl"];

/// A prop model's file under `models/` (as `portal_extract` names it).
pub fn prop_glb(model: &str, skin: i32) -> String {
    let stem = model.trim_start_matches("models/").trim_end_matches(".mdl");
    if skin > 0 { format!("map/{stem}_skin{skin}") } else { format!("map/{stem}") }
}

/// And back: a model file's MDL and skin.
pub fn glb_model(file: &str) -> (String, i32) {
    let stem = file.trim_start_matches("map/");
    match stem.rsplit_once("_skin").and_then(|(m, k)| Some((m, k.parse::<i32>().ok()?))) {
        Some((m, k)) => (format!("models/{m}.mdl"), k),
        None => (format!("models/{stem}.mdl"), 0),
    }
}

fn floats(v: &str) -> Vec<f32> {
    v.split_whitespace().filter_map(|x| x.parse().ok()).collect()
}

fn vec3(v: &str) -> Vec3 {
    let f = floats(v);
    Vec3::new(f.first().copied().unwrap_or(0.0), f.get(1).copied().unwrap_or(0.0), f.get(2).copied().unwrap_or(0.0))
}

/// Read `maps/<name>.props` (see `portal_extract`'s `maps.rs`).
pub fn parse_props(text: &str) -> Vec<ArtProp> {
    let mut out = Vec::new();
    for line in text.lines() {
        let f: Vec<&str> = line.split('|').collect();
        if f.len() < 6 {
            continue;
        }
        let at = source::to_ours(vec3(f[2]));
        let (parent, name) = (f[4].to_ascii_lowercase(), f[5].to_ascii_lowercase());
        let (kind, turn) = match f[0] {
            "particles" => {
                let a = vec3(f[3]);
                (PropKind::Particles, source::source_turn(a.x, a.y, a.z))
            }
            "sprite" => {
                let v = floats(f[3]);
                let g = |k: usize, d: f32| v.get(k).copied().unwrap_or(d);
                (PropKind::Sprite { size: g(0, 64.0) * 0.0254, color: Vec3::new(g(1, 255.0), g(2, 255.0), g(3, 255.0)) / 255.0, alpha: g(4, 255.0) / 255.0 }, Quat::IDENTITY)
            }
            k => {
                let a = vec3(f[3]);
                let kind = match k {
                    "static" => PropKind::Static,
                    "physics" => PropKind::Physics { bottom: f.get(6).and_then(|b| b.trim().parse::<f32>().ok()).map(|b| b * 0.0254) },
                    _ => PropKind::Dynamic,
                };
                (kind, source::source_turn(a.x, a.y, a.z))
            }
        };
        out.push(ArtProp { kind, model: f[1].to_string(), at, turn, parent, name });
    }
    out
}

/// Read `maps/<name>.overlays`.
pub fn parse_overlays(text: &str) -> Vec<ArtOverlay> {
    let mut out = Vec::new();
    for line in text.lines() {
        let f: Vec<&str> = line.split('|').collect();
        if f.len() < 5 {
            continue;
        }
        let v = floats(f[3]);
        let n = vec3(f[4]);
        if v.len() < 20 || n == Vec3::ZERO {
            continue;
        }
        let corners = std::array::from_fn(|k| (Vec3::new(v[k * 5], v[k * 5 + 1], v[k * 5 + 2]), [v[k * 5 + 3], v[k * 5 + 4]]));
        out.push(ArtOverlay { mat: f[0].to_string(), flags: f[1].parse().ok(), name: f[2].to_ascii_lowercase(), corners, normal: n });
    }
    out
}

impl MapArt {
    /// Map `name`'s, from `portal_extract`'s files (its faces stay in its mesh file).
    pub fn from_files(name: &str, props: &str, overlays: &str, ents: &str) -> MapArt {
        MapArt { mesh: Some(name.to_string()), faces: Vec::new(), props: parse_props(props), overlays: parse_overlays(overlays), ropes: crate::rope::hang(&source::parse_entities(ents)) }
    }
}

/// The props, sprites and grill particles among a map's entities (`at` and `turn` each one's,
/// in our space), as `portal_extract` writes them out: a static prop is a `prop_static`
/// entity here, as in Hammer.
pub fn entity_props(ents: &[(Entity, Vec3, Quat)]) -> Vec<ArtProp> {
    let mut props = Vec::new();
    let mut sprites = Vec::new();
    let mut particles = Vec::new();
    let lower = |e: &Entity, k: &str| key(e, k).unwrap_or("").to_ascii_lowercase();
    let num = |e: &Entity, k: &str| key(e, k).and_then(|v| v.trim().parse::<f32>().ok());
    for (e, at, turn) in ents {
        let class = key(e, "classname").unwrap_or("");
        let (parent, name) = (lower(e, "parentname"), lower(e, "targetname"));
        match class {
            "env_sprite" => {
                let Some(model) = key(e, "model") else { continue };
                let mat = model.to_ascii_lowercase().replace('\\', "/");
                let mat = mat.trim_start_matches("materials/").trim_end_matches(".vmt").trim_end_matches(".spr").to_string();
                if mat.contains("sphere_silhouette") {
                    continue;
                }
                let width = num(e, "_width").unwrap_or(64.0);
                let scale = num(e, "scale").filter(|s| *s > 0.0).unwrap_or(1.0);
                let c = floats(key(e, "rendercolor").unwrap_or("255 255 255"));
                let g = |k: usize| c.get(k).copied().unwrap_or(255.0) / 255.0;
                let alpha = num(e, "renderamt").unwrap_or(255.0) / 255.0;
                sprites.push(ArtProp { kind: PropKind::Sprite { size: width * scale * 0.0254, color: Vec3::new(g(0), g(1), g(2)), alpha }, model: mat, at: *at, turn: Quat::IDENTITY, parent, name });
            }
            "info_particle_system" => {
                let effect = lower(e, "effect_name");
                if effect == "portal_cleanser" || effect == "cleanser_edge_1" {
                    particles.push(ArtProp { kind: PropKind::Particles, model: effect, at: *at, turn: *turn, parent, name });
                }
            }
            "prop_static" | "prop_dynamic" | "prop_dynamic_override" | "prop_physics" | "prop_physics_override" | "prop_physics_multiplayer" | "prop_portal_stats_display" => {
                // (The elevator car with the stats screen in it has no model key: Portal's
                // server always gives it the round elevator's body.)
                let model = if class == "prop_portal_stats_display" { "models/props/round_elevator_body.mdl".to_string() } else { lower(e, "model").replace('\\', "/") };
                if OWN_MODELS.contains(&model.as_str()) || model.contains("portal_cleanser") || !model.ends_with(".mdl") {
                    continue;
                }
                let skin = key(e, "skin").and_then(|s| s.trim().parse().ok()).unwrap_or(0);
                let kind = match class {
                    "prop_static" => PropKind::Static,
                    c if c.starts_with("prop_physics") => PropKind::Physics { bottom: num(e, "_bottom").map(|b| b * 0.0254) },
                    _ => PropKind::Dynamic,
                };
                props.push(ArtProp { kind, model: prop_glb(&model, skin), at: *at, turn: *turn, parent, name });
            }
            _ => {}
        }
    }
    // (In the order the extractor writes them: props, then sprites and particles.)
    props.extend(sprites);
    props.extend(particles);
    props
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn model_files_name_their_model() {
        assert_eq!(prop_glb("models/props/round_elevator_body.mdl", 0), "map/props/round_elevator_body");
        assert_eq!(glb_model("map/props_animsigns/signage_num00_skin2"), ("models/props_animsigns/signage_num00.mdl".to_string(), 2));
        assert_eq!(glb_model("map/props/radio_reference"), ("models/props/radio_reference.mdl".to_string(), 0));
    }
}
