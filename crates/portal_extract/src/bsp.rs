//! Portal's maps (Source BSP, version 20): their faces to draw, their brushes as boxes to
//! collide with, and their entities, written out for the game (`maps/<name>.mesh`,
//! `.solids`, `.ents`).
//!
//! Faces are drawn as they are, a mesh per material (and per brush model, so a brush entity's
//! faces can move with it). Collision is boxes, as the rest of the game's chambers are:
//! an axis-aligned brush is its box; an angled one is cut into thin slices along its longest
//! side, each slice's box standing in for it (stairs on a ramp, steps along a slanted wall).
//! Portal's angled brushes are mostly behind the scenes and in elevator shafts.

use std::collections::{HashMap, HashSet};

use glam::Vec3;

/// The lumps used here.
const ENTITIES: usize = 0;
const PLANES: usize = 1;
const TEXDATA: usize = 2;
const VERTEXES: usize = 3;
const NODES: usize = 5;
const TEXINFO: usize = 6;
const FACES: usize = 7;
const LEAFS: usize = 10;
const EDGES: usize = 12;
const SURFEDGES: usize = 13;
const MODELS: usize = 14;
const LEAFBRUSHES: usize = 17;
const GAME_LUMP: usize = 35;
const BRUSHES: usize = 18;
const BRUSHSIDES: usize = 19;
const TEXDATA_STRING_DATA: usize = 43;
const TEXDATA_STRING_TABLE: usize = 44;
const OVERLAYS: usize = 45;
const DISPINFO: usize = 26;
const DISP_VERTS: usize = 33;


/// Texinfo flags of faces that aren't drawn: sky, nodraw, hint, skip, trigger.
pub const SURF_HIDDEN: i32 = 0x4 | 0x80 | 0x100 | 0x200 | 0x40;

pub struct Plane {
    pub normal: Vec3,
    pub dist: f32,
}

pub struct TexInfo {
    pub s: [f32; 4],
    pub t: [f32; 4],
    pub flags: i32,
    pub texdata: i32,
}

pub struct TexData {
    pub name: String,
    pub width: i32,
    pub height: i32,
}

pub struct Face {
    pub first_edge: usize,
    pub num_edges: usize,
    pub texinfo: i32,
    /// Its displacement (an index into `disps`), if it's one.
    pub disp: Option<usize>,
}

/// A displacement: a grid of (2^power + 1)^2 points over its face, each pushed out along its
/// own direction (a dirt floor's mounds). Source space.
pub struct Disp {
    pub start: Vec3,
    pub power: u32,
    /// Each point's push: direction times distance.
    pub offsets: Vec<Vec3>,
}

pub struct Model {
    pub headnode: i32,
    pub first_face: usize,
    pub num_faces: usize,
}

pub struct Brush {
    pub first_side: usize,
    pub num_sides: usize,
    pub contents: i32,
}

pub struct BrushSide {
    pub plane: usize,
    pub texinfo: i32,
    pub bevel: bool,
}

/// A static prop (the game lump's `sprp`): Source space.
pub struct StaticProp {
    pub model: String,
    pub origin: Vec3,
    /// Pitch, yaw, roll (degrees).
    pub angles: Vec3,
    pub skin: i32,
}

pub struct Bsp {
    pub entities: String,
    pub static_props: Vec<StaticProp>,
    pub planes: Vec<Plane>,
    pub texinfo: Vec<TexInfo>,
    pub texdata: Vec<TexData>,
    pub vertices: Vec<Vec3>,
    pub edges: Vec<[u16; 2]>,
    pub surfedges: Vec<i32>,
    pub faces: Vec<Face>,
    pub models: Vec<Model>,
    /// Nodes: (children); a negative child is leaf `-1 - child`.
    pub nodes: Vec<[i32; 2]>,
    /// Leaves: (first leafbrush, count).
    pub leafs: Vec<(usize, usize)>,
    pub leafbrushes: Vec<u16>,
    pub brushes: Vec<Brush>,
    pub sides: Vec<BrushSide>,
    pub overlays: Vec<Overlay>,
    pub disps: Vec<Disp>,
}

