//! Portal with RTX's remastered materials (NVIDIA's RTX Remix): physically based maps (base
//! colour, normal, roughness, metallic, emissive) for many of Portal's textures.
//!
//! A replacement material is named `mat_<HASH>` in `rtx-remix/mods/gameReadyAssets/*.usda`,
//! where the hash is an XXH64 of the original texture's top mip as the game uploads it (the
//! VTF's raw block data). Its textures are DDS files, mostly kept in that folder's `.pkg` asset
//! packages: dxvk-remix's `AssetPackage` (`src/dxvk/rtx_render/rtx_asset_package.h`, MIT) holds
//! each mip level as a blob, GDeflate-compressed for RTX IO.

use std::collections::HashMap;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

use image::RgbaImage;

/// A texture's block format (from a VkFormat or a DXGI format).
#[derive(Clone, Copy, Debug, PartialEq)]
enum Format {
    Bc1,
    Bc2,
    Bc3,
    Bc4,
    Bc5,
    Bc7,
    Rgba8,
    Bgra8,
}

impl Format {
    fn from_vk(f: u8) -> Option<Format> {
        Some(match f {
            131..=134 => Format::Bc1,
            135 | 136 => Format::Bc2,
            137 | 138 => Format::Bc3,
            139 | 140 => Format::Bc4,
            141 | 142 => Format::Bc5,
            145 | 146 => Format::Bc7,
            37 | 43 => Format::Rgba8,
            44 | 50 => Format::Bgra8,
            _ => return None,
        })
    }

    fn from_dxgi(f: u32) -> Option<Format> {
        Some(match f {
            70..=72 => Format::Bc1,
            73..=75 => Format::Bc2,
            76..=78 => Format::Bc3,
            79..=81 => Format::Bc4,
            82..=84 => Format::Bc5,
            97..=99 => Format::Bc7,
            27..=29 => Format::Rgba8,
            87 | 91 => Format::Bgra8,
            _ => return None,
        })
    }

    /// The top mip's size in bytes.
    fn size(self, w: usize, h: usize) -> usize {
        let blocks = w.div_ceil(4) * h.div_ceil(4);
        match self {
            Format::Bc1 | Format::Bc4 => blocks * 8,
            Format::Bc2 | Format::Bc3 | Format::Bc5 | Format::Bc7 => blocks * 16,
            Format::Rgba8 | Format::Bgra8 => w * h * 4,
        }
    }
}

/// Decode a top mip to RGBA. One-channel (BC4) data lands in red, two-channel (BC5) in red
/// and green.
fn decode(format: Format, w: usize, h: usize, data: &[u8]) -> Result<RgbaImage, String> {
    let mut px = vec![0u32; w * h];
    let r = match format {
        Format::Bc1 => texture2ddecoder::decode_bc1(data, w, h, &mut px),
        Format::Bc2 => texture2ddecoder::decode_bc2(data, w, h, &mut px),
        Format::Bc3 => texture2ddecoder::decode_bc3(data, w, h, &mut px),
        Format::Bc4 => texture2ddecoder::decode_bc4(data, w, h, &mut px),
        Format::Bc5 => texture2ddecoder::decode_bc5(data, w, h, &mut px),
        Format::Bc7 => texture2ddecoder::decode_bc7(data, w, h, &mut px),
        Format::Rgba8 | Format::Bgra8 => {
            let mut out = data.get(..w * h * 4).ok_or("short image data")?.to_vec();
            if format == Format::Bgra8 {
                out.chunks_exact_mut(4).for_each(|p| p.swap(0, 2));
            }
            return RgbaImage::from_raw(w as u32, h as u32, out).ok_or_else(|| "bad image size".into());
        }
    };
    r.map_err(|e| e.to_string())?;
    // The decoder packs each pixel as BGRA.
    let rgba = px.iter().flat_map(|p| {
        let [b, g, r, a] = p.to_le_bytes();
        [r, g, b, a]
    });
    RgbaImage::from_raw(w as u32, h as u32, rgba.collect()).ok_or_else(|| "bad image size".into())
}

