//! Going fast, felt: near Faith's top running speed the view widens a little, and streaks
//! rush past the edges of the screen. Both grow past her top speed (flings, faith plates,
//! propulsion gel), and both can be turned down or off in the settings. Landing faster than
//! she can run, she skids, kicking up dust.

use bevy::asset::{load_internal_asset, uuid_handle};
use bevy::prelude::*;
use bevy::render::render_resource::{AsBindGroup, ShaderType};
use bevy::shader::ShaderRef;
use bevy::ui_render::prelude::MaterialNode;
use bevy::ui_render::ui_material::UiMaterial;
use bevy::ui_render::UiMaterialPlugin;

use crate::{settings, Game, PlayerCamera};

const SPEED_LINES_SHADER: Handle<Shader> = uuid_handle!("3f1c9e52-8a47-4d0b-b6e3-2a9d7c41f805");

/// Where the effects start, as a fraction of her top running speed.
pub const START: f32 = 0.98;
/// The speed (m/s) at which they're at their strongest.
pub const FULL: f32 = 20.0;
/// Degrees of horizontal field of view added at top running speed, and at `FULL`.
pub const FOV_AT_TOP: f32 = 4.0;
pub const FOV_AT_FULL: f32 = 12.0;

pub struct SpeedFxPlugin;

impl Plugin for SpeedFxPlugin {
    fn build(&self, app: &mut App) {
        load_internal_asset!(app, SPEED_LINES_SHADER, "shaders/speed_lines.wgsl", Shader::from_wgsl);
        app.add_plugins(UiMaterialPlugin::<SpeedLinesMaterial>::default()).add_systems(Startup, setup);
        app.add_systems(Update, skid_dust);
    }
}

#[derive(ShaderType, Debug, Clone, Default)]
pub struct SpeedLinesUniform {
    /// x: strength (0..1), y: time, z: the screen's aspect ratio.
    pub params: Vec4,
}

#[derive(Asset, TypePath, AsBindGroup, Debug, Clone)]
pub struct SpeedLinesMaterial {
    #[uniform(0)]
    pub uniform: SpeedLinesUniform,
}

impl UiMaterial for SpeedLinesMaterial {
    fn fragment_shader() -> ShaderRef {
        SPEED_LINES_SHADER.into()
    }
}

#[derive(Resource, Default)]
pub struct SpeedFx {
    /// How fast she's going, past where the effects start: 0 below, 1 at her top running
    /// speed, 2 at `FULL` (smoothed).
    pub amount: f32,
    material: Handle<SpeedLinesMaterial>,
    time: f32,
}

#[derive(Component)]
pub struct SpeedLines;

/// A puff of dust from a skid.
#[derive(Component)]
pub struct Dust {
    vel: Vec3,
    life: f32,
}

#[derive(Resource)]
pub struct DustKit {
    mesh: Handle<Mesh>,
    material: Handle<StandardMaterial>,
    /// Puffs owed (fractional, by time).
    acc: f32,
    n: u32,
}

/// Puffs a second while skidding, and how long each lasts.
const DUST_RATE: f32 = 45.0;
const DUST_LIFE: f32 = 0.6;

/// The effects' strength for a speed: 0 below `START` of `top`, rising to 1 at `top` and on
/// to 2 at `FULL`.
pub fn amount_for(speed: f32, top: f32) -> f32 {
    let start = top * START;
    if speed <= start {
        0.0
    } else if speed <= top {
        (speed - start) / (top - start)
    } else {
        1.0 + ((speed - top) / (FULL - top).max(0.1)).min(1.0)
    }
}

/// Degrees of field of view added for an amount.
pub fn fov_for(amount: f32) -> f32 {
    FOV_AT_TOP * amount.min(1.0) + (FOV_AT_FULL - FOV_AT_TOP) * (amount - 1.0).clamp(0.0, 1.0)
}

fn setup(
    mut commands: Commands,
    mut materials: ResMut<Assets<SpeedLinesMaterial>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut std_materials: ResMut<Assets<StandardMaterial>>,
) {
    commands.insert_resource(DustKit {
        mesh: meshes.add(Sphere::new(0.11).mesh().ico(1).unwrap()),
        material: std_materials.add(StandardMaterial { base_color: Color::srgba(0.42, 0.41, 0.40, 0.55), alpha_mode: AlphaMode::Blend, unlit: true, ..default() }),
        acc: 0.0,
        n: 0,
    });
    let material = materials.add(SpeedLinesMaterial { uniform: SpeedLinesUniform::default() });
    commands.spawn((
        SpeedLines,
        MaterialNode(material.clone()),
        Node { position_type: PositionType::Absolute, width: percent(100.0), height: percent(100.0), ..default() },
        // Over the world, under the HUD's text.
        GlobalZIndex(-2),
        Visibility::Hidden,
    ));
    commands.insert_resource(SpeedFx { material, ..default() });
}

