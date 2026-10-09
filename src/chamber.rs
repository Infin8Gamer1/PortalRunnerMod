//! Drawing a test chamber: its boxes by material, and every puzzle piece, kept in step with
//! the simulation each frame.

use std::f32::consts::FRAC_PI_2;

use bevy::asset::RenderAssetUsages;
use bevy::image::{ImageAddressMode, ImageSampler, ImageSamplerDescriptor};
use bevy::light::NotShadowCaster;
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};

use aperture::gel::{GelMap, CELL};
use aperture::sim::{Owner, BRIDGE_HALF_WIDTH, BUTTON_TOP, CUBE_HALF, PLATE_HALF, PLATE_TOP};
use aperture::{CubeKind, Mat, Paint, Sim};
use faith_move::{Aabb, Fixture};

use crate::extracted::Extracted;

/// Everything drawn for the current chamber (despawned when it changes).
#[derive(Component)]
pub struct ChamberGeom;

/// What a drawn piece shows, for keeping it in step with the sim.
#[derive(Component, Clone, Copy, Debug)]
pub enum Vis {
    Button(usize),
    Door { door: usize, side: f32 },
    Fizzler(usize),
    /// A faith plate's flap (`part` 0) and its arrow (1).
    PlateFlap { plate: usize, part: usize },
    Catcher(usize),
    Relay(usize),
    /// A relay model's glow (only while lit).
    RelayGlow(usize),
    Pedestal(usize),
    Hatch { dropper: usize, side: f32 },
    Wire(usize),
    /// A Mirror's Edge door swinging on its hinge.
    Swing { fixture: usize, hinge: Vec3, away: Vec3 },
    /// One of the pooled pieces (cubes, beams, bridges, blobs).
    Pool,
}

#[derive(Component)]
pub struct GelVis(Paint);

/// Entities reused frame to frame for things whose number changes (beam pieces, bridge
/// pieces, gel blobs).
#[derive(Resource, Default)]
pub struct Pools {
    beams: Vec<Entity>,
    glows: Vec<Entity>,
    bridges: Vec<Entity>,
    blobs: Vec<Entity>,
    props: Vec<Entity>,
    /// Each prop slot's models (storage, companion, reflector), when there are models.
    prop_models: Vec<Option<[Entity; 3]>>,
    gel_version: u64,
}

#[derive(Resource)]
pub struct ChamberMats {
    pub white: Handle<StandardMaterial>,
    pub black: Handle<StandardMaterial>,
    /// What up-facing white and black surfaces wear (floor tiles).
    pub white_floor: Handle<StandardMaterial>,
    pub black_floor: Handle<StandardMaterial>,
    pub glass: Handle<StandardMaterial>,
    pub grate: Handle<StandardMaterial>,
    pub runner: Handle<StandardMaterial>,
    pub prop: Handle<StandardMaterial>,
    pub roof: Handle<StandardMaterial>,
    pub wall: Handle<StandardMaterial>,
    pub dark: Handle<StandardMaterial>,
    pub button_off: Handle<StandardMaterial>,
    pub button_on: Handle<StandardMaterial>,
    pub cube: [Handle<StandardMaterial>; 3],
    /// The same, faintly lit up while Faith is holding it.
    pub cube_held: [Handle<StandardMaterial>; 3],
    pub cube_fizzle: Handle<StandardMaterial>,
    pub cube_coat: [Handle<StandardMaterial>; 2],
    pub fizzler: Handle<StandardMaterial>,
    pub bridge: Handle<StandardMaterial>,
    pub laser: Handle<StandardMaterial>,
    pub laser_glow: Handle<StandardMaterial>,
    pub lens_off: Handle<StandardMaterial>,
    pub lens_on: Handle<StandardMaterial>,
    pub paint: [Handle<StandardMaterial>; 3],
    pub goo: Handle<StandardMaterial>,
    pub wire_off: Handle<StandardMaterial>,
    pub wire_on: Handle<StandardMaterial>,
    pub exit: Handle<StandardMaterial>,
    pub lamp: Handle<StandardMaterial>,
    pub plate: Handle<StandardMaterial>,
    pub metal: Handle<StandardMaterial>,
    pub cube_mesh: Handle<Mesh>,
    pub unit_cube: Handle<Mesh>,
    pub beam: Handle<Mesh>,
    pub glow: Handle<Mesh>,
    pub blob: Handle<Mesh>,
}

fn image(n: u32, data: Vec<u8>, repeat: bool) -> Image {
    let mut img = Image::new(
        Extent3d { width: n, height: n, depth_or_array_layers: 1 },
        TextureDimension::D2,
        data,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::default(),
    );
    let mode = if repeat { ImageAddressMode::Repeat } else { ImageAddressMode::ClampToEdge };
    img.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor {
        address_mode_u: mode,
        address_mode_v: mode,
        anisotropy_clamp: 8,
        ..ImageSamplerDescriptor::linear()
    });
    img
}

fn hash(x: u32, y: u32) -> f32 {
    let mut h = x.wrapping_mul(374761393) ^ y.wrapping_mul(668265263);
    h = (h ^ (h >> 13)).wrapping_mul(1274126177);
    (h ^ (h >> 16)) as f32 / u32::MAX as f32
}

/// One metre of Aperture panelling: four panels with dark seams, a little grain.
fn panel_texture(base: [f32; 3], seam: f32, grain: f32) -> Image {
    const N: u32 = 256;
    let mut data = Vec::with_capacity((N * N * 4) as usize);
    for y in 0..N {
        for x in 0..N {
            let (px, py) = (x % (N / 4), y % (N / 4));
            let edge = px.min(py).min(N / 4 - 1 - px).min(N / 4 - 1 - py);
            let shade = match edge {
                0 => seam,
                1 => seam * 0.4 + 0.6,
                2 => 0.94,
                _ => 1.0,
            };
            let g = 1.0 - grain + grain * 2.0 * hash(x / 2, y / 2);
            let v = |c: f32| ((c * shade * g).clamp(0.0, 1.0) * 255.0) as u8;
            data.extend_from_slice(&[v(base[0]), v(base[1]), v(base[2]), 255]);
        }
    }
    image(N, data, true)
}