/// Undo RTX IO's GDeflate: a run of tile streams, each starting on a 4 MiB boundary of the blob.
/// A stream is an 8-byte header (`04 FB`, then a word holding its tile count in bits 4..14 and
/// its last tile's size in bits 14..32, 0 for a whole tile), a table of `u32`s (the first is
/// the last tile's compressed size, the rest each later tile's offset), then the tiles: 64 KiB
/// each uncompressed, each compressed on its own.
fn gdeflate(blob: &[u8], out_len: usize) -> Result<Vec<u8>, String> {
    const TILE: usize = 64 << 10;
    let mut out = vec![0u8; out_len];
    let d = Decompressor::new()?;
    let (mut at, mut written) = (0usize, 0usize);
    let u32_at = |p: usize| blob.get(p..p + 4).map(|b| u32::from_le_bytes(b.try_into().unwrap()) as usize).ok_or("GDeflate stream runs past its blob");
    while written < out_len {
        if blob.get(at..at + 2) != Some(&[4, 0xfb]) {
            return Err(format!("no GDeflate stream at {at}"));
        }
        let word = u32_at(at + 4)?;
        let (tiles, last) = ((word >> 4) & 0x3ff, word >> 14);
        if tiles == 0 {
            return Err("empty GDeflate stream".into());
        }
        let table: Vec<usize> = (0..tiles).map(|t| u32_at(at + 8 + t * 4)).collect::<Result<_, _>>()?;
        let data = at + 8 + tiles * 4;
        for t in 0..tiles {
            let start = if t == 0 { 0 } else { table[t] };
            let end = if t + 1 < tiles { table[t + 1] } else { start + table[0] };
            let len = if t + 1 < tiles || last == 0 { TILE } else { last };
            let src = blob.get(data + start..data + end).ok_or("GDeflate tile runs past its blob")?;
            let dst = out.get_mut(written..written + len).ok_or("GDeflate data larger than the image")?;
            d.tile(src, dst)?;
            written += len;
        }
        let last_start = if tiles > 1 { table[tiles - 1] } else { 0 };
        at = (data + last_start + table[0]).next_multiple_of(4 << 20);
    }
    Ok(out)
}

struct Decompressor(*mut gdeflate_sys::libdeflate_gdeflate_decompressor);

impl Decompressor {
    fn new() -> Result<Self, String> {
        let d = unsafe { gdeflate_sys::libdeflate_alloc_gdeflate_decompressor() };
        if d.is_null() { Err("no GDeflate decompressor".into()) } else { Ok(Decompressor(d)) }
    }

    fn tile(&self, src: &[u8], dst: &mut [u8]) -> Result<(), String> {
        let mut page = gdeflate_sys::libdeflate_gdeflate_in_page { data: src.as_ptr().cast(), nbytes: src.len() };
        let mut got = 0usize;
        // SAFETY: the page covers `src`, and the output pointer and length are `dst`'s.
        let r = unsafe { gdeflate_sys::libdeflate_gdeflate_decompress(self.0, &mut page, 1, dst.as_mut_ptr().cast(), dst.len(), &mut got) };
        if r != 0 || got != dst.len() {
            return Err(format!("GDeflate tile failed ({r}, {got} of {} bytes)", dst.len()));
        }
        Ok(())
    }
}

impl Drop for Decompressor {
    fn drop(&mut self) {
        unsafe { gdeflate_sys::libdeflate_free_gdeflate_decompressor(self.0) }
    }
}

/// An image in a package: its format, size and the blob holding its top mip.
struct Asset {
    format: u8,
    width: usize,
    height: usize,
    blob: usize,
}

struct Blob {
    offset: u64,
    compressed: bool,
    size: usize,
}

/// One `.pkg` file's table of contents.
struct Package {
    path: PathBuf,
    assets: HashMap<String, Asset>,
    blobs: Vec<Blob>,
}

