//! First-person arms, animated procedurally from the movement state.
//!
//! The arms live on their own render layer, drawn by a second camera on top of
//! the world with its depth cleared, so they never clip into walls and don't
//! stretch with the sprint FOV kick. Each arm is a forearm + hand "stick"
//! posed in camera space; a few poses (ledge hang) are anchored in the world.

use std::f32::consts::{FRAC_PI_2, PI};

use bevy::camera::visibility::RenderLayers;
use bevy::light::NotShadowCaster;
use bevy::prelude::*;
use faith_move::{Controller, Event as MoveEvent, State as MoveState, TraverseKind};

use crate::{Game, PlayerCamera};

pub const VIEWMODEL_LAYER: usize = 1;

#[derive(Component)]
pub struct ViewmodelCamera;

#[derive(Component)]
pub struct Arm {
    /// +1 right arm, -1 left arm.
    side: f32,
    pos: Vec3,
    rot: Quat,
}

#[derive(Resource, Default)]
pub struct ViewmodelState {
    sway: Vec2,
    last_yaw: f32,
    last_pitch: f32,
    dodge: Option<(Vec3, f32)>,
    time: f32,
}

pub fn setup(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let layer = RenderLayers::layer(VIEWMODEL_LAYER);
    let mat = |c: Color, rough: f32| StandardMaterial { base_color: c, perceptual_roughness: rough, ..default() };
    let skin = materials.add(mat(Color::srgb(0.80, 0.60, 0.50), 0.7));
    let sleeve = materials.add(mat(Color::srgb(0.10, 0.10, 0.12), 0.9));
    let glove = materials.add(mat(Color::srgb(0.16, 0.15, 0.16), 0.8));
    let band = materials.add(StandardMaterial {
        base_color: Color::srgb(0.86, 0.10, 0.07),
        emissive: LinearRgba::rgb(0.15, 0.0, 0.0),
        perceptual_roughness: 0.5,
        ..default()
    });

    let forearm = meshes.add(Capsule3d::new(0.036, 0.24));
    let upper = meshes.add(Capsule3d::new(0.045, 0.12));
    let wrist_band = meshes.add(Cylinder::new(0.041, 0.035));
    let palm = meshes.add(Cuboid::new(0.082, 0.03, 0.085));
    let fingers = meshes.add(Cuboid::new(0.078, 0.024, 0.075));
    let thumb = meshes.add(Cuboid::new(0.022, 0.022, 0.055));

    let cam = commands
        .spawn((
            ViewmodelCamera,
            crate::graphics::OverlayCamera,
            Camera3d::default(),
            Camera { order: 1, clear_color: ClearColorConfig::None, ..default() },
            // Fixed FOV: the arms shouldn't warp when sprinting widens the world FOV.
            Projection::Perspective(PerspectiveProjection { fov: 62f32.to_radians(), near: 0.01, ..default() }),
            Transform::default(),
            layer.clone(),
        ))
        .id();

    // Soft fill so the arms read even when the sun is behind them.
    commands.spawn((
        DirectionalLight { illuminance: 2500.0, shadow_maps_enabled: false, ..default() },
        Transform::from_xyz(-1.0, 2.0, 3.0).looking_at(Vec3::ZERO, Vec3::Y),
        layer.clone(),
    ));

    for side in [-1.0f32, 1.0] {
        // Local space: the wrist is at the origin; the hand points -Z, the
        // forearm runs back along +Z toward the elbow. Palm faces -Y.
        let along_z = Quat::from_rotation_x(FRAC_PI_2);
        let arm = commands
            .spawn((
                Arm { side, pos: Vec3::new(side * 0.3, -0.7, -0.3), rot: Quat::IDENTITY },
                Transform::default(),
                Visibility::default(),
                layer.clone(),
            ))
            .with_children(|p| {
                p.spawn((Mesh3d(forearm.clone()), MeshMaterial3d(skin.clone()),
                    Transform::from_xyz(0.0, 0.0, 0.16).with_rotation(along_z), layer.clone(), NotShadowCaster));
                p.spawn((Mesh3d(upper.clone()), MeshMaterial3d(sleeve.clone()),
                    Transform::from_xyz(0.0, 0.0, 0.36).with_rotation(along_z), layer.clone(), NotShadowCaster));
                p.spawn((Mesh3d(wrist_band.clone()), MeshMaterial3d(band.clone()),
                    Transform::from_xyz(0.0, 0.0, 0.035).with_rotation(along_z), layer.clone(), NotShadowCaster));
                p.spawn((Mesh3d(palm.clone()), MeshMaterial3d(glove.clone()),
                    Transform::from_xyz(0.0, 0.0, -0.045), layer.clone(), NotShadowCaster));
                p.spawn((Mesh3d(fingers.clone()), MeshMaterial3d(glove.clone()),
                    Transform::from_xyz(0.0, -0.008, -0.115).with_rotation(Quat::from_rotation_x(-0.35)), layer.clone(), NotShadowCaster));
                p.spawn((Mesh3d(thumb.clone()), MeshMaterial3d(glove.clone()),
                    Transform::from_xyz(-side * 0.048, -0.004, -0.035)
                        .with_rotation(Quat::from_rotation_y(side * 0.5)), layer.clone(), NotShadowCaster));
            })
            .id();
        commands.entity(cam).add_child(arm);
    }
    commands.insert_resource(ViewmodelState::default());
}