/// Grating: a grid of bars (holes are see-through).
fn grate_texture() -> Image {
    const N: u32 = 128;
    let mut data = Vec::with_capacity((N * N * 4) as usize);
    for y in 0..N {
        for x in 0..N {
            let bar = x % 16 < 3 || y % 16 < 3;
            let a = if bar { 255 } else { 0 };
            data.extend_from_slice(&[70, 72, 76, a]);
        }
    }
    image(N, data, true)
}

/// A storage cube's face: grey, dark corner brackets, a lit circle in the middle.
fn cube_texture(accent: [u8; 3]) -> Image {
    const N: u32 = 128;
    let mut data = Vec::with_capacity((N * N * 4) as usize);
    for y in 0..N {
        for x in 0..N {
            let (fx, fy) = (x as f32 / N as f32 - 0.5, y as f32 / N as f32 - 0.5);
            let r = (fx * fx + fy * fy).sqrt();
            let edge = fx.abs().max(fy.abs());
            let corner = fx.abs() > 0.3 && fy.abs() > 0.3;
            let c: [u8; 3] = if edge > 0.46 || corner {
                [62, 64, 68]
            } else if (0.13..0.19).contains(&r) {
                accent
            } else if r < 0.13 {
                [200, 202, 205]
            } else {
                let g = 168 + (hash(x, y) * 10.0) as u8;
                [g, g, g + 3]
            };
            data.extend_from_slice(&[c[0], c[1], c[2], 255]);
        }
    }
    image(N, data, false)
}

/// Box mesh with world-space UVs (one texture tile per metre).
pub fn box_mesh(b: &Aabb) -> Mesh {
    box_faces(b, |_| true)
}

/// The faces of a box whose outward normal passes `keep`, with world-space UVs.
pub fn box_faces(b: &Aabb, keep: impl Fn(Vec3) -> bool) -> Mesh {
    let (min, max) = (b.min, b.max);
    let mut pos = Vec::new();
    let mut nrm = Vec::new();
    let mut uv = Vec::new();
    let mut idx: Vec<u32> = Vec::new();
    let faces: [(Vec3, [Vec3; 4]); 6] = [
        (Vec3::X, [Vec3::new(max.x, min.y, max.z), Vec3::new(max.x, min.y, min.z), Vec3::new(max.x, max.y, min.z), Vec3::new(max.x, max.y, max.z)]),
        (Vec3::NEG_X, [Vec3::new(min.x, min.y, min.z), Vec3::new(min.x, min.y, max.z), Vec3::new(min.x, max.y, max.z), Vec3::new(min.x, max.y, min.z)]),
        (Vec3::Y, [Vec3::new(min.x, max.y, max.z), Vec3::new(max.x, max.y, max.z), Vec3::new(max.x, max.y, min.z), Vec3::new(min.x, max.y, min.z)]),
        (Vec3::NEG_Y, [Vec3::new(min.x, min.y, min.z), Vec3::new(max.x, min.y, min.z), Vec3::new(max.x, min.y, max.z), Vec3::new(min.x, min.y, max.z)]),
        (Vec3::Z, [Vec3::new(min.x, min.y, max.z), Vec3::new(max.x, min.y, max.z), Vec3::new(max.x, max.y, max.z), Vec3::new(min.x, max.y, max.z)]),
        (Vec3::NEG_Z, [Vec3::new(max.x, min.y, min.z), Vec3::new(min.x, min.y, min.z), Vec3::new(min.x, max.y, min.z), Vec3::new(max.x, max.y, min.z)]),
    ];
    for (n, corners) in faces {
        if !keep(n) {
            continue;
        }
        let base = pos.len() as u32;
        for c in corners {
            pos.push(c.to_array());
            nrm.push(n.to_array());
            let t = if n.x.abs() > 0.5 {
                Vec2::new(c.z, c.y)
            } else if n.y.abs() > 0.5 {
                Vec2::new(c.x, c.z)
            } else {
                Vec2::new(c.x, c.y)
            };
            uv.push([t.x, -t.y]);
        }
        idx.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
    }
    Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default())
        .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, pos)
        .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, nrm)
        .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, uv)
        .with_inserted_indices(Indices::U32(idx))
}

/// A unit cube whose every face shows the whole texture (for cubes).
fn cube_mesh(half: f32) -> Mesh {
    Cuboid::new(half * 2.0, half * 2.0, half * 2.0).into()
}

