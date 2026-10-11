//! Portal's own models for the puzzle pieces (extracted by `portal_extract`): buttons,
//! pedestal buttons, cubes, droppers, faith plates, bridge and laser emitters, laser catchers
//! and relays, emancipation grill emitters and chamber doors. Each model's moving parts are
//! named glTF nodes (one per bone); they're found as the scene spawns and moved from the
//! simulation's state each frame. The faith plate's launch is Portal 2's own animation, played
//! at the simulation's clock. Anything without a model keeps its stand-in.

use std::collections::{HashMap, HashSet};

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
    "turret",
    "rocket_turret",
    "rocket",
    "portalgun_world",
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
    /// A pellet launcher's mouth: open, then close.
    launcher_fire: Option<(Handle<AnimationGraph>, [AnimationNodeIndex; 2])>,
    catcher_close: Option<(Handle<AnimationGraph>, AnimationNodeIndex)>,
    /// A sentry turret's: deploy, fire, retract.
    turret_clips: Option<(Handle<AnimationGraph>, [AnimationNodeIndex; 3])>,
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
    /// A sentry turret (standing, or falling over).
    Turret(usize),
    /// A portal gun on a pedestal, till she takes it.
    GunPickup(usize),
    /// The escape's rocket turret, aiming.
    RocketTurret(usize),
    /// A high energy pellet launcher (its mouth plays when it fires).
    PelletLauncher(usize),
    /// A high energy pellet catcher (it closes when it's caught one).
    PelletCatcher(usize),
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
    // pellet_launcher.glb writes idle_closed, open, close, idle_open. The mouth plays open
    // then close; the rest pose is already shut.
    let launcher_fire = extracted.model("pellet_launcher").map(|path| {
        let clips = [
            assets.load(GltfAssetLabel::Animation(1).from_asset(path.clone())),
            assets.load(GltfAssetLabel::Animation(2).from_asset(path)),
        ];
        let (graph, nodes) = AnimationGraph::from_clips(clips);
        (graphs.add(graph), [nodes[0], nodes[1]])
    });
    // pellet_catcher.glb writes idle, close, closed: it closes when it catches.
    let catcher_close = extracted.model("pellet_catcher").map(|path| {
        let (graph, node) = AnimationGraph::from_clip(assets.load(GltfAssetLabel::Animation(1).from_asset(path)));
        (graphs.add(graph), node)
    });
    // turret.glb writes idle (its rest pose, no keys), deploy, fire, retract.
    let turret_clips = extracted.model("turret").map(|path| {
        let clips: [Handle<AnimationClip>; 3] = std::array::from_fn(|i| assets.load(GltfAssetLabel::Animation(i + 1).from_asset(path.clone())));
        let (graph, nodes) = AnimationGraph::from_clips(clips);
        (graphs.add(graph), [nodes[0], nodes[1], nodes[2]])
    });
    commands.insert_resource(PropModels { scenes, plate_launch, launcher_fire, catcher_close, turret_clips });
}

fn spawn_model(commands: &mut Commands, scene: Handle<WorldAsset>, tf: Transform, kind: PropKind) -> Entity {
    commands.spawn((ChamberGeom, kind, WorldAssetRoot(scene), tf, Visibility::default())).id()
}

/// Where rocket turret `i`'s model is, turned along its aim (its barrel is along the model's
/// -X).
fn rocket_turret_pose(sim: &Sim, i: usize) -> Transform {
    let aim = sim.rocket_turrets[i].aim;
    let yaw = (-aim.x).atan2(-aim.z);
    let pitch = aim.y.clamp(-1.0, 1.0).asin();
    let turn = Quat::from_rotation_y(yaw) * Quat::from_rotation_x(pitch) * Quat::from_rotation_y(-std::f32::consts::FRAC_PI_2);
    Transform::from_translation(sim.rocket_turret_at(i)).with_rotation(turn)
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
    pub turrets: bool,
}