impl Package {
    fn open(path: &Path) -> Result<Package, String> {
        let mut f = File::open(path).map_err(|e| e.to_string())?;
        let mut head = [0u8; 16];
        f.read_exact(&mut head).map_err(|e| e.to_string())?;
        if head[..8] != [0x0d, 0xd0, 0xad, 0xba, 1, 0, 0, 0] {
            return Err("not a version 1 asset package".into());
        }
        let dict = u64::from_le_bytes(head[8..].try_into().unwrap());
        f.seek(SeekFrom::Start(dict)).map_err(|e| e.to_string())?;
        let mut d = Vec::new();
        f.read_to_end(&mut d).map_err(|e| e.to_string())?;
        let u16_at = |p: usize| d.get(p..p + 2).map(|b| u16::from_le_bytes([b[0], b[1]]) as usize).ok_or("truncated asset package");
        let (assets, blobs) = (u16_at(0)?, u16_at(2)?);
        let blob_at = 4 + assets * 20;
        let names_at = blob_at + blobs * 16;
        let names: Vec<String> = d.get(names_at..).ok_or("truncated asset package")?.split(|b| *b == 0).map(|n| String::from_utf8_lossy(n).to_ascii_lowercase().replace('\\', "/")).collect();
        let mut out = Package { path: path.to_path_buf(), assets: HashMap::new(), blobs: Vec::new() };
        for b in 0..blobs {
            let p = blob_at + b * 16;
            let q = u64::from_le_bytes(d[p..p + 8].try_into().unwrap());
            out.blobs.push(Blob { offset: q & ((1 << 40) - 1), compressed: (q >> 40) & 0xff != 0, size: u32::from_le_bytes(d[p + 8..p + 12].try_into().unwrap()) as usize });
        }
        for a in 0..assets {
            let p = 4 + a * 20;
            // AssetDesc: name, type, format, width, height, depth, mips, tail mips, layers,
            // first blob, tail blob. Only 2D images matter here.
            let (name, kind, format) = (u16_at(p)?, d[p + 2], d[p + 3]);
            if kind != 2 {
                continue;
            }
            let (mips, tail) = (u16_at(p + 10)?, u16_at(p + 12)?);
            let blob = if mips > tail { u16_at(p + 16)? } else { u16_at(p + 18)? };
            if let Some(n) = names.get(name) {
                out.assets.insert(n.clone(), Asset { format, width: u16_at(p + 4)?, height: u16_at(p + 6)?, blob });
            }
        }
        Ok(out)
    }

    fn image(&self, a: &Asset) -> Result<RgbaImage, String> {
        let format = Format::from_vk(a.format).ok_or_else(|| format!("unsupported format {}", a.format))?;
        let blob = self.blobs.get(a.blob).ok_or("missing blob")?;
        let mut f = File::open(&self.path).map_err(|e| e.to_string())?;
        f.seek(SeekFrom::Start(blob.offset)).map_err(|e| e.to_string())?;
        let mut data = vec![0u8; blob.size];
        f.read_exact(&mut data).map_err(|e| e.to_string())?;
        let len = format.size(a.width, a.height);
        let data = if blob.compressed { gdeflate(&data, len)? } else { data };
        decode(format, a.width, a.height, &data)
    }
}

/// A loose DDS file's top mip.
fn dds(bytes: &[u8]) -> Result<RgbaImage, String> {
    let u32_at = |p: usize| bytes.get(p..p + 4).map(|b| u32::from_le_bytes(b.try_into().unwrap())).ok_or("truncated DDS");
    if bytes.get(..4) != Some(b"DDS ") {
        return Err("not a DDS file".into());
    }
    let (h, w) = (u32_at(12)? as usize, u32_at(16)? as usize);
    let (format, data) = match bytes.get(84..88) {
        Some(b"DX10") => (Format::from_dxgi(u32_at(128)?).ok_or("unsupported DDS format")?, 148),
        Some(b"DXT1") => (Format::Bc1, 128),
        Some(b"DXT3") => (Format::Bc2, 128),
        Some(b"DXT5") => (Format::Bc3, 128),
        Some(b"ATI1") | Some(b"BC4U") => (Format::Bc4, 128),
        Some(b"ATI2") | Some(b"BC5U") => (Format::Bc5, 128),
        _ => return Err("unsupported DDS format".into()),
    };
    decode(format, w, h, bytes.get(data..).ok_or("truncated DDS")?)
}