pub fn setup(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut images: ResMut<Assets<Image>>,
    extracted: Res<Extracted>,
) {
    let tex = |images: &mut Assets<Image>, name: &str, fallback: Image| -> Handle<Image> {
        match extracted.texture(name) {
            Some(img) => images.add(img),
            None => images.add(fallback),
        }
    };
    let white_tex = tex(&mut images, "white_ceiling", panel_texture([0.93, 0.93, 0.95], 0.62, 0.025));
    let white_floor_tex = tex(&mut images, "white_floor", panel_texture([0.78, 0.79, 0.80], 0.55, 0.04));
    let black_tex = tex(&mut images, "black_panel", panel_texture([0.16, 0.165, 0.18], 0.45, 0.06));
    let black_floor_tex = tex(&mut images, "black_floor", panel_texture([0.13, 0.13, 0.14], 0.45, 0.06));
    // Portal's tile textures are four tiles across; a tile is 60 cm.
    let tiles = bevy::math::Affine2::from_scale(Vec2::splat(1.0 / 2.4));
    let grid = images.add(crate::grid_texture());
    let grate_tex = images.add(grate_texture());
    let cube_tex = [
        images.add(cube_texture([235, 120, 30])),
        images.add(cube_texture([235, 110, 170])),
        images.add(cube_texture([60, 200, 230])),
    ];
    let std = |base: Color, tex: Option<Handle<Image>>, rough: f32, metal: f32| StandardMaterial {
        base_color: base,
        base_color_texture: tex,
        perceptual_roughness: rough,
        metallic: metal,
        ..default()
    };
    let glow = |c: LinearRgba, alpha: f32| StandardMaterial {
        // Unlit: the base colour is what shows.
        base_color: Color::LinearRgba(LinearRgba { alpha, ..c }),
        unlit: true,
        alpha_mode: if alpha < 1.0 { AlphaMode::Add } else { AlphaMode::Opaque },
        double_sided: true,
        cull_mode: None,
        ..default()
    };
    let tiled = |m: StandardMaterial| StandardMaterial { uv_transform: tiles, ..m };
    let mats = ChamberMats {
        white: materials.add(tiled(std(Color::srgb(1.08, 1.08, 1.1), Some(white_tex), 0.7, 0.0))),
        black: materials.add(tiled(std(Color::srgb(0.75, 0.75, 0.78), Some(black_tex), 0.45, 0.4))),
        white_floor: materials.add(tiled(std(Color::srgb(1.0, 1.0, 1.0), Some(white_floor_tex), 0.8, 0.0))),
        black_floor: materials.add(tiled(std(Color::srgb(0.8, 0.8, 0.82), Some(black_floor_tex), 0.5, 0.4))),
        glass: materials.add(StandardMaterial {
            base_color: Color::srgba(0.65, 0.85, 0.95, 0.18),
            alpha_mode: AlphaMode::Blend,
            perceptual_roughness: 0.05,
            reflectance: 0.6,
            double_sided: true,
            cull_mode: None,
            ..default()
        }),
        grate: materials.add(StandardMaterial {
            base_color: Color::WHITE,
            base_color_texture: Some(grate_tex),
            alpha_mode: AlphaMode::Mask(0.5),
            metallic: 0.6,
            perceptual_roughness: 0.5,
            double_sided: true,
            cull_mode: None,
            ..default()
        }),
        runner: materials.add(StandardMaterial {
            base_color: Color::srgb(0.86, 0.10, 0.07),
            base_color_texture: Some(grid.clone()),
            emissive: LinearRgba::rgb(0.25, 0.01, 0.0),
            perceptual_roughness: 0.5,
            ..default()
        }),
        prop: materials.add(std(Color::srgb(0.55, 0.58, 0.62), None, 0.5, 0.4)),
        roof: materials.add(std(Color::srgb(0.80, 0.81, 0.83), Some(grid.clone()), 0.9, 0.0)),
        wall: materials.add(std(Color::srgb(0.95, 0.95, 0.96), Some(grid), 0.8, 0.0)),
        dark: materials.add(std(Color::srgb(0.10, 0.10, 0.11), None, 0.6, 0.3)),
        button_off: materials.add(StandardMaterial {
            base_color: Color::srgb(0.75, 0.12, 0.08),
            emissive: LinearRgba::rgb(0.3, 0.02, 0.0),
            perceptual_roughness: 0.4,
            ..default()
        }),
        button_on: materials.add(StandardMaterial {
            base_color: Color::srgb(1.0, 0.55, 0.1),
            emissive: LinearRgba::rgb(3.0, 1.2, 0.1),
            ..default()
        }),
        cube: cube_tex.clone().map(|t| materials.add(std(Color::WHITE, Some(t), 0.55, 0.2))),
        cube_held: cube_tex.map(|t| materials.add(StandardMaterial { emissive: LinearRgba::rgb(0.10, 0.16, 0.26), ..std(Color::WHITE, Some(t), 0.55, 0.2) })),
        cube_fizzle: materials.add(StandardMaterial {
            base_color: Color::srgba(0.1, 0.1, 0.1, 0.6),
            emissive: LinearRgba::rgb(0.6, 0.9, 1.4),
            alpha_mode: AlphaMode::Blend,
            ..default()
        }),
        cube_coat: [
            materials.add(std(Color::srgb(0.15, 0.45, 1.0), None, 0.15, 0.0)),
            materials.add(std(Color::srgb(1.0, 0.45, 0.05), None, 0.15, 0.0)),
        ],
        fizzler: materials.add(glow(LinearRgba::rgb(0.25, 0.55, 1.1), 0.25)),
        bridge: materials.add(glow(LinearRgba::rgb(0.25, 0.75, 1.3), 0.45)),
        laser: materials.add(glow(LinearRgba::rgb(6.0, 0.3, 0.2), 1.0)),
        laser_glow: materials.add(glow(LinearRgba::rgb(1.2, 0.05, 0.03), 0.3)),
        lens_off: materials.add(std(Color::srgb(0.25, 0.08, 0.06), None, 0.2, 0.0)),
        lens_on: materials.add(StandardMaterial { base_color: Color::srgb(1.0, 0.4, 0.2), emissive: LinearRgba::rgb(5.0, 1.2, 0.4), ..default() }),
        paint: [
            materials.add(StandardMaterial { base_color: Color::srgb(0.1, 0.42, 1.0), emissive: LinearRgba::rgb(0.02, 0.08, 0.3), perceptual_roughness: 0.12, ..default() }),
            materials.add(StandardMaterial { base_color: Color::srgb(1.0, 0.42, 0.04), emissive: LinearRgba::rgb(0.3, 0.08, 0.0), perceptual_roughness: 0.12, ..default() }),
            materials.add(StandardMaterial { base_color: Color::srgb(0.97, 0.97, 0.97), perceptual_roughness: 0.25, ..default() }),
        ],
        goo: materials.add(StandardMaterial {
            base_color: Color::srgba(0.25, 0.22, 0.05, 0.92),
            emissive: LinearRgba::rgb(0.05, 0.06, 0.0),
            alpha_mode: AlphaMode::Blend,
            perceptual_roughness: 0.1,
            ..default()
        }),
        wire_off: materials.add(StandardMaterial { base_color: Color::srgb(0.1, 0.35, 0.8), emissive: LinearRgba::rgb(0.1, 0.4, 1.2), ..default() }),
        wire_on: materials.add(StandardMaterial { base_color: Color::srgb(1.0, 0.55, 0.1), emissive: LinearRgba::rgb(2.5, 1.0, 0.1), ..default() }),
        exit: materials.add(glow(LinearRgba::rgb(0.6, 1.2, 0.7), 0.25)),
        lamp: materials.add(StandardMaterial { base_color: Color::WHITE, emissive: LinearRgba::rgb(6.0, 6.0, 5.6), ..default() }),
        plate: materials.add(std(Color::srgb(0.85, 0.86, 0.88), None, 0.4, 0.3)),
        metal: materials.add(std(Color::srgb(0.18, 0.19, 0.21), None, 0.4, 0.8)),
        cube_mesh: meshes.add(cube_mesh(CUBE_HALF)),
        unit_cube: meshes.add(Cuboid::new(1.0, 1.0, 1.0)),
        beam: meshes.add(Cylinder::new(0.025, 1.0)),
        glow: meshes.add(Cylinder::new(0.08, 1.0)),
        blob: meshes.add(Sphere::new(1.0).mesh().ico(1).unwrap()),
    };
    commands.insert_resource(mats);
    commands.insert_resource(Pools::default());
}

