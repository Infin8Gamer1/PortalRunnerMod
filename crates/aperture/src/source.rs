//! Portal's own maps as levels: what `portal_extract` wrote from a map's BSP (its collision
//! boxes, each brush model's bounds, and the entity lump) turned into a [`Level`].
//!
//! The faces to draw are the game's business (`Level::source` names the map). What's read
//! here: the world's boxes (white ones take portals), goo, the start, cubes, the portal gun's
//! colours, no-portal volumes and lights. Brush entities (doors, the buttons' moving tops,
//! elevators) stand where the map has them, solid.

use std::collections::HashMap;

use glam::Vec3;

use faith_move::Aabb;

use crate::level::{CubeKind, Gun, Item, Level, Mat, SourceLight, Solid};

/// A Source entity: its keys in order (a key can repeat: outputs).
pub type Entity = Vec<(String, String)>;

/// The entity lump's entities.
pub fn parse_entities(text: &str) -> Vec<Entity> {
    let mut out = Vec::new();
    let mut cur: Option<Entity> = None;
    let mut pending: Option<String> = None;
    let mut chars = text.chars();
    while let Some(c) = chars.next() {
        match c {
            '{' => {
                cur = Some(Vec::new());
                pending = None;
            }
            '}' => {
                if let Some(e) = cur.take() {
                    out.push(e);
                }
            }
            '"' => {
                let s: String = chars.by_ref().take_while(|&c| c != '"').collect();
                match pending.take() {
                    None => pending = Some(s),
                    Some(k) => {
                        if let Some(e) = cur.as_mut() {
                            e.push((k, s));
                        }
                    }
                }
            }
            _ => {}
        }
    }
    out
}

pub fn key<'a>(e: &'a Entity, k: &str) -> Option<&'a str> {
    e.iter().find(|(a, _)| a.eq_ignore_ascii_case(k)).map(|(_, v)| v.as_str())
}

fn floats(v: &str) -> Vec<f32> {
    v.split_whitespace().filter_map(|s| s.parse().ok()).collect()
}

/// Source's space (inches, Z up, X forward) to ours (metres, Y up, -Z forward).
pub fn to_ours(p: Vec3) -> Vec3 {
    Vec3::new(-p.y, p.z, -p.x) * 0.0254
}

/// A direction from Source's pitch and yaw (degrees).
pub fn direction(pitch: f32, yaw: f32) -> Vec3 {
    let (p, y) = (pitch.to_radians(), yaw.to_radians());
    let source = Vec3::new(p.cos() * y.cos(), p.cos() * y.sin(), -p.sin());
    Vec3::new(-source.y, source.z, -source.x)
}

fn origin(e: &Entity) -> Option<Vec3> {
    let v = floats(key(e, "origin")?);
    (v.len() >= 3).then(|| to_ours(Vec3::new(v[0], v[1], v[2])))
}

/// One line of a `.solids` file.
#[derive(Clone, Debug, PartialEq)]
pub struct MapBox {
    pub kind: String,
    pub model: usize,
    pub b: Aabb,
}

pub fn parse_solids(text: &str) -> Vec<MapBox> {
    text.lines()
        .filter_map(|l| {
            let mut it = l.split_whitespace();
            let kind = it.next()?.to_string();
            let model = it.next()?.parse().ok()?;
            let v: Vec<f32> = it.filter_map(|s| s.parse().ok()).collect();
            (v.len() == 6).then(|| MapBox { kind, model, b: Aabb::new(Vec3::new(v[0], v[1], v[2]), Vec3::new(v[3], v[4], v[5])) })
        })
        .collect()
}

/// Brush entities that stand solid where the map has them (until their logic moves them).
fn solid_class(class: &str, e: &Entity) -> bool {
    match class {
        "func_brush" => key(e, "Solidity") != Some("1") && key(e, "StartDisabled") != Some("1"),
        "func_door" | "func_door_rotating" | "func_tracktrain" | "func_movelinear" | "func_rotating" | "func_physbox" | "func_wall_toggle" => true,
        _ => false,
    }
}

