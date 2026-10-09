//! The portal gun in Faith's hands: drawn by its own camera over the world (like Portal's
//! viewmodel, at Portal's narrower field of view), with recoil, sway, a glowing core in the
//! colour last fired, shot tracers, and sparks where the shots land.

use bevy::camera::visibility::RenderLayers;
use bevy::light::NotShadowCaster;
use bevy::prelude::*;

use aperture::{PortalColor, SimEvent};

use crate::extracted::Extracted;
use crate::portal_render::color_of;
use crate::{Game, PlayerCamera};

/// The gun's own layer and camera.
pub const GUN_LAYER: usize = 3;

#[derive(Component)]
pub struct GunCamera;

/// The gun's root (child of the gun camera).
#[derive(Component)]
pub struct GunRoot;

/// The glowing core's material (it changes colour).
#[derive(Component)]
pub struct GunCore;

/// A model's scene (the extracted gun): its meshes go on the gun layer.
#[derive(Component)]
pub struct GunModel;

#[derive(Component)]
pub struct Tracer {
    from: Vec3,
    to: Vec3,
    t: f32,
    len: f32,
}

#[derive(Component)]
pub struct Spark {
    vel: Vec3,
    life: f32,
}

/// The beam from the gun to what it's holding.
#[derive(Component)]
pub struct HoldBeam;

#[derive(Resource)]
pub struct GunState {
    recoil: f32,
    sway: Vec2,
    last_yaw: f32,
    last_pitch: f32,
    core: [Handle<StandardMaterial>; 3],
    spark_mats: [Handle<StandardMaterial>; 2],
    spark_mesh: Handle<Mesh>,
    tracer_mesh: Handle<Mesh>,
    last: Option<PortalColor>,
    pub visible: bool,
    time: f32,
}