fn mat_for(m: &ChamberMats, mat: Mat) -> Handle<StandardMaterial> {
    match mat {
        Mat::White => m.white.clone(),
        Mat::Black => m.black.clone(),
        Mat::Glass => m.glass.clone(),
        Mat::Grate => m.grate.clone(),
        Mat::Runner => m.runner.clone(),
        Mat::Prop => m.prop.clone(),
        Mat::Roof => m.roof.clone(),
        Mat::Wall => m.wall.clone(),
        Mat::Concrete => m.roof.clone(),
    }
}

fn geom(commands: &mut Commands, mesh: Handle<Mesh>, mat: Handle<StandardMaterial>, tf: Transform) -> Entity {
    commands.spawn((ChamberGeom, Mesh3d(mesh), MeshMaterial3d(mat), tf)).id()
}

/// Spawn everything for the sim's chamber.
pub fn spawn(commands: &mut Commands, meshes: &mut Assets<Mesh>, m: &ChamberMats, sim: &Sim, pools: &mut Pools, models: &crate::props::PropModels) {
    *pools = Pools { gel_version: u64::MAX, ..default() };
    let l = &sim.level;
    // Portal's models where they were extracted; the stand-ins below for the rest.
    let r = crate::props::spawn(commands, models, sim);
    // The boxes behind a laser or bridge (theirs now, when the emitter's a model).
    let emitter_modelled = |b: &Aabb| {
        (r.lasers && sim.lasers.iter().any(|x| aperture::geom::contains(b, x.at - x.dir * 0.32)))
            || (r.bridges && sim.bridges.iter().any(|x| aperture::geom::contains(b, x.at - x.dir * 0.2)))
    };
    // The chamber's boxes (and the boxes of emitters and the like).
    for s in &sim.statics {
        match s.owner {
            Owner::Level(_) if matches!(s.mat, Mat::White | Mat::Black) => {
                // Floor tiles on top, panels on the sides and underneath.
                let (side, top) = if s.mat == Mat::White { (&m.white, &m.white_floor) } else { (&m.black, &m.black_floor) };
                geom(commands, meshes.add(box_faces(&s.b, |n| n.y < 0.5)), side.clone(), Transform::default());
                geom(commands, meshes.add(box_faces(&s.b, |n| n.y >= 0.5)), top.clone(), Transform::default());
            }
            Owner::Emitter if emitter_modelled(&s.b) => {}
            Owner::Catcher(_) if r.catchers => {}
            Owner::Level(_) | Owner::Emitter | Owner::Catcher(_) => {
                let mat = if matches!(s.owner, Owner::Level(_)) { mat_for(m, s.mat) } else { m.metal.clone() };
                let e = geom(commands, meshes.add(box_mesh(&s.b)), mat, Transform::default());
                if s.mat == Mat::Glass {
                    commands.entity(e).insert(NotShadowCaster);
                }
            }
            _ => {}
        }
    }
    for (b, mat) in &l.decor {
        geom(commands, meshes.add(box_mesh(b)), mat_for(m, *mat), Transform::default());
    }
    // Mirror's Edge's fixtures.
    for (i, f) in l.fixtures.iter().enumerate() {
        match *f {
            Fixture::ZipLine { a, b } | Fixture::SwingPole { a, b } => {
                let thick = if matches!(f, Fixture::ZipLine { .. }) { 0.025 } else { 0.06 };
                let len = a.distance(b);
                let rot = Quat::from_rotation_arc(Vec3::Z, (b - a) / len);
                geom(commands, meshes.add(Cuboid::new(thick, thick, len)), m.metal.clone(), Transform::from_translation((a + b) * 0.5).with_rotation(rot));
            }
            Fixture::Ladder(lad) => {
                for (a, b, thick) in lad.rods() {
                    let len = a.distance(b);
                    let rot = Quat::from_rotation_arc(Vec3::Z, (b - a) / len);
                    geom(commands, meshes.add(Cuboid::new(thick, thick, len)), m.metal.clone(), Transform::from_translation((a + b) * 0.5).with_rotation(rot));
                }
            }
            Fixture::Door { b, n } => {
                let hinge = Vec3::new(b.min.x, b.min.y, (b.min.z + b.max.z) * 0.5);
                let size = b.max - b.min;
                commands
                    .spawn((ChamberGeom, Vis::Swing { fixture: i, hinge, away: -n }, Transform::from_translation(hinge), Visibility::default()))
                    .with_child((
                        Mesh3d(meshes.add(Cuboid::new(size.x, size.y, size.z))),
                        MeshMaterial3d(m.prop.clone()),
                        Transform::from_translation(Vec3::new(size.x * 0.5, size.y * 0.5, 0.0)),
                    ));
            }
            Fixture::BarbedWire { b } => {
                geom(commands, meshes.add(box_mesh(&b)), m.metal.clone(), Transform::default());
            }
            _ => {}
        }
    }
    // Buttons: a dark base, a red top that goes down and lights when pressed.
    let base = meshes.add(Cylinder::new(0.95, BUTTON_TOP));
    let top = meshes.add(Cylinder::new(0.72, 0.06));
    for (i, b) in sim.buttons.iter().enumerate().filter(|_| !r.buttons) {
        geom(commands, base.clone(), m.dark.clone(), Transform::from_translation(b.at + Vec3::Y * BUTTON_TOP * 0.5));
        let e = geom(commands, top.clone(), m.button_off.clone(), Transform::from_translation(b.at + Vec3::Y * (BUTTON_TOP + 0.03)));
        commands.entity(e).insert(Vis::Button(i));
        if b.cube_only {
            // Cube-only buttons wear a ring of black.
            geom(commands, meshes.add(Torus::new(0.74, 0.84)), m.dark.clone(), Transform::from_translation(b.at + Vec3::Y * (BUTTON_TOP + 0.01)));
        }
    }
    let post = meshes.add(Cylinder::new(0.08, 1.0));
    let knob = meshes.add(Cylinder::new(0.14, 0.08));
    for (i, p) in sim.pedestals.iter().enumerate().filter(|_| !r.pedestals) {
        geom(commands, post.clone(), m.plate.clone(), Transform::from_translation(p.at + Vec3::Y * 0.5));
        geom(commands, meshes.add(Cuboid::new(0.36, 0.08, 0.36)), m.dark.clone(), Transform::from_translation(p.at + Vec3::Y * 0.96));
        let e = geom(commands, knob.clone(), m.button_off.clone(), Transform::from_translation(p.at + Vec3::Y * 1.03));
        commands.entity(e).insert(Vis::Pedestal(i));
    }
    // Doors: two halves that slide apart across the doorway.
    for (i, d) in sim.doors.iter().enumerate().filter(|_| !r.doors) {
        let s = d.b.size();
        let along_x = s.x >= s.z;
        let half_size = if along_x { Vec3::new(s.x * 0.5, s.y, s.z) } else { Vec3::new(s.x, s.y, s.z * 0.5) };
        for side in [-1.0f32, 1.0] {
            let e = geom(commands, meshes.add(Cuboid::from_size(half_size)), m.black.clone(), Transform::default());
            commands.entity(e).insert(Vis::Door { door: i, side });
        }
    }
    for (i, f) in sim.fizzlers.iter().enumerate() {
        let e = geom(commands, meshes.add(box_mesh(&f.b)), m.fizzler.clone(), Transform::default());
        commands.entity(e).insert((Vis::Fizzler(i), NotShadowCaster));
        // Emitter posts at both ends of its long side.
        if r.fizzlers {
            continue;
        }
        let s = f.b.size();
        let c = f.b.center();
        let along = if s.x >= s.z { Vec3::X } else { Vec3::Z };
        let half = (s.x.max(s.z)) * 0.5;
        for sign in [-1.0f32, 1.0] {
            let p = c + along * sign * (half + 0.06);
            geom(commands, meshes.add(Cuboid::new(0.14, s.y, 0.14)), m.metal.clone(), Transform::from_translation(p));
        }
    }
    let plate = meshes.add(Cuboid::new(PLATE_HALF * 2.0, PLATE_TOP, PLATE_HALF * 2.0));
    let arrow = meshes.add(Cuboid::new(0.25, 0.01, 0.9));
    for (i, p) in sim.plates.iter().enumerate() {
        geom(commands, meshes.add(Cuboid::new(PLATE_HALF * 2.2, 0.04, PLATE_HALF * 2.2)), m.dark.clone(), Transform::from_translation(p.at + Vec3::Y * 0.02));
        let to = p.target - p.at;
        let yaw = (-to.x).atan2(-to.z);
        let e = commands
            .spawn((ChamberGeom, Transform::from_translation(p.at + Vec3::Y * 0.04).with_rotation(Quat::from_rotation_y(yaw)), Visibility::default()))
            .with_children(|c| {
                c.spawn((Vis::PlateFlap { plate: i, part: 0 }, Mesh3d(plate.clone()), MeshMaterial3d(m.plate.clone()), Transform::from_xyz(0.0, PLATE_TOP * 0.5, 0.0)));
                c.spawn((Vis::PlateFlap { plate: i, part: 1 }, Mesh3d(arrow.clone()), MeshMaterial3d(m.wire_on.clone()), Transform::from_xyz(0.0, PLATE_TOP + 0.006, 0.0)));
            })
            .id();
        let _ = e;
    }
    let lens = meshes.add(Sphere::new(0.22).mesh().uv(16, 10));
    for (i, c) in sim.catchers.iter().enumerate().filter(|_| !r.catchers) {
        let e = geom(commands, lens.clone(), m.lens_off.clone(), Transform::from_translation(c.at));
        commands.entity(e).insert(Vis::Catcher(i));
    }
    for (i, rl) in sim.relays.iter().enumerate() {
        if r.relays {
            // The model's lens glows when a laser passes through it.
            let e = geom(commands, meshes.add(Sphere::new(0.14)), m.lens_on.clone(), Transform::from_translation(rl.at + Vec3::Y * 0.75));
            commands.entity(e).insert((Vis::RelayGlow(i), NotShadowCaster));
            continue;
        }
        geom(commands, meshes.add(Cylinder::new(0.12, 2.0)), m.metal.clone(), Transform::from_translation(rl.at + Vec3::Y * 1.0));
        let e = geom(commands, meshes.add(Cylinder::new(0.2, 0.5)), m.lens_off.clone(), Transform::from_translation(rl.at + Vec3::Y * 1.0));
        commands.entity(e).insert(Vis::Relay(i));
    }
    for d in &sim.dispensers {
        let paint = match d.paint {
            Paint::Bounce => m.paint[0].clone(),
            Paint::Speed => m.paint[1].clone(),
            _ => m.paint[2].clone(),
        };
        let rot = Quat::from_rotation_arc(Vec3::Y, d.dir);
        geom(commands, meshes.add(Cylinder::new(0.3, 0.2)), paint, Transform::from_translation(d.at + d.dir * 0.02).with_rotation(rot));
    }
    for l in sim.lasers.iter().filter(|_| !r.lasers) {
        let rot = Quat::from_rotation_arc(Vec3::Y, l.dir);
        geom(commands, meshes.add(Cylinder::new(0.16, 0.06)), m.lens_on.clone(), Transform::from_translation(l.at + l.dir * 0.01).with_rotation(rot));
    }
    for b in sim.bridges.iter().filter(|_| !r.bridges) {
        let w = b.dir.cross(b.up).normalize_or_zero();
        let rot = Quat::from_mat3(&Mat3::from_cols(w, b.up, -b.dir));
        geom(commands, meshes.add(Cuboid::new(BRIDGE_HALF_WIDTH * 2.0, 0.05, 0.04)), m.bridge.clone(), Transform::from_translation(b.at + b.dir * 0.03 - b.up * 0.03).with_rotation(rot));
    }
    // Droppers: a tube above the drop point, and a hatch under it.
    for (i, d) in sim.droppers.iter().enumerate().filter(|_| !r.droppers) {
        let top = d.at + Vec3::Y * (CUBE_HALF + 0.1);
        for (off, size) in [
            (Vec3::new(0.55, 1.5, 0.0), Vec3::new(0.1, 3.0, 1.2)),
            (Vec3::new(-0.55, 1.5, 0.0), Vec3::new(0.1, 3.0, 1.2)),
            (Vec3::new(0.0, 1.5, 0.55), Vec3::new(1.2, 3.0, 0.1)),
            (Vec3::new(0.0, 1.5, -0.55), Vec3::new(1.2, 3.0, 0.1)),
        ] {
            geom(commands, meshes.add(Cuboid::from_size(size)), m.white.clone(), Transform::from_translation(top + off));
        }
        for side in [-1.0f32, 1.0] {
            let e = geom(commands, meshes.add(Cuboid::new(0.5, 0.05, 1.0)), m.dark.clone(), Transform::from_translation(top + Vec3::new(side * 0.25, 0.0, 0.0)));
            commands.entity(e).insert(Vis::Hatch { dropper: i, side });
        }
    }
    for g in &sim.goo {
        let top = Aabb::new(Vec3::new(g.min.x, g.max.y - 0.3, g.min.z), g.max);
        let e = geom(commands, meshes.add(box_mesh(&top)), m.goo.clone(), Transform::default());
        commands.entity(e).insert(NotShadowCaster);
    }
    // Indicator lights along each wire.
    let dot = meshes.add(Cuboid::new(0.07, 0.07, 0.07));
    for w in &l.wires {
        for seg in w.points.windows(2) {
            let (a, b) = (seg[0], seg[1]);
            let n = (a.distance(b) / 0.35).floor().max(1.0) as usize;
            for k in 0..=n {
                let p = a.lerp(b, k as f32 / n as f32);
                let e = geom(commands, dot.clone(), m.wire_off.clone(), Transform::from_translation(p));
                commands.entity(e).insert((Vis::Wire(w.sig), NotShadowCaster));
            }
        }
    }
    if let Some(x) = l.exit {
        let e = geom(commands, meshes.add(box_mesh(&x)), m.exit.clone(), Transform::default());
        commands.entity(e).insert(NotShadowCaster);
    }
    // Lamps: a glowing panel and a light.
    for p in &l.lamps {
        geom(commands, meshes.add(Cuboid::new(1.6, 0.06, 0.5)), m.lamp.clone(), Transform::from_translation(*p));
        commands.spawn((
            ChamberGeom,
            PointLight { intensity: 900_000.0, range: 40.0, radius: 0.3, shadow_maps_enabled: false, color: Color::srgb(1.0, 0.97, 0.92), ..default() },
            Transform::from_translation(*p - Vec3::Y * 0.3),
            // The world, her own legs, and her third-person body (seen through portals).
            bevy::camera::visibility::RenderLayers::from_layers(&[0, crate::portal_render::PLAYER_LAYER, crate::body3p::BODY_LAYER]),
        ));
    }
    for (paint, mat) in [(Paint::Bounce, &m.paint[0]), (Paint::Speed, &m.paint[1]), (Paint::Portal, &m.paint[2])] {
        let mesh = meshes.add(gel_mesh(&GelMap::default(), paint));
        commands.spawn((ChamberGeom, GelVis(paint), Mesh3d(mesh), MeshMaterial3d(mat.clone()), Transform::default(), NotShadowCaster));
    }
}

