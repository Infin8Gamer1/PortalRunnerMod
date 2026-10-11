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

use crate::io;
use crate::level::{CubeKind, Gun, Inputs, Item, Launch, Level, Mat, MoverDef, SourceLight, Solid, Swing};
use crate::portal::{Portal, PortalColor};

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

/// Source's angles (pitch, yaw, roll; degrees) as a turn in our space (what's ahead along
/// Source's +X is ahead along our -Z).
pub fn source_turn(pitch: f32, yaw: f32, roll: f32) -> glam::Quat {
    let r = glam::Quat::from_rotation_z(yaw.to_radians()) * glam::Quat::from_rotation_y(pitch.to_radians()) * glam::Quat::from_rotation_x(roll.to_radians());
    let to_ours = glam::Quat::from_mat3(&glam::Mat3::from_cols(Vec3::NEG_Z, Vec3::NEG_X, Vec3::Y));
    to_ours * r * to_ours.inverse()
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

/// Source's angles (pitch, yaw, roll; degrees) to our forward and up.
pub fn angle_vectors(pitch: f32, yaw: f32, roll: f32) -> (Vec3, Vec3) {
    let (sp, cp) = pitch.to_radians().sin_cos();
    let (sy, cy) = yaw.to_radians().sin_cos();
    let (sr, cr) = roll.to_radians().sin_cos();
    let forward = Vec3::new(cp * cy, cp * sy, -sp);
    let up = Vec3::new(cr * sp * cy + sr * sy, cr * sp * sy - sr * cy, cr * cp);
    let ours = |v: Vec3| Vec3::new(-v.y, v.z, -v.x).normalize_or_zero();
    (ours(forward), ours(up))
}

fn angles(e: &Entity, k: &str) -> (f32, f32, f32) {
    let a = key(e, k).map(floats).unwrap_or_default();
    (a.first().copied().unwrap_or(0.0), a.get(1).copied().unwrap_or(0.0), a.get(2).copied().unwrap_or(0.0))
}

fn num(e: &Entity, k: &str, default: f32) -> f32 {
    key(e, k).and_then(|v| v.trim().parse().ok()).unwrap_or(default)
}

fn flags(e: &Entity) -> u32 {
    key(e, "spawnflags").and_then(|v| v.trim().parse().ok()).unwrap_or(0)
}

fn mat_of(kind: &str) -> Option<Mat> {
    Some(match kind {
        "white" => Mat::White,
        "black" => Mat::Black,
        "glass" => Mat::Glass,
        "grate" => Mat::Grate,
        "clip" => Mat::Clip,
        _ => return None,
    })
}

fn is_metal_box(e: &Entity) -> bool {
    matches!(key(e, "classname"), Some("prop_physics" | "prop_physics_override" | "prop_physics_multiplayer"))
        && key(e, "model").is_some_and(|m| m.eq_ignore_ascii_case("models/props/metal_box.mdl"))
}

fn cube_kind(e: &Entity) -> CubeKind {
    if key(e, "skin") == Some("1") { CubeKind::Companion } else { CubeKind::Storage }
}

/// GLaDOS's scenes (`maps/scenes.txt`): scene file to its length, lines (start, sound) and
/// the triggers it fires (start, n: its `OnTriggerN`).
pub type Scenes = HashMap<String, SceneInfo>;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct SceneInfo {
    pub length: f32,
    pub lines: Vec<(f32, String)>,
    pub triggers: Vec<(f32, u32)>,
}

/// Read `maps/scenes.txt` (a line a scene: `file|length|start:sound;...|start:n;...`).
pub fn parse_scenes(text: &str) -> Scenes {
    text.lines()
        .filter_map(|l| {
            let mut f = l.split('|');
            let (file, length, lines, triggers) = (f.next()?, f.next()?.parse().ok()?, f.next().unwrap_or(""), f.next().unwrap_or(""));
            let lines = lines.split(';').filter_map(|x| x.split_once(':')).filter_map(|(t, s)| Some((t.parse().ok()?, s.to_string()))).collect();
            let triggers = triggers.split(';').filter_map(|x| x.split_once(':')).filter_map(|(t, n)| Some((t.parse().ok()?, n.parse().ok()?))).collect();
            Some((file.to_string(), SceneInfo { length, lines, triggers }))
        })
        .collect()
}

/// A map as a level. `name` is the map's (`testchmb_a_00`), `title` what the game calls it.
pub fn level(name: &str, title: &'static str, solids: &str, ents: &str) -> Level {
    level_with_scenes(name, title, solids, ents, &Scenes::new())
}

/// The same, GLaDOS speaking her scenes' lines.
pub fn level_with_scenes(name: &str, title: &'static str, solids: &str, ents: &str, scenes: &Scenes) -> Level {
    let mut l = Level::new(title);
    l.source = Some(name.to_string());
    let boxes = parse_solids(solids);
    let ents = parse_entities(ents);
    let model_of = |e: &Entity| key(e, "model").and_then(|m| m.strip_prefix('*')).and_then(|m| m.parse::<usize>().ok());
    let bounds: HashMap<usize, Aabb> = boxes.iter().filter(|b| b.kind == "bounds").map(|b| (b.model, b.b)).collect();
    let mut model_boxes: HashMap<usize, Vec<Solid>> = HashMap::new();
    for b in &boxes {
        if let Some(mat) = mat_of(&b.kind) {
            model_boxes.entry(b.model).or_default().push(Solid { b: b.b, mat });
        }
    }
    let named: HashMap<String, &Entity> = ents.iter().filter_map(|e| Some((key(e, "targetname")?.to_ascii_lowercase(), e))).collect();
    // Entities a template makes: not there until it's told to make them.
    let mut templated = std::collections::HashSet::new();
    for e in &ents {
        if key(e, "classname") == Some("point_template") {
            for (k, v) in e {
                if k.to_ascii_lowercase().starts_with("template") {
                    templated.insert(v.to_ascii_lowercase());
                }
            }
        }
    }
    // A template's cube: where it was put, where the template is (what it's placed from), and
    // its kind.
    let template_cube = |template: &str| -> Option<(Vec3, Vec3, CubeKind)> {
        let t = named.get(&template.to_ascii_lowercase())?;
        let cube = t.iter().filter(|(k, _)| k.to_ascii_lowercase().starts_with("template")).filter_map(|(_, v)| named.get(&v.to_ascii_lowercase())).find(|c| is_metal_box(c))?;
        let at = origin(cube)?;
        Some((at, origin(t).unwrap_or(at), cube_kind(cube)))
    };

    // The world.
    for b in &boxes {
        if b.model != 0 {
            continue;
        }
        match b.kind.as_str() {
            "goo" => l.items.push(Item::Goo { b: b.b }),
            k => {
                if let Some(mat) = mat_of(k) {
                    l.solids.push(Solid { b: b.b, mat });
                }
            }
        }
    }

    let mut next_sig = 0usize;
    let mut sig = || {
        next_sig += 1;
        next_sig - 1
    };
    let mut io_ents: Vec<io::Ent> = Vec::new();
    let mut initial = Vec::new();
    let mut movers = 0usize;
    let mut buttons: Vec<Aabb> = Vec::new();
    // Train paths: each path node's (mover, node).
    let mut path_nodes: HashMap<String, (usize, usize)> = HashMap::new();
    let mut spawned = false;
    let mut spawn_gun = Gun::NONE;

    for e in &ents {
        let class = key(e, "classname").unwrap_or("");
        let name = key(e, "targetname").unwrap_or("").to_string();
        let model = model_of(e);
        let mb = model.and_then(|m| bounds.get(&m).copied());
        let start_disabled = key(e, "StartDisabled") == Some("1");
        let mut kind = io::Kind::Other;
        match class {
            "info_player_start" if !spawned => {
                if let Some(at) = origin(e) {
                    l.spawn(at.to_array(), angles(e, "angles").1);
                    spawned = true;
                }
            }
            _ if is_metal_box(e) && !templated.contains(&name.to_ascii_lowercase()) => {
                if let Some(at) = origin(e) {
                    l.items.push(Item::Cube { at: at + Vec3::Y * 0.02, kind: cube_kind(e) });
                }
            }
            // A gun on a pedestal (the map hears when it's picked up) is to be picked up; the
            // rest are left by the start so a map played by itself hands her what she'd have
            // by then.
            "weapon_portalgun" => {
                let gun = Gun { blue: key(e, "CanFirePortal1") != Some("0"), orange: key(e, "CanFirePortal2") == Some("1") };
                if e.iter().any(|(k, _)| k.eq_ignore_ascii_case("OnPlayerPickup")) {
                    if let Some(at) = origin(e) {
                        let (p, y, r) = angles(e, "angles");
                        let index = l.items.iter().filter(|i| matches!(i, Item::GunPickup { .. })).count();
                        l.items.push(Item::GunPickup { at, turn: source_turn(p, y, r), gun, mover: None });
                        kind = io::Kind::GunPickup { index };
                    }
                } else {
                    spawn_gun.blue |= gun.blue;
                    spawn_gun.orange |= gun.orange;
                }
            }
            "func_noportal_volume" => {
                if let Some(b) = mb {
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
                    let a = angles(e, "angles");
                    let pitch = key(e, "pitch").and_then(|p| p.parse().ok()).unwrap_or(a.0);
                    (direction(pitch, a.1), num(e, "_cone", 45.0).to_radians())
                });
                l.lights.push(SourceLight { at, color, brightness, spot });
            }
            // A floor button's moving top: the game's own button stands in for it.
            "func_door" if name.to_ascii_lowercase().contains("button") && mb.is_some_and(|b| b.size().x < 2.2 && b.size().z < 2.2) => {
                let b = mb.unwrap();
                let c = b.center();
                let floor = l
                    .solids
                    .iter()
                    .filter(|s| s.b.min.x <= c.x && s.b.max.x >= c.x && s.b.min.z <= c.z && s.b.max.z >= c.z && s.b.max.y <= b.min.y + 0.1)
                    .map(|s| s.b.max.y)
                    .fold(f32::MIN, f32::max);
                let at = Vec3::new(c.x, if floor > b.min.y - 1.0 { floor } else { b.min.y }, c.z);
                l.items.push(Item::FloorButton { sig: sig(), at, cube_only: false });
                kind = io::Kind::ButtonDoor { button: buttons.len(), down: false };
                buttons.push(b);
                l.hidden_models.push(model.unwrap());
            }
            "func_door" | "func_movelinear" | "func_door_rotating" | "func_brush" | "func_wall_toggle" | "func_physbox" | "func_rotating" | "func_tracktrain" | "func_breakable_surf" => {
                let Some(m) = model else { continue };
                let mut solids = model_boxes.remove(&m).unwrap_or_default();
                let mut speed = num(e, "speed", 100.0).max(1.0) * 0.0254;
                let mut shift = Vec3::ZERO;
                let mut swing = None;
                // A door shuts itself `wait` seconds after it's open (-1: it stays).
                let reset = matches!(class, "func_door" | "func_door_rotating").then(|| num(e, "wait", 4.0)).filter(|w| *w >= 0.0);
                let (path, start, mk, present) = match class {
                    "func_door_rotating" => {
                        // Turns `distance` degrees about its origin (Source's Z, or X or Y by its
                        // flags; the other way with Reverse), at `speed` degrees a second. Its
                        // path is how far round it's turned.
                        let f = flags(e);
                        let axis = if f & 64 != 0 { Vec3::X } else if f & 128 != 0 { Vec3::Y } else { Vec3::Z };
                        let angle = num(e, "distance", 90.0).to_radians() * if f & 2 != 0 { -1.0 } else { 1.0 };
                        swing = Some(Swing { pivot: origin(e).unwrap_or(Vec3::ZERO), axis: to_ours(axis).normalize(), angle });
                        speed = num(e, "speed", 100.0).max(1.0).to_radians();
                        (vec![Vec3::ZERO, Vec3::X * angle.abs()], usize::from(f & 1 != 0), io::MoverKind::Door, true)
                    }
                    "func_door" | "func_movelinear" => {
                        let (p, y, _) = angles(e, "movedir");
                        let dir = direction(p, y);
                        let travel = if class == "func_movelinear" {
                            num(e, "movedistance", 100.0) * 0.0254
                        } else {
                            let size = mb.map(|b| b.size()).unwrap_or(Vec3::ONE);
                            (size.dot(dir.abs()) - num(e, "lip", 0.0) * 0.0254).max(0.0)
                        };
                        let open = flags(e) & 1 != 0 && class == "func_door";
                        (vec![Vec3::ZERO, dir * travel], usize::from(open), io::MoverKind::Door, true)
                    }
                    "func_tracktrain" => {
                        // (The extractor put it on its first path node; models riding it go too.)
                        // Its path: from the node it starts on, following each node's target.
                        let mut pts = Vec::new();
                        let mut names = Vec::new();
                        let mut at = key(e, "target").map(str::to_ascii_lowercase);
                        while let Some(n) = at.take() {
                            if names.contains(&n) {
                                break;
                            }
                            let Some(node) = named.get(&n) else { break };
                            let Some(p) = origin(node) else { break };
                            pts.push(p);
                            names.push(n);
                            at = key(node, "target").map(str::to_ascii_lowercase);
                        }
                        let first = pts.first().copied().unwrap_or(Vec3::ZERO);
                        for (k, n) in names.into_iter().enumerate() {
                            path_nodes.insert(n, (movers, k));
                        }
                        let path = if pts.is_empty() { vec![Vec3::ZERO] } else { pts.iter().map(|p| *p - first).collect() };
                        if let (Some(p0), Some(o)) = (pts.first(), origin(e)) {
                            let height = num(e, "height", 0.0) * 0.0254;
                            shift = *p0 + Vec3::Y * height - o;
                        }
                        (path, 0, io::MoverKind::Train, true)
                    }
                    _ => {
                        let present = !start_disabled;
                        if class == "func_brush" && key(e, "Solidity") == Some("1") {
                            solids.clear();
                        }
                        (vec![Vec3::ZERO], 0, io::MoverKind::Brush, present)
                    }
                };
                // (A train runs at its top speed; an elevator's is around 65 units a second.)
                let speed = if class == "func_tracktrain" { num(e, "startspeed", 0.0).max(num(e, "speed", 0.0)).max(64.0) * 0.0254 } else { speed };
                // An elevator's floor is its model's (Portal's round elevator, 2.5 m across):
                // a slab where that sits.
                for p in &ents {
                    let rides = key(p, "parentname").is_some_and(|n| !name.is_empty() && n.eq_ignore_ascii_case(&name));
                    if rides && key(p, "model").is_some_and(|m| m.eq_ignore_ascii_case("models/props/round_elevator_body.mdl")) {
                        if let Some(o) = origin(p) {
                            let c = o + shift;
                            solids.push(Solid { b: Aabb::new(c + Vec3::new(-1.75, -0.2, -1.75), c + Vec3::new(1.75, 0.0, 1.75)), mat: Mat::Black });
                        }
                    }
                }
                l.items.push(Item::Mover(MoverDef { model: m, boxes: solids, path, speed, start, present, shift, inputs: None, shuttle: false, swing, parent: None, follows: false, reset }));
                kind = io::Kind::Mover { index: movers, kind: mk };
                movers += 1;
            }
            "trigger_portal_cleanser" => {
                if let Some(b) = mb {
                    let s = sig();
                    l.items.push(Item::Fizzler { b, inputs: Inputs::on(s) });
                    initial.push((s, !start_disabled));
                    kind = io::Kind::Switch { sig: s };
                    l.hidden_models.extend(model);
                }
            }
            "trigger_once" | "trigger_multiple" | "trigger_look" => {
                if let Some(b) = mb {
                    let f = flags(e);
                    // A filter that lets only pellets through: Portal's pellet catchers.
                    let filter = key(e, "filtername").and_then(|f| named.get(&f.to_ascii_lowercase()));
                    let pellets = filter.is_some_and(|f| key(f, "classname") == Some("filter_combineball_type"));
                    // And one that lets only rockets through (a rocket turret's).
                    let rockets = filter.is_some_and(|f| key(f, "filterclass").is_some_and(|c| c.eq_ignore_ascii_case("rocket_turret_projectile")) && key(f, "Negated") != Some("1"));
                    let (players, cubes) = if pellets || rockets { (false, false) } else { (f & 1 != 0 || f & 64 != 0 || class == "trigger_look", f & 8 != 0 || f & 64 != 0) };
                    let enabled = !start_disabled;
                    kind = match buttons.iter().position(|bt| class == "trigger_multiple" && b.overlaps(&Aabb::new(bt.min - Vec3::splat(0.6), bt.max + Vec3::splat(0.6)))) {
                        Some(button) => io::Kind::ButtonTrigger { button, enabled, players, cubes, on: false },
                        None => io::Kind::Trigger { b, once: class != "trigger_multiple", enabled, players, cubes, pellets, rockets, player_in: false, cubes_in: Vec::new() },
                    };
                }
            }
            "prop_portal" => {
                if let Some(at) = origin(e) {
                    let (p, y, r) = angles(e, "angles");
                    let (normal, up) = angle_vectors(p, y, r);
                    let color = if key(e, "PortalTwo") == Some("1") { PortalColor::Orange } else { PortalColor::Blue };
                    kind = io::Kind::Portal { color, portal: Portal::new(at, normal, up), start: key(e, "Activated") == Some("1") };
                }
            }
            "npc_portal_turret_floor" => {
                if let Some(at) = origin(e) {
                    l.items.push(Item::Turret { at, yaw: angles(e, "angles").1.to_radians() });
                }
            }
            "npc_rocket_turret" => {
                if let Some(at) = origin(e) {
                    let s = sig();
                    l.items.push(Item::RocketTurret { at, yaw: angles(e, "angles").1.to_radians(), inputs: Inputs::on(s), mover: None });
                    // (Its first flag: it starts off.)
                    initial.push((s, flags(e) & 1 == 0));
                    kind = io::Kind::Switch { sig: s };
                }
            }
            "point_energy_ball_launcher" => {
                if let Some(at) = origin(e) {
                    let (p, y, r) = angles(e, "angles");
                    let s = sig();
                    let index = l.items.iter().filter(|i| matches!(i, Item::PelletLauncher { .. })).count();
                    // Its own speed (Source picks between its min and max), lifetimes (-1: for
                    // good) and reload (-1, or no balls of its own: only on LaunchBall).
                    let life = num(e, "BallLifetime", 12.0);
                    let respawn = num(e, "ballrespawntime", 2.0);
                    let launch = Launch {
                        speed: (num(e, "minspeed", 275.0) + num(e, "maxspeed", 275.0)) * 0.5 * 0.0254,
                        after_portal: num(e, "MinLifeAfterPortal", 6.0),
                        reload: (respawn >= 0.0 && num(e, "ballcount", 1.0) > 0.0).then_some(respawn),
                    };
                    let life = if life < 0.0 { f32::INFINITY } else { life };
                    l.items.push(Item::PelletLauncher { at, dir: angle_vectors(p, y, r).0, inputs: Inputs::on(s), life, launch });
                    initial.push((s, !start_disabled));
                    kind = io::Kind::Launcher { sig: s, index };
                }
            }
            "logic_relay" => kind = io::Kind::Relay { enabled: !start_disabled, once: flags(e) & 1 != 0 },
            "logic_timer" => {
                let random = (key(e, "UseRandomTime") == Some("1")).then(|| (num(e, "LowerRandomBound", 1.0), num(e, "UpperRandomBound", 1.0)));
                let refire = num(e, "RefireTime", 1.0);
                kind = io::Kind::Timer { enabled: !start_disabled, refire, next: refire, random };
            }
            "math_counter" => kind = io::Kind::Counter { value: num(e, "startvalue", 0.0), min: num(e, "min", 0.0), max: num(e, "max", 0.0) },
            "logic_case" => kind = io::Kind::Case { cases: (1..=16).map(|k| key(e, &format!("Case{k:02}")).unwrap_or("").to_string()).collect() },
            "logic_auto" => kind = io::Kind::Auto,
            "info_landmark" if !name.is_empty() => {
                if let Some(at) = origin(e) {
                    l.landmarks.push((name.to_ascii_lowercase(), at));
                }
            }
            "logic_choreographed_scene" => {
                let file = key(e, "SceneFile").unwrap_or("").replace('\\', "/").to_ascii_lowercase();
                let SceneInfo { length, lines, triggers } = scenes.get(&file).cloned().unwrap_or_default();
                kind = io::Kind::Scene { lines, length, triggers };
            }
            "trigger_changelevel" => {
                kind = io::Kind::ChangeLevel {
                    map: key(e, "map").unwrap_or("").to_ascii_lowercase(),
                    landmark: key(e, "landmark").unwrap_or("").to_ascii_lowercase(),
                    // Walked into, unless it's flagged not to be (the escape's way on).
                    touch: mb.filter(|_| flags(e) & 2 == 0),
                    armed: false,
                }
            }
            "prop_dynamic" | "prop_dynamic_override" if !name.is_empty() => kind = io::Kind::Animated,
            "env_sprite" if !name.is_empty() => kind = io::Kind::Sprite,
            "env_texturetoggle" => kind = io::Kind::TextureToggle { target: key(e, "target").unwrap_or("").to_ascii_lowercase() },
            // The maker puts the template where it is (turned as it's turned): the cube keeps
            // its place relative to the template, so it sits on the floor as the map's had it.
            "env_entity_maker" => {
                if let (Some(maker), Some((cube, template, k))) = (origin(e), key(e, "EntityTemplate").and_then(&template_cube)) {
                    let turn = glam::Quat::from_rotation_y(angles(e, "angles").1.to_radians());
                    let at = maker + turn * (cube - template);
                    // (Where that grazes a wall, a dropper's chute, Source's physics slides off it
                    // and boxes catch on it: in from the wall, towards the maker, till it's clear.)
                    let clear = |p: Vec3| !l.solids.iter().any(|s| s.b.overlaps(&Aabb::from_center_size(p, Vec3::splat(0.9))));
                    let mut at = (0..=20).map(|k| at.lerp(maker, k as f32 / 20.0)).find(|p| clear(*p)).unwrap_or(at);
                    // Over a dropper, its funnel centres the cube (Source's physics slides it down
                    // to the hole; a box doesn't slide), so it drops through.
                    let dropper = ents.iter().filter(|d| key(d, "model").is_some_and(|m| m.to_ascii_lowercase().ends_with("box_dropper.mdl"))).filter_map(origin).find(|d| d.y < at.y && Vec3::new(d.x - at.x, 0.0, d.z - at.z).length() < 1.6);
                    if let Some(d) = dropper {
                        at = Vec3::new(d.x, at.y, d.z);
                    }
                    kind = io::Kind::Maker { at, kind: k };
                }
            }
            "point_template" => {
                if let Some((at, _, k)) = template_cube(&name) {
                    kind = io::Kind::Maker { at, kind: k };
                }
            }
            _ => {}
        }
        let outputs: Vec<io::Output> = e.iter().filter(|(k, _)| k.starts_with("On") || k.starts_with("Out")).filter_map(|(k, v)| io::Output::parse(k, v)).collect();
        io_ents.push(io::Ent { name, class: class.to_string(), kind, outputs, alive: true });
    }
    // Brushes parented to a moving one ride with it: the same path (or swing), driven by it.
    let mover_named: HashMap<String, usize> = io_ents.iter().filter_map(|e| match e.kind {
        io::Kind::Mover { index, .. } if !e.name.is_empty() => Some((e.name.to_ascii_lowercase(), index)),
        _ => None,
    }).collect();
    // (The mover something's on: its parent, or its parent's, through what doesn't move by
    // itself: a rocket turret's mount on the turret, on its lift.)
    let parent_mover = |e: &Entity| -> Option<usize> {
        let mut p = key(e, "parentname")?.to_ascii_lowercase();
        for _ in 0..8 {
            if let Some(&m) = mover_named.get(&p) {
                return Some(m);
            }
            p = key(named.get(&p)?, "parentname")?.to_ascii_lowercase();
        }
        None
    };
    let mut children = Vec::new();
    for (e, ent) in ents.iter().zip(&io_ents) {
        if let (io::Kind::Mover { index, .. }, Some(parent)) = (&ent.kind, parent_mover(e).as_ref())
            && parent != index
        {
            children.push((*index, *parent));
        }
    }
    // (Through parents' parents, up to the first that moves by itself: a launcher's panel on
    // its swinging hatch carries more brushes.)
    let defs: Vec<MoverDef> = l.items.iter().filter_map(|i| if let Item::Mover(m) = i { Some(m.clone()) } else { None }).collect();
    let moves = |k: usize| defs[k].path.len() >= 2 || defs[k].swing.is_some();
    let root = |mut k: usize| {
        for _ in 0..8 {
            match children.iter().find(|(c, _)| *c == k) {
                Some(&(_, p)) => k = p,
                None => break,
            }
            if moves(k) {
                break;
            }
        }
        k
    };
    let mut k = 0;
    for item in &mut l.items {
        let Item::Mover(m) = item else { continue };
        let still = m.path.len() < 2 && m.swing.is_none();
        if let Some(parent) = children.iter().any(|(c, _)| *c == k).then(|| root(k)).filter(|&p| p != k) {
            if still {
                let p = &defs[parent];
                m.parent = Some(parent);
                m.follows = true;
                m.path = p.path.clone();
                m.swing = p.swing;
                m.start = p.start;
                m.shift = p.shift;
            } else if let Some(&(_, direct)) = children.iter().find(|(c, _)| *c == k) {
                m.parent = Some(direct);
            }
        }
        k += 1;
    }
    // A pickup gun parented to a mover rides it.
    for (e, ent) in ents.iter().zip(&io_ents) {
        let io::Kind::GunPickup { index } = ent.kind else { continue };
        let Some(&mover) = key(e, "parentname").and_then(|p| mover_named.get(&p.to_ascii_lowercase())) else { continue };
        if let Some(Item::GunPickup { mover: m, .. }) = l.items.iter_mut().filter(|i| matches!(i, Item::GunPickup { .. })).nth(index) {
            *m = Some(mover);
        }
    }
    // A rocket turret on a lift rides it.
    let lifts: Vec<Option<usize>> = ents.iter().filter(|e| key(e, "classname") == Some("npc_rocket_turret") && origin(e).is_some()).map(parent_mover).collect();
    for (item, lift) in l.items.iter_mut().filter(|i| matches!(i, Item::RocketTurret { .. })).zip(lifts) {
        if let Item::RocketTurret { mover, .. } = item {
            *mover = lift;
        }
    }
    // What rockets hurt: whatever the map has hear of its damage. GLaDOS (her body hangs from
    // her origin, some 5 m tall) stops a rocket too.
    let mut damageable = Vec::new();
    for (k, e) in ents.iter().enumerate() {
        if !e.iter().any(|(o, _)| o.eq_ignore_ascii_case("OnTakeDamage")) {
            continue;
        }
        let Some(at) = origin(e) else { continue };
        if key(e, "model").is_some_and(|m| m.eq_ignore_ascii_case("models/props_bts/glados_body.mdl")) {
            let (at, radius) = (at - Vec3::Y * 1.6, 2.6);
            l.items.push(Item::RocketTarget { at, radius });
            damageable.push((k, at, radius));
        } else {
            damageable.push((k, at, 0.6));
        }
    }
    // Path nodes know their train.
    for ent in &mut io_ents {
        if ent.class == "path_track" {
            if let Some(&(mover, node)) = path_nodes.get(&ent.name.to_ascii_lowercase()) {
                ent.kind = io::Kind::PathNode { mover, node };
            }
        }
    }
    let signals = l.signal_count().max(next_sig);
    // Props that start disabled, and sprites that start off, aren't shown till they're turned on.
    let hidden = ents
        .iter()
        .zip(&io_ents)
        .enumerate()
        .filter(|(_, (e, ent))| match ent.kind {
            io::Kind::Animated => key(e, "StartDisabled") == Some("1"),
            // (A named sprite is off unless it starts on.)
            io::Kind::Sprite => flags(e) & 1 == 0,
            _ => false,
        })
        .map(|(k, _)| k)
        .collect();
    let mut logic = io::Io::new(io_ents, initial, signals);
    logic.hidden = hidden;
    logic.damageable = damageable;
    l.io = Some(logic);

    // The gun she starts with: what the map leaves by the start (none: Portal's chambers
    // before you find it, and Chamber 02, where it's on a pedestal).
    l.gun = spawn_gun;
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
        // The world's two boxes; the door moves (open: its width along +X, Source's default
        // direction, our -Z); the trigger isn't solid.
        assert_eq!(l.solids.len(), 2);
        let door = l.items.iter().find_map(|i| match i { Item::Mover(m) => Some(m), _ => None }).expect("the door");
        assert_eq!(door.boxes.len(), 1);
        assert!((door.path[1] - Vec3::new(0.0, 0.0, -1.0)).length() < 1e-4, "{:?}", door.path);
        assert_eq!(l.solids.iter().filter(|s| s.mat == Mat::White).count(), 1);
        assert!(l.items.iter().any(|i| matches!(i, Item::Goo { .. })));
        assert!(l.items.iter().any(|i| matches!(i, Item::Cube { kind: CubeKind::Storage, .. })));
        assert!((l.checkpoints[0].yaw - 90f32.to_radians()).abs() < 1e-5);
        assert_eq!(l.gun, Gun::NONE);
    }
}
