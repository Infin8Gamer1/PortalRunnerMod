//! Mirror's Edge's materials remastered for ray tracing (#101): each keeps the game's own art,
//! and gets from what it's made of (the table in `assets/remaster/me.ron`) a physically based
//! detail layer (CC0 sets in `assets/remaster/surfaces/`) tiled at its real size, a roughness
//! and a metalness. Baked once per material and tiling, here, into the three maps Bevy's
//! material takes: colour, normal, and metal / roughness. Every Mirror's Edge texture gets a
//! mip chain too (the game's are read at one size), so nothing shimmers in the distance.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use bevy::asset::RenderAssetUsages;
use bevy::image::{ImageAddressMode, ImageFilterMode, ImageSampler, ImageSamplerDescriptor};
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
use me_assets::{MaterialSlot, Rgba};
use serde::Deserialize;

/// The biggest a baked map gets (a detail layer needs room to show).
const BAKED: u32 = 1024;
/// The brightest a colour may be (linear): fresh white paint reflects about this much. The
/// game's whites, made for its lightmaps, are brighter, and blow out under real light.
const WHITEST: f32 = 0.8;

#[derive(Deserialize, Clone, Debug)]
pub struct Surface {
    pub detail: Option<String>,
    pub size: f32,
    pub roughness: f32,
    pub metallic: f32,
    pub strength: f32,
}

#[derive(Deserialize)]
struct Rule {
    #[serde(rename = "match")]
    words: Vec<String>,
    surface: Option<String>,
}

#[derive(Deserialize)]
struct Table {
    surfaces: HashMap<String, Surface>,
    rules: Vec<Rule>,
}

/// A detail set: its maps (1024 square), and its albedo's and roughness's mean (what it varies
/// about).
struct Detail {
    size: u32,
    lum: Vec<f32>,
    normal: Vec<[f32; 2]>,
    rough: Vec<f32>,
}

struct Remaster {
    root: PathBuf,
    table: Table,
    details: HashMap<String, Option<Detail>>,
}

/// Where the remaster is: `assets/remaster` here, beside the game, or in the source tree.
fn root() -> Option<PathBuf> {
    let mut tries = vec![PathBuf::from("assets/remaster")];
    if let Ok(exe) = std::env::current_exe() {
        tries.extend(exe.ancestors().skip(1).take(4).map(|d| d.join("assets/remaster")));
    }
    tries.push(Path::new(env!("CARGO_MANIFEST_DIR")).join("assets/remaster"));
    tries.into_iter().find(|p| p.join("me.ron").is_file())
}

fn remaster() -> Option<&'static std::sync::Mutex<Remaster>> {
    static R: OnceLock<Option<std::sync::Mutex<Remaster>>> = OnceLock::new();
    R.get_or_init(|| {
        let root = root()?;
        let text = std::fs::read_to_string(root.join("me.ron")).ok()?;
        match ron::from_str::<Table>(&text) {
            Ok(table) => Some(std::sync::Mutex::new(Remaster { root, table, details: HashMap::new() })),
            Err(e) => {
                error!("assets/remaster/me.ron: {e}");
                None
            }
        }
    })
    .as_ref()
}

/// The surface a material's made of (by its path), if the table gives it one.
pub fn surface_of(path: &str) -> Option<Surface> {
    let r = remaster()?.lock().ok()?;
    let low = path.to_ascii_lowercase();
    let rule = r.table.rules.iter().find(|rule| rule.words.iter().any(|w| low.contains(&w.to_ascii_lowercase())))?;
    r.table.surfaces.get(rule.surface.as_ref()?).cloned()
}

fn srgb_to_linear(c: u8) -> f32 {
    let c = c as f32 / 255.0;
    if c <= 0.04045 { c / 12.92 } else { ((c + 0.055) / 1.055).powf(2.4) }
}

fn linear_to_srgb(c: f32) -> u8 {
    let c = c.clamp(0.0, 1.0);
    let s = if c <= 0.003_130_8 { c * 12.92 } else { 1.055 * c.powf(1.0 / 2.4) - 0.055 };
    (s * 255.0 + 0.5) as u8
}