/// How far gel is drawn off its surface, at most (portal surfaces sit above it).
pub const GEL_MAX_LIFT: f32 = 0.008;
const _: () = assert!(GEL_MAX_LIFT < crate::portal_render::SURFACE_LIFT);

/// The paint of one kind, as a mesh: a square per cell, a hair off its surface; cells at the
/// edge of a patch are drawn smaller and turned, for a blobby edge.
fn gel_mesh(gel: &GelMap, paint: Paint) -> Mesh {
    let mut pos = Vec::new();
    let mut nrm = Vec::new();
    let mut idx = Vec::new();
    for (k, p) in &gel.cells {
        if *p != paint {
            continue;
        }
        let (c, n) = GelMap::cell_center(k);
        let (u, v) = GelMap::cell_axes(k);
        let neighbours = [(1, 0), (-1, 0), (0, 1), (0, -1)].iter().filter(|(di, dj)| gel.cells.get(&(k.0, k.1, k.2 + di, k.3 + dj)) == Some(p)).count();
        let jitter = ((k.2.wrapping_mul(73856093) ^ k.3.wrapping_mul(19349663)) & 1023) as f32 / 1023.0;
        let (half, turn) = if neighbours == 4 { (CELL * 0.5 + 0.01, 0.0) } else { (CELL * (0.38 + 0.12 * jitter), 0.6 * jitter) };
        let (cu, su) = (turn.cos(), turn.sin());
        let (uu, vv) = (u * cu + v * su, v * cu - u * su);
        let lift = n * (GEL_MAX_LIFT - 0.002 * jitter);
        let base = pos.len() as u32;
        for (a, b) in [(-1.0, -1.0), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)] {
            pos.push((c + lift + uu * a * half + vv * b * half).to_array());
            nrm.push(n.to_array());
        }
        // Wind towards the normal.
        let face = (uu.cross(vv)).dot(n) > 0.0;
        if face {
            idx.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
        } else {
            idx.extend_from_slice(&[base, base + 2, base + 1, base, base + 3, base + 2]);
        }
    }
    if pos.is_empty() {
        // No paint: one degenerate triangle (the renderer doesn't like empty meshes).
        pos = vec![[0.0, -1000.0, 0.0]; 3];
        nrm = vec![[0.0, 1.0, 0.0]; 3];
        idx = vec![0, 1, 2];
    }
    let n = pos.len();
    Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default())
        .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, pos)
        .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, nrm)
        .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, vec![[0.0f32, 0.0f32]; n])
        .with_inserted_indices(Indices::U32(idx))
}

