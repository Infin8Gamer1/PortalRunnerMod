//! Bounce light: a little global illumination, baked per chamber as it loads. The light that
//! falls on the chamber's boxes (from the ceiling lamps and the sun) bounces once off them,
//! coloured by what they're made of, into a grid of Bevy irradiance-volume voxels (Valve's
//! ambient cubes, as Portal itself used), which light everything inside it: walls, floors,
//! cubes and Faith. White chambers fill with soft light; a red wall tints the floor by it.
//!
//! The chambers are all boxes, so a plain ray caster is fast enough: a few hundred
//! milliseconds on a background thread, after which the volume switches on.

use bevy::asset::RenderAssetUsages;
use bevy::light::IrradianceVolume;
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
use bevy::tasks::{block_on, futures_lite::future, AsyncComputeTaskPool, Task};

use aperture::geom;
use faith_move::Aabb;
use aperture::Mat;

use crate::Game;

/// How far apart the voxels are (m), at most, and how many there may be.
const VOXEL: f32 = 1.5;
const MAX_VOXELS: usize = 48_000;
/// Rays per voxel (spread evenly over the sphere).
const RAYS: usize = 128;
/// The lamps' and the sun's light (as `chamber.rs` / `main.rs` set them).
const LAMP_LUMENS: f32 = 900_000.0;

/// The level's light as the baker sees it.
#[derive(Clone)]
struct Scene {
    boxes: Vec<(Aabb, Vec3)>,
    lamps: Vec<Vec3>,
    /// Direction the sunlight travels, and its illuminance (lux).
    sun: (Vec3, f32),
    /// What a ray that escapes sees (nits): the sky outdoors, nothing indoors.
    sky: Vec3,
}

pub struct Baked {
    /// (x, y, z) voxels, the region they span, and the packed texture's texels.
    res: UVec3,
    min: Vec3,
    max: Vec3,
    texels: Vec<[f32; 3]>,
}

#[derive(Component)]
pub struct GiVolume;

#[derive(Resource, Default)]
pub struct GiState {
    task: Option<(u64, Task<Baked>)>,
    level: u64,
}

fn albedo(m: Mat) -> Option<Vec3> {
    Some(match m {
        Mat::White => Vec3::new(0.78, 0.78, 0.80),
        Mat::Black => Vec3::splat(0.10),
        // See-through: light passes.
        Mat::Glass => return None,
        Mat::Grate => Vec3::splat(0.25),
        Mat::Runner => Vec3::new(0.70, 0.09, 0.06),
        Mat::Prop => Vec3::splat(0.35),
        Mat::Roof | Mat::Wall | Mat::Concrete => Vec3::splat(0.55),
    })
}

/// Evenly spread directions over the sphere (a Fibonacci spiral).
fn directions(n: usize) -> Vec<Vec3> {
    let golden = std::f32::consts::PI * (3.0 - 5f32.sqrt());
    (0..n)
        .map(|i| {
            let y = 1.0 - 2.0 * (i as f32 + 0.5) / n as f32;
            let r = (1.0 - y * y).sqrt();
            let a = golden * i as f32;
            Vec3::new(a.cos() * r, y, a.sin() * r)
        })
        .collect()
}

impl Scene {
    fn hit(&self, o: Vec3, d: Vec3, max: f32) -> Option<(f32, Vec3, Vec3)> {
        let mut best: Option<(f32, Vec3, Vec3)> = None;
        for (b, col) in &self.boxes {
            if let Some((t, n)) = geom::ray_aabb(o, d, b) {
                if t > 1e-3 && t < max && best.is_none_or(|(bt, _, _)| t < bt) {
                    best = Some((t, n, *col));
                }
            }
        }
        best
    }

    fn inside(&self, p: Vec3) -> bool {
        self.boxes.iter().any(|(b, _)| geom::contains(b, p))
    }

    /// The light leaving a surface at `p` facing `n` with albedo `col` (nits).
    fn surface(&self, p: Vec3, n: Vec3, col: Vec3) -> Vec3 {
        let p = p + n * 0.02;
        let lamp_cd = LAMP_LUMENS / (4.0 * std::f32::consts::PI);
        let mut e = 0.0;
        for l in &self.lamps {
            let to = *l - p;
            let r2 = to.length_squared().max(0.25);
            let cos = n.dot(to / r2.sqrt());
            if cos > 0.0 {
                e += lamp_cd * cos / r2;
            }
        }
        let (dir, lux) = self.sun;
        let cos = n.dot(-dir);
        if lux > 0.0 && cos > 0.0 && self.hit(p, -dir, 500.0).is_none() {
            e += lux * cos;
        }
        col * e / std::f32::consts::PI
    }