/// Spawn the models for the chamber's pieces; says which stand-ins they replace.
pub fn spawn(commands: &mut Commands, models: &PropModels, sim: &Sim) -> Replaced {
    let mut r = Replaced::default();
    // Floor buttons: Portal with RTX's remaster or Portal 2's (each 3.2 m across, at 84%),
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
            // Floor buttons read about 33-50% small at 0.6. 0.84 is 40% larger; the top stays
            // at BUTTON_TOP because the sink is `top * s`.
            let (off, on, s, top) = if b.cube_only { (&cube_off, Some(&cube_on), 0.72, 0.356) } else { (&floor, floor_on.as_ref(), 0.84, 0.346) };
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
    // A source map already places these as props, and those are the ones its logic animates.
    if sim.level.source.is_none() {
        if let (Some(launcher), Some(catcher)) = (models.get("pellet_launcher"), models.get("pellet_catcher")) {
            r.pellets = true;
            let s = 0.6;
            for (i, l) in sim.launchers.iter().enumerate() {
                let (right, up) = frame_for(l.dir);
                let tf = Transform::from_translation(l.at - l.dir * 1.09 * s).with_rotation(basis(right, up, -l.dir)).with_scale(Vec3::splat(s));
                spawn_model(commands, launcher.clone(), tf, PropKind::PelletLauncher(i));
            }
            // A catcher's opening is its model's front (-Z) once its idle stands it up, as Portal's
            // maps place it: facing out of its wall, the lens in its cup.
            for (i, c) in sim.pellet_catchers.iter().enumerate() {
                let (right, up) = frame_for(c.facing);
                let tf = Transform::from_translation(c.at).with_rotation(basis(right, up, -c.facing)).with_scale(Vec3::splat(s));
                spawn_model(commands, catcher.clone(), tf, PropKind::PelletCatcher(i));
            }
        }
    }
    if let Some(scene) = models.get("portalgun_world") {
        for i in 0..sim.gun_pickups.len() {
            let (at, turn) = sim.gun_pose(i);
            spawn_model(commands, scene.clone(), Transform::from_translation(at).with_rotation(turn), PropKind::GunPickup(i));
        }
    }
    if let Some(scene) = models.get("turret") {
        r.turrets = true;
        for (i, t) in sim.turrets.iter().enumerate() {
            spawn_model(commands, scene.clone(), Transform::from_translation(t.at).with_rotation(Quat::from_rotation_y(t.yaw)), PropKind::Turret(i));
        }
    }
    if let Some(scene) = models.get("rocket_turret") {
        for i in 0..sim.rocket_turrets.len() {
            spawn_model(commands, scene.clone(), rocket_turret_pose(sim, i), PropKind::RocketTurret(i));
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
                // The launcher's arms are posed by its animation. Tagging them would hold the
                // rest pose over it.
                if depth > 0 && !matches!(kind, PropKind::PelletLauncher(_)) {
                    commands.entity(e).insert(PropPart { kind: *kind, name: name.as_str().to_string(), rest: *tf });
                }
                // A turret's wings and guns turn to its aim.
                if let PropKind::Turret(i) = *kind {
                    if name.contains("Wing") || name.contains("Gun") {
                        commands.entity(e).insert(TurretGun(i));
                    }
                }
                break;
            }
        }
    }
}

/// A turret's wing or gun (its model's part), and which turret.
#[derive(Component)]
pub struct TurretGun(usize);

/// How far a turret's guns turn to its aim: either way (radians), and up or down.
const GUN_TURN: f32 = 0.5;
const GUN_TILT: f32 = 0.35;