fn pooled(commands: &mut Commands, pool: &mut Vec<Entity>, n: usize, spawn: impl Fn(&mut Commands) -> Entity) {
    while pool.len() < n {
        let e = spawn(commands);
        pool.push(e);
    }
}

/// Keep everything that moves in step with the sim.
#[allow(clippy::type_complexity)]
pub fn update(
    mut commands: Commands,
    game: Res<crate::Game>,
    time: Res<Time>,
    m: Res<ChamberMats>,
    mut pools: ResMut<Pools>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut vis: Query<(&Vis, &mut Transform, &mut Visibility, Option<&mut MeshMaterial3d<StandardMaterial>>)>,
    gel: Query<(&GelVis, &Mesh3d)>,
    models: Res<crate::props::PropModels>,
    mut cube_models: Query<(&mut Transform, &mut Visibility), Without<Vis>>,
) {
    let sim = &game.sim;
    let t = time.elapsed_secs();
    let lit = |on: bool, a: &Handle<StandardMaterial>, b: &Handle<StandardMaterial>| if on { a.clone() } else { b.clone() };
    for (v, mut tf, mut visible, mat) in &mut vis {
        let want: Option<Handle<StandardMaterial>> = match *v {
            Vis::Button(i) => {
                let s = &sim.buttons[i];
                let y = s.at.y + BUTTON_TOP + if s.pressed { -0.01 } else { 0.03 };
                tf.translation.y += (y - tf.translation.y) * 0.3;
                Some(lit(s.pressed, &m.button_on, &m.button_off))
            }
            Vis::Door { door, side } => {
                let d = &sim.doors[door];
                let s = d.b.size();
                let along = if s.x >= s.z { Vec3::X } else { Vec3::Z };
                let w = s.dot(along) * 0.5;
                let k = 1.0 - (1.0 - d.open).powi(2);
                tf.translation = d.b.center() + along * side * (w * 0.5 + w * k);
                *visible = if d.open >= 0.999 { Visibility::Hidden } else { Visibility::Inherited };
                None
            }
            Vis::Fizzler(i) => {
                *visible = if sim.fizzlers[i].on { Visibility::Inherited } else { Visibility::Hidden };
                None
            }
            Vis::PlateFlap { plate, part } => {
                // Flips up about its back edge when it fires.
                let f = sim.plates[plate].fired;
                let k = if f < 0.1 { f / 0.1 } else { (1.0 - (f - 0.1) / 0.5).max(0.0) };
                let r = Quat::from_rotation_x(k * 0.9);
                let local = Vec3::new(0.0, if part == 0 { PLATE_TOP * 0.5 } else { PLATE_TOP + 0.006 }, 0.0);
                let hinge = Vec3::new(0.0, 0.0, PLATE_HALF);
                tf.translation = hinge + r * (local - hinge);
                tf.rotation = r;
                None
            }
            Vis::Catcher(i) => Some(lit(sim.catchers[i].lit, &m.lens_on, &m.lens_off)),
            Vis::Relay(i) => Some(lit(sim.relays[i].lit, &m.lens_on, &m.lens_off)),
            Vis::RelayGlow(i) => {
                *visible = if sim.relays[i].lit { Visibility::Inherited } else { Visibility::Hidden };
                None
            }
            Vis::Pedestal(i) => Some(lit(sim.pedestals[i].left > 0.0, &m.button_on, &m.button_off)),
            Vis::Wire(sig) => Some(lit(sim.signals.get(sig).copied().unwrap_or(false), &m.wire_on, &m.wire_off)),
            Vis::Hatch { dropper, side } => {
                let d = &sim.droppers[dropper];
                let open = if d.opened < 0.7 { (d.opened / 0.15).min(1.0) * (1.0 - ((d.opened - 0.5) / 0.2).clamp(0.0, 1.0)) } else { 0.0 };
                let top = d.at + Vec3::Y * (CUBE_HALF + 0.1);
                let r = Quat::from_rotation_z(-side * open * FRAC_PI_2);
                tf.translation = top + Vec3::new(side * 0.5, 0.0, 0.0) + r * Vec3::new(-side * 0.25, 0.0, 0.0);
                tf.rotation = r;
                None
            }
            Vis::Swing { fixture, hinge, away } => {
                let open = game.ctrl.doors_open.get(fixture).copied().unwrap_or(0.0);
                let k = 1.0 - (1.0 - open).powi(3);
                let sign = if away.z < 0.0 { 1.0 } else { -1.0 };
                tf.translation = hinge;
                tf.rotation = Quat::from_rotation_y(sign * k * 100f32.to_radians());
                None
            }
            Vis::Pool => None,
        };
        if let (Some(w), Some(mut mm)) = (want, mat) {
            if mm.0 != w {
                mm.0 = w;
            }
        }
    }
    if pools.gel_version != sim.gel.version {
        pools.gel_version = sim.gel.version;
        for (g, mesh) in &gel {
            if let Some(mut mm) = meshes.get_mut(&mesh.0) {
                *mm = gel_mesh(&sim.gel, g.0);
            }
        }
    }

    // ---- pooled: cubes, beams, bridges, blobs
    let spawn_pooled = |c: &mut Commands, mesh: &Handle<Mesh>, mat: &Handle<StandardMaterial>, shadow: bool| {
        let mut e = c.spawn((ChamberGeom, Vis::Pool, Mesh3d(mesh.clone()), MeshMaterial3d(mat.clone()), Transform::default(), Visibility::Hidden));
        if !shadow {
            e.insert(NotShadowCaster);
        }
        e.id()
    };
    pooled(&mut commands, &mut pools.props, sim.props.len(), |c| spawn_pooled(c, &m.cube_mesh, &m.cube[0], true));
    while pools.prop_models.len() < pools.props.len() {
        let slot = pools.prop_models.len();
        let e = crate::props::spawn_cubes(&mut commands, &models, slot);
        pools.prop_models.push(e);
    }
    let beams: Vec<(Vec3, Vec3)> = sim.lasers.iter().filter(|l| l.on).flat_map(|l| l.path.iter().copied()).collect();
    pooled(&mut commands, &mut pools.beams, beams.len(), |c| spawn_pooled(c, &m.beam, &m.laser, false));
    pooled(&mut commands, &mut pools.glows, beams.len(), |c| spawn_pooled(c, &m.glow, &m.laser_glow, false));
    let segs: Vec<Aabb> = sim.bridges.iter().flat_map(|b| b.segs.iter().map(|s| s.aabb)).collect();
    pooled(&mut commands, &mut pools.bridges, segs.len(), |c| spawn_pooled(c, &m.unit_cube, &m.bridge, false));
    pooled(&mut commands, &mut pools.blobs, sim.blobs.len(), |c| spawn_pooled(c, &m.blob, &m.paint[0], false));

    let mut set = |e: Entity, tf: Option<Transform>, mat: Option<&Handle<StandardMaterial>>| {
        if let Ok((_, mut t, mut v, mm)) = vis.get_mut(e) {
            match tf {
                Some(x) => {
                    *t = x;
                    *v = Visibility::Inherited;
                }
                None => *v = Visibility::Hidden,
            }
            if let (Some(h), Some(mut mm)) = (mat, mm) {
                if mm.0 != *h {
                    mm.0 = h.clone();
                }
            }
        }
    };
    for (i, e) in pools.props.iter().enumerate() {
        // Portal's cube models, unless it's coated in gel or vaporising (the stand-in shows that).
        let model = pools.prop_models.get(i).copied().flatten();
        let modelled = sim.props.get(i).filter(|p| p.alive && p.fizzle.is_none() && p.coat.is_none()).map(|p| p.kind);
        if let Some(ents) = model {
            for (k, kind) in [CubeKind::Storage, CubeKind::Companion, CubeKind::Reflector].into_iter().enumerate() {
                if let Ok((mut tf, mut v)) = cube_models.get_mut(ents[k]) {
                    let want = modelled == Some(kind);
                    if want {
                        let p = &sim.props[i];
                        tf.translation = p.pos;
                        tf.rotation = p.rot;
                    }
                    let w = if want { Visibility::Inherited } else { Visibility::Hidden };
                    if *v != w {
                        *v = w;
                    }
                }
            }
            if modelled.is_some() {
                set(*e, None, None);
                continue;
            }
        }
        match sim.props.get(i) {
            Some(p) if p.alive => {
                let mut tf = Transform::from_translation(p.pos).with_rotation(p.rot);
                let mat = if let Some(f) = p.fizzle {
                    tf.scale = Vec3::splat((1.0 - f * 0.3).max(0.2));
                    &m.cube_fizzle
                } else {
                    let base = if sim.held == Some(i) { &m.cube_held } else { &m.cube };
                    match (p.coat, p.kind) {
                        (Some(Paint::Bounce), _) => &m.cube_coat[0],
                        (Some(Paint::Speed), _) => &m.cube_coat[1],
                        (_, CubeKind::Storage) => &base[0],
                        (_, CubeKind::Companion) => &base[1],
                        (_, CubeKind::Reflector) => &base[2],
                    }
                };
                set(*e, Some(tf), Some(mat));
            }
            _ => set(*e, None, None),
        }
    }
    let flicker = 1.0 + 0.15 * (t * 40.0).sin();
    for (i, (e, g)) in pools.beams.iter().zip(&pools.glows).enumerate() {
        match beams.get(i) {
            Some((a, b)) => {
                let d = *b - *a;
                let len = d.length().max(1e-3);
                let rot = Quat::from_rotation_arc(Vec3::Y, d / len);
                let tf = Transform { translation: (*a + *b) * 0.5, rotation: rot, scale: Vec3::new(1.0, len, 1.0) };
                set(*e, Some(tf), None);
                set(*g, Some(Transform { scale: Vec3::new(flicker, len, flicker), ..tf }), None);
            }
            None => {
                set(*e, None, None);
                set(*g, None, None);
            }
        }
    }
    for (i, e) in pools.bridges.iter().enumerate() {
        match segs.get(i) {
            Some(s) => set(*e, Some(Transform::from_translation(s.center()).with_scale(s.size())), None),
            None => set(*e, None, None),
        }
    }
    for (i, e) in pools.blobs.iter().enumerate() {
        match sim.blobs.get(i) {
            Some(b) => {
                let mat = match b.paint {
                    Paint::Bounce => &m.paint[0],
                    Paint::Speed => &m.paint[1],
                    _ => &m.paint[2],
                };
                let stretch = 1.0 + b.v.length() * 0.03;
                let rot = Quat::from_rotation_arc(Vec3::Y, b.v.normalize_or(Vec3::Y));
                set(*e, Some(Transform { translation: b.p, rotation: rot, scale: Vec3::new(b.r, b.r * stretch, b.r) }), Some(mat));
            }
            None => set(*e, None, None),
        }
    }
}
