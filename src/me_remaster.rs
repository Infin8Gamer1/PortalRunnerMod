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
    /// The colour parameter (its name) that tints it, whatever the material's called.
    #[serde(default)]
    pub tint: Option<String>,
    /// Its colour map is a mask: dark where it's black, the tint where it's white (an
    /// advert's graphic).
    #[serde(default)]
    pub mask: bool,
    /// How brightly it glows, its colour times this (backlit signs): 0, it doesn't.
    #[serde(default)]
    pub glow: f32,
    /// Its colour map packs other maps into its channels: `"grey_r_spec_b"`, its colour the
    /// red channel's grey, its specular the blue (the far city's buildings).
    #[serde(default)]
    pub pack: Option<String>,
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

/// sRGB to linear, by table (a million pixels a map: `powf` each was most of a level's load).
fn srgb_to_linear(c: u8) -> f32 {
    static T: OnceLock<[f32; 256]> = OnceLock::new();
    T.get_or_init(|| {
        std::array::from_fn(|i| {
            let c = i as f32 / 255.0;
            if c <= 0.04045 { c / 12.92 } else { ((c + 0.055) / 1.055).powf(2.4) }
        })
    })[c as usize]
}

/// Linear to sRGB, by table (4096 steps across 0-1: finer than 8 bits even in the darks).
fn linear_to_srgb(c: f32) -> u8 {
    const N: usize = 4096;
    static T: OnceLock<Vec<u8>> = OnceLock::new();
    let t = T.get_or_init(|| {
        (0..=N)
            .map(|i| {
                let c = i as f32 / N as f32;
                let s = if c <= 0.003_130_8 { c * 12.92 } else { 1.055 * c.powf(1.0 / 2.4) - 0.055 };
                (s * 255.0 + 0.5) as u8
            })
            .collect()
    });
    t[(c.clamp(0.0, 1.0) * N as f32 + 0.5) as usize]
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
    // (Unreal's, DirectX's): Y flipped to match them, so the two blend as one (and the result
    // written OpenGL's way up again).
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
    // (Tinted as the material instance has it: an orange catwalk; or as the surface says, by
    // a parameter of its own. A mask goes from near-black up to the tint.)
    let named = surface.tint.as_ref().and_then(|n| slot.vectors.iter().find(|(v, _)| v.eq_ignore_ascii_case(n)).map(|(_, c)| *c));
    // (Else as its material tints it, where on its map it does.)
    let (bw, bh) = (base.width.max(1), base.height.max(1));
    let tint_at = |i: usize| named.unwrap_or_else(|| slot.tint_at((i as u32 % bw) as f32 / bw as f32 + 0.5 / bw as f32, (i as u32 / bw) as f32 / bh as f32 + 0.5 / bh as f32));
    // (A packed colour map: grey from its red, specular from its blue.)
    let grey_r = surface.pack.as_deref() == Some("grey_r_spec_b");
    let packed_spec: Option<Rgba> = grey_r.then(|| Rgba { width: base.width, height: base.height, pixels: base.pixels.chunks_exact(4).flat_map(|c| [c[2], c[2], c[2], 255]).collect() });
    let base_lin: Vec<[f32; 4]> = to_lin(base)
        .into_iter()
        .map(|c| if grey_r { [c[0], c[0], c[0], c[3]] } else { c })
        .enumerate()
        .map(|(i, c)| {
            let tint = tint_at(i);
            if surface.mask {
                let m = 0.2126 * c[0] + 0.7152 * c[1] + 0.0722 * c[2];
                let k = |t: f32| lerp(0.02, t, m);
                [k(tint[0]), k(tint[1]), k(tint[2]), c[3]]
            } else {
                [c[0] * tint[0], c[1] * tint[1], c[2] * tint[2], c[3]]
            }
        })
        .collect();
    let normal_map: Option<(Vec<[f32; 2]>, u32, u32)> = slot.normal.as_ref().map(|t| {
        (t.pixels.chunks_exact(4).map(|c| [c[0] as f32 / 127.5 - 1.0, c[1] as f32 / 127.5 - 1.0]).collect(), t.width, t.height)
    });
    // The specular map: its brightness about its mean (varying the roughness), and where it's
    // brightest (glass in a facade's windows, polished metal) glossy.
    let spec: Option<(Vec<[f32; 2]>, u32, u32)> = packed_spec.as_ref().or(slot.specular.as_ref()).map(|t| {
        let v: Vec<f32> = t.pixels.chunks_exact(4).map(|c| (c[0] as f32 * 0.299 + c[1] as f32 * 0.587 + c[2] as f32 * 0.114) / 255.0).collect();
        let mean = (v.iter().sum::<f32>() / v.len() as f32).max(1e-3);
        (v.into_iter().map(|x| [x / mean, x]).collect(), t.width, t.height)
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
                        // (Written OpenGL's way up, Y flipped, as Bevy and Solari read maps.)
                        n_row[o..o + 4].copy_from_slice(&[enc(nx), enc(-ny), enc(nz), 255]);
                        // Roughness: the surface's, varied by the detail's and (shinier where it's
                        // brighter) the game's specular map; metalness the surface's.
                        let sp = spec.as_ref().map_or([1.0, 0.0], |(sm, sw, sh)| bilinear(sm, *sw, *sh, u, v, mix2));
                        let rough = (surface.roughness * lerp(1.0, dr, k) * lerp(1.0, 1.0 / sp[0].max(0.25), 0.35)).clamp(0.04, 1.0);
                        let gloss = ((sp[1] - 0.45) / 0.4).clamp(0.0, 1.0);
                        let rough = lerp(rough, 0.06, gloss * gloss * (3.0 - 2.0 * gloss));
                        m_row[o..o + 4].copy_from_slice(&[255, (rough * 255.0 + 0.5) as u8, (surface.metallic * 255.0 + 0.5) as u8, 255]);
                    }
                }
            });
        }
    });
    Some(Baked {
        color: compressed(w, h, color, if surface.mask { Map::Color } else { color_map(slot) }),
        normal: compressed(w, h, normal, Map::Normal),
        metal_rough: compressed(w, h, mr, Map::Data),
    })
}