/// Forearm orientation from yaw (turn toward -X is positive), pitch (hand up
/// is positive) and roll (twist; positive rolls the palm toward +X... for the
/// right arm, inward).
fn orient(yaw: f32, pitch: f32, roll: f32) -> Quat {
    Quat::from_euler(EulerRot::YXZ, yaw, pitch, roll)
}

struct Pose {
    pos: Vec3,
    rot: Quat,
    /// How quickly to blend toward this pose.
    rate: f32,
}

fn pose(pos: Vec3, yaw: f32, pitch: f32, roll: f32) -> Pose {
    Pose { pos, rot: orient(yaw, pitch, roll), rate: 16.0 }
}

/// Arm pumping in the running gait.
fn run_pose(s: f32, phase: f32, amount: f32) -> Pose {
    // Opposite arm to leg: offset the two arms by half a cycle.
    let sw = (phase + if s > 0.0 { 0.0 } else { PI }).sin() * amount;
    let pos = Vec3::new(s * (0.25 - 0.03 * sw), -0.27 + 0.06 * sw + 0.02 * sw.abs(), -0.46 - 0.12 * sw);
    pose(pos, -s * 0.3, 0.30 + 0.40 * sw, s * 0.65)
}

fn hidden_pose(s: f32) -> Pose {
    pose(Vec3::new(s * 0.30, -0.62, -0.30), -s * 0.3, 0.2, s * 0.6)
}