/// An overlay (info_overlay: a decal laid on faces; Portal's indicator light strips and painted
/// signs), as the compiled map has it: Source space.
pub struct Overlay {
    pub id: i32,
    pub texinfo: i32,
    /// Its texture's range across it: U from, to; V from, to.
    pub u: [f32; 2],
    pub v: [f32; 2],
    /// Its corners: where they are, from `origin`, in its basis (U, V = normal x U).
    pub corners: [Vec3; 4],
    pub origin: Vec3,
    pub normal: Vec3,
    pub basis_u: Vec3,
}

impl Overlay {
    /// Its corners in the map, with their texture coordinates.
    pub fn quad(&self) -> [(Vec3, [f32; 2]); 4] {
        let v = self.normal.cross(self.basis_u);
        let at = |k: usize| self.origin + self.basis_u * self.corners[k].x + v * self.corners[k].y;
        [(at(0), [self.u[0], self.v[0]]), (at(1), [self.u[0], self.v[1]]), (at(2), [self.u[1], self.v[1]]), (at(3), [self.u[1], self.v[0]])]
    }
}

fn rd<T: Copy>(b: &[u8], at: usize, f: impl Fn([u8; 4]) -> T) -> T {
    f(b[at..at + 4].try_into().unwrap())
}
fn f32_at(b: &[u8], at: usize) -> f32 {
    rd(b, at, f32::from_le_bytes)
}
fn i32_at(b: &[u8], at: usize) -> i32 {
    rd(b, at, i32::from_le_bytes)
}
fn u16_at(b: &[u8], at: usize) -> u16 {
    u16::from_le_bytes([b[at], b[at + 1]])
}
fn i16_at(b: &[u8], at: usize) -> i16 {
    i16::from_le_bytes([b[at], b[at + 1]])
}