/// The baked maps kept on disk (`assets/extracted/me_remaster/`), so a level's materials are
/// baked once: by the material, its tiling, its surface, the table and this.
const CACHE_VERSION: u32 = 3;

fn cache_path(slot: &MaterialSlot, surface: &Surface, repeats: u32) -> Option<PathBuf> {
    use std::hash::{Hash, Hasher};
    let dir = crate::extracted::find_root()?.join("me_remaster");
    let table = remaster().and_then(|r| r.lock().ok().map(|r| std::fs::read_to_string(r.root.join("me.ron")).unwrap_or_default())).unwrap_or_default();
    let mut h = std::collections::hash_map::DefaultHasher::new();
    // (What its maps hold, sampled: a material whose maps are read differently is baked again.)
    let sampled = |m: &Option<me_assets::Rgba>| m.as_ref().map(|d| (d.width, d.height, d.pixels.iter().step_by(4099).copied().collect::<Vec<u8>>()));
    // (Its tint, where it is: sampled across its map.)
    let tints: Vec<[u32; 3]> = (0..64).map(|k| slot.tint_at((k % 8) as f32 / 8.0 + 0.06, (k / 8) as f32 / 8.0 + 0.06).map(f32::to_bits)).collect();
    (CACHE_VERSION, &slot.path, repeats, format!("{surface:?}"), table, sampled(&slot.diffuse), sampled(&slot.normal), tints).hash(&mut h);
    Some(dir.join(format!("{:016x}.bin", h.finish())))
}

fn format_code(f: TextureFormat) -> Option<u8> {
    [TextureFormat::Rgba8Unorm, TextureFormat::Rgba8UnormSrgb, TextureFormat::Bc1RgbaUnorm, TextureFormat::Bc1RgbaUnormSrgb, TextureFormat::Bc3RgbaUnormSrgb, TextureFormat::Bc5RgUnorm]
        .iter()
        .position(|x| *x == f)
        .map(|i| i as u8)
}

fn format_of(code: u8) -> Option<TextureFormat> {
    [TextureFormat::Rgba8Unorm, TextureFormat::Rgba8UnormSrgb, TextureFormat::Bc1RgbaUnorm, TextureFormat::Bc1RgbaUnormSrgb, TextureFormat::Bc3RgbaUnormSrgb, TextureFormat::Bc5RgUnorm].get(code as usize).copied()
}

