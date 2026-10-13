//! A course's beacon (#104): a column of red light over the checkpoint to make for next, as
//! Mirror's Edge's time trials show theirs, then over the finish once they've all been
//! passed. Only in a level whose checkpoints are a course.

use bevy::light::NotShadowCaster;
use bevy::prelude::*;

use crate::Game;

/// How tall the column stands, from the floor of the checkpoint, and how thick it is.
const HEIGHT: f32 = 40.0;
const RADIUS: f32 = 0.12;

#[derive(Component)]
pub struct Beacon;

pub fn setup(mut commands: Commands, mut meshes: ResMut<Assets<Mesh>>, mut materials: ResMut<Assets<StandardMaterial>>) {
    let red = Color::srgb(0.86, 0.10, 0.07);
    commands.spawn((
        Beacon,
        Mesh3d(meshes.add(Cylinder::new(RADIUS, HEIGHT).mesh().resolution(16))),
        MeshMaterial3d(materials.add(StandardMaterial { base_color: red, emissive: red.to_linear() * 6.0, unlit: false, ..default() })),
        NotShadowCaster,
        Transform::default(),
        Visibility::Hidden,
    ));
}

/// Over the next checkpoint (or the finish), or hidden.
pub fn update(game: Res<Game>, mut q: Query<(&mut Transform, &mut Visibility), With<Beacon>>) {
    let Ok((mut tf, mut vis)) = q.single_mut() else { return };
    let l = &game.sim.level;
    let next = if !l.course || game.sim.complete {
        None
    } else if let Some(cp) = l.checkpoints.get(game.sim.checkpoint + 1) {
        Some(cp.region)
    } else {
        l.exit
    };
    let want = if next.is_some() { Visibility::Inherited } else { Visibility::Hidden };
    if *vis != want {
        *vis = want;
    }
    if let Some(r) = next {
        let at = Vec3::new((r.min.x + r.max.x) * 0.5, r.min.y + HEIGHT * 0.5, (r.min.z + r.max.z) * 0.5);
        if tf.translation != at {
            tf.translation = at;
        }
    }
}