fn load_detail(root: &Path, name: &str) -> Option<Detail> {
    let dir = root.join("surfaces").join(name);
    let open = |f: &str| image::open(dir.join(f)).map_err(|e| warn!("remaster {name}/{f}: {e}")).ok();
    let (a, n, r) = (open("albedo.jpg")?.to_rgb8(), open("normal.jpg")?.to_rgb8(), open("rough.jpg")?.to_luma8());
    let size = a.width();
    if a.height() != size || n.dimensions() != (size, size) || r.dimensions() != (size, size) {
        warn!("remaster {name}: its maps aren't all {size} square");
        return None;
    }
    // Brightness about its mean (1: average), roughness the same.
    let lum: Vec<f32> = a.pixels().map(|p| 0.2126 * srgb_to_linear(p[0]) + 0.7152 * srgb_to_linear(p[1]) + 0.0722 * srgb_to_linear(p[2])).collect();
    let mean = (lum.iter().sum::<f32>() / lum.len() as f32).max(1e-3);
    let lum = lum.into_iter().map(|l| l / mean).collect();
    let rough: Vec<f32> = r.pixels().map(|p| p[0] as f32 / 255.0).collect();
    let rmean = (rough.iter().sum::<f32>() / rough.len() as f32).max(1e-3);
    let rough = rough.into_iter().map(|x| x / rmean).collect();
    // Its normals' X and Y (OpenGL's: Y up the image). The game's maps are the other way up
    // (Unreal's, DirectX's): Y flipped to match them, so the two blend as one (and the material
    // reads the result as DirectX's: `flip_normal_map_y`).
    let normal = n.pixels().map(|p| [p[0] as f32 / 127.5 - 1.0, -(p[1] as f32 / 127.5 - 1.0)]).collect();
    Some(Detail { size, lum, normal, rough })
}

/// Bilinear sample of a channel grid `w` x `h` at (u, v), wrapping.
fn bilinear<T: Copy>(data: &[T], w: u32, h: u32, u: f32, v: f32, mix: impl Fn(T, T, f32) -> T) -> T {
    let x = u.rem_euclid(1.0) * w as f32 - 0.5;
    let y = v.rem_euclid(1.0) * h as f32 - 0.5;
    let (x0, y0) = (x.floor(), y.floor());
    let (fx, fy) = (x - x0, y - y0);
    let wrap = |a: f32, n: u32| (a as i64).rem_euclid(n as i64) as usize;
    let (xa, xb, ya, yb) = (wrap(x0, w), wrap(x0 + 1.0, w), wrap(y0, h), wrap(y0 + 1.0, h));
    let at = |x: usize, y: usize| data[y * w as usize + x];
    mix(mix(at(xa, ya), at(xb, ya), fx), mix(at(xa, yb), at(xb, yb), fx), fy)
}

fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

/// The three maps a remastered material takes.
pub struct Baked {
    pub color: Image,
    pub normal: Image,
    pub metal_rough: Image,
}

