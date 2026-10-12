//! The lights a level places (the level editor's point and spot lights): Bevy's own, and,
//! under ray-traced lighting (which lights by what glows, not by lights), a glowing bulb in the
//! traced scene for each, as bright: a sphere for a point light; for a spot light, a disc facing
//! the way it shines, with a short black tube on in front, long enough that light only gets out
//! within its cone.

use bevy::camera::visibility::RenderLayers;
use bevy::prelude::*;
use bevy::solari::prelude::RaytracingMesh3d;

use crate::chamber::ChamberGeom;
use crate::Game;

/// One of the level's lights (or its bulb).
#[derive(Component)]
pub struct PlacedLightPart;

/// Spawn the level's lights when it loads, and their bulbs when the lighting's traced (made or
/// taken away as it's switched).
pub fn spawn_for_level(
    mut commands: Commands,
    game: Res<Game>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    existing: Query<Entity, With<PlacedLightPart>>,
    mut built: Local<Option<(u64, bool)>>,
) {
    let want = (game.loads, crate::raytrace::active());
    if *built == Some(want) {
        return;
    }
    *built = Some(want);
    for e in &existing {
        commands.entity(e).despawn();
    }
    // The world, her own legs, and her third-person body (seen through portals): as the lamps.
    let layers = RenderLayers::from_layers(&[0, crate::portal_render::PLAYER_LAYER, crate::body3p::BODY_LAYER, crate::body3p::THROUGH_LAYER]);
    for l in &game.sim.level.placed_lights {
        let color = Color::srgb(l.color.x, l.color.y, l.color.z);
        let at = Transform::from_translation(l.at);
        match l.spot {
            None => {
                commands.spawn((ChamberGeom, PlacedLightPart, PointLight { color, intensity: l.intensity, range: l.range, radius: l.radius, shadow_maps_enabled: l.shadows, ..default() }, at, layers.clone()));
            }
            Some((dir, outer, inner)) => {
                let up = if dir.y.abs() > 0.99 { Vec3::Z } else { Vec3::Y };
                commands.spawn((
                    ChamberGeom,
                    PlacedLightPart,
                    SpotLight { color, intensity: l.intensity, range: l.range, radius: l.radius, outer_angle: outer, inner_angle: inner.min(outer), shadow_maps_enabled: l.shadows, ..default() },
                    at.looking_to(dir, up),
                    layers.clone(),
                ));
            }
        }
        if !want.1 {
            continue;
        }
        // The bulb: as much light, from glow. A sphere of radius r glowing L sends out
        // 4 pi^2 r^2 L lumens; a disc facing the way it shines, as much straight ahead. (The
        // glow is set in what shows on screen: the traced light scales it to physical units.)
        let r = l.radius.max(0.03);
        let glow = l.intensity / (4.0 * std::f32::consts::PI * std::f32::consts::PI * r * r) / crate::raytrace::EMISSIVE_SCALE;
        let c = color.to_linear();
        let material = materials.add(StandardMaterial { base_color: Color::BLACK, emissive: LinearRgba::rgb(c.red * glow, c.green * glow, c.blue * glow), ..default() });
        let (mesh, tf) = match l.spot {
            None => {
                let mut m = Sphere::new(r).mesh().ico(1).unwrap();
                crate::raytrace::make_ready(&mut m);
                (m, at)
            }
            Some((dir, outer, _)) => {
                let dir = dir.normalize_or(Vec3::NEG_Y);
                // (A circle faces +Z: turned to face the way it shines.)
                let turn = Quat::from_rotation_arc(Vec3::Z, dir);
                // The tube: from the disc (at the light, so it's never inside what the light's on)
                // forward, as long as its width over the cone's tangent (so no light leaves wider
                // than the cone: from one edge of the disc past the other at most).
                let length = (2.0 * r / outer.clamp(0.02, 1.5).tan()).min(40.0 * r);
                let mut tube = tube_mesh(r * 1.02, length);
                crate::raytrace::make_ready(&mut tube);
                let black = materials.add(StandardMaterial { base_color: Color::BLACK, perceptual_roughness: 1.0, cull_mode: None, ..default() });
                let mouth = Transform::from_translation(l.at + dir * length).with_rotation(turn);
                commands.spawn((ChamberGeom, PlacedLightPart, RaytracingMesh3d(meshes.add(tube)), MeshMaterial3d(black), mouth));
                let mut m = Circle::new(r).mesh().resolution(16).build();
                crate::raytrace::make_ready(&mut m);
                (m, Transform::from_translation(l.at).with_rotation(turn))
            }
        };
        commands.spawn((ChamberGeom, PlacedLightPart, RaytracingMesh3d(meshes.add(mesh)), MeshMaterial3d(material), tf));
    }
}

/// A tube of radius `r` from z = -`length` to 0, open at both ends, its walls faced both ways.
fn tube_mesh(r: f32, length: f32) -> Mesh {
    use bevy::mesh::{Indices, PrimitiveTopology};
    const SIDES: u32 = 16;
    let (mut pos, mut nor, mut idx) = (Vec::new(), Vec::new(), Vec::new());
    for face in [1.0f32, -1.0] {
        let base = pos.len() as u32;
        for k in 0..=SIDES {
            let a = k as f32 * std::f32::consts::TAU / SIDES as f32;
            let (x, y) = (a.cos(), a.sin());
            for z in [0.0, -length] {
                pos.push([x * r, y * r, z]);
                nor.push([x * face, y * face, 0.0]);
            }
        }
        for k in 0..SIDES {
            let (a, b, c, d) = (base + 2 * k, base + 2 * k + 1, base + 2 * k + 2, base + 2 * k + 3);
            if face > 0.0 {
                idx.extend([a, b, c, c, b, d]);
            } else {
                idx.extend([a, c, b, c, d, b]);
            }
        }
    }
    let n = pos.len();
    Mesh::new(PrimitiveTopology::TriangleList, default())
        .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, pos)
        .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, nor)
        .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, vec![[0.0f32, 0.0]; n])
        .with_inserted_indices(Indices::U32(idx))
}
