//! An emancipation grill's particles, as Portal's own systems have them (particles/
//! cleansers.pcf), where its maps put them (info_particle_system):
//! - `portal_cleanser`, over the field: 300 a second, each a short blue streak a second long,
//!   drifting sideways across a 128-unit square in the grill's plane, fading in and out.
//! - `Cleanser_edge_1`, up a post: 150 a second, white sparks drifting off it into the field,
//!   jostled as they go.
//!
//! A particle's life is all known from when it's born (where, how fast, which way), so there's
//! nothing to simulate: each frame the live ones are worked out from the simulation's clock and
//! drawn as one mesh of quads, flat in the grill's plane. They show while the grill's on.

use bevy::asset::RenderAssetUsages;
use bevy::camera::visibility::NoFrustumCulling;
use bevy::light::NotShadowCaster;
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;

use crate::Game;

/// Source units to metres.
const UNIT: f32 = 0.0254;
/// How brightly they glow over their colour (additive).
const GLOW: f32 = 3.0;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    Field,
    Edge,
}

impl Kind {
    pub fn of(effect: &str) -> Option<Kind> {
        match effect {
            "portal_cleanser" => Some(Kind::Field),
            "cleanser_edge_1" => Some(Kind::Edge),
            _ => None,
        }
    }

    /// Born a second; each lives a second.
    fn rate(self) -> f32 {
        match self {
            Kind::Field => 300.0,
            Kind::Edge => 150.0,
        }
    }
}

/// One of a map's grill particle systems.
#[derive(Component)]
pub struct GrillFx {
    kind: Kind,
    at: Vec3,
    /// The system's own axes, in ours: Source's local X (which way they drift), Y (across the
    /// grill) and Z (up).
    axes: [Vec3; 3],
    /// The grill it's on (it shows while that's on).
    fizzler: Option<usize>,
    seed: u32,
}

/// The two materials (the field's streaks, the edges' sparks), made once.
#[derive(Default)]
pub struct Mats(Option<[Handle<StandardMaterial>; 2]>);

/// Put a system where the map has it (`turn`: its angles, as a turn in our space).
#[allow(clippy::too_many_arguments)]
pub fn spawn(commands: &mut Commands, meshes: &mut Assets<Mesh>, materials: &mut Assets<StandardMaterial>, images: &mut Assets<Image>, extracted: &crate::extracted::Extracted, mats: &mut Mats, sim: &aperture::Sim, kind: Kind, at: Vec3, turn: Quat, seed: u32) {
    let [field, edge] = mats
        .0
        .get_or_insert_with(|| {
            let mut mat = |tex: &str, c: Vec3| {
                materials.add(StandardMaterial {
                    base_color: Color::linear_rgb(c.x * GLOW, c.y * GLOW, c.z * GLOW),
                    base_color_texture: extracted.texture(tex).map(|t| images.add(t)),
                    alpha_mode: AlphaMode::Add,
                    unlit: true,
                    double_sided: true,
                    cull_mode: None,
                    ..default()
                })
            };
            [mat("cleanser_particle", Vec3::new(106.0, 179.0, 241.0) / 255.0), mat("cleanser_edge", Vec3::ONE)]
        })
        .clone();
    // Source's local X, Y, Z in ours, turned as the map turns it.
    let axes = [turn * Vec3::NEG_Z, turn * Vec3::NEG_X, turn * Vec3::Y];
    // The grill it's over: the nearest one it's in or by.
    let fizzler = sim
        .fizzlers
        .iter()
        .enumerate()
        .map(|(i, f)| (i, (at - at.clamp(f.b.min, f.b.max)).length()))
        .filter(|(_, d)| *d < 1.5)
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(i, _)| i);
    // (Something in it from the start: see `update`.)
    let mesh = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default())
        .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, vec![at.to_array(); 3])
        .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, vec![[0.0f32, 0.0]; 3])
        .with_inserted_attribute(Mesh::ATTRIBUTE_COLOR, vec![[0.0f32; 4]; 3])
        .with_inserted_indices(Indices::U32(vec![0, 1, 2]));
    commands.spawn((
        crate::chamber::ChamberGeom,
        GrillFx { kind, at, axes, fizzler, seed },
        Mesh3d(meshes.add(mesh)),
        MeshMaterial3d(if kind == Kind::Field { field } else { edge }),
        Transform::default(),
        Visibility::default(),
        NotShadowCaster,
        NoFrustumCulling,
    ));
}

/// A number in [0, 1) for particle `k`'s `n`th random choice.
fn rand(seed: u32, k: u32, n: u32) -> f32 {
    let mut h = seed.wrapping_mul(0x9E37_79B9) ^ k.wrapping_mul(0x85EB_CA6B) ^ n.wrapping_mul(0xC2B2_AE35);
    h ^= h >> 16;
    h = h.wrapping_mul(0x7FEB_352D);
    h ^= h >> 15;
    h = h.wrapping_mul(0x846C_A68B);
    h ^= h >> 16;
    (h >> 8) as f32 / (1u32 << 24) as f32
}

