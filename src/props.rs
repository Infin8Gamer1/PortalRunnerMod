//! Portal's own models for the puzzle pieces (extracted by `portal_extract`): buttons,
//! pedestal buttons, cubes, droppers, bridge and laser emitters, laser catchers
//! and relays, emancipation grill emitters and chamber doors. Each model's moving parts are
//! named glTF nodes (one per bone); they're found as the scene spawns and moved from the
//! simulation's state each frame. Anything without a model keeps its stand-in.

use std::collections::HashMap;

use bevy::prelude::*;

use aperture::sim::{BRIDGE_HALF_WIDTH, BUTTON_TOP, CUBE_HALF};
use aperture::{CubeKind, Sim};

use crate::chamber::ChamberGeom;
use crate::extracted::Extracted;
use crate::Game;

/// Every model the game uses, by its extracted name.
pub const NAMES: &[&str] = &[
    "metal_box",
    "companion_cube",
    "reflection_cube",
    "floor_button",
    "floor_button_on",
    "cube_button",
    "cube_button_on",
    "pedestal_button",
    "box_dropper",
    "box_dropper_cover",
    "bridge_emitter",
    "laser_emitter",
    "laser_catcher",
    "laser_catcher_on",
    "laser_relay",
    "fizzler_emitter",
    "door_upper",
    "door_lower",
    "door_frame",
];

#[derive(Resource, Default)]
pub struct PropModels {
    scenes: HashMap<&'static str, Handle<WorldAsset>>,
}

impl PropModels {
    pub fn get(&self, name: &str) -> Option<Handle<WorldAsset>> {
        self.scenes.get(name).cloned()
    }

    pub fn has(&self, name: &str) -> bool {
        self.scenes.contains_key(name)
    }
}

/// What a model stands for, so its parts can follow the simulation.
#[derive(Component, Clone, Copy, Debug, PartialEq)]
pub enum PropKind {
    /// A floor button, and which of its two looks (unlit, lit) this is.
    Button(usize, bool),
    Pedestal(usize),
    DropperCover(usize),
    /// A catcher, unlit or lit.
    Catcher(usize, bool),
    DoorUpper(usize),
    DoorLower(usize),
    /// A cube in the pool: which prop slot, and the kind of cube this model is.
    Cube(usize, CubeKind),
    Static,
}

/// A named part of a prop's model, and where it rests.
#[derive(Component)]
pub struct PropPart {
    kind: PropKind,
    name: String,
    rest: Transform,
}

pub fn setup(mut commands: Commands, extracted: Res<Extracted>, assets: Res<AssetServer>) {
    let mut scenes = HashMap::new();
    for name in NAMES {
        if let Some(path) = extracted.model(name) {
            scenes.insert(*name, assets.load(GltfAssetLabel::Scene(0).from_asset(path)));
        }
    }
    if !scenes.is_empty() {
        info!("Portal models: {} of {}", scenes.len(), NAMES.len());
    }
    commands.insert_resource(PropModels { scenes });
}

fn spawn_model(commands: &mut Commands, scene: Handle<WorldAsset>, tf: Transform, kind: PropKind) -> Entity {
    commands.spawn((ChamberGeom, kind, WorldAssetRoot(scene), tf, Visibility::default())).id()
}

/// A rotation taking the model's right (+X), up (+Y) and back (+Z) to these.
fn basis(right: Vec3, up: Vec3, back: Vec3) -> Quat {
    Quat::from_mat3(&Mat3::from_cols(right.normalize(), up.normalize(), back.normalize()))
}

/// Which pieces the models replace, so the chamber doesn't also draw their stand-ins.
#[derive(Default, Clone, Copy)]
pub struct Replaced {
    pub buttons: bool,
    pub pedestals: bool,
    pub droppers: bool,
    pub bridges: bool,
    pub lasers: bool,
    pub catchers: bool,
    pub relays: bool,
    pub fizzlers: bool,
    pub doors: bool,
}