pub fn setup(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    extracted: Res<Extracted>,
    assets: Res<AssetServer>,
) {
    let layer = RenderLayers::layer(GUN_LAYER);
    let cam = commands
        .spawn((
            GunCamera,
            Camera3d::default(),
            Camera { order: 2, clear_color: ClearColorConfig::None, ..default() },
            Projection::Perspective(PerspectiveProjection { fov: 52f32.to_radians(), near: 0.01, ..default() }),
            Transform::default(),
            layer.clone(),
        ))
        .id();
    // Light for the gun alone.
    commands.spawn((DirectionalLight { illuminance: 6000.0, shadow_maps_enabled: false, ..default() }, Transform::from_xyz(-1.0, 3.0, 1.0).looking_at(Vec3::ZERO, Vec3::Y), layer.clone()));
    commands.spawn((DirectionalLight { illuminance: 1500.0, shadow_maps_enabled: false, ..default() }, Transform::from_xyz(2.0, -1.0, 2.0).looking_at(Vec3::ZERO, Vec3::Y), layer.clone()));

    let core = [
        materials.add(StandardMaterial { base_color: Color::srgb(0.9, 0.95, 1.0), unlit: true, ..default() }),
        materials.add(StandardMaterial { base_color: Color::LinearRgba(color_of(PortalColor::Blue) * 2.0), unlit: true, ..default() }),
        materials.add(StandardMaterial { base_color: Color::LinearRgba(color_of(PortalColor::Orange) * 2.0), unlit: true, ..default() }),
    ];
    let root = commands.spawn((GunRoot, Transform::default(), Visibility::default(), layer.clone())).id();
    commands.entity(cam).add_child(root);

    if let Some(path) = extracted.model("w_portalgun") {
        // Portal's gun (the world model: in metres, its prongs down -Y): turned to point
        // ahead, its back near the eye.
        let scene = assets.load(GltfAssetLabel::Scene(0).from_asset(path));
        let model = commands
            .spawn((
                GunModel,
                WorldAssetRoot(scene),
                Transform::from_xyz(0.0, 0.015, 0.2).with_rotation(Quat::from_rotation_x(std::f32::consts::FRAC_PI_2)).with_scale(Vec3::splat(0.62)),
                layer.clone(),
            ))
            .id();
        commands.entity(root).add_child(model);
        // Its light, between the prongs.
        let glow = commands
            .spawn((GunCore, Mesh3d(meshes.add(Sphere::new(0.016))), MeshMaterial3d(core[0].clone()), Transform::from_xyz(0.006, 0.0, -0.15), layer.clone(), NotShadowCaster))
            .id();
        commands.entity(root).add_child(glow);
    } else {
        // A stand-in: a white body, a dark grip, three prongs, and the core.
        let white = materials.add(StandardMaterial { base_color: Color::srgb(0.92, 0.93, 0.95), perceptual_roughness: 0.25, ..default() });
        let black = materials.add(StandardMaterial { base_color: Color::srgb(0.08, 0.08, 0.09), perceptual_roughness: 0.5, ..default() });
        let along = Quat::from_rotation_x(std::f32::consts::FRAC_PI_2);
        let parts: Vec<(Handle<Mesh>, Handle<StandardMaterial>, Transform)> = vec![
            (meshes.add(Capsule3d::new(0.055, 0.26)), white.clone(), Transform::from_xyz(0.0, 0.0, 0.0).with_rotation(along)),
            (meshes.add(Cylinder::new(0.065, 0.12)), black.clone(), Transform::from_xyz(0.0, 0.0, 0.12).with_rotation(along)),
            (meshes.add(Cuboid::new(0.04, 0.11, 0.05)), black.clone(), Transform::from_xyz(0.0, -0.08, 0.08).with_rotation(Quat::from_rotation_x(-0.3))),
            (meshes.add(Cylinder::new(0.035, 0.1)), black.clone(), Transform::from_xyz(0.0, 0.0, -0.2).with_rotation(along)),
        ];
        for (mesh, mat, tf) in parts {
            let e = commands.spawn((Mesh3d(mesh), MeshMaterial3d(mat), tf, layer.clone(), NotShadowCaster)).id();
            commands.entity(root).add_child(e);
        }
        let prong = meshes.add(Cuboid::new(0.012, 0.012, 0.1));
        for k in 0..3 {
            let a = k as f32 * std::f32::consts::TAU / 3.0 + 0.5;
            let tf = Transform::from_xyz(a.cos() * 0.045, a.sin() * 0.045, -0.29).with_rotation(Quat::from_rotation_x(0.0) * Quat::from_axis_angle(Vec3::new(-a.sin(), a.cos(), 0.0), 0.35));
            let e = commands.spawn((Mesh3d(prong.clone()), MeshMaterial3d(black.clone()), tf, layer.clone(), NotShadowCaster)).id();
            commands.entity(root).add_child(e);
        }
        let glow = commands
            .spawn((GunCore, Mesh3d(meshes.add(Cylinder::new(0.03, 0.14))), MeshMaterial3d(core[0].clone()), Transform::from_xyz(0.0, 0.0, -0.06).with_rotation(along), layer.clone(), NotShadowCaster))
            .id();
        commands.entity(root).add_child(glow);
    }

    let spark_mats = [color_of(PortalColor::Blue), color_of(PortalColor::Orange)]
        .map(|c| materials.add(StandardMaterial { base_color: Color::LinearRgba(c * 2.5), unlit: true, ..default() }));
    let beam_mat = materials.add(StandardMaterial {
        base_color: Color::srgba(0.8, 0.9, 1.0, 0.2),
        emissive: LinearRgba::rgb(0.4, 0.6, 1.0),
        unlit: true,
        alpha_mode: AlphaMode::Add,
        ..default()
    });
    commands.spawn((HoldBeam, Mesh3d(meshes.add(Cylinder::new(0.005, 1.0))), MeshMaterial3d(beam_mat), Transform::default(), Visibility::Hidden, NotShadowCaster));
    commands.insert_resource(GunState {
        recoil: 0.0,
        sway: Vec2::ZERO,
        last_yaw: 0.0,
        last_pitch: 0.0,
        core,
        spark_mats,
        spark_mesh: meshes.add(Cuboid::new(0.03, 0.03, 0.03)),
        tracer_mesh: meshes.add(Cylinder::new(0.008, 1.0)),
        last: None,
        visible: true,
        time: 0.0,
    });
}