/// How a material's normal map is stored.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum NormalEncoding {
    /// Two channels, a unit vector folded onto an octahedron (RTX Remix's own).
    Octahedral,
    /// Three channels, green up (as glTF wants).
    OpenGl,
    /// Three channels, green down.
    DirectX,
}

/// One replacement material's maps (paths as the USD gives them, relative to the
/// `gameReadyAssets` folder once [`Material::resolve`]d) and constants.
#[derive(Debug, Default, Clone)]
pub struct Material {
    pub albedo: Option<String>,
    pub normal: Option<(String, NormalEncoding)>,
    pub roughness: Option<String>,
    pub metallic: Option<String>,
    pub emissive: Option<String>,
    pub roughness_constant: Option<f32>,
    pub metallic_constant: Option<f32>,
    /// How bright what glows is (Remix's `emissive_intensity`), when emission is on.
    pub emissive_intensity: Option<f32>,
    pub emission_enabled: Option<bool>,
    /// See-through (Remix's translucent shader, for glass).
    pub translucent: bool,
    encoding: Option<NormalEncoding>,
}

/// A shader input's value.
pub enum Input {
    Asset(String),
    Number(f32),
    Flag(bool),
}

impl Material {
    /// Take one AperturePBR shader input (`inputs:<name>`).
    pub fn set(&mut self, name: &str, value: Input) {
        let asset = |v: &Input| match v {
            Input::Asset(a) if !a.is_empty() => Some(a.clone()),
            _ => None,
        };
        let number = |v: &Input| match v {
            Input::Number(n) => Some(*n),
            _ => None,
        };
        match name {
            "diffuse_texture" => self.albedo = asset(&value),
            "normalmap_texture" => {
                let encoding = self.normal.as_ref().map_or(NormalEncoding::Octahedral, |n| n.1);
                self.normal = asset(&value).map(|p| (p, encoding));
            }
            "reflectionroughness_texture" => self.roughness = asset(&value),
            "metallic_texture" => self.metallic = asset(&value),
            "emissive_mask_texture" => self.emissive = asset(&value),
            "reflection_roughness_constant" => self.roughness_constant = number(&value),
            "metallic_constant" => self.metallic_constant = number(&value),
            "emissive_intensity" => self.emissive_intensity = number(&value),
            "enable_emission" => {
                self.emission_enabled = match value {
                    Input::Flag(b) => Some(b),
                    Input::Number(n) => Some(n != 0.0),
                    _ => None,
                }
            }
            "transmittance_texture" | "transmittance_color" => self.translucent = true,
            "encoding" => {
                let e = match number(&value).unwrap_or(0.0) as i32 {
                    1 => NormalEncoding::OpenGl,
                    2 => NormalEncoding::DirectX,
                    _ => NormalEncoding::Octahedral,
                };
                // The encoding can come before or after the map it describes.
                self.encoding = Some(e);
                if let Some(n) = &mut self.normal {
                    n.1 = e;
                }
            }
            _ => {}
        }
        if let (Some(e), Some(n)) = (self.encoding, &mut self.normal) {
            n.1 = e;
        }
    }

    /// Make its texture paths relative to the `gameReadyAssets` folder, given the folder (also
    /// relative to it) of the USD file that names them.
    pub fn resolve(&mut self, usd_dir: &str) {
        let fix = |p: &mut String| *p = join(usd_dir, p);
        for p in [&mut self.albedo, &mut self.roughness, &mut self.metallic, &mut self.emissive].into_iter().flatten() {
            fix(p);
        }
        if let Some((p, _)) = &mut self.normal {
            fix(p);
        }
    }
}