/// A map as a level. `name` is the map's (`testchmb_a_00`), `title` what the game calls it.
pub fn level(name: &str, title: &'static str, solids: &str, ents: &str) -> Level {
    let mut l = Level::new(title);
    l.source = Some(name.to_string());
    let boxes = parse_solids(solids);
    let ents = parse_entities(ents);
    // Which entity each brush model belongs to.
    let mut owner: HashMap<usize, &Entity> = HashMap::new();
    for e in &ents {
        if let Some(n) = key(e, "model").and_then(|m| m.strip_prefix('*')).and_then(|m| m.parse().ok()) {
            owner.insert(n, e);
        }
    }
    let bounds: HashMap<usize, Aabb> = boxes.iter().filter(|b| b.kind == "bounds").map(|b| (b.model, b.b)).collect();

    for b in &boxes {
        let solid = b.model == 0 || owner.get(&b.model).is_some_and(|e| solid_class(key(e, "classname").unwrap_or(""), e));
        let mat = match b.kind.as_str() {
            "white" => Mat::White,
            "black" => Mat::Black,
            "glass" => Mat::Glass,
            "grate" => Mat::Grate,
            "clip" => Mat::Clip,
            "goo" if b.model == 0 => {
                l.items.push(Item::Goo { b: b.b });
                continue;
            }
            _ => continue,
        };
        if solid {
            l.solids.push(Solid { b: b.b, mat });
        }
    }

    let mut spawned = false;
    for e in &ents {
        let class = key(e, "classname").unwrap_or("");
        let model_bounds = || key(e, "model").and_then(|m| m.strip_prefix('*')).and_then(|m| m.parse::<usize>().ok()).and_then(|m| bounds.get(&m).copied());
        match class {
            "info_player_start" if !spawned => {
                if let Some(at) = origin(e) {
                    let yaw = key(e, "angles").map(floats).and_then(|a| a.get(1).copied()).unwrap_or(0.0);
                    l.spawn(at.to_array(), yaw);
                    spawned = true;
                }
            }
            "prop_physics" | "prop_physics_override" if key(e, "model").is_some_and(|m| m.eq_ignore_ascii_case("models/props/metal_box.mdl")) => {
                if let Some(at) = origin(e) {
                    let kind = if key(e, "skin") == Some("1") { CubeKind::Companion } else { CubeKind::Storage };
                    l.items.push(Item::Cube { at: at + Vec3::Y * 0.02, kind });
                }
            }
            "weapon_portalgun" => {
                l.gun = Gun { blue: key(e, "CanFirePortal1") != Some("0"), orange: key(e, "CanFirePortal2") == Some("1") };
            }
            "func_noportal_volume" => {
                if let Some(b) = model_bounds() {
                    l.repaint(Mat::Black, b.min.to_array(), b.max.to_array());
                }
            }
            "light" | "light_spot" => {
                let Some(at) = origin(e) else { continue };
                let v = key(e, "_light").map(floats).unwrap_or_default();
                let (color, brightness) = match v.as_slice() {
                    [r, g, b, k, ..] => (Vec3::new(*r, *g, *b) / 255.0, *k),
                    [r, g, b] => (Vec3::new(*r, *g, *b) / 255.0, 200.0),
                    _ => (Vec3::ONE, 200.0),
                };
                let spot = (class == "light_spot").then(|| {
                    let a = key(e, "angles").map(floats).unwrap_or_default();
                    let pitch = key(e, "pitch").and_then(|p| p.parse().ok()).or(a.first().copied()).unwrap_or(0.0);
                    let cone = key(e, "_cone").and_then(|c| c.parse::<f32>().ok()).unwrap_or(45.0);
                    (direction(pitch, a.get(1).copied().unwrap_or(0.0)), cone.to_radians())
                });
                l.lights.push(SourceLight { at, color, brightness, spot });
            }
            _ => {}
        }
    }
    // Without a gun in the map, Portal's chambers before you find it: none.
    if !ents.iter().any(|e| key(e, "classname") == Some("weapon_portalgun")) {
        l.gun = Gun::NONE;
    }
    if !spawned {
        l.spawn([0.0, 0.0, 0.0], 0.0);
    }
    // Falling out of the map: well below its lowest box.
    let lowest = l.solids.iter().map(|s| s.b.min.y).fold(f32::MAX, f32::min);
    l.kill_y = if lowest.is_finite() { lowest - 10.0 } else { -30.0 };
    l
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn entities_keep_repeated_keys() {
        let e = parse_entities("{\n\"classname\" \"logic_relay\"\n\"OnTrigger\" \"a,Open,,0,-1\"\n\"OnTrigger\" \"b,Close,,1,-1\"\n}\n{\n\"classname\" \"worldspawn\"\n}");
        assert_eq!(e.len(), 2);
        assert_eq!(e[0].iter().filter(|(k, _)| k == "OnTrigger").count(), 2);
        assert_eq!(key(&e[1], "classname"), Some("worldspawn"));
    }

    #[test]
    fn source_axes_turn_into_ours() {
        // Source's forward (+X) is our -Z; its left (+Y) our -X; its up our up.
        assert!((to_ours(Vec3::new(100.0, 0.0, 0.0)) - Vec3::new(0.0, 0.0, -2.54)).length() < 1e-5);
        assert!((to_ours(Vec3::new(0.0, 100.0, 0.0)) - Vec3::new(-2.54, 0.0, 0.0)).length() < 1e-5);
        assert!((direction(90.0, 0.0) - Vec3::NEG_Y).length() < 1e-5);
        assert!((direction(0.0, 90.0) - Vec3::NEG_X).length() < 1e-5);
    }

    #[test]
    fn a_map_becomes_a_level() {
        let solids = "bounds 0 -10 -1 -10 10 5 10\nwhite 0 -10 -1 -10 10 0 10\nblack 0 -10 0 -10 -9 5 10\nbounds 1 0 0 0 1 2 1\nblack 1 0 0 0 1 2 1\nbounds 2 2 0 2 3 2 3\nblack 2 2 0 2 3 2 3\ngoo 0 -5 -1 -5 -4 -0.5 -4\n";
        let ents = r#"{ "classname" "worldspawn" }
{ "classname" "info_player_start" "origin" "0 0 0" "angles" "0 90 0" }
{ "classname" "func_door" "model" "*1" }
{ "classname" "trigger_once" "model" "*2" }
{ "classname" "prop_physics" "model" "models/props/metal_box.mdl" "origin" "40 0 20" }"#;
        let l = level("test", "Test", solids, ents);
        // The world's two boxes and the door; not the trigger.
        assert_eq!(l.solids.len(), 3);
        assert_eq!(l.solids.iter().filter(|s| s.mat == Mat::White).count(), 1);
        assert!(l.items.iter().any(|i| matches!(i, Item::Goo { .. })));
        assert!(l.items.iter().any(|i| matches!(i, Item::Cube { kind: CubeKind::Storage, .. })));
        assert!((l.checkpoints[0].yaw - 90f32.to_radians()).abs() < 1e-5);
        assert_eq!(l.gun, Gun::NONE);
    }
}