fn target(
    s: f32,
    c: &Controller,
    step_phase: f32,
    gait: f32,
    time: f32,
    cam: &Transform,
    dodge: Option<(Vec3, f32)>,
) -> Pose {
    let tu = &c.tuning;
    let speed = c.horizontal_speed();
    let right = Vec3::new(c.yaw.cos(), 0.0, -c.yaw.sin());
    let to_cam = |world: Vec3| cam.rotation.inverse() * (world - cam.translation);
    let rot_to_cam = |world_rot: Quat| cam.rotation.inverse() * world_rot;

    match c.state {
        MoveState::Ground => {
            let amount = (speed / tu.sprint_speed).clamp(0.0, 1.0) * gait;
            if speed < 0.6 {
                hidden_pose(s)
            } else {
                run_pose(s, step_phase, 0.35 + 0.65 * amount)
            }
        }
        MoveState::Air => {
            if c.is_coiled() {
                return pose(Vec3::new(s * 0.2, -0.24, -0.46), -s * 0.2, 0.25, s * 0.9);
            }
            let fall = (-c.vel.y / 8.0).clamp(0.0, 1.0);
            let mut p = pose(
                Vec3::new(s * (0.30 + 0.06 * fall), -0.22 + 0.10 * fall, -0.42),
                -s * 0.25,
                0.45 + 0.3 * fall,
                s * 1.0,
            );
            if let Some((dir, age)) = dodge {
                // Swing both arms toward the dodge.
                let k = (1.0 - age / 0.4).clamp(0.0, 1.0);
                p.pos.x += dir.dot(right) * 0.14 * k;
                p.rate = 22.0;
            }
            p
        }
        MoveState::Slide { .. } => {
            if s < 0.0 {
                // Trailing hand down by the floor.
                pose(Vec3::new(-0.36, -0.27, -0.42), 0.45, -0.3, -0.6)
            } else {
                pose(Vec3::new(0.24, -0.24, -0.46), -0.3, 0.35, 0.8)
            }
        }
        MoveState::Roll { .. } => pose(Vec3::new(s * 0.14, -0.46, -0.30), -s * 0.4, -0.3, s * 0.8),
        MoveState::Stunned { .. } => pose(Vec3::new(s * 0.22, -0.34, -0.44), -s * 0.2, -0.25, s * 0.3),
        MoveState::WallRun { normal, t } => {
            // The wall is on the side the normal points away from.
            let wall_side = -normal.dot(right).signum();
            if s == wall_side {
                // Palm against the wall, slipping along it.
                let slip = ((t * 7.0).sin() * 0.03).abs();
                pose(Vec3::new(s * 0.40, -0.10, -0.44 + slip), -s * 0.25, 0.25, s * 1.45)
            } else {
                run_pose(s, t * 9.0, 0.8)
            }
        }
        MoveState::WallClimb { t, .. } => {
            let reach = (t * 16.0 + if s > 0.0 { 0.0 } else { PI }).sin();
            pose(
                Vec3::new(s * 0.17, 0.02 + 0.12 * reach.max(0.0), -0.36 - 0.04 * reach),
                -s * 0.15,
                1.25,
                s * 0.25,
            )
        }
        MoveState::WallClimbTurned { .. } => pose(Vec3::new(s * 0.40, -0.14, -0.40), -s * 0.35, 0.2, s * 1.25),
        MoveState::LedgeHang { normal, ledge_y, turned } => {
            if turned {
                return hidden_pose(s);
            }
            // Anchored in the world: hands on the lip of the ledge.
            let along = Vec3::new(normal.z, 0.0, -normal.x);
            let along = if along.dot(right) < 0.0 { -along } else { along };
            let grip = Vec3::new(c.feet.x, ledge_y + 0.02, c.feet.z)
                + -normal * (tu.half_width + 0.05)
                + along * s * 0.21;
            let dir = (-normal * 0.55 + Vec3::Y * 0.85).normalize();
            let world_rot = Quat::from_rotation_arc(Vec3::NEG_Z, dir) * Quat::from_rotation_z(s * 0.2);
            Pose { pos: to_cam(grip), rot: rot_to_cam(world_rot), rate: 30.0 }
        }
        MoveState::Traverse(tr) => {
            let k = tr.t.clamp(0.0, 1.0);
            match tr.kind {
                TraverseKind::Vault => {
                    if s < 0.0 {
                        // Lead hand plants on the obstacle and slides back past us.
                        pose(Vec3::new(-0.14, -0.22 - 0.06 * k, -0.52 + 0.28 * k), 0.3, -0.2, -0.4)
                    } else {
                        pose(Vec3::new(0.38, -0.12, -0.42), -0.35, 0.3, 1.0)
                    }
                }
                TraverseKind::Mantle | TraverseKind::PullUp => {
                    pose(Vec3::new(s * 0.22, -0.25 - 0.10 * k, -0.50 + 0.20 * k), -s * 0.2, -0.3, s * 0.2)
                }
                TraverseKind::SpringBoard => pose(Vec3::new(s * 0.32, -0.18, -0.44), -s * 0.25, 0.4, s * 0.9),
            }
        }
        MoveState::Vault(v) => {
            let k = (v.t / v.duration()).clamp(0.0, 1.0);
            if v.onto() {
                pose(Vec3::new(s * 0.22, -0.25 - 0.10 * k, -0.50 + 0.20 * k), -s * 0.2, -0.3, s * 0.2)
            } else if s < 0.0 {
                pose(Vec3::new(-0.14, -0.22 - 0.06 * k, -0.52 + 0.28 * k), 0.3, -0.2, -0.4)
            } else {
                pose(Vec3::new(0.38, -0.12, -0.42), -0.35, 0.3, 1.0)
            }
        }
        // Arms out to the sides, lying on your back.
        MoveState::LayOnGround { .. } | MoveState::SoftLand { .. } | MoveState::Stumble { .. } => {
            pose(Vec3::new(s * 0.40, -0.30, -0.25), -s * 0.4, -0.2, s * 0.8)
        }
        // Shoulder or boot into the door; reaching for someone (the app has no one to take down).
        MoveState::Barge { .. } | MoveState::AirBarge { .. } | MoveState::Vertigo { .. } | MoveState::Takedown { .. } | MoveState::StepUp { .. } | MoveState::RumpSlide { .. } | MoveState::GrabTransfer { .. } => pose(Vec3::new(s * 0.18, -0.15, -0.30), -s * 0.3, 0.2, s * 0.6),
        MoveState::Balance { lean, .. } => {
            // Arms out for balance, dipping on the side you're leaning to.
            pose(Vec3::new(s * 0.46, -0.16 - 0.10 * lean * s, -0.36), -s * 0.5, 0.1, s * 1.5)
        }
        // Both hands overhead on the cable / bar.
        MoveState::ZipLine { .. } | MoveState::Swing { .. } | MoveState::SwingJump { .. } | MoveState::IntoClimb { .. } | MoveState::Climb { .. } | MoveState::ClimbExit { .. } => {
            pose(Vec3::new(s * 0.12, 0.30, -0.30), -s * 0.1, 1.45, s * 0.1)
        }
    }
    .with_idle_float(time)
}