/// `rel` (with `./` and `../`) from folder `dir`, both relative to the same root.
fn join(dir: &str, rel: &str) -> String {
    let mut parts: Vec<&str> = dir.split(['/', '\\']).filter(|p| !p.is_empty() && *p != ".").collect();
    for p in rel.split(['/', '\\']) {
        match p {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            p => parts.push(p),
        }
    }
    parts.join("/")
}

/// A material's maps, decoded and ready to save: its base colour, a tangent-space normal map
/// (green up), glTF's metallic-roughness map (roughness in green, metallic in blue) and what
/// glows.
pub struct Baked {
    pub albedo: Option<RgbaImage>,
    pub normal: Option<RgbaImage>,
    pub metallic_roughness: Option<RgbaImage>,
    pub emissive: Option<RgbaImage>,
}

/// Shrink an image to fit `to` pixels square, keeping its shape.
pub fn shrink(img: RgbaImage, to: u32) -> RgbaImage {
    if img.width() <= to && img.height() <= to {
        return img;
    }
    let k = to as f32 / img.width().max(img.height()) as f32;
    let (w, h) = (((img.width() as f32 * k).round() as u32).max(1), ((img.height() as f32 * k).round() as u32).max(1));
    image::imageops::resize(&img, w, h, image::imageops::FilterType::Triangle)
}

/// Portal with RTX's replacements: its packages, and its USD layers' text.
pub struct Remix {
    root: PathBuf,
    packages: Vec<Package>,
    usda: String,
}

impl Remix {
    /// Open the remaster in a Portal with RTX install (`install`/rtx-remix/mods/gameReadyAssets).
    pub fn open(install: &Path) -> Option<Remix> {
        let root = install.join("rtx-remix/mods/gameReadyAssets");
        let mut usda = String::new();
        let mut packages = Vec::new();
        let mut files: Vec<PathBuf> = std::fs::read_dir(&root).ok()?.filter_map(|e| e.ok().map(|e| e.path())).collect();
        files.sort();
        for p in files {
            match p.extension().and_then(|e| e.to_str()) {
                Some("usda") => usda.push_str(&std::fs::read_to_string(&p).unwrap_or_default()),
                Some("pkg") => match Package::open(&p) {
                    Ok(pkg) => packages.push(pkg),
                    Err(e) => eprintln!("  {}: {e}", p.display()),
                },
                _ => {}
            }
        }
        (!usda.is_empty()).then_some(Remix { root, packages, usda })
    }

    /// The replacement for the texture with this hash, if Portal with RTX has one.
    pub fn material(&self, hash: u64) -> Option<Material> {
        let name = format!("over \"mat_{hash:016X}\"");
        // A material can be named more than once (once per layer); the one with a shader wins.
        let mut from = 0;
        while let Some(i) = self.usda[from..].find(&name) {
            let start = from + i;
            let end = self.usda[start + name.len()..].find("\n        over \"").map(|e| start + name.len() + e).unwrap_or(self.usda.len());
            let block = &self.usda[start..end];
            if block.contains("over \"Shader\"") {
                return Some(parse_material(block));
            }
            from = start + name.len();
        }
        None
    }

    /// A texture the USD names (`./SubUSDs/textures/x.dds`), from the packages or loose.
    pub fn image(&self, path: &str) -> Result<RgbaImage, String> {
        let key = path.trim_start_matches("./").replace('\\', "/").to_ascii_lowercase();
        for p in &self.packages {
            if let Some(a) = p.assets.get(&key) {
                return p.image(a);
            }
        }
        let loose = self.root.join(path.trim_start_matches("./"));
        match std::fs::read(&loose) {
            Ok(bytes) if loose.extension().is_some_and(|e| e.eq_ignore_ascii_case("dds")) => dds(&bytes),
            Ok(bytes) => image::load_from_memory(&bytes).map(|i| i.to_rgba8()).map_err(|e| e.to_string()),
            Err(_) => Err(format!("{path}: not found")),
        }
    }