/// Spawn the models for the chamber's pieces; says which stand-ins they replace.
pub fn spawn(commands: &mut Commands, models: &PropModels, sim: &Sim) -> Replaced {
    let mut r = Replaced::default();
    // Floor buttons: Portal 2's (3.2 m across) at 60%, sunk so the top is where a cube rests.
    // Cube-only buttons are Portal 2's box sockets.
    let floor = (models.get("floor_button"), models.get("floor_button_on"));
    let socket = (models.get("cube_button"), models.get("cube_button_on"));
    if let ((Some(off), Some(on)), (Some(cube_off), Some(cube_on))) = (floor, socket) {
        r.buttons = true;
        for (i, b) in sim.buttons.iter().enumerate() {
            let (off, on, s, top) = if b.cube_only { (&cube_off, &cube_on, 0.72, 0.356) } else { (&off, &on, 0.6, 0.346) };
            let tf = Transform::from_translation(b.at + Vec3::Y * (BUTTON_TOP - top * s)).with_scale(Vec3::splat(s));
            spawn_model(commands, off.clone(), tf, PropKind::Button(i, false));
            spawn_model(commands, on.clone(), tf, PropKind::Button(i, true));
        }
    }
    if let Some(scene) = models.get("pedestal_button") {
        r.pedestals = true;
        for (i, p) in sim.pedestals.iter().enumerate() {
            spawn_model(commands, scene.clone(), Transform::from_translation(p.at).with_scale(Vec3::splat(0.9)), PropKind::Pedestal(i));
        }
    }
    // Droppers: the tube above the drop point, its iris underneath.
    if let (Some(body), Some(cover)) = (models.get("box_dropper"), models.get("box_dropper_cover")) {
        r.droppers = true;
        for (i, d) in sim.droppers.iter().enumerate() {
            let tf = Transform::from_translation(d.at + Vec3::Y * (CUBE_HALF + 0.1)).with_scale(Vec3::splat(0.5));
            spawn_model(commands, body.clone(), tf, PropKind::Static);
            spawn_model(commands, cover.clone(), tf, PropKind::DropperCover(i));
        }
    }
    // (Portal 2's faith plate is left out: its bind pose has the paddle sunk in its pit,
    // and only its animation raises it; the stand-in plate is used.)
    // Light bridge emitters, on the emitter's face, as wide as the bridge.
    if let Some(scene) = models.get("bridge_emitter") {
        r.bridges = true;
        for b in &sim.bridges {
            let w = b.dir.cross(b.up).normalize_or_zero();
            let s = BRIDGE_HALF_WIDTH * 2.0 / 3.0;
            let tf = Transform::from_translation(b.at - b.up * 0.03).with_rotation(basis(w, b.up, -b.dir)).with_scale(Vec3::splat(s * 1.15));
            spawn_model(commands, scene.clone(), tf, PropKind::Static);
        }
    }
    // Laser emitters and catchers: Portal 2's wall panels, shrunk to their boxes. An emitter
    // usually sits in a wall (Portal 2 sets the panel into a recess our walls don't have), so
    // its back goes on the wall and it stands out of it; a catcher's lens (0.762 in front of
    // the model's middle) is where the beam arrives.
    if let Some(scene) = models.get("laser_emitter") {
        r.lasers = true;
        for l in &sim.lasers {
            let (right, up) = frame_for(l.dir);
            let s = 0.6 / 1.64;
            let tf = Transform::from_translation(l.at + l.dir * 0.41 * s).with_rotation(basis(right, up, -l.dir)).with_scale(Vec3::splat(s));
            spawn_model(commands, scene.clone(), tf, PropKind::Static);
        }
    }
    if let (Some(off), Some(on)) = (models.get("laser_catcher"), models.get("laser_catcher_on")) {
        r.catchers = true;
        for (i, c) in sim.catchers.iter().enumerate() {
            let (right, up) = frame_for(c.facing);
            let s = 0.8 / 1.64;
            let tf = Transform::from_translation(c.at - c.facing * 0.762 * s).with_rotation(basis(right, up, -c.facing)).with_scale(Vec3::splat(s));
            spawn_model(commands, off.clone(), tf, PropKind::Catcher(i, false));
            spawn_model(commands, on.clone(), tf, PropKind::Catcher(i, true));
        }
    }
    if let Some(scene) = models.get("laser_relay") {
        r.relays = true;
        for rl in &sim.relays {
            spawn_model(commands, scene.clone(), Transform::from_translation(rl.at), PropKind::Static);
        }
    }
    // Emancipation grills: an emitter post at each end.
    if let Some(scene) = models.get("fizzler_emitter") {
        r.fizzlers = true;
        for f in &sim.fizzlers {
            let s = f.b.size();
            let c = f.b.center();
            let along = if s.x >= s.z { Vec3::X } else { Vec3::Z };
            let half = s.x.max(s.z) * 0.5;
            let k = s.y / 3.26;
            for sign in [-1.0f32, 1.0] {
                // The post's inside face towards the field.
                let inward = -along * sign;
                let tf = Transform::from_translation(c + along * sign * half).with_rotation(basis(inward, Vec3::Y, inward.cross(Vec3::Y))).with_scale(Vec3::splat(k));
                spawn_model(commands, scene.clone(), tf, PropKind::Static);
            }
        }
    }
    // Doors: Portal 2's, upper and lower halves parting vertically, fitted to the doorway.
    if let (Some(upper), Some(lower)) = (models.get("door_upper"), models.get("door_lower")) {
        r.doors = true;
        for (i, d) in sim.doors.iter().enumerate() {
            let s = d.b.size();
            let along = if s.x >= s.z { Vec3::X } else { Vec3::Z };
            let width = s.dot(along);
            let thick = s.x.min(s.z);
            let normal = along.cross(Vec3::Y);
            let scale = Vec3::new(width / 3.26, s.y / 3.26, (thick / 0.338).max(0.6));
            let seam = d.b.min.y + s.y * (1.73 / 3.26);
            let at = Vec3::new(d.b.center().x, seam, d.b.center().z) - normal * thick * 0.5;
            let tf = Transform::from_translation(at).with_rotation(basis(along, Vec3::Y, normal)).with_scale(scale);
            spawn_model(commands, upper.clone(), tf, PropKind::DoorUpper(i));
            spawn_model(commands, lower.clone(), tf, PropKind::DoorLower(i));
        }
    }
    r
}