/// A material's maps, remastered as `surface` (its detail tiled `repeats` times across its
/// texture). Its diffuse map (else its plain colour) is the colour; its normal map, if any,
/// the normal, with the detail's over it; its specular map, if any, varies the roughness.
pub fn bake(slot: &MaterialSlot, surface: &Surface, repeats: u32) -> Option<Baked> {
    let base = slot.diffuse.as_ref()?;
    let m = remaster()?;
    let mut r = m.lock().ok()?;
    let detail = match &surface.detail {
        Some(name) => {
            let root = r.root.clone();
            r.details.entry(name.clone()).or_insert_with(|| load_detail(&root, name));
            r.details.get(name).and_then(|d| d.as_ref())
        }
        None => None,
    };
    // Big enough to show the detail on what's big (walls, roofs: a detail a metre or more
    // across), half that on props; never smaller than the game's.
    let own = base.width.max(base.height);
    let n = match detail {
        Some(_) if surface.size >= 1.0 && own >= BAKED / 2 => BAKED,
        Some(_) => own.max(BAKED / 2),
        None => own,
    };
    let (w, h) = (n, n);
    let to_lin = |t: &Rgba| -> Vec<[f32; 4]> {
        t.pixels.chunks_exact(4).map(|c| [srgb_to_linear(c[0]), srgb_to_linear(c[1]), srgb_to_linear(c[2]), c[3] as f32 / 255.0]).collect()
    };
    let mix4 = |a: [f32; 4], b: [f32; 4], t: f32| [lerp(a[0], b[0], t), lerp(a[1], b[1], t), lerp(a[2], b[2], t), lerp(a[3], b[3], t)];
    let mix2 = |a: [f32; 2], b: [f32; 2], t: f32| [lerp(a[0], b[0], t), lerp(a[1], b[1], t)];
    // (Tinted as the material instance has it: an orange catwalk.)
    let tint = slot.tint.unwrap_or([1.0; 3]);
    let base_lin: Vec<[f32; 4]> = to_lin(base).into_iter().map(|c| [c[0] * tint[0], c[1] * tint[1], c[2] * tint[2], c[3]]).collect();
    let normal_map: Option<(Vec<[f32; 2]>, u32, u32)> = slot.normal.as_ref().map(|t| {
        (t.pixels.chunks_exact(4).map(|c| [c[0] as f32 / 127.5 - 1.0, c[1] as f32 / 127.5 - 1.0]).collect(), t.width, t.height)
    });
    let spec: Option<(Vec<f32>, u32, u32)> = slot.specular.as_ref().map(|t| {
        let v: Vec<f32> = t.pixels.chunks_exact(4).map(|c| (c[0] as f32 * 0.299 + c[1] as f32 * 0.587 + c[2] as f32 * 0.114) / 255.0).collect();
        let mean = (v.iter().sum::<f32>() / v.len() as f32).max(1e-3);
        (v.into_iter().map(|x| x / mean).collect(), t.width, t.height)
    });
    let k = surface.strength;
    let reps = repeats.max(1) as f32;
    let mut color = vec![0u8; (w * h * 4) as usize];
    let mut normal = vec![0u8; (w * h * 4) as usize];
    let mut mr = vec![0u8; (w * h * 4) as usize];
    // (Rows split among threads: a 1024 square is a million pixels a map.)
    let threads = std::thread::available_parallelism().map_or(4, |n| n.get()).min(16);
    let rows_per = (h as usize).div_ceil(threads);
    std::thread::scope(|s| {
        for ((c_rows, n_rows), (m_rows, t)) in color
            .chunks_mut(rows_per * w as usize * 4)
            .zip(normal.chunks_mut(rows_per * w as usize * 4))
            .zip(mr.chunks_mut(rows_per * w as usize * 4).zip(0..))
        {
            let (base_lin, normal_map, spec) = (&base_lin, &normal_map, &spec);
            s.spawn(move || {
                let y0 = t * rows_per;
                for (k_row, ((c_row, n_row), m_row)) in c_rows.chunks_mut(w as usize * 4).zip(n_rows.chunks_mut(w as usize * 4)).zip(m_rows.chunks_mut(w as usize * 4)).enumerate() {
                    let y = y0 + k_row;
                    let v = (y as f32 + 0.5) / h as f32;
                    for x in 0..w as usize {
                        let u = (x as f32 + 0.5) / w as f32;
                        let b = bilinear(base_lin, base.width, base.height, u, v, mix4);
                        let (dl, dn, dr) = match detail {
                            Some(d) => (
                                bilinear(&d.lum, d.size, d.size, u * reps, v * reps, lerp),
                                bilinear(&d.normal, d.size, d.size, u * reps, v * reps, mix2),
                                bilinear(&d.rough, d.size, d.size, u * reps, v * reps, lerp),
                            ),
                            None => (1.0, [0.0, 0.0], 1.0),
                        };
                        // Colour: the game's, its brightness varied by the detail's, under real
                        // paint's white.
                        let shade = lerp(1.0, dl, k);
                        let mut rgb = [b[0] * shade, b[1] * shade, b[2] * shade];
                        let top = rgb[0].max(rgb[1]).max(rgb[2]);
                        if top > WHITEST {
                            let f = WHITEST / top;
                            rgb = [rgb[0] * f, rgb[1] * f, rgb[2] * f];
                        }
                        let o = x * 4;
                        c_row[o..o + 4].copy_from_slice(&[linear_to_srgb(rgb[0]), linear_to_srgb(rgb[1]), linear_to_srgb(rgb[2]), (b[3] * 255.0 + 0.5) as u8]);
                        // Normal: the game's, with the detail's slopes added (whiteout blend).
                        let g = normal_map.as_ref().map_or([0.0, 0.0], |(nm, nw, nh)| bilinear(nm, *nw, *nh, u, v, mix2));
                        let (nx, ny) = (g[0] + dn[0] * k, g[1] + dn[1] * k);
                        let nz = (1.0 - g[0] * g[0] - g[1] * g[1]).max(0.0).sqrt();
                        let len = (nx * nx + ny * ny + nz * nz).sqrt().max(1e-6);
                        let enc = |c: f32| ((c / len * 0.5 + 0.5) * 255.0 + 0.5) as u8;
                        n_row[o..o + 4].copy_from_slice(&[enc(nx), enc(ny), enc(nz), 255]);
                        // Roughness: the surface's, varied by the detail's and (shinier where it's
                        // brighter) the game's specular map; metalness the surface's.
                        let sp = spec.as_ref().map_or(1.0, |(sm, sw, sh)| bilinear(sm, *sw, *sh, u, v, lerp));
                        let rough = (surface.roughness * lerp(1.0, dr, k) * lerp(1.0, 1.0 / sp.max(0.25), 0.35)).clamp(0.04, 1.0);
                        m_row[o..o + 4].copy_from_slice(&[255, (rough * 255.0 + 0.5) as u8, (surface.metallic * 255.0 + 0.5) as u8, 255]);
                    }
                }
            });
        }
    });
    Some(Baked {
        color: with_mips(w, h, color, true),
        normal: with_mips(w, h, normal, false),
        metal_rough: with_mips(w, h, mr, false),
    })
}

