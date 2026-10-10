//! Portal's own models for the puzzle pieces (extracted by `portal_extract`): buttons,
//! pedestal buttons, cubes, droppers, faith plates, bridge and laser emitters, laser catchers
//! and relays, emancipation grill emitters and chamber doors. Each model's moving parts are
//! named glTF nodes (one per bone); they're found as the scene spawns and moved from the
//! simulation's state each frame. The faith plate's launch is Portal 2's own animation, played
//! at the simulation's clock. Anything without a model keeps its stand-in.

use std::collections::HashMap;

use bevy::animation::graph::AnimationNodeIndex;
use bevy::prelude::*;

use aperture::sim::{BRIDGE_HALF_WIDTH, BUTTON_TOP, CUBE_HALF};
use aperture::{CubeKind, Sim};

use crate::chamber::ChamberGeom;
use crate::extracted::Extracted;
use crate::Game;

/// Every model the game uses, by its extracted name.
pub const NAMES: &[&str] = &[
    // Portal with RTX's remasters, preferred where they were extracted.
    "rtx_storage_cube",
    "rtx_pedestal_button",
    "rtx_floor_button",
    "rtx_dropper_cover",
    "rtx_door",
    "rtx_fizzler_emitter",
    // Portal 2's.
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
    "faith_plate",
    "bridge_emitter",
    "laser_emitter",
    "laser_catcher",
    "laser_catcher_on",
    "laser_relay",
    "fizzler_emitter",
    "pellet_launcher",
    "pellet_catcher",
    "door_upper",
    "door_lower",
    "door_frame",
];

/// The faith plate's animations, in the order `portal_extract` writes them: thrown at an
/// angle, and straight up.
const PLATE_CLIPS: usize = 2;

#[derive(Resource, Default)]
pub struct PropModels {
    scenes: HashMap<&'static str, Handle<WorldAsset>>,
    /// The faith plate's launches (angled, straight up), as an animation graph.
    plate_launch: Option<(Handle<AnimationGraph>, [AnimationNodeIndex; PLATE_CLIPS])>,
}

impl PropModels {
    pub fn get(&self, name: &str) -> Option<Handle<WorldAsset>> {
        self.scenes.get(name).cloned()
    }

    /// The first of these models that was extracted, and which it was.
    fn first<'a>(&self, names: &[&'a str]) -> Option<(Handle<WorldAsset>, &'a str)> {
        names.iter().find_map(|n| Some((self.get(n)?, *n)))
    }
}