    fn bake(&self) -> Baked {
        let (mut min, mut max) = (Vec3::splat(f32::INFINITY), Vec3::splat(f32::NEG_INFINITY));
        for (b, _) in &self.boxes {
            min = min.min(b.min);
            max = max.max(b.max);
        }
        // Not out into the void under a rooftop course.
        min.y = min.y.max(max.y - 40.0);
        let size = (max - min).max(Vec3::splat(1.0));
        let mut step = VOXEL;
        let mut res = (size / step).ceil().as_uvec3().max(UVec3::ONE);
        while (res.x * res.y * res.z) as usize > MAX_VOXELS {
            step *= 1.2;
            res = (size / step).ceil().as_uvec3().max(UVec3::ONE);
        }
        let dirs = directions(RAYS);
        let faces = [Vec3::NEG_X, Vec3::X, Vec3::NEG_Y, Vec3::Y, Vec3::NEG_Z, Vec3::Z];
        let n = (res.x * res.y * res.z) as usize;
        // Per voxel: the six sides' light, and whether it's in the open.
        let mut cube = vec![[Vec3::ZERO; 6]; n];
        let mut open = vec![false; n];
        let idx = |x: u32, y: u32, z: u32| ((z * res.y + y) * res.x + x) as usize;
        for z in 0..res.z {
            for y in 0..res.y {
                for x in 0..res.x {
                    let p = min + (Vec3::new(x as f32, y as f32, z as f32) + 0.5) / res.as_vec3() * size;
                    if self.inside(p) {
                        continue;
                    }
                    let i = idx(x, y, z);
                    open[i] = true;
                    let mut sides = [Vec3::ZERO; 6];
                    let mut weights = [0.0f32; 6];
                    for d in &dirs {
                        let l = match self.hit(p, *d, 200.0) {
                            Some((t, nrm, col)) => self.surface(p + *d * t, nrm, col),
                            None => self.sky,
                        };
                        for (k, f) in faces.iter().enumerate() {
                            let w = d.dot(*f).max(0.0);
                            sides[k] += l * w;
                            weights[k] += w;
                        }
                    }
                    for k in 0..6 {
                        cube[i][k] = sides[k] / weights[k].max(1e-4);
                    }
                }
            }
        }
        // Voxels inside walls take their open neighbours' light, so surfaces beside them
        // aren't darkened by the interpolation.
        for _ in 0..3 {
            let mut filled = open.clone();
            let prev = cube.clone();
            for z in 0..res.z as i32 {
                for y in 0..res.y as i32 {
                    for x in 0..res.x as i32 {
                        let i = idx(x as u32, y as u32, z as u32);
                        if open[i] {
                            continue;
                        }
                        let mut sum = [Vec3::ZERO; 6];
                        let mut count = 0.0;
                        for (dx, dy, dz) in [(1, 0, 0), (-1, 0, 0), (0, 1, 0), (0, -1, 0), (0, 0, 1), (0, 0, -1)] {
                            let (a, b, c) = (x + dx, y + dy, z + dz);
                            if a < 0 || b < 0 || c < 0 || a >= res.x as i32 || b >= res.y as i32 || c >= res.z as i32 {
                                continue;
                            }
                            let j = idx(a as u32, b as u32, c as u32);
                            if open[j] {
                                for k in 0..6 {
                                    sum[k] += prev[j][k];
                                }
                                count += 1.0;
                            }
                        }
                        if count > 0.0 {
                            for k in 0..6 {
                                cube[i][k] = sum[k] / count;
                            }
                            filled[i] = true;
                        }
                    }
                }
            }
            open = filled;
        }
        // Pack as Bevy wants: (Rx, 2Ry, 3Rz), -X/+X, -Y/+Y, -Z/+Z.
        let (w, h, dpt) = (res.x, 2 * res.y, 3 * res.z);
        let mut texels = vec![[0.0; 3]; (w * h * dpt) as usize];
        for z in 0..res.z {
            for y in 0..res.y {
                for x in 0..res.x {
                    let c = cube[idx(x, y, z)];
                    for (k, v) in c.iter().enumerate() {
                        let t = y + if k % 2 == 1 { res.y } else { 0 };
                        let p = z + (k as u32 / 2) * res.z;
                        texels[((p * h + t) * w + x) as usize] = v.to_array();
                    }
                }
            }
        }
        Baked { res, min, max, texels }
    }
}

fn image(b: &Baked) -> Image {
    let mut data = Vec::with_capacity(b.texels.len() * 8);
    for t in &b.texels {
        for v in [t[0], t[1], t[2], 1.0] {
            data.extend_from_slice(&half::f16::from_f32(v).to_le_bytes());
        }
    }
    Image::new(
        Extent3d { width: b.res.x, height: 2 * b.res.y, depth_or_array_layers: 3 * b.res.z },
        TextureDimension::D3,
        data,
        TextureFormat::Rgba16Float,
        RenderAssetUsages::RENDER_WORLD,
    )
}