/// Three images (format, size, mips, data) to bytes, and back.
fn write_baked(b: &Baked) -> Option<Vec<u8>> {
    let mut out = Vec::new();
    for img in [&b.color, &b.normal, &b.metal_rough] {
        let d = &img.texture_descriptor;
        out.push(format_code(d.format)?);
        for v in [d.size.width, d.size.height, d.mip_level_count] {
            out.extend_from_slice(&v.to_le_bytes());
        }
        let data = img.data.as_ref()?;
        out.extend_from_slice(&(data.len() as u32).to_le_bytes());
        out.extend_from_slice(data);
    }
    Some(out)
}

fn read_baked(bytes: &[u8]) -> Option<Baked> {
    let mut at = 0;
    let u32_at = |at: &mut usize| -> Option<u32> {
        let v = u32::from_le_bytes(bytes.get(*at..*at + 4)?.try_into().ok()?);
        *at += 4;
        Some(v)
    };
    let mut imgs = Vec::new();
    for _ in 0..3 {
        let format = format_of(*bytes.get(at)?)?;
        at += 1;
        let (w, h, levels, len) = (u32_at(&mut at)?, u32_at(&mut at)?, u32_at(&mut at)?, u32_at(&mut at)? as usize);
        let data = bytes.get(at..at + len)?.to_vec();
        at += len;
        let mut img = Image::default();
        img.data = Some(data);
        img.texture_descriptor.size = Extent3d { width: w, height: h, depth_or_array_layers: 1 };
        img.texture_descriptor.dimension = TextureDimension::D2;
        img.texture_descriptor.format = format;
        imgs.push(sampled(img, levels));
    }
    let metal_rough = imgs.pop()?;
    let normal = imgs.pop()?;
    let color = imgs.pop()?;
    Some(Baked { color, normal, metal_rough })
}

/// [`bake`], kept on disk: read back if it's been baked before.
pub fn bake_kept(slot: &MaterialSlot, surface: &Surface, repeats: u32) -> Option<Baked> {
    let path = cache_path(slot, surface, repeats);
    if let Some(b) = path.as_ref().and_then(|p| std::fs::read(p).ok()).and_then(|bytes| read_baked(&bytes)) {
        return Some(b);
    }
    let b = bake(slot, surface, repeats)?;
    if let (Some(p), Some(bytes)) = (path, write_baked(&b)) {
        let _ = std::fs::create_dir_all(p.parent().unwrap_or(Path::new(".")));
        let _ = std::fs::write(p, bytes);
    }
    Some(b)
}

/// How a material's colour map is kept: a cut-out's with its clip.
pub fn color_map(slot: &MaterialSlot) -> Map {
    match slot.blend {
        me_assets::Blend::Masked(clip) => Map::Cutout(clip.clamp(0.01, 0.99)),
        _ => Map::Color,
    }
}

/// What a map holds, for how it's compressed: a colour (sRGB), a cut-out's colour (its alpha
/// cut at this), a normal (its X and Y), or data (roughness and metalness, linear).
#[derive(Clone, Copy, PartialEq)]
pub enum Map {
    Color,
    Cutout(f32),
    Normal,
    Data,
}

/// Whether the GPU takes block-compressed textures (BC1-7): it's asked at startup.
pub static BC: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(true);

/// An alpha scaled by `k`.
fn scaled(a: u8, k: f32) -> u8 {
    (a as f32 * k + 0.5).min(255.0) as u8
}

/// How much of a map's alpha is at or over `clip`, its alpha scaled by `k`.
fn coverage(pixels: &[u8], clip: f32, k: f32) -> f32 {
    let n = (pixels.len() / 4).max(1);
    pixels.chunks_exact(4).filter(|c| scaled(c[3], k) as f32 / 255.0 >= clip).count() as f32 / n as f32
}