impl Bsp {
    pub fn parse(d: &[u8]) -> Result<Bsp, String> {
        if d.len() < 1036 || &d[0..4] != b"VBSP" {
            return Err("not a BSP".into());
        }
        let version = i32_at(d, 4);
        if !(19..=21).contains(&version) {
            return Err(format!("BSP version {version}"));
        }
        let lump = |i: usize| -> Result<(&[u8], i32), String> {
            let at = 8 + 16 * i;
            let (ofs, len, ver) = (i32_at(d, at) as usize, i32_at(d, at + 4) as usize, i32_at(d, at + 8));
            d.get(ofs..ofs + len).map(|b| (b, ver)).ok_or_else(|| format!("lump {i} out of range"))
        };
        let recs = |i: usize, size: usize| -> Result<(&[u8], usize), String> {
            let (b, _) = lump(i)?;
            Ok((b, b.len() / size))
        };

        let entities = String::from_utf8_lossy(lump(ENTITIES)?.0).trim_end_matches('\0').to_string();
        let (b, n) = recs(PLANES, 20)?;
        let planes = (0..n)
            .map(|i| {
                let a = i * 20;
                Plane { normal: Vec3::new(f32_at(b, a), f32_at(b, a + 4), f32_at(b, a + 8)), dist: f32_at(b, a + 12) }
            })
            .collect();
        let (b, n) = recs(TEXINFO, 72)?;
        let texinfo = (0..n)
            .map(|i| {
                let a = i * 72;
                let v = |k: usize| f32_at(b, a + k * 4);
                TexInfo { s: [v(0), v(1), v(2), v(3)], t: [v(4), v(5), v(6), v(7)], flags: i32_at(b, a + 64), texdata: i32_at(b, a + 68) }
            })
            .collect();
        let strings = lump(TEXDATA_STRING_DATA)?.0;
        let (table, nt) = recs(TEXDATA_STRING_TABLE, 4)?;
        let string = |id: i32| -> String {
            if id < 0 || id as usize >= nt {
                return String::new();
            }
            let at = i32_at(table, id as usize * 4) as usize;
            let end = strings[at..].iter().position(|&c| c == 0).map_or(strings.len(), |e| at + e);
            String::from_utf8_lossy(&strings[at..end]).to_string()
        };
        let (b, n) = recs(TEXDATA, 32)?;
        let texdata = (0..n)
            .map(|i| {
                let a = i * 32;
                TexData { name: string(i32_at(b, a + 12)), width: i32_at(b, a + 16), height: i32_at(b, a + 20) }
            })
            .collect();
        let (b, n) = recs(VERTEXES, 12)?;
        let vertices = (0..n).map(|i| Vec3::new(f32_at(b, i * 12), f32_at(b, i * 12 + 4), f32_at(b, i * 12 + 8))).collect();
        let (b, n) = recs(EDGES, 4)?;
        let edges = (0..n).map(|i| [u16_at(b, i * 4), u16_at(b, i * 4 + 2)]).collect();
        let (b, n) = recs(SURFEDGES, 4)?;
        let surfedges = (0..n).map(|i| i32_at(b, i * 4)).collect();
        let (b, n) = recs(FACES, 56)?;
        let faces = (0..n)
            .map(|i| {
                let a = i * 56;
                Face { first_edge: i32_at(b, a + 4) as usize, num_edges: i16_at(b, a + 8) as usize, texinfo: i16_at(b, a + 10) as i32, disp: usize::try_from(i16_at(b, a + 12)).ok() }
            })
            .collect();
        let (b, n) = recs(MODELS, 48)?;
        let models = (0..n)
            .map(|i| {
                let a = i * 48;
                Model { headnode: i32_at(b, a + 36), first_face: i32_at(b, a + 40) as usize, num_faces: i32_at(b, a + 44) as usize }
            })
            .collect();
        let (b, n) = recs(NODES, 32)?;
        let nodes = (0..n).map(|i| [i32_at(b, i * 32 + 4), i32_at(b, i * 32 + 8)]).collect();
        let (b, ver) = lump(LEAFS)?;
        let leaf_size = if ver == 0 { 56 } else { 32 };
        let leafs = (0..b.len() / leaf_size)
            .map(|i| {
                let a = i * leaf_size;
                (u16_at(b, a + 24) as usize, u16_at(b, a + 26) as usize)
            })
            .collect();
        let (b, n) = recs(LEAFBRUSHES, 2)?;
        let leafbrushes = (0..n).map(|i| u16_at(b, i * 2)).collect();
        let (b, n) = recs(BRUSHES, 12)?;
        let brushes = (0..n)
            .map(|i| {
                let a = i * 12;
                Brush { first_side: i32_at(b, a) as usize, num_sides: i32_at(b, a + 4) as usize, contents: i32_at(b, a + 8) }
            })
            .collect();
        let (b, n) = recs(BRUSHSIDES, 8)?;
        let sides = (0..n)
            .map(|i| {
                let a = i * 8;
                BrushSide { plane: u16_at(b, a) as usize, texinfo: i16_at(b, a + 2) as i32, bevel: i16_at(b, a + 6) != 0 }
            })
            .collect();
        let static_props = static_props(lump(GAME_LUMP)?.0, d).unwrap_or_default();
        // Overlays (doverlay_t, 352 bytes): the basis's U is kept in the corners' z.
        let (b, n) = recs(OVERLAYS, 352)?;
        let overlays = (0..n)
            .map(|i| {
                let a = i * 352;
                let v3 = |at: usize| Vec3::new(f32_at(b, at), f32_at(b, at + 4), f32_at(b, at + 8));
                let c: [Vec3; 4] = std::array::from_fn(|k| v3(a + 280 + 12 * k));
                Overlay {
                    id: i32_at(b, a),
                    texinfo: i16_at(b, a + 4) as i32,
                    u: [f32_at(b, a + 264), f32_at(b, a + 268)],
                    v: [f32_at(b, a + 272), f32_at(b, a + 276)],
                    corners: c,
                    origin: v3(a + 328),
                    normal: v3(a + 340),
                    basis_u: Vec3::new(c[0].z, c[1].z, c[2].z),
                }
            })
            .collect();
        // Displacements (ddispinfo_t, 176 bytes; dDispVert, 20: direction, distance, alpha).
        let (verts, _) = recs(DISP_VERTS, 20)?;
        let (b, n) = recs(DISPINFO, 176)?;
        let disps = (0..n)
            .map(|i| {
                let a = i * 176;
                let (first, power) = (i32_at(b, a + 12).max(0) as usize, i32_at(b, a + 20).clamp(0, 4) as u32);
                let side = (1usize << power) + 1;
                let offsets = (0..side * side)
                    .map(|k| {
                        let at = (first + k) * 20;
                        if at + 16 > verts.len() {
                            return Vec3::ZERO;
                        }
                        Vec3::new(f32_at(verts, at), f32_at(verts, at + 4), f32_at(verts, at + 8)) * f32_at(verts, at + 12)
                    })
                    .collect();
                Disp { start: Vec3::new(f32_at(b, a), f32_at(b, a + 4), f32_at(b, a + 8)), power, offsets }
            })
            .collect();
        Ok(Bsp { entities, static_props, planes, texinfo, texdata, vertices, edges, surfedges, faces, models, nodes, leafs, leafbrushes, brushes, sides, overlays, disps })
    }