/// Start baking for a newly loaded chamber; put the volume in when it's done.
pub fn update(
    mut commands: Commands,
    game: Res<Game>,
    settings: Res<crate::settings::Settings>,
    mut state: ResMut<GiState>,
    mut images: ResMut<Assets<Image>>,
    mut volumes: Query<(Entity, &mut IrradianceVolume), With<GiVolume>>,
    sun: Query<(&DirectionalLight, &GlobalTransform), With<crate::Sun>>,
) {
    let on = settings.bounce_light > 0.0;
    for (_, mut v) in &mut volumes {
        let want = if on { GI_INTENSITY * settings.bounce_light } else { 0.0 };
        if v.intensity != want {
            v.intensity = want;
        }
    }
    if state.level != game.loads {
        state.level = game.loads;
        for (e, _) in &volumes {
            commands.entity(e).despawn();
        }
        let level = &game.sim.level;
        let boxes = game
            .sim
            .statics
            .iter()
            .filter(|s| matches!(s.owner, aperture::sim::Owner::Level(_)))
            .filter_map(|s| albedo(s.mat).map(|a| (s.b, a)))
            .collect();
        let lamps = level.lamps.iter().map(|p| *p - Vec3::Y * 0.3).collect();
        let (dir, lux) = sun.single().map(|(l, t)| (t.forward().as_vec3(), l.illuminance)).unwrap_or((Vec3::NEG_Y, 0.0));
        let sky = if level.indoor { Vec3::ZERO } else { Vec3::new(0.62, 0.78, 0.93) * 3000.0 };
        let scene = Scene { boxes, lamps, sun: (dir, lux), sky };
        let task = AsyncComputeTaskPool::get().spawn(async move { scene.bake() });
        state.task = Some((game.loads, task));
    }
    let done = match &mut state.task {
        Some((_, task)) => block_on(future::poll_once(task)),
        None => None,
    };
    if let Some(baked) = done {
        let (loads, _) = state.task.take().unwrap();
        if loads != game.loads {
            return;
        }
        let voxels = images.add(image(&baked));
        let center = (baked.min + baked.max) * 0.5;
        commands.spawn((
            GiVolume,
            crate::chamber::ChamberGeom,
            IrradianceVolume { voxels, intensity: if on { GI_INTENSITY * settings.bounce_light } else { 0.0 }, ..default() },
            Transform::from_translation(center).with_scale(baked.max - baked.min),
        ));
        info!("Bounce light baked: {}x{}x{} voxels", baked.res.x, baked.res.y, baked.res.z);
    }
}

/// The volume's brightness at 100% (one bounce alone undersells how much light a white
/// chamber gathers).
const GI_INTENSITY: f32 = 3.0;

#[cfg(test)]
mod tests {
    use super::*;

    /// A white box room lit from its ceiling: the floor's voxels see light from below them
    /// (the floor bounces it), and a red wall tints the light beside it red.
    #[test]
    fn a_lit_room_bounces_light_and_colour() {
        let w = Vec3::splat(0.78);
        let red = Vec3::new(0.7, 0.09, 0.06);
        let b = |a: [f32; 3], c: [f32; 3]| Aabb::new(Vec3::from(a), Vec3::from(c));
        let scene = Scene {
            boxes: vec![
                (b([-6.0, -1.0, -6.0], [6.0, 0.0, 6.0]), w),
                (b([-6.0, 6.0, -6.0], [6.0, 7.0, 6.0]), w),
                (b([-7.0, 0.0, -6.0], [-6.0, 6.0, 6.0]), red),
                (b([6.0, 0.0, -6.0], [7.0, 6.0, 6.0]), w),
                (b([-6.0, 0.0, -7.0], [6.0, 6.0, -6.0]), w),
                (b([-6.0, 0.0, 6.0], [6.0, 6.0, 7.0]), w),
            ],
            lamps: vec![Vec3::new(0.0, 5.7, 0.0)],
            sun: (Vec3::NEG_Y, 0.0),
            sky: Vec3::ZERO,
        };
        let baked = scene.bake();
        assert!(baked.texels.iter().all(|t| t.iter().all(|v| v.is_finite() && *v >= 0.0)));
        let r = baked.res;
        // The -Y side (light coming up off the floor) of a voxel low in the middle.
        let (x, y, z) = (r.x / 2, 1, r.z / 2);
        let h = 2 * r.y;
        let up_from_floor = baked.texels[((z + r.z) * h + y) as usize * r.x as usize + x as usize];
        assert!(up_from_floor[0] > 1.0, "no bounce off the floor: {up_from_floor:?}");
        // Beside the red wall, the light from -X (the wall's side) is redder than from +X.
        let (x, y, z) = (0, r.y / 2, r.z / 2);
        let from_neg_x = baked.texels[((z * h + y) * r.x + x) as usize];
        let from_pos_x = baked.texels[((z * h + y + r.y) * r.x + x) as usize];
        let redness = |c: [f32; 3]| c[0] / (c[1] + 1e-3);
        assert!(redness(from_neg_x) > redness(from_pos_x) * 2.0, "{from_neg_x:?} vs {from_pos_x:?}");
    }
}