/// Each live particle: where it is (local, units), how long its trail is, its radius (units)
/// and how bright (0..1).
fn particles(kind: Kind, seed: u32, t: f32) -> Vec<(Vec3, f32, f32, f32)> {
    let rate = kind.rate();
    let first = ((t - 1.0) * rate).ceil().max(0.0) as u32;
    let last = (t * rate).floor().max(0.0) as u32;
    let mut out = Vec::with_capacity((last + 1 - first.min(last + 1)) as usize);
    for k in first..=last {
        let age = t - k as f32 / rate;
        if !(0.0..1.0).contains(&age) {
            continue;
        }
        let r = |n| rand(seed, k, n);
        match kind {
            Kind::Field => {
                // Anywhere in the square, drifting sideways at up to 100 units a second; it
                // fades in over its first half second and out over its second.
                let off = Vec3::new(-64.0 + 128.0 * r(0), -1.0 + 2.0 * r(1), -64.0 + 128.0 * r(2));
                let vx = -100.0 + 200.0 * r(3);
                let alpha = if age < 0.5 { age / 0.5 } else { (1.0 - age) / 0.5 };
                out.push((off + Vec3::X * vx * age, vx * 0.1, 2.5 * (1.0 - age / 5.0), alpha));
            }
            Kind::Edge => {
                // Up the post, drifting off it into the field at 10 to 100 units a second,
                // jostled; it fades out over its second half second.
                let off = Vec3::new(0.0, 0.0, -64.0 + 128.0 * r(0));
                let vx = -10.0 - 90.0 * r(1);
                let jostle = Vec3::new((age * (7.0 + 6.0 * r(2)) + 6.28 * r(3)).sin() * 8.0 * age, 0.0, (age * (5.0 + 4.0 * r(4)) + 6.28 * r(5)).sin() * 2.0 * age);
                let alpha = if age < 0.5 { 1.0 } else { (1.0 - age) / 0.5 };
                out.push((off + Vec3::X * vx * age + jostle, vx * 0.1, 5.0 * (1.0 - age / 5.0).powf(0.75), alpha));
            }
        }
    }
    out
}

/// Each frame: the live particles of each system whose grill is on.
pub fn update(game: Res<Game>, mut q: Query<(&GrillFx, &Mesh3d, &mut Visibility)>, mut meshes: ResMut<Assets<Mesh>>) {
    let sim = &game.sim;
    for (fx, mesh, mut vis) in &mut q {
        let on = fx.fizzler.is_none_or(|i| sim.fizzlers.get(i).is_some_and(|f| f.on));
        let want = if on { Visibility::Inherited } else { Visibility::Hidden };
        if *vis != want {
            *vis = want;
        }
        if !on {
            continue;
        }
        let Some(mut m) = meshes.get_mut(&mesh.0) else { continue };
        let ps = particles(fx.kind, fx.seed, sim.time);
        let (mut pos, mut uv, mut col, mut idx) = (Vec::with_capacity(ps.len() * 4), Vec::with_capacity(ps.len() * 4), Vec::with_capacity(ps.len() * 4), Vec::with_capacity(ps.len() * 6));
        let [ax, _, az] = fx.axes;
        let world = |l: Vec3| fx.at + (fx.axes[0] * l.x + fx.axes[1] * l.y + fx.axes[2] * l.z) * UNIT;
        for (p, trail, radius, alpha) in ps {
            // A streak from where it was a tenth of a second ago to where it is, as wide as
            // the particle, flat in the grill's plane.
            let head = world(p);
            let tail = world(p - Vec3::X * trail);
            let along = if trail.abs() * UNIT > 1e-4 { (head - tail).normalize() } else { ax };
            let r = radius * UNIT;
            let (a, b) = (tail - along * r, head + along * r);
            let side = az * r;
            let n = pos.len() as u32;
            pos.extend([(a - side).to_array(), (b - side).to_array(), (b + side).to_array(), (a + side).to_array()]);
            // (The texture runs down its length: Source draws a trail along V.)
            uv.extend([[0.0f32, 1.0], [0.0, 0.0], [1.0, 0.0], [1.0, 1.0]]);
            col.extend([[alpha, alpha, alpha, alpha]; 4]);
            idx.extend([n, n + 1, n + 2, n, n + 2, n + 3]);
        }
        // (Never empty: a mesh with nothing in it isn't given room on the GPU, and filling it
        // again later reads from room it hasn't got.)
        if pos.is_empty() {
            pos.extend([[fx.at.x, fx.at.y, fx.at.z]; 3]);
            uv.extend([[0.0f32, 0.0]; 3]);
            col.extend([[0.0f32; 4]; 3]);
            idx.extend([0, 1, 2]);
        }
        m.insert_attribute(Mesh::ATTRIBUTE_POSITION, pos);
        m.insert_attribute(Mesh::ATTRIBUTE_UV_0, uv);
        m.insert_attribute(Mesh::ATTRIBUTE_COLOR, col);
        m.insert_indices(Indices::U32(idx));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_field_keeps_about_a_seconds_worth_of_streaks_in_its_square() {
        for t in [0.5f32, 3.0, 100.25] {
            let ps = particles(Kind::Field, 7, t);
            let want = 300.0 * t.min(1.0);
            assert!((ps.len() as f32 - want).abs() <= 2.0, "{} at {t}", ps.len());
            for (p, _, r, a) in ps {
                assert!(p.x.abs() <= 64.0 + 100.0 && p.y.abs() <= 1.0 && p.z.abs() <= 64.0, "{p}");
                assert!(r > 0.0 && (0.0..=1.0).contains(&a));
            }
        }
        // The same moment, the same particles.
        assert_eq!(particles(Kind::Field, 7, 3.0), particles(Kind::Field, 7, 3.0));
    }
}