/// An image with its mip chain (box-filtered; a colour map's in linear light), repeating, and
/// filtered across mips and at a slant (anisotropic).
pub fn with_mips(w: u32, h: u32, pixels: Vec<u8>, srgb: bool) -> Image {
    let mut data = pixels.clone();
    let (mut cw, mut ch, mut cur) = (w, h, pixels);
    let mut levels = 1;
    while cw > 1 || ch > 1 {
        let (nw, nh) = ((cw / 2).max(1), (ch / 2).max(1));
        let mut next = vec![0u8; (nw * nh * 4) as usize];
        for y in 0..nh {
            for x in 0..nw {
                for c in 0..4 {
                    let at = |xx: u32, yy: u32| cur[((yy.min(ch - 1) * cw + xx.min(cw - 1)) * 4 + c) as usize];
                    let px = [at(2 * x, 2 * y), at(2 * x + 1, 2 * y), at(2 * x, 2 * y + 1), at(2 * x + 1, 2 * y + 1)];
                    next[((y * nw + x) * 4 + c) as usize] = if srgb && c < 3 {
                        linear_to_srgb(px.iter().map(|&p| srgb_to_linear(p)).sum::<f32>() / 4.0)
                    } else {
                        ((px.iter().map(|&p| p as u32).sum::<u32>() + 2) / 4) as u8
                    };
                }
            }
        }
        data.extend_from_slice(&next);
        (cw, ch, cur) = (nw, nh, next);
        levels += 1;
    }
    let mut img = Image::new(
        Extent3d { width: w, height: h, depth_or_array_layers: 1 },
        TextureDimension::D2,
        data,
        if srgb { TextureFormat::Rgba8UnormSrgb } else { TextureFormat::Rgba8Unorm },
        RenderAssetUsages::default(),
    );
    img.texture_descriptor.mip_level_count = levels;
    img.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor {
        address_mode_u: ImageAddressMode::Repeat,
        address_mode_v: ImageAddressMode::Repeat,
        mag_filter: ImageFilterMode::Linear,
        min_filter: ImageFilterMode::Linear,
        mipmap_filter: ImageFilterMode::Linear,
        anisotropy_clamp: 16,
        ..default()
    });
    img
}