    /// A texinfo's texture name, as the map has it (lower case, forward slashes).
    pub fn texture(&self, texinfo: i32) -> Option<(&TexData, &TexInfo)> {
        let ti = self.texinfo.get(usize::try_from(texinfo).ok()?)?;
        let td = self.texdata.get(usize::try_from(ti.texdata).ok()?)?;
        Some((td, ti))
    }


    /// A face's corners, in order.
    pub fn face_points(&self, f: &Face) -> Vec<Vec3> {
        (0..f.num_edges)
            .map(|k| {
                let se = self.surfedges[f.first_edge + k];
                let e = self.edges[se.unsigned_abs() as usize];
                self.vertices[if se >= 0 { e[0] } else { e[1] } as usize]
            })
            .collect()
    }

    /// Every brush under a model's tree.
    pub fn model_brushes(&self, model: usize) -> Vec<usize> {
        let mut out = HashSet::new();
        let mut stack = vec![self.models[model].headnode];
        while let Some(n) = stack.pop() {
            if n < 0 {
                let Some(&(first, count)) = self.leafs.get((-1 - n) as usize) else { continue };
                out.extend(self.leafbrushes[first..first + count].iter().map(|&b| b as usize));
            } else if let Some(c) = self.nodes.get(n as usize) {
                stack.extend_from_slice(c);
            }
        }
        let mut v: Vec<usize> = out.into_iter().collect();
        v.sort_unstable();
        v
    }



}

/// The static props in the game lump (`sprp`, versions 4-6). Its entries' offsets are into
/// the whole file.
fn static_props(lump: &[u8], file: &[u8]) -> Option<Vec<StaticProp>> {
    let count = i32_at(lump, 0) as usize;
    for i in 0..count {
        let a = 4 + i * 16;
        let id = u32::from_le_bytes(lump.get(a..a + 4)?.try_into().ok()?);
        if id != u32::from_be_bytes(*b"sprp") {
            continue;
        }
        let version = u16_at(lump, a + 6);
        let (ofs, len) = (i32_at(lump, a + 8) as usize, i32_at(lump, a + 12) as usize);
        let b = file.get(ofs..ofs + len)?;
        let size = match version {
            4 => 56,
            5 => 60,
            6 => 64,
            _ => return None,
        };
        let names_n = i32_at(b, 0) as usize;
        let names: Vec<String> = (0..names_n)
            .map(|k| {
                let n = &b[4 + 128 * k..4 + 128 * (k + 1)];
                String::from_utf8_lossy(&n[..n.iter().position(|&c| c == 0).unwrap_or(128)]).to_ascii_lowercase().replace('\\', "/")
            })
            .collect();
        let mut p = 4 + 128 * names_n;
        let leaves = i32_at(b, p) as usize;
        p += 4 + 2 * leaves;
        let props = i32_at(b, p) as usize;
        p += 4;
        let mut out = Vec::with_capacity(props);
        for k in 0..props {
            let a = p + k * size;
            let v = |o: usize| Vec3::new(f32_at(b, a + o), f32_at(b, a + o + 4), f32_at(b, a + o + 8));
            let model = names.get(u16_at(b, a + 24) as usize).cloned().unwrap_or_default();
            out.push(StaticProp { model, origin: v(0), angles: v(12), skin: i32_at(b, a + 32) });
        }
        return Some(out);
    }
    Some(Vec::new())
}


/// A Source entity: its keys in order (a key can repeat: outputs).
pub type Entity = Vec<(String, String)>;