/// A map's mip chain, from itself down to a pixel (box-filtered; a colour map's in linear
/// light). A cut-out's (`clip`) covers as much at each size as at its own: averaged, thin wire
/// would fade under its clip and the fence vanish a few metres off.
fn mip_levels(w: u32, h: u32, pixels: Vec<u8>, srgb: bool, clip: Option<f32>) -> Vec<(u32, u32, Vec<u8>)> {
    let covered = clip.map(|c| coverage(&pixels, c, 1.0));
    let mut levels = vec![(w, h, pixels)];
    loop {
        let (cw, ch, cur) = levels.last().unwrap();
        let (cw, ch) = (*cw, *ch);
        if cw <= 1 && ch <= 1 {
            break;
        }
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
        if let (Some(clip), Some(want)) = (clip, covered) {
            // (The scale of its alpha that covers nearest the same: found by halves, then the
            // nearer of just under and just over. Where its alpha's even, a small mip's, that's
            // all of it or none: a wire fence a little covered is none at a distance, not a
            // sheet.)
            let (mut lo, mut hi) = (0.0f32, 8.0f32);
            for _ in 0..12 {
                let k = (lo + hi) * 0.5;
                if coverage(&next, clip, k) < want { lo = k } else { hi = k }
            }
            let k = if want - coverage(&next, clip, lo) < coverage(&next, clip, hi) - want { lo } else { hi };
            for c in next.chunks_exact_mut(4) {
                c[3] = scaled(c[3], k);
            }
        }
        levels.push((nw, nh, next));
    }
    levels
}