/// Turn each standing turret's wings and guns a little towards where it's aiming (its sweep,
/// or her): over whatever its animation has them at, so after the animations run.
pub fn aim_turret_guns(game: Res<Game>, models: Res<PropModels>, mut parts: Query<(&TurretGun, &mut Transform)>) {
    // (Only over its animations, which set these parts afresh each frame: without them the
    // turn would add up frame on frame.)
    if models.turret_clips.is_none() {
        return;
    }
    for (g, mut tf) in &mut parts {
        let Some(t) = game.sim.turrets.get(g.0) else { continue };
        if !t.upright() {
            continue;
        }
        // Its aim in the model's own frame (it faces -Z, turned by its yaw).
        let local = Quat::from_rotation_y(-t.yaw) * t.aim;
        let yaw = (-local.x).atan2(-local.z).clamp(-GUN_TURN, GUN_TURN);
        let pitch = local.y.clamp(-1.0, 1.0).asin().clamp(-GUN_TILT, GUN_TILT);
        tf.rotation = Quat::from_euler(EulerRot::YXZ, yaw, pitch, 0.0) * tf.rotation;
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

/// Where a launcher's mouth is, `since` seconds after it fired: `open` first, then `close`,
/// held shut once both have played. `(opening, time within that clip)`.
pub fn launcher_mouth(since: f32, open: f32, close: f32) -> (bool, f32) {
    if since < open { (true, since.max(0.0)) } else { (false, (since - open).clamp(0.0, close.max(1e-3))) }
}

/// A pellet launcher's animation player, and which launcher it's for.
#[derive(Component)]
pub struct LauncherPlayer(usize);

/// A pellet catcher's animation player, and which catcher it's for.
#[derive(Component)]
pub struct CatcherPlayer(usize);

/// Give each pellet launcher's animation player its open and close clips, and each catcher's
/// its close.
pub fn tag_launcher_players(
    mut commands: Commands,
    added: Query<Entity, Added<AnimationPlayer>>,
    parents: Query<&ChildOf>,
    roots: Query<&PropKind>,
    models: Res<PropModels>,
) {
    for e in &added {
        let mut cur = e;
        for _ in 0..8 {
            let Ok(p) = parents.get(cur) else { break };
            cur = p.parent();
            if let (Ok(PropKind::Turret(i)), Some((graph, _))) = (roots.get(cur), &models.turret_clips) {
                commands.entity(e).insert((AnimationGraphHandle(graph.clone()), TurretPlayer { turret: *i, fired: false, lost: 0.0 }));
                break;
            }
            match (roots.get(cur), &models.launcher_fire, &models.catcher_close) {
                (Ok(PropKind::PelletLauncher(i)), Some((graph, _)), _) => {
                    commands.entity(e).insert((AnimationGraphHandle(graph.clone()), LauncherPlayer(*i)));
                    break;
                }
                (Ok(PropKind::PelletCatcher(i)), _, Some((graph, _))) => {
                    commands.entity(e).insert((AnimationGraphHandle(graph.clone()), CatcherPlayer(*i)));
                    break;
                }
                _ => {}
            }
        }
    }
}

/// A sentry turret's animation player: which turret, whether it's been firing (it folds its
/// wings away when it stops), and how long ago it stopped (seconds, on the simulation's clock).
#[derive(Component)]
pub struct TurretPlayer {
    turret: usize,
    fired: bool,
    lost: f32,
}

/// Pose each turret from what it's doing: shut while it searches (or she carries it); wings
/// out and guns firing once it's spotted her, or as it falls flailing; folding away when it's
/// lost her, and as it dies. The simulation's clock drives it.
pub fn animate_turrets(
    game: Res<Game>,
    models: Res<PropModels>,
    clips: Res<Assets<AnimationClip>>,
    graphs: Res<Assets<AnimationGraph>>,
    mut players: Query<(&mut TurretPlayer, &mut AnimationPlayer)>,
    mut clock: Local<Option<f32>>,
) {
    use aperture::sim::TurretState;
    let Some((graph, nodes)) = &models.turret_clips else { return };
    let Some(graph) = graphs.get(graph) else { return };
    let len = |i: usize| {
        graph.get(nodes[i]).and_then(|n| match &n.node_type {
            AnimationNodeType::Clip(h) => clips.get(h).map(|c| c.duration().max(1e-3)),
            _ => None,
        })
    };
    let (Some(deploy), Some(fire), Some(retract)) = (len(0), len(1), len(2)) else { return };
    let dt = clock.map_or(0.0, |c| (game.sim.time - c).max(0.0));
    *clock = Some(game.sim.time);
    for (mut tp, mut player) in &mut players {
        let Some(t) = game.sim.turrets.get(tp.turret) else { continue };
        // (Which clip, and how far into it.)
        let firing = |since: f32| if since < deploy { (0, since) } else { (1, (since - deploy) % fire) };
        let (clip, at) = match t.state {
            TurretState::Firing if !t.held => {
                tp.fired = true;
                tp.lost = 0.0;
                firing(t.t)
            }
            TurretState::Tipped => firing(t.t),
            TurretState::Retired => (2, t.t.min(retract)),
            _ if tp.fired && !t.held => {
                tp.lost += dt;
                if tp.lost >= retract {
                    tp.fired = false;
                }
                (2, tp.lost.min(retract))
            }
            _ => (2, retract),
        };
        let node = nodes[clip];
        if !player.is_playing_animation(node) {
            player.stop_all();
            player.start(node).pause();
        }
        if let Some(active) = player.animation_mut(node) {
            active.set_seek_time(at);
        }
    }
}

/// A catcher stays open till it's caught a pellet, then closes on it (once, held shut).
pub fn animate_catchers(game: Res<Game>, models: Res<PropModels>, mut players: Query<(&CatcherPlayer, &mut AnimationPlayer)>) {
    let Some((_, node)) = &models.catcher_close else { return };
    for (cp, mut player) in &mut players {
        let lit = game.sim.pellet_catchers.get(cp.0).is_some_and(|c| c.lit);
        if !player.is_playing_animation(*node) {
            player.start(*node).pause();
        }
        let Some(active) = player.animation_mut(*node) else { continue };
        match (lit, active.is_paused()) {
            (true, true) => {
                active.resume();
            }
            (false, false) => {
                active.pause().seek_to(0.0);
            }
            (false, true) if active.seek_time() != 0.0 => {
                active.seek_to(0.0);
            }
            _ => {}
        }
    }
}

/// Pose each launcher's mouth from how long ago it fired. The simulation's clock drives it.
pub fn animate_launchers(
    game: Res<Game>,
    models: Res<PropModels>,
    clips: Res<Assets<AnimationClip>>,
    graphs: Res<Assets<AnimationGraph>>,
    mut players: Query<(&LauncherPlayer, &mut AnimationPlayer)>,
) {
    let Some((graph, nodes)) = &models.launcher_fire else { return };
    let Some(graph) = graphs.get(graph) else { return };
    let len = |i: usize| {
        graph.get(nodes[i]).and_then(|n| match &n.node_type {
            AnimationNodeType::Clip(h) => clips.get(h).map(|c| c.duration().max(1e-3)),
            _ => None,
        })
    };
    let (Some(open), Some(close)) = (len(0), len(1)) else { return };
    for (lp, mut player) in &mut players {
        let Some(l) = game.sim.launchers.get(lp.0) else { continue };
        let (opening, t) = launcher_mouth(l.since_shot, open, close);
        let node = nodes[if opening { 0 } else { 1 }];
        if !player.is_playing_animation(node) {
            player.stop_all();
            player.start(node).pause();
        }
        if let Some(active) = player.animation_mut(node) {
            active.set_seek_time(t);
        }
    }
}

/// The RTX floor button's lamp is an orange circle in its emissive texture, and the file's
/// emissive strength (25) is then multiplied again for ray tracing, which clips it to a pale
/// warm white. Recolor that circle red and bring the strength down to something the tonemap
/// can still show as red. Cyan ticks on the same texture stay. The button face's own red
/// overlay is a different material and is left alone.
pub fn tint_floor_buttons(
    meshes: Query<(Entity, &MeshMaterial3d<StandardMaterial>)>,
    parents: Query<&ChildOf>,
    roots: Query<&PropKind>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut images: ResMut<Assets<Image>>,
    mut done: Local<HashSet<AssetId<StandardMaterial>>>,
) {
    for (e, mat) in &meshes {
        let id = mat.id();
        if done.contains(&id) {
            continue;
        }
        let mut cur = e;
        let mut floor = false;
        for _ in 0..12 {
            if let Ok(PropKind::Button(_, None)) = roots.get(cur) {
                floor = true;
                break;
            }
            let Ok(p) = parents.get(cur) else { break };
            cur = p.parent();
        }
        if !floor {
            continue;
        }
        let Some(m) = materials.get(id) else { continue };
        let Some(tex) = m.emissive_texture.clone() else {
            done.insert(id);
            continue;
        };
        // The loader writes the factor when the model arrives. Black is that material a frame early.
        let factor = m.emissive.to_vec3();
        if factor.length_squared() < 1e-4 {
            continue;
        }
        let Some(img) = images.get(&tex) else { continue };
        let red = match floor_lamp_texture(img) {
            // Not uploaded yet.
            LampImage::Wait => continue,
            // Loaded, and it isn't the lamp (the face overlay is red across the whole image).
            LampImage::Leave => {
                done.insert(id);
                continue;
            }
            LampImage::Red(img) => img,
        };
        let handle = images.add(red);
        if let Some(mut m) = materials.get_mut(id) {
            m.emissive_texture = Some(handle);
            // Above 40 the ray-trace scale has already been applied (25 × 1000). Either way the
            // lamp should land near 1100, which stays red instead of clipping to white.
            let mag = if factor.x > 40.0 { 1100.0 } else { 1.1 };
            m.emissive = LinearRgba::rgb(mag, mag, mag);
        }
        done.insert(id);
    }
}

/// What to do with one floor-button emissive image.
enum LampImage {
    /// Pixels aren't available yet.
    Wait,
    /// Loaded, and this isn't the lamp.
    Leave,
    /// The lamp, with its warm circle turned red.
    Red(Image),
}

/// The floor lamp's emissive image is mostly black, with one warm circle and some cyan ticks.
fn floor_lamp_texture(src: &Image) -> LampImage {
    let Some(data) = src.data.as_ref() else { return LampImage::Wait };
    let w = src.texture_descriptor.size.width as usize;
    let h = src.texture_descriptor.size.height as usize;
    let Some(pixels) = w.checked_mul(h) else { return LampImage::Leave };
    // One RGBA8 layer (mips, if present, follow and are the same layout).
    if pixels == 0 || data.len() < pixels * 4 || data.len() % 4 != 0 {
        return LampImage::Leave;
    }
    // The circle is orange. The little screen on the rim is cyan, and at the file's strength
    // it clipped to the same pale warm white. Both are this lamp.
    let lit = |px: &[u8]| {
        let (r, g, b) = (px[0] as u16, px[1] as u16, px[2] as u16);
        let warm = r > 40 && r + 20 > g && g + 15 >= b && r > b + 25;
        let cool = b > 70 && b + 10 >= g && g > r + 10 && b > r + 15;
        warm || cool
    };
    let lit_count = data.chunks_exact(4).filter(|px| lit(px)).count();
    // The lamp is a black image with one circle and a few ticks. The button face's overlay
    // is warm across the whole image.
    if lit_count < 1000 || lit_count * 8 > data.len() / 4 {
        return LampImage::Leave;
    }
    let mut out = src.clone();
    let Some(bytes) = out.data.as_mut() else { return LampImage::Wait };
    for px in bytes.chunks_exact_mut(4) {
        if lit(px) {
            let v = px[0].max(px[1]).max(px[2]) as u16;
            px[0] = v as u8;
            px[1] = (v / 12) as u8;
            px[2] = (v / 60) as u8;
        }
    }
    LampImage::Red(out)
}

/// Move the models' parts, and show the right look, from the simulation.
pub fn animate(game: Res<Game>, mut parts: Query<(&PropPart, &mut Transform)>, mut roots: Query<(&PropKind, &mut Visibility, &mut Transform), Without<PropPart>>) {
    let sim = &game.sim;
    for (kind, mut vis, mut tf) in &mut roots {
        // A turret falls over the way it was knocked.
        if let PropKind::Turret(i) = *kind {
            if let Some(t) = sim.turrets.get(i) {
                let angle = t.fallen * std::f32::consts::FRAC_PI_2;
                let tilt = if t.fall_dir == Vec3::ZERO { Quat::IDENTITY } else { Quat::from_axis_angle(Vec3::Y.cross(t.fall_dir).normalize(), angle) };
                // (Where it is now: one can drop through a portal in the floor under it. Lying
                // down it's raised by its half-thickness, so its back's on the floor, not in it,
                // as its box in the simulation lies.)
                tf.translation = t.at + Vec3::Y * aperture::sim::TURRET_HALF * angle.sin();
                tf.rotation = tilt * Quat::from_rotation_y(t.yaw);
            }
            continue;
        }
        if let PropKind::RocketTurret(i) = *kind {
            if i < sim.rocket_turrets.len() {
                *tf = rocket_turret_pose(sim, i);
            }
            continue;
        }
        // A pedestal's gun turns as its pedestal does, and is gone once she's taken it.
        if let PropKind::GunPickup(i) = *kind {
            if i < sim.gun_pickups.len() {
                let (at, turn) = sim.gun_pose(i);
                (tf.translation, tf.rotation) = (at, turn);
                let want = if sim.gun_pickups[i].taken { Visibility::Hidden } else { Visibility::Inherited };
                if *vis != want {
                    *vis = want;
                }
            }
            continue;
        }
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