/// Put the extracted gun's meshes on the gun layer as its scene spawns.
pub fn tag_model(
    mut commands: Commands,
    added: Query<Entity, Added<Mesh3d>>,
    parents: Query<&ChildOf>,
    models: Query<(), With<GunModel>>,
) {
    for e in &added {
        let mut cur = e;
        let mut hops = 0;
        while let Ok(p) = parents.get(cur) {
            cur = p.parent();
            hops += 1;
            if models.contains(cur) {
                commands.entity(e).insert((RenderLayers::layer(GUN_LAYER), NotShadowCaster));
                break;
            }
            if hops > 16 {
                break;
            }
        }
    }
}

#[allow(clippy::type_complexity, clippy::too_many_arguments)]
pub fn update(
    mut commands: Commands,
    time: Res<Time>,
    game: Res<Game>,
    mut st: ResMut<GunState>,
    main: Query<(&Transform, &Projection), (With<PlayerCamera>, Without<GunCamera>)>,
    mut gun_cam: Query<(&mut Transform, &mut Camera), (With<GunCamera>, Without<PlayerCamera>, Without<GunRoot>)>,
    mut root: Query<(&mut Transform, &mut Visibility), (With<GunRoot>, Without<GunCamera>, Without<PlayerCamera>)>,
    mut core: Query<&mut MeshMaterial3d<StandardMaterial>, With<GunCore>>,
    mut tracers: Query<(Entity, &mut Tracer, &mut Transform), (Without<GunRoot>, Without<GunCamera>, Without<PlayerCamera>, Without<Spark>, Without<HoldBeam>)>,
    mut sparks: Query<(Entity, &mut Spark, &mut Transform), (Without<GunRoot>, Without<GunCamera>, Without<PlayerCamera>, Without<Tracer>, Without<HoldBeam>)>,
    mut beam: Query<(&mut Transform, &mut Visibility), (With<HoldBeam>, Without<GunRoot>, Without<GunCamera>, Without<PlayerCamera>, Without<Tracer>, Without<Spark>)>,
) {
    let dt = time.delta_secs().min(0.1);
    st.time += dt;
    let Ok((main_tf, _)) = main.single() else { return };
    let c = &game.ctrl;
    if let Ok((mut tf, mut cam)) = gun_cam.single_mut() {
        *tf = *main_tf;
        cam.is_active = st.visible;
    }

    // Shots fired this frame.
    for e in &game.sim.events {
        if let SimEvent::Shot { color, from, to, .. } = *e {
            st.recoil = 1.0;
            st.last = Some(color);
            // The first leg starts at the gun's muzzle, not the eye.
            let muzzle = main_tf.translation + main_tf.rotation * Vec3::new(0.12, -0.12, -0.6);
            let start = if from.distance(main_tf.translation) < 0.5 { muzzle } else { from };
            let len = start.distance(to);
            commands.spawn((
                Tracer { from: start, to, t: 0.0, len },
                Mesh3d(st.tracer_mesh.clone()),
                MeshMaterial3d(st.spark_mats[color.index()].clone()),
                Transform::from_translation(start),
                NotShadowCaster,
            ));
        }
        let burst = match *e {
            SimEvent::PortalOpened { color, at, normal } => Some((color, at, normal, 18)),
            SimEvent::PortalFailed { color, at, normal } => Some((color, at, normal, 10)),
            _ => None,
        };
        if let Some((color, at, normal, n)) = burst {
            for k in 0..n {
                let a = k as f32 * 2.399 + st.time * 13.0;
                let side = normal.any_orthonormal_vector();
                let up = normal.cross(side);
                let dir = (normal * 1.2 + side * a.cos() + up * a.sin()).normalize();
                commands.spawn((
                    Spark { vel: dir * (2.5 + (k % 5) as f32), life: 0.45 + (k % 3) as f32 * 0.12 },
                    Mesh3d(st.spark_mesh.clone()),
                    MeshMaterial3d(st.spark_mats[color.index()].clone()),
                    Transform::from_translation(at + normal * 0.05),
                    NotShadowCaster,
                ));
            }
        }
    }
    for (e, mut tr, mut tf) in &mut tracers {
        tr.t += dt;
        // Portal's shots are quick: 100 m/s, with a short streak.
        let head = (tr.t * 100.0).min(tr.len);
        let tail = (tr.t * 100.0 - 3.0).clamp(0.0, tr.len);
        let d = (tr.to - tr.from).normalize_or_zero();
        let (a, b) = (tr.from + d * tail, tr.from + d * head);
        let len = a.distance(b).max(0.01);
        tf.translation = (a + b) * 0.5;
        tf.rotation = Quat::from_rotation_arc(Vec3::Y, d);
        tf.scale = Vec3::new(1.0, len, 1.0);
        if tail >= tr.len - 0.01 {
            commands.entity(e).despawn();
        }
    }
    for (e, mut sp, mut tf) in &mut sparks {
        sp.life -= dt;
        sp.vel.y -= 9.0 * dt;
        tf.translation += sp.vel * dt;
        tf.scale = Vec3::splat((sp.life * 2.0).clamp(0.0, 1.0));
        if sp.life <= 0.0 {
            commands.entity(e).despawn();
        }
    }

    // Recoil and sway: the gun lags the view a little and kicks when it fires.
    st.recoil = (st.recoil - dt * 5.0).max(0.0);
    let dyaw = c.yaw - st.last_yaw;
    let dpitch = c.pitch - st.last_pitch;
    st.last_yaw = c.yaw;
    st.last_pitch = c.pitch;
    let dyaw = if dyaw.abs() > 1.0 { 0.0 } else { dyaw };
    let target = Vec2::new(-dyaw, dpitch) * 2.5;
    st.sway = st.sway.lerp(target.clamp(Vec2::splat(-0.08), Vec2::splat(0.08)), (dt * 10.0).min(1.0));
    let speed = c.horizontal_speed();
    let bob_t = st.time * (6.0 + speed);
    let on_ground = matches!(c.state, faith_move::State::Ground);
    let bob = if on_ground { (speed / 7.0).min(1.0) } else { 0.0 };
    let k = st.recoil * st.recoil;
    if let Ok((mut tf, mut vis)) = root.single_mut() {
        tf.translation = Vec3::new(
            0.16 + st.sway.x * 0.3 + bob * 0.01 * bob_t.sin(),
            -0.15 + st.sway.y * 0.3 - bob * 0.008 * (bob_t * 2.0).sin().abs() - k * 0.01,
            -0.36 + k * 0.05,
        );
        tf.rotation = Quat::from_euler(EulerRot::YXZ, 0.05 + st.sway.x, k * 0.12 + st.sway.y, st.sway.x * 0.5);
        *vis = if st.visible { Visibility::Inherited } else { Visibility::Hidden };
    }
    let idx = match st.last {
        None => 0,
        Some(c) => 1 + c.index(),
    };
    for mut m in &mut core {
        if m.0 != st.core[idx] {
            m.0 = st.core[idx].clone();
        }
    }
    // The hold beam.
    if let Ok((mut tf, mut vis)) = beam.single_mut() {
        match game.sim.held.map(|h| game.sim.props[h].pos) {
            Some(p) if st.visible => {
                let muzzle = main_tf.translation + main_tf.rotation * Vec3::new(0.16, -0.17, -0.6);
                let d = p - muzzle;
                let len = d.length();
                if len > 0.05 && len < 3.0 {
                    tf.translation = (muzzle + p) * 0.5;
                    tf.rotation = Quat::from_rotation_arc(Vec3::Y, d / len);
                    tf.scale = Vec3::new(1.0 + 0.3 * (st.time * 20.0).sin(), len, 1.0);
                    *vis = Visibility::Inherited;
                } else {
                    *vis = Visibility::Hidden;
                }
            }
            _ => *vis = Visibility::Hidden,
        }
    }
}