impl Pose {
    /// A tiny bit of life so arms are never perfectly still.
    fn with_idle_float(mut self, time: f32) -> Self {
        self.pos.y += (time * 1.7).sin() * 0.004;
        self
    }
}

pub fn animate(
    time: Res<Time>,
    game: Res<Game>,
    mut state: ResMut<ViewmodelState>,
    main_cam: Query<&Transform, (With<PlayerCamera>, Without<ViewmodelCamera>, Without<Arm>)>,
    mut vm_cam: Query<(&mut Transform, &mut Projection), (With<ViewmodelCamera>, Without<PlayerCamera>, Without<Arm>)>,
    mut arms: Query<(&mut Arm, &mut Transform), (Without<PlayerCamera>, Without<ViewmodelCamera>)>,
) {
    let dt = time.delta_secs();
    let Ok(cam) = main_cam.single() else { return };
    if let Ok((mut vt, mut proj)) = vm_cam.single_mut() {
        *vt = *cam;
        // Procedural arms use their own fixed FOV (the Mirror's Edge body
        // overrides this with the world projection when it's active).
        *proj = Projection::Perspective(PerspectiveProjection { fov: 62f32.to_radians(), near: 0.01, ..default() });
    }
    let c = &game.ctrl;
    let shot = game.shot;
    state.time += dt;

    for e in &c.events {
        if let MoveEvent::Dodge { dir } | MoveEvent::WallRunDodge { dir } = *e {
            state.dodge = Some((dir, 0.0));
        }
    }
    if let Some((_, age)) = &mut state.dodge {
        *age += dt;
    }
    if state.dodge.is_some_and(|(_, a)| a > 0.5) {
        state.dodge = None;
    }

    // Arms lag behind fast looks (sway), then catch up.
    let dyaw = shot.view.yaw - state.last_yaw;
    let dpitch = shot.view.pitch - state.last_pitch;
    state.last_yaw = shot.view.yaw;
    state.last_pitch = shot.view.pitch;
    let big_move = dyaw.abs() > 1.0 || dpitch.abs() > 1.0; // teleports, 180s, roll flips
    let target_sway = if big_move || dt <= 0.0 {
        Vec2::ZERO
    } else {
        Vec2::new(dyaw, dpitch).clamp_length_max(0.08) * 1.6
    };
    let cur = state.sway;
    state.sway = cur + (target_sway - cur) * (1.0 - (-12.0 * dt).exp());

    for (mut arm, mut tf) in &mut arms {
        let s = arm.side;
        let p = target(s, c, shot.step_phase, shot.gait, state.time, cam, state.dodge);
        let k = 1.0 - (-p.rate * dt).exp();
        arm.pos = arm.pos.lerp(p.pos, k);
        arm.rot = arm.rot.slerp(p.rot, k);
        let sway = Vec3::new(state.sway.x * 0.35, -state.sway.y * 0.3, 0.0);
        tf.translation = arm.pos + sway;
        tf.rotation = Quat::from_rotation_y(state.sway.x * 0.4) * arm.rot;
    }
}