    /// Decode a material's maps, each shrunk to at most `max` pixels square.
    pub fn bake(&self, m: &Material, max: u32) -> Result<Baked, String> {
        let albedo = m.albedo.as_deref().map(|p| self.image(p).map(|i| shrink(i, max))).transpose()?;
        let normal = m.normal.as_ref().map(|(p, e)| self.image(p).map(|i| shrink(tangent_normals(&i, *e), max))).transpose()?;
        let rough = m.roughness.as_deref().map(|p| self.image(p)).transpose()?;
        let metal = m.metallic.as_deref().map(|p| self.image(p)).transpose()?;
        let metallic_roughness = (rough.is_some() || metal.is_some() || m.roughness_constant.is_some() || m.metallic_constant.is_some()).then(|| {
            let size = [&rough, &metal].iter().filter_map(|i| i.as_ref().map(|i| i.width().max(i.height()))).max().unwrap_or(4).min(max);
            let fit = |i: Option<RgbaImage>| i.map(|i| image::imageops::resize(&i, size, size, image::imageops::FilterType::Triangle));
            let (rough, metal) = (fit(rough), fit(metal));
            let constant = |c: Option<f32>, fallback: f32| (c.unwrap_or(fallback).clamp(0.0, 1.0) * 255.0).round() as u8;
            RgbaImage::from_fn(size, size, |x, y| {
                let g = rough.as_ref().map_or(constant(m.roughness_constant, 0.5), |i| i.get_pixel(x, y).0[0]);
                let b = metal.as_ref().map_or(constant(m.metallic_constant, 0.0), |i| i.get_pixel(x, y).0[0]);
                image::Rgba([255, g, b, 255])
            })
        });
        let glows = m.emission_enabled.unwrap_or(m.emissive.is_some());
        let emissive = m.emissive.as_deref().filter(|_| glows).map(|p| self.image(p).map(|i| shrink(i, max))).transpose()?;
        Ok(Baked { albedo, normal, metallic_roughness, emissive })
    }

    /// The `gameReadyAssets` folder.
    pub fn root(&self) -> &Path {
        &self.root
    }
}

/// A material block's shader inputs (from the USDA text).
fn parse_material(block: &str) -> Material {
    let mut m = Material::default();
    for line in block.lines() {
        let l = line.trim();
        let Some(rest) = l.split_once("inputs:").map(|(_, r)| r) else { continue };
        let Some((key, value)) = rest.split_once('=') else { continue };
        let (key, value) = (key.trim(), value.trim());
        let input = if let Some(v) = value.strip_prefix('@') {
            Input::Asset(v.split('@').next().unwrap_or_default().to_string())
        } else {
            let v = value.split_whitespace().next().unwrap_or_default();
            match v {
                "true" => Input::Flag(true),
                "false" => Input::Flag(false),
                v => match v.parse::<f32>() {
                    Ok(n) => Input::Number(n),
                    Err(_) => continue,
                },
            }
        };
        m.set(key, input);
    }
    m
}

/// A normal map as glTF wants it (tangent space, green up, 0.5 grey for flat).
pub fn tangent_normals(img: &RgbaImage, encoding: NormalEncoding) -> RgbaImage {
    let mut out = img.clone();
    for p in out.pixels_mut() {
        let [r, g, b, _] = p.0;
        let n = match encoding {
            NormalEncoding::Octahedral => {
                let e = glam::Vec2::new(r as f32, g as f32) / 255.0 * 2.0 - 1.0;
                let mut n = glam::Vec3::new(e.x, e.y, 1.0 - e.x.abs() - e.y.abs());
                if n.z < 0.0 {
                    let (x, y) = (n.x, n.y);
                    n.x = (1.0 - y.abs()) * x.signum();
                    n.y = (1.0 - x.abs()) * y.signum();
                }
                n.normalize_or(glam::Vec3::Z)
            }
            NormalEncoding::OpenGl => glam::Vec3::new(r as f32, g as f32, b as f32) / 127.5 - 1.0,
            NormalEncoding::DirectX => glam::Vec3::new(r as f32, 255.0 - g as f32, b as f32) / 127.5 - 1.0,
        };
        let c = (n * 0.5 + 0.5) * 255.0;
        p.0 = [c.x.round() as u8, c.y.round() as u8, c.z.round() as u8, 255];
    }
    out
}