pub fn update(
    time: Res<Time>,
    game: Res<Game>,
    settings: Res<settings::Settings>,
    menu: Res<crate::menu::Menu>,
    mut fx: ResMut<SpeedFx>,
    mut materials: ResMut<Assets<SpeedLinesMaterial>>,
    mut camera: Query<&mut Projection, With<PlayerCamera>>,
    mut lines: Query<&mut Visibility, With<SpeedLines>>,
) {
    let dt = time.delta_secs().min(0.1);
    fx.time += dt;
    let c = &game.ctrl;
    // Through the air, falling counts for less than going along (the wind sound has that).
    let speed = (c.horizontal_speed().powi(2) + (0.5 * c.vel.y).powi(2)).sqrt();
    let target = amount_for(speed, c.tuning.loco.ground_speed);
    // Quick to come on, slower to fade.
    let rate = if target > fx.amount { 4.0 } else { 2.0 };
    fx.amount += (target - fx.amount) * (1.0 - (-rate * dt).exp());

    if let Ok(mut proj) = camera.single_mut() {
        if let Projection::Perspective(p) = &mut *proj {
            let add = fov_for(fx.amount) * settings.fov_kick;
            if add > 0.01 {
                // The camera's FOV is vertical; the kick is in horizontal degrees.
                let h = 2.0 * ((p.fov * 0.5).tan() * p.aspect_ratio).atan() + add.to_radians();
                p.fov = 2.0 * ((h.min(3.0) * 0.5).tan() / p.aspect_ratio).atan();
            }
        }
    }

    let strength = if settings.speed_lines > 0.5 && !menu.open { ((fx.amount - 0.3) / 1.7).clamp(0.0, 1.0) } else { 0.0 };
    if let Ok(mut v) = lines.single_mut() {
        let want = if strength > 0.001 { Visibility::Inherited } else { Visibility::Hidden };
        if *v != want {
            *v = want;
        }
    }
    if strength > 0.001 {
        let aspect = camera.single().ok().and_then(|p| if let Projection::Perspective(p) = p { Some(p.aspect_ratio) } else { None }).unwrap_or(16.0 / 9.0);
        let t = fx.time;
        if let Some(mut m) = materials.get_mut(&fx.material) {
            m.uniform.params = Vec4::new(strength.min(1.0), t, aspect, 0.0);
        }
    }
}

/// Dust kicked up round her feet while she skids, thrown forward and out.
pub fn skid_dust(
    mut commands: Commands,
    time: Res<Time>,
    game: Res<Game>,
    mut kit: ResMut<DustKit>,
    mut dust: Query<(Entity, &mut Dust, &mut Transform)>,
) {
    let dt = time.delta_secs().min(0.1);
    let c = &game.ctrl;
    if c.skidding {
        kit.acc += DUST_RATE * dt;
        let fwd = Vec3::new(-c.yaw.sin(), 0.0, -c.yaw.cos());
        let side = Vec3::new(fwd.z, 0.0, -fwd.x);
        let dir = Vec3::new(c.vel.x, 0.0, c.vel.z).normalize_or(fwd);
        while kit.acc >= 1.0 {
            kit.acc -= 1.0;
            kit.n = kit.n.wrapping_add(1);
            let r = |k: u32| ((kit.n.wrapping_mul(2654435761).wrapping_add(k * 40503) >> 8) & 1023) as f32 / 1023.0 * 2.0 - 1.0;
            let at = c.feet + dir * (0.8 + 0.4 * r(1)) + side * 0.45 * r(2) + Vec3::Y * 0.05;
            // Kicked forward at about her own speed, so it hangs in front of her as she slows.
            let vel = Vec3::new(c.vel.x, 0.0, c.vel.z) * (0.9 + 0.1 * r(3)) + side * 1.6 * r(4) + Vec3::Y * (1.4 + 0.6 * r(5));
            commands.spawn((
                Dust { vel, life: DUST_LIFE },
                Mesh3d(kit.mesh.clone()),
                MeshMaterial3d(kit.material.clone()),
                Transform::from_translation(at).with_scale(Vec3::splat(0.5)),
                bevy::light::NotShadowCaster,
            ));
        }
    } else {
        kit.acc = 0.0;
    }
    for (e, mut d, mut tf) in &mut dust {
        d.life -= dt;
        d.vel *= (-1.5 * dt).exp();
        d.vel.y -= 1.5 * dt;
        tf.translation += d.vel * dt;
        let k = (d.life / DUST_LIFE).clamp(0.0, 1.0);
        // Grows as it spreads, then shrinks away.
        tf.scale = Vec3::splat((0.5 + 2.0 * (1.0 - k)) * k.sqrt());
        if d.life <= 0.0 {
            commands.entity(e).despawn();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn effects_start_near_top_speed_and_grow_past_it() {
        let top = 7.2;
        assert_eq!(amount_for(6.0, top), 0.0);
        assert_eq!(amount_for(top * 0.97, top), 0.0);
        assert!(amount_for(top * 0.99, top) > 0.0);
        assert!((amount_for(top, top) - 1.0).abs() < 1e-5);
        assert!((amount_for(FULL, top) - 2.0).abs() < 1e-5);
        assert_eq!(amount_for(100.0, top), 2.0);
        assert_eq!(fov_for(0.0), 0.0);
        assert!((fov_for(1.0) - FOV_AT_TOP).abs() < 1e-5);
        assert!((fov_for(2.0) - FOV_AT_FULL).abs() < 1e-5);
    }
}
