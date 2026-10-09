//! Going fast, felt: near Faith's top running speed the view widens a little, and streaks
//! rush past the edges of the screen. Both grow past her top speed (flings, faith plates,
//! propulsion gel), and both can be turned down or off in the settings.

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
pub const START: f32 = 0.95;
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

fn setup(mut commands: Commands, mut materials: ResMut<Assets<SpeedLinesMaterial>>) {
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
    menu: Res<settings::Menu>,
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn effects_start_near_top_speed_and_grow_past_it() {
        let top = 7.2;
        assert_eq!(amount_for(6.0, top), 0.0);
        assert!(amount_for(top * 0.97, top) > 0.0);
        assert!((amount_for(top, top) - 1.0).abs() < 1e-5);
        assert!((amount_for(FULL, top) - 2.0).abs() < 1e-5);
        assert_eq!(amount_for(100.0, top), 2.0);
        assert_eq!(fov_for(0.0), 0.0);
        assert!((fov_for(1.0) - FOV_AT_TOP).abs() < 1e-5);
        assert!((fov_for(2.0) - FOV_AT_FULL).abs() < 1e-5);
    }
}