/// What a model stands for, so its parts can follow the simulation.
#[derive(Component, Clone, Copy, Debug, PartialEq)]
pub enum PropKind {
    /// A floor button, and which of its two looks (unlit, lit) this is: `None` for a model
    /// with only the one (Portal with RTX's).
    Button(usize, Option<bool>),
    Pedestal(usize),
    DropperCover(usize),
    /// A catcher, unlit or lit.
    Catcher(usize, bool),
    DoorUpper(usize),
    DoorLower(usize),
    /// Portal's round door (both leaves, `door_left` and `door_right`, which slide apart).
    DoorLeft(usize),
    Plate(usize),
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

pub fn setup(mut commands: Commands, extracted: Res<Extracted>, assets: Res<AssetServer>, mut graphs: ResMut<Assets<AnimationGraph>>) {
    let mut scenes = HashMap::new();
    for name in NAMES {
        if let Some(path) = extracted.model(name) {
            scenes.insert(*name, assets.load(GltfAssetLabel::Scene(0).from_asset(path)));
        }
    }
    if !scenes.is_empty() {
        info!("Portal models: {} of {}", scenes.len(), NAMES.len());
    }
    let plate_launch = extracted.model("faith_plate").map(|path| {
        let clips: [Handle<AnimationClip>; PLATE_CLIPS] = std::array::from_fn(|i| assets.load(GltfAssetLabel::Animation(i).from_asset(path.clone())));
        let (graph, nodes) = AnimationGraph::from_clips(clips);
        (graphs.add(graph), std::array::from_fn(|i| nodes[i]))
    });
    commands.insert_resource(PropModels { scenes, plate_launch });
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
    pub plates: bool,
    pub bridges: bool,
    pub lasers: bool,
    pub catchers: bool,
    pub relays: bool,
    pub fizzlers: bool,
    pub doors: bool,
    pub pellets: bool,
}

/// Spawn the models for the chamber's pieces; says which stand-ins they replace.
pub fn spawn(commands: &mut Commands, models: &PropModels, sim: &Sim) -> Replaced {
    let mut r = Replaced::default();
    // Floor buttons: Portal with RTX's remaster or Portal 2's (each 3.2 m across, at 60%),
    // sunk so the top is where a cube rests; Portal 2's comes unlit and lit. Cube-only buttons
    // are Portal 2's box sockets (Portal has none).
    let floor = match (models.get("rtx_floor_button"), models.get("floor_button"), models.get("floor_button_on")) {
        (Some(one), ..) => Some((one, None)),
        (None, Some(off), Some(on)) => Some((off, Some(on))),
        _ => None,
    };
    let socket = (models.get("cube_button"), models.get("cube_button_on"));
    if let (Some((floor, floor_on)), (Some(cube_off), Some(cube_on))) = (floor, socket) {
        r.buttons = true;
        for (i, b) in sim.buttons.iter().enumerate() {
            let (off, on, s, top) = if b.cube_only { (&cube_off, Some(&cube_on), 0.72, 0.356) } else { (&floor, floor_on.as_ref(), 0.6, 0.346) };
            let tf = Transform::from_translation(b.at + Vec3::Y * (BUTTON_TOP - top * s)).with_scale(Vec3::splat(s));
            match on {
                Some(on) => {
                    spawn_model(commands, off.clone(), tf, PropKind::Button(i, Some(false)));
                    spawn_model(commands, on.clone(), tf, PropKind::Button(i, Some(true)));
                }
                None => {
                    spawn_model(commands, off.clone(), tf, PropKind::Button(i, None));
                }
            }
        }
    }
    // Pedestal buttons: Portal with RTX's remaster or Portal 2's (the same size and parts).
    if let Some((scene, _)) = models.first(&["rtx_pedestal_button", "pedestal_button"]) {
        r.pedestals = true;
        for (i, p) in sim.pedestals.iter().enumerate() {
            spawn_model(commands, scene.clone(), Transform::from_translation(p.at).with_scale(Vec3::splat(0.9)), PropKind::Pedestal(i));
        }
    }
    // Droppers: the tube above the drop point, its iris underneath (Portal with RTX remastered
    // the iris, the same size and parts as Portal 2's, but not the tube).
    if let (Some(body), Some((cover, _))) = (models.get("box_dropper"), models.first(&["rtx_dropper_cover", "box_dropper_cover"])) {
        r.droppers = true;
        for (i, d) in sim.droppers.iter().enumerate() {
            let tf = Transform::from_translation(d.at + Vec3::Y * (CUBE_HALF + 0.1)).with_scale(Vec3::splat(0.5));
            spawn_model(commands, body.clone(), tf, PropKind::Static);
            spawn_model(commands, cover.clone(), tf, PropKind::DropperCover(i));
        }
    }
    // Faith plates: Portal 2's, its paddle on the plate and thrown towards the target. Like
    // Portal 2's, the plate lies square to the room, so it's turned to whichever way is
    // nearest.
    if let (Some(scene), Some(_)) = (models.get("faith_plate"), &models.plate_launch) {
        r.plates = true;
        for (i, p) in sim.plates.iter().enumerate() {
            spawn_model(commands, scene.clone(), plate_transform(p), PropKind::Plate(i));
        }
    }
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
    // High energy pellet launchers and catchers: Portal's, at 60%. A launcher's mouth (1.09 m
    // along the model's front) is where the pellet leaves; a catcher's opening faces its way.
    if let (Some(launcher), Some(catcher)) = (models.get("pellet_launcher"), models.get("pellet_catcher")) {
        r.pellets = true;
        let s = 0.6;
        for l in &sim.launchers {
            let (right, up) = frame_for(l.dir);
            let tf = Transform::from_translation(l.at - l.dir * 1.09 * s).with_rotation(basis(right, up, -l.dir)).with_scale(Vec3::splat(s));
            spawn_model(commands, launcher.clone(), tf, PropKind::Static);
        }
        for c in &sim.pellet_catchers {
            let (right, up) = frame_for(c.facing);
            let tf = Transform::from_translation(c.at).with_rotation(basis(right, up, c.facing)).with_scale(Vec3::splat(s));
            spawn_model(commands, catcher.clone(), tf, PropKind::Static);
        }
    }
    if let Some(scene) = models.get("laser_relay") {
        r.relays = true;
        for rl in &sim.relays {
            spawn_model(commands, scene.clone(), Transform::from_translation(rl.at), PropKind::Static);
        }
    }
    // Emancipation grills: an emitter post at each end, Portal with RTX's remaster (3.29 tall)
    // or Portal 2's (3.26), each from its middle.
    if let Some((scene, which)) = models.first(&["rtx_fizzler_emitter", "fizzler_emitter"]) {
        r.fizzlers = true;
        let height = if which == "rtx_fizzler_emitter" { 3.29 } else { 3.26 };
        for f in &sim.fizzlers {
            let s = f.b.size();
            let c = f.b.center();
            let along = if s.x >= s.z { Vec3::X } else { Vec3::Z };
            let half = s.x.max(s.z) * 0.5;
            let k = s.y / height;
            for sign in [-1.0f32, 1.0] {
                // The post's inside face towards the field.
                let inward = -along * sign;
                let tf = Transform::from_translation(c + along * sign * half).with_rotation(basis(inward, Vec3::Y, inward.cross(Vec3::Y))).with_scale(Vec3::splat(k));
                spawn_model(commands, scene.clone(), tf, PropKind::Static);
            }
        }
    }
    // Doors: Portal with RTX's remaster of Portal's round door, its frame fitted to the
    // doorway and its two leaves sliding apart into the wall.
    if let Some(door) = models.get("rtx_door") {
        r.doors = true;
        for (i, d) in sim.doors.iter().enumerate() {
            let s = d.b.size();
            let along = if s.x >= s.z { Vec3::X } else { Vec3::Z };
            let thick = s.x.min(s.z);
            let normal = along.cross(Vec3::Y);
            // The frame is 3.24 wide and 2.75 tall, from the middle of its bottom edge; the
            // leaves are 0.37 thick.
            let scale = Vec3::new(s.dot(along) / 3.244, s.y / 2.749, (thick / 0.37).max(0.6));
            let at = Vec3::new(d.b.center().x, d.b.min.y, d.b.center().z);
            let tf = Transform::from_translation(at).with_rotation(basis(along, Vec3::Y, normal)).with_scale(scale);
            spawn_model(commands, door.clone(), tf, PropKind::DoorLeft(i));
        }
    }
    // Else Portal 2's, upper and lower halves parting vertically, fitted to the doorway.
    else if let (Some(upper), Some(lower)) = (models.get("door_upper"), models.get("door_lower")) {
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

/// Which way a faith plate's model lies: its front (-Z, the way the paddle throws) along
/// whichever of the room's four directions is nearest its target's.
fn plate_facing(p: &aperture::sim::Plate) -> Quat {
    let to = p.target - p.at;
    let yaw = (-to.x).atan2(-to.z);
    Quat::from_rotation_y((yaw / std::f32::consts::FRAC_PI_2).round() * std::f32::consts::FRAC_PI_2)
}

/// Portal 2's faith plate: its frame's half size (a 64 by 128 inch hole in the floor, the
/// mechanism in a pit below it), and the middle of its paddle, in the model.
const PLATE_FRAME_HALF: Vec2 = Vec2::new(0.8128, 1.6256);
const PLATE_PADDLE: Vec3 = Vec3::new(0.03, 0.0, 0.33);

/// Where a faith plate's model goes: its paddle on the plate, its frame level with the floor.
fn plate_transform(p: &aperture::sim::Plate) -> Transform {
    let r = plate_facing(p);
    Transform::from_translation(p.at - r * PLATE_PADDLE).with_rotation(r)
}

/// The hole a faith plate's model needs in the floor it's set in (a little wider than its
/// frame, so the floor's edge doesn't fight the frame's walls).
pub fn plate_pit(p: &aperture::sim::Plate) -> faith_move::Aabb {
    let tf = plate_transform(p);
    let h = (tf.rotation * Vec3::new(PLATE_FRAME_HALF.x, 0.0, PLATE_FRAME_HALF.y)).abs() + Vec3::new(0.005, 0.0, 0.005);
    let c = tf.translation;
    faith_move::Aabb::new(Vec3::new(c.x - h.x, c.y - 2.0, c.z - h.z), Vec3::new(c.x + h.x, c.y + 0.01, c.z + h.z))
}

/// Which launch a plate plays: straight up when its target's (nearly) over it.
fn plate_clip(p: &aperture::sim::Plate) -> usize {
    let to = p.target - p.at;
    if Vec2::new(to.x, to.z).length() < 1.0 { 1 } else { 0 }
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
    // Each kind's models, best first, with each one's half size. Portal with RTX remastered the
    // storage cube only.
    let kinds: [(CubeKind, &[(&str, f32)]); 3] = [
        (CubeKind::Storage, &[("rtx_storage_cube", 0.503), ("metal_box", 0.514)]),
        (CubeKind::Companion, &[("companion_cube", 0.514)]),
        (CubeKind::Reflector, &[("reflection_cube", 0.47)]),
    ];
    let found = kinds.map(|(kind, options)| options.iter().find_map(|(n, half)| Some((kind, models.get(n)?, CUBE_HALF / half))));
    if found.iter().any(Option::is_none) {
        return None;
    }
    Some(found.map(|f| {
        let (kind, scene, s) = f.unwrap();
        let e = spawn_model(commands, scene, Transform::from_scale(Vec3::splat(s)), PropKind::Cube(slot, kind));
        commands.entity(e).insert(Visibility::Hidden);
        e
    }))
}

/// Tag a model's named parts as its scene spawns: the glTF nodes under its top node. (Not the
/// top node itself, nor the mesh entities under each node, which carry the node's name too:
/// tagged, they'd move along with it a second time.)
pub fn tag_parts(
    mut commands: Commands,
    added: Query<(Entity, &Name, &Transform), (Added<Name>, Without<Mesh3d>)>,
    parents: Query<&ChildOf>,
    roots: Query<&PropKind>,
) {
    for (e, name, tf) in &added {
        let mut cur = e;
        for depth in 0..8 {
            let Ok(p) = parents.get(cur) else { break };
            cur = p.parent();
            if let Ok(kind) = roots.get(cur) {
                if depth > 0 {
                    commands.entity(e).insert(PropPart { kind: *kind, name: name.as_str().to_string(), rest: *tf });
                }
                break;
            }
        }
    }
}

/// A faith plate model's animation player, and which plate it's for.
#[derive(Component)]
pub struct PlatePlayer(usize);

/// Give each faith plate's animation player (on its model's root, as the scene spawns) the
/// launch animations.
pub fn tag_plate_players(
    mut commands: Commands,
    added: Query<Entity, Added<AnimationPlayer>>,
    parents: Query<&ChildOf>,
    roots: Query<&PropKind>,
    models: Res<PropModels>,
) {
    let Some((graph, _)) = &models.plate_launch else { return };
    for e in &added {
        let mut cur = e;
        for _ in 0..8 {
            let Ok(p) = parents.get(cur) else { break };
            cur = p.parent();
            if let Ok(PropKind::Plate(i)) = roots.get(cur) {
                commands.entity(e).insert((AnimationGraphHandle(graph.clone()), PlatePlayer(*i)));
                break;
            }
        }
    }
}

/// Pose each faith plate at its launch animation's time since it last fired (its last frame,
/// at rest, once that's past). The simulation's clock drives it, so it pauses with the game.
pub fn animate_plates(game: Res<Game>, models: Res<PropModels>, clips: Res<Assets<AnimationClip>>, graphs: Res<Assets<AnimationGraph>>, mut players: Query<(&PlatePlayer, &mut AnimationPlayer)>) {
    let Some((graph, nodes)) = &models.plate_launch else { return };
    let Some(graph) = graphs.get(graph) else { return };
    for (pp, mut player) in &mut players {
        let Some(plate) = game.sim.plates.get(pp.0) else { continue };
        let node = nodes[plate_clip(plate)];
        let Some(clip) = graph.get(node).and_then(|n| match &n.node_type {
            AnimationNodeType::Clip(h) => clips.get(h),
            _ => None,
        }) else {
            continue;
        };
        if !player.is_playing_animation(node) {
            player.stop_all();
            player.start(node).pause();
        }
        if let Some(active) = player.animation_mut(node) {
            active.set_seek_time(plate.fired.clamp(0.0, clip.duration()));
        }
    }
}

/// Move the models' parts, and show the right look, from the simulation.
pub fn animate(game: Res<Game>, mut parts: Query<(&PropPart, &mut Transform)>, mut roots: Query<(&PropKind, &mut Visibility)>) {
    let sim = &game.sim;
    for (kind, mut vis) in &mut roots {
        let show = match *kind {
            PropKind::Button(i, Some(lit)) => sim.buttons.get(i).is_some_and(|b| b.pressed == lit),
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
            PropKind::DoorLeft(i) if p.name.starts_with("door_") => {
                // Each leaf slides its own width (and a little) into the wall.
                let d = &sim.doors[i];
                let k = 1.0 - (1.0 - d.open).powi(2);
                let dir = if p.name == "door_left" { -1.0 } else { 1.0 };
                tf.translation = p.rest.translation + Vec3::X * dir * k * 1.5;
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