/// The entity lump's entities.
pub fn parse_entities(text: &str) -> Vec<Entity> {
    let mut out = Vec::new();
    let mut cur: Option<Entity> = None;
    let mut chars = text.chars().peekable();
    let mut strings = Vec::new();
    while let Some(c) = chars.next() {
        match c {
            '{' => cur = Some(Vec::new()),
            '}' => {
                if let Some(e) = cur.take() {
                    out.push(e);
                }
                strings.clear();
            }
            '"' => {
                let mut s = String::new();
                for c in chars.by_ref() {
                    if c == '"' {
                        break;
                    }
                    s.push(c);
                }
                strings.push(s);
                if strings.len() == 2 {
                    let v = strings.pop().unwrap();
                    let k = strings.pop().unwrap();
                    if let Some(e) = cur.as_mut() {
                        e.push((k, v));
                    }
                }
            }
            _ => {}
        }
    }
    out
}

pub fn key<'a>(e: &'a Entity, k: &str) -> Option<&'a str> {
    e.iter().find(|(a, _)| a.eq_ignore_ascii_case(k)).map(|(_, v)| v.as_str())
}

pub fn vec3_key(e: &Entity, k: &str) -> Option<Vec3> {
    let v: Vec<f32> = key(e, k)?.split_whitespace().filter_map(|s| s.parse().ok()).collect();
    (v.len() >= 3).then(|| Vec3::new(v[0], v[1], v[2]))
}

/// Where each brush model is when the map starts (Source space): its entity's origin (its
/// faces and brushes are stored relative to it), or for a train, the path node it starts on
/// (a `func_tracktrain` jumps to its first `path_track`, `height` above it).
pub fn model_origins(ents: &[Entity]) -> HashMap<usize, Vec3> {
    let named: HashMap<&str, &Entity> = ents.iter().filter_map(|e| Some((key(e, "targetname")?, e))).collect();
    let mut m = HashMap::new();
    for e in ents {
        if let Some(n) = key(e, "model").and_then(|v| v.strip_prefix('*')).and_then(|v| v.parse::<usize>().ok()) {
            let mut at = vec3_key(e, "origin").unwrap_or(Vec3::ZERO);
            if key(e, "classname") == Some("func_tracktrain") {
                if let Some(p) = key(e, "target").and_then(|t| named.get(t)).and_then(|p| vec3_key(p, "origin")) {
                    let height = key(e, "height").and_then(|h| h.parse::<f32>().ok()).unwrap_or(0.0);
                    at = p + Vec3::Z * height;
                }
            }
            m.insert(n, at);
        }
    }
    m
}

/// The base material behind a map's cubemap-patched copy
/// (`maps/testchmb_a_00/metal/metal_lift001_-537_-308_126` → `metal/metal_lift001`).
pub fn base_material(name: &str) -> String {
    let n = name.replace('\\', "/").to_ascii_lowercase();
    let Some(rest) = n.strip_prefix("maps/") else { return n };
    let Some((_, rest)) = rest.split_once('/') else { return n };
    let parts: Vec<&str> = rest.rsplitn(4, '_').collect();
    if parts.len() == 4 && parts[..3].iter().all(|p| p.parse::<i32>().is_ok()) {
        parts[3].to_string()
    } else {
        rest.to_string()
    }
}

/// Whether a texture isn't drawn (tools, sky).
pub fn tool_texture(name: &str) -> bool {
    let n = name.to_ascii_lowercase();
    n.starts_with("tools/") || n.contains("toolsskybox")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cubemap_patches_name_their_base() {
        assert_eq!(base_material("maps/testchmb_a_00/metal/metal_lift001_-537_-308_126"), "metal/metal_lift001");
        assert_eq!(base_material("maps/testchmb_a_00/metal/metal_lift001_gradient_-537_-308_126"), "metal/metal_lift001_gradient");
        assert_eq!(base_material("METAL/METALWALL048B"), "metal/metalwall048b");
    }

    #[test]
    fn entities_parse_with_repeated_keys() {
        let e = parse_entities("{\n\"classname\" \"logic_relay\"\n\"OnTrigger\" \"a,Open,,0,-1\"\n\"OnTrigger\" \"b,Close,,1,-1\"\n}\n{\n\"classname\" \"worldspawn\"\n}");
        assert_eq!(e.len(), 2);
        assert_eq!(e[0].iter().filter(|(k, _)| k == "OnTrigger").count(), 2);
        assert_eq!(key(&e[1], "classname"), Some("worldspawn"));
    }
}