/// A right and up for something facing `dir` (up is world up unless it faces straight up or
/// down).
fn frame_for(dir: Vec3) -> (Vec3, Vec3) {
    let up = if dir.y.abs() > 0.9 { Vec3::Z } else { Vec3::Y };
    let right = up.cross(-dir).normalize();
    (right, (-dir).cross(right).normalize())
}

/// Spawn the cube models for a pooled prop slot (one per kind; the pool shows the right one).
pub fn spawn_cubes(commands: &mut Commands, models: &PropModels, slot: usize) -> Option<[Entity; 3]> {
    let kinds = [(CubeKind::Storage, "metal_box", CUBE_HALF / 0.514), (CubeKind::Companion, "companion_cube", CUBE_HALF / 0.514), (CubeKind::Reflector, "reflection_cube", CUBE_HALF / 0.47)];
    if !kinds.iter().all(|(_, n, _)| models.has(n)) {
        return None;
    }
    Some(kinds.map(|(kind, name, s)| {
        let e = spawn_model(commands, models.get(name).unwrap(), Transform::from_scale(Vec3::splat(s)), PropKind::Cube(slot, kind));
        commands.entity(e).insert(Visibility::Hidden);
        e
    }))
}

/// Tag a model's named parts as its scene spawns.
pub fn tag_parts(
    mut commands: Commands,
    added: Query<(Entity, &Name, &Transform), Added<Name>>,
    parents: Query<&ChildOf>,
    roots: Query<&PropKind>,
) {
    for (e, name, tf) in &added {
        let mut cur = e;
        for _ in 0..8 {
            let Ok(p) = parents.get(cur) else { break };
            cur = p.parent();
            if let Ok(kind) = roots.get(cur) {
                commands.entity(e).insert(PropPart { kind: *kind, name: name.as_str().to_string(), rest: *tf });
                break;
            }
        }
    }
}

/// Move the models' parts, and show the right look, from the simulation.
pub fn animate(game: Res<Game>, mut parts: Query<(&PropPart, &mut Transform)>, mut roots: Query<(&PropKind, &mut Visibility)>) {
    let sim = &game.sim;
    for (kind, mut vis) in &mut roots {
        let show = match *kind {
            PropKind::Button(i, lit) => sim.buttons.get(i).is_some_and(|b| b.pressed == lit),
            PropKind::Catcher(i, lit) => sim.catchers.get(i).is_some_and(|c| c.lit == lit),
            _ => continue,
        };
        let want = if show { Visibility::Inherited } else { Visibility::Hidden };
        if *vis != want {
            *vis = want;
        }
    }
    for (p, mut tf) in &mut parts {
        match p.kind {
            PropKind::Button(i, _) if p.name.ends_with("_top") => {
                let down = if sim.buttons.get(i).is_some_and(|b| b.pressed) { 0.12 } else { 0.0 };
                tf.translation = p.rest.translation - Vec3::Y * down;
            }
            PropKind::Pedestal(i) if p.name == "Button" => {
                let down = if sim.pedestals.get(i).is_some_and(|s| s.left > 0.0) { 0.04 } else { 0.0 };
                tf.translation = p.rest.translation - Vec3::Y * down;
            }
            PropKind::DropperCover(i) if p.name.starts_with("Cover_") => {
                // The iris: each leaf slides out from the middle while it's open.
                let d = &sim.droppers[i];
                let open = if d.opened < 0.7 { (d.opened / 0.15).min(1.0) * (1.0 - ((d.opened - 0.5) / 0.2).clamp(0.0, 1.0)) } else { 0.0 };
                let out = Vec3::new(p.rest.translation.x, 0.0, p.rest.translation.z).normalize_or_zero();
                tf.translation = p.rest.translation + out * open * 1.0;
            }
            PropKind::DoorUpper(i) | PropKind::DoorLower(i) => {
                let d = &sim.doors[i];
                let k = 1.0 - (1.0 - d.open).powi(2);
                let dir = if matches!(p.kind, PropKind::DoorUpper(_)) { 1.0 } else { -1.0 };
                tf.translation = p.rest.translation + Vec3::Y * dir * k * 1.75;
            }
            _ => {}
        }
    }
}