/// A game texture as it is, with mips (the colour map: `srgb`).
pub fn plain(t: &Rgba, srgb: bool) -> Image {
    with_mips(t.width, t.height, t.pixels.clone(), srgb)
}

/// The game's colour map tinted (`tint`, linear) and its white kept under real paint's.
pub fn tamed(t: &Rgba, tint: Option<[f32; 3]>) -> Rgba {
    let k = tint.unwrap_or([1.0; 3]);
    let pixels = t
        .pixels
        .chunks_exact(4)
        .flat_map(|c| {
            let rgb = [srgb_to_linear(c[0]) * k[0], srgb_to_linear(c[1]) * k[1], srgb_to_linear(c[2]) * k[2]];
            let top = rgb[0].max(rgb[1]).max(rgb[2]);
            let f = if top > WHITEST { WHITEST / top } else { 1.0 };
            [linear_to_srgb(rgb[0] * f), linear_to_srgb(rgb[1] * f), linear_to_srgb(rgb[2] * f), c[3]]
        })
        .collect();
    Rgba { width: t.width, height: t.height, pixels }
}

/// How many times a surface's detail tiles across a texture `m_per_uv` metres across (whole
/// times, so it tiles with the texture).
pub fn repeats(surface: &Surface, m_per_uv: f32) -> u32 {
    (m_per_uv / surface.size.max(0.01)).round().clamp(1.0, 64.0) as u32
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The committed table reads, every surface's detail set is there, and the Training map's
    /// materials get what they're made of.
    #[test]
    fn the_table_gives_materials_their_surfaces() {
        let r = remaster().expect("assets/remaster/me.ron reads");
        let root = r.lock().unwrap().root.clone();
        for (name, s) in &r.lock().unwrap().table.surfaces {
            if let Some(d) = &s.detail {
                assert!(load_detail(&root, d).is_some(), "{name}: its detail set {d}");
            }
        }
        let of = |path: &str| surface_of(path).map(|s| (s.detail, s.metallic));
        let detail = |d: &str| Some(d.to_string());
        assert_eq!(of("M_Rooftops.M_C_RooftopsMat_01"), Some((detail("concrete_floor"), 0.0)));
        assert_eq!(of("P_RooftopStructures.Materials.M_RooftopStructure_01"), Some((detail("concrete"), 0.0)));
        assert_eq!(of("P_Ventilation.AirductSystem_02.M_AirductSystem_02"), Some((detail("painted_metal"), 0.0)));
        assert_eq!(of("P_Renovation.Scaffolding_02.M_Scaffolding_02_Pipes"), Some((detail("brushed_metal"), 1.0)));
        assert_eq!(of("P_Renovation.LoadingPallet_01.M_LoadingPallet_01"), Some((detail("plywood"), 0.0)));
        assert_eq!(of("B_R_05.M_R_05_Facade_01"), Some((detail("plaster"), 0.0)));
        // The far city and plants keep their look.
        assert_eq!(of("B_Vista.SP00.M_Vista_Buildings"), None);
        assert_eq!(of("G_Vegetation.M_BushA_Leaves_White_01"), None);
    }

    /// A mip chain down to one pixel, a colour map's averaged in linear light.
    #[test]
    fn mips_go_down_to_a_pixel() {
        // Half black, half white: the last mip is mid-grey in light (sRGB 188), not 128.
        let px: Vec<u8> = (0..16 * 16).flat_map(|i| if i % 16 < 8 { [0, 0, 0, 255] } else { [255, 255, 255, 255] }).collect();
        let img = with_mips(16, 16, px, true);
        assert_eq!(img.texture_descriptor.mip_level_count, 5);
        let data = img.data.unwrap();
        assert_eq!(data.len(), (16 * 16 + 8 * 8 + 4 * 4 + 2 * 2 + 1) * 4);
        let last = &data[data.len() - 4..];
        assert!((185..=191).contains(&last[0]), "{last:?}");
    }
}