fn sampled(mut img: Image, levels: u32) -> Image {
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

/// An image with its mip chain, uncompressed (RGBA), repeating, and filtered across mips and at
/// a slant (anisotropic).
pub fn with_mips(w: u32, h: u32, pixels: Vec<u8>, srgb: bool) -> Image {
    with_mips_cut(w, h, pixels, srgb, None)
}

fn with_mips_cut(w: u32, h: u32, pixels: Vec<u8>, srgb: bool, clip: Option<f32>) -> Image {
    let levels = mip_levels(w, h, pixels, srgb, clip);
    let n = levels.len() as u32;
    let data: Vec<u8> = levels.into_iter().flat_map(|(_, _, p)| p).collect();
    let img = Image::new(
        Extent3d { width: w, height: h, depth_or_array_layers: 1 },
        TextureDimension::D2,
        data,
        if srgb { TextureFormat::Rgba8UnormSrgb } else { TextureFormat::Rgba8Unorm },
        RenderAssetUsages::default(),
    );
    sampled(img, n)
}

/// The same, block-compressed for the GPU where it can be (a fifth of the memory): a colour as
/// BC1 (BC3 if it has any see-through), a normal as BC5 (X and Y; the shader works out Z), data
/// as BC1.
pub fn compressed(w: u32, h: u32, pixels: Vec<u8>, map: Map) -> Image {
    let srgb = matches!(map, Map::Color | Map::Cutout(_));
    let clip = match map {
        Map::Cutout(c) => Some(c),
        _ => None,
    };
    if !BC.load(std::sync::atomic::Ordering::Relaxed) || w % 4 != 0 || h % 4 != 0 {
        return with_mips_cut(w, h, pixels, srgb, clip);
    }
    let alpha = srgb && pixels.chunks_exact(4).any(|c| c[3] < 250);
    let (format, wgpu) = match map {
        Map::Color | Map::Cutout(_) if alpha => (texpresso::Format::Bc3, TextureFormat::Bc3RgbaUnormSrgb),
        Map::Color | Map::Cutout(_) => (texpresso::Format::Bc1, TextureFormat::Bc1RgbaUnormSrgb),
        Map::Normal => (texpresso::Format::Bc5, TextureFormat::Bc5RgUnorm),
        Map::Data => (texpresso::Format::Bc1, TextureFormat::Bc1RgbaUnorm),
    };
    let params = texpresso::Params { algorithm: texpresso::Algorithm::RangeFit, ..default() };
    let levels = mip_levels(w, h, pixels, srgb, clip);
    let n = levels.len() as u32;
    let mut data = Vec::new();
    for (lw, lh, px) in &levels {
        let mut out = vec![0u8; format.compressed_size(*lw as usize, *lh as usize)];
        format.compress(px, *lw as usize, *lh as usize, params, &mut out);
        data.extend_from_slice(&out);
    }
    let mut img = Image::default();
    img.data = Some(data);
    img.texture_descriptor.size = Extent3d { width: w, height: h, depth_or_array_layers: 1 };
    img.texture_descriptor.dimension = TextureDimension::D2;
    img.texture_descriptor.format = wgpu;
    sampled(img, n)
}

/// A game texture as it is, with mips, compressed. A normal map OpenGL's way up: the game's
/// (Unreal's, DirectX's) are the other way up, and Bevy and Solari both read OpenGL's (the
/// `normaltest` capture shows Bevy's).
pub fn plain(t: &Rgba, map: Map) -> Image {
    let mut px = t.pixels.clone();
    if map == Map::Normal {
        for c in px.chunks_exact_mut(4) {
            c[1] = 255 - c[1];
        }
    }
    compressed(t.width, t.height, px, map)
}

/// The game's colour map tinted as its material tints it (`slot`'s tint, where it has it) and
/// its white kept under real paint's.
pub fn tamed(t: &Rgba, slot: &MaterialSlot) -> Rgba {
    let (w, h) = (t.width.max(1), t.height.max(1));
    let pixels = t
        .pixels
        .chunks_exact(4)
        .enumerate()
        .flat_map(|(i, c)| {
            let k = slot.tint_at((i as u32 % w) as f32 / w as f32 + 0.5 / w as f32, (i as u32 / w) as f32 / h as f32 + 0.5 / h as f32);
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

    /// Compressed: an opaque colour as BC1 (8 bytes a 4x4 block, down to the last mip's one
    /// block), one with any see-through as BC3, a normal as BC5.
    #[test]
    fn maps_compress_to_blocks() {
        let px = |a: u8| (0..16 * 16).flat_map(|i| [(i * 7 % 256) as u8, 128, 200, a]).collect::<Vec<u8>>();
        let img = compressed(16, 16, px(255), Map::Color);
        assert_eq!(img.texture_descriptor.format, TextureFormat::Bc1RgbaUnormSrgb);
        assert_eq!(img.texture_descriptor.mip_level_count, 5);
        assert_eq!(img.data.as_ref().unwrap().len(), (16 + 4 + 1 + 1 + 1) * 8);
        assert_eq!(compressed(16, 16, px(100), Map::Color).texture_descriptor.format, TextureFormat::Bc3RgbaUnormSrgb);
        let n = compressed(16, 16, px(255), Map::Normal);
        assert_eq!(n.texture_descriptor.format, TextureFormat::Bc5RgUnorm);
        assert_eq!(n.data.as_ref().unwrap().len(), (16 + 4 + 1 + 1 + 1) * 16);
        // (Not a whole number of blocks across: left as it is.)
        assert_eq!(compressed(6, 6, vec![255; 6 * 6 * 4], Map::Color).texture_descriptor.format, TextureFormat::Rgba8UnormSrgb);
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

    /// A cut-out's wire stays at every size: averaged, a line a pixel wide is half as opaque a
    /// mip down, under its clip.
    #[test]
    fn cut_out_wire_stays_in_mips() {
        // A wire every other column: half of it covered.
        let px: Vec<u8> = (0..16 * 16).flat_map(|i| if i % 2 == 0 { [200, 200, 200, 255] } else { [0, 0, 0, 0] }).collect();
        let plain = mip_levels(16, 16, px.clone(), true, None);
        assert_eq!(coverage(&plain[1].2, 0.6, 1.0), 0.0);
        let kept = mip_levels(16, 16, px, true, Some(0.6));
        for (w, _, l) in &kept[1..] {
            assert!(coverage(l, 0.6, 1.0) >= 0.5, "{w}: {}", coverage(l, 0.6, 1.0));
        }
        // A strand in sixteen rows (barbed wire): not a sheet when it's a pixel.
        let px: Vec<u8> = (0..16 * 16).flat_map(|i| if i / 16 == 8 { [200, 200, 200, 255] } else { [0, 0, 0, 0] }).collect();
        let kept = mip_levels(16, 16, px, true, Some(0.5));
        assert!(coverage(&kept[1].2, 0.5, 1.0) > 0.0);
        assert_eq!(coverage(&kept.last().unwrap().2, 0.5, 1.0), 0.0);
    }
}