/// The XXH64 Remix names a replacement by: the VTF's top mip, as stored (the last image in
/// the file). `None` for formats it can't size.
pub fn vtf_hash(vtf: &[u8]) -> Option<u64> {
    if vtf.get(..4)? != b"VTF\0" {
        return None;
    }
    let (w, h) = (u16::from_le_bytes([vtf[16], vtf[17]]) as usize, u16::from_le_bytes([vtf[18], vtf[19]]) as usize);
    let format = i32::from_le_bytes(vtf.get(52..56)?.try_into().ok()?);
    let blocks = w.div_ceil(4) * h.div_ceil(4);
    let size = match format {
        13 | 20 => blocks * 8,  // DXT1
        14 | 15 => blocks * 16, // DXT3, DXT5
        0 | 1 | 11 | 12 | 16 => w * h * 4,
        2 | 3 => w * h * 3,
        4 | 17 | 18 | 19 | 21 => w * h * 2,
        5 | 8 => w * h,
        _ => return None,
    };
    let data = vtf.get(vtf.len().checked_sub(size)?..)?;
    Some(xxhash_rust::xxh64::xxh64(data, 0))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn octahedral_normals_unfold() {
        let normal = |rgb: [u8; 3], e| tangent_normals(&RgbaImage::from_raw(1, 1, vec![rgb[0], rgb[1], rgb[2], 255]).unwrap(), e).get_pixel(0, 0).0;
        // The middle is straight out of the surface.
        assert_eq!(normal([128, 128, 0], NormalEncoding::Octahedral)[..3], [128, 128, 255]);
        // Halfway to an edge leans half-way over: (0.5, 0, 0.5), normalised.
        let n = normal([191, 128, 0], NormalEncoding::Octahedral);
        let v = glam::Vec3::new(n[0] as f32, n[1] as f32, n[2] as f32) / 127.5 - 1.0;
        assert!(v.abs_diff_eq(glam::Vec3::new(1.0, 0.0, 1.0).normalize(), 0.02), "{v}");
        // Green down flips.
        assert_eq!(normal([128, 0, 128], NormalEncoding::DirectX)[1], 255);
    }

    #[test]
    fn materials_read_from_their_block() {
        let block = r#"over "mat_00D2EE9BA855B235" (
            {
                over "Shader"
                {
                    asset inputs:diffuse_texture = @./SubUSDs/textures/a_albedo.dds@ (
                    int inputs:encoding = 0 (
                    asset inputs:metallic_texture = @@ (
                    asset inputs:normalmap_texture = @./SubUSDs/textures/a_normal_OTH.dds@ (
                    float inputs:reflection_roughness_constant = 0.25 (
"#;
        let m = parse_material(block);
        assert_eq!(m.albedo.as_deref(), Some("./SubUSDs/textures/a_albedo.dds"));
        assert_eq!(m.normal, Some(("./SubUSDs/textures/a_normal_OTH.dds".into(), NormalEncoding::Octahedral)));
        assert_eq!(m.metallic, None);
        assert_eq!(m.roughness_constant, Some(0.25));
    }

    #[test]
    fn vtf_hashes_cover_the_top_mip() {
        // A 4x4 DXT1 VTF: the header, then one 8-byte block, the top mip, last.
        let mut v = vec![0u8; 80];
        v[..4].copy_from_slice(b"VTF\0");
        v[16..18].copy_from_slice(&4u16.to_le_bytes());
        v[18..20].copy_from_slice(&4u16.to_le_bytes());
        v[52..56].copy_from_slice(&13i32.to_le_bytes());
        v.extend_from_slice(&[1, 2, 3, 4, 5, 6, 7, 8]);
        assert_eq!(vtf_hash(&v), Some(xxhash_rust::xxh64::xxh64(&[1, 2, 3, 4, 5, 6, 7, 8], 0)));
    }
}
