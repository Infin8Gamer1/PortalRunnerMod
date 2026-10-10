//! Writes one of Portal's maps out for the game (see `bsp.rs` for what's read):
//!
//! - `maps/<name>.mesh`: the faces to draw, in our space (metres, Y up), a group per brush
//!   model and material: positions, normals, UVs and triangles. (Binary; read by the game's
//!   `source_map.rs`.)
//! - `maps/<name>.solids`: collision boxes, a line each: `<kind> <model> <min xyz> <max xyz>`,
//!   kind one of `bounds` (a brush model's bounds), `white` (takes portals), `black`, `glass`, `grate`, `clip` (stops Faith,
//!   nothing else), `goo`, `water`.
//! - `maps/<name>.ents`: the entity lump as it is.
//! - `maps/<name>.props`: its props (static, dynamic, physics), their models converted once
//!   each into `models/map/`.
//! - `textures/map/<material>.png` (and Portal with RTX's `_normal`, `_mr`, `_emissive` where
//!   it has them): each material's texture, once.

use std::collections::HashMap;
use std::path::Path;

use glam::Vec3;

use crate::bsp::{self, Bsp};

/// The largest texture written for a map's materials (texels a side).
const MAP_TEXTURE_MAX: u32 = 1024;

/// What a material looks like and does.
#[derive(Clone, Debug, Default)]
pub struct MapMaterial {
    /// Our name for it: its base material's path (`metal/metalwall048b`).
    pub name: String,
    pub portalable: bool,
    pub translucent: bool,
    pub alphatest: bool,
    pub additive: bool,
    /// Glows (light panels: UnlitGeneric / $selfillum).
    pub emissive: bool,
    /// Its texture was written.
    pub textured: bool,
}

impl MapMaterial {
    fn flags(&self) -> u8 {
        (self.translucent as u8) | (self.alphatest as u8) << 1 | (self.emissive as u8) << 2 | (self.additive as u8) << 3 | (self.portalable as u8) << 4 | (self.textured as u8) << 5
    }
}

/// Our space from Source's (see `convert_point`).
fn ours(p: Vec3) -> Vec3 {
    Vec3::from(crate::convert_point(p.to_array()))
}

/// Read a material's VMT (following a patch's include), and write its texture.
fn material(name: &str, read: &dyn Fn(&str) -> Option<Vec<u8>>, remix: Option<&crate::remix::Remix>, tex_dir: &Path) -> MapMaterial {
    let mut text = read(&format!("materials/{name}.vmt")).map(|b| String::from_utf8_lossy(&b).into_owned()).unwrap_or_default();
    for _ in 0..3 {
        let Some(inc) = crate::vmt_value(&text, "include") else { break };
        if crate::vmt_base_texture(&text).is_some() {
            break;
        }
        match read(&inc.replace('\\', "/").to_ascii_lowercase()) {
            Some(b) => text = format!("{}\n{}", String::from_utf8_lossy(&b), text),
            None => break,
        }
    }
    let flag = |k: &str| crate::vmt_value(&text, k).is_some_and(|v| v.trim() == "1");
    let shader = text.trim_start().trim_start_matches(|c: char| c == '/' || c.is_whitespace()).split(['\n', '{']).next().unwrap_or("").trim().trim_matches('"').to_ascii_lowercase();
    let mut m = MapMaterial {
        name: name.to_string(),
        portalable: !flag("%noportal") && !text.is_empty(),
        translucent: flag("$translucent"),
        alphatest: flag("$alphatest"),
        additive: flag("$additive"),
        emissive: flag("$selfillum") || shader.starts_with("unlitgeneric") || name.starts_with("lights/"),
        textured: false,
    };
    let png = tex_dir.join(format!("{name}.png"));
    if png.exists() {
        m.textured = true;
        return m;
    }
    let Some(base) = crate::vmt_base_texture(&text) else { return m };
    let Some(vtf) = read(&format!("materials/{base}.vtf")) else { return m };
    let Ok(img) = crate::decode_vtf(&vtf) else { return m };
    let _ = std::fs::create_dir_all(png.parent().unwrap());
    let img = crate::remix::shrink(img, MAP_TEXTURE_MAX);
    if img.save(&png).is_err() {
        return m;
    }
    m.textured = true;
    // Portal with RTX's maps for it, where it has them (not glass: Portal with RTX's is a
    // translucent shader, its colour map without the panes' alpha).
    if name.starts_with("glass/") {
        return m;
    }
    if let Some((r, rm)) = remix.zip(crate::remix::vtf_hash(&vtf)).and_then(|(r, h)| Some((r, r.material(h)?))) {
        match r.bake(&rm, MAP_TEXTURE_MAX) {
            Ok(baked) => {
                for (img, suffix) in [(&baked.albedo, ""), (&baked.normal, "_normal"), (&baked.metallic_roughness, "_mr"), (&baked.emissive, "_emissive")] {
                    if let Some(img) = img {
                        let _ = img.save(tex_dir.join(format!("{name}{suffix}.png")));
                    }
                }
            }
            Err(e) => eprintln!("    {name}: Portal with RTX's maps: {e}"),
        }
    }
    m
}

#[derive(Default)]
struct Group {
    pos: Vec<[f32; 3]>,
    nrm: Vec<[f32; 3]>,
    uv: Vec<[f32; 2]>,
    idx: Vec<u32>,
}

/// Models the game draws its own way in Portal's maps (cubes, buttons, grills).
const OWN_MODELS: &[&str] = &["models/props/metal_box.mdl", "models/props/button_top_reference.mdl", "models/props/button_base_reference.mdl"];

/// A prop model's file under `models/` (`map/props/door_01_frame_wide_reference`, with
/// `_skinN` for another skin), as the game names it.
pub fn prop_glb(model: &str, skin: i32) -> String {
    let stem = model.trim_start_matches("models/").trim_end_matches(".mdl");
    if skin > 0 { format!("map/{stem}_skin{skin}") } else { format!("map/{stem}") }
}

/// Write a map's props out (`maps/<name>.props`, a line each: `kind|model file|x y z|pitch
/// yaw roll|parent|name`, Source space), and say which (model, skin)s they use.
fn props(bsp: &Bsp, ents: &[bsp::Entity], path: &Path) -> Result<Vec<(String, i32)>, String> {
    let mut text = String::new();
    let mut used = Vec::new();
    let mut add = |kind: &str, model: &str, skin: i32, o: Vec3, a: Vec3, parent: &str, name: &str| {
        let model = model.to_ascii_lowercase().replace('\\', "/");
        if OWN_MODELS.contains(&model.as_str()) || model.contains("portal_cleanser") || !model.ends_with(".mdl") {
            return;
        }
        text.push_str(&format!("{kind}|{}|{} {} {}|{} {} {}|{parent}|{name}\n", prop_glb(&model, skin), o.x, o.y, o.z, a.x, a.y, a.z));
        if !used.contains(&(model.clone(), skin)) {
            used.push((model, skin));
        }
    };
    for p in &bsp.static_props {
        add("static", &p.model, p.skin, p.origin, p.angles, "", "");
    }
    for e in ents {
        let class = bsp::key(e, "classname").unwrap_or("");
        if !matches!(class, "prop_dynamic" | "prop_dynamic_override" | "prop_physics" | "prop_physics_override" | "prop_physics_multiplayer") {
            continue;
        }
        let (Some(model), Some(o)) = (bsp::key(e, "model"), bsp::vec3_key(e, "origin")) else { continue };
        let a = bsp::vec3_key(e, "angles").unwrap_or(Vec3::ZERO);
        let skin = bsp::key(e, "skin").and_then(|s| s.parse().ok()).unwrap_or(0);
        let kind = if class.starts_with("prop_physics") { "physics" } else { "dynamic" };
        add(kind, model, skin, o, a, bsp::key(e, "parentname").unwrap_or(""), bsp::key(e, "targetname").unwrap_or(""));
    }
    std::fs::write(path, text).map_err(|e| e.to_string())?;
    Ok(used)
}

/// Write a map out (see the module notes). Returns (groups, boxes, materials) written, and
/// the prop models it uses.
pub fn extract(name: &str, bytes: &[u8], read: &dyn Fn(&str) -> Option<Vec<u8>>, remix: Option<&crate::remix::Remix>, out: &Path) -> Result<(usize, usize, usize, Vec<(String, i32)>), String> {
    let bsp = Bsp::parse(bytes)?;
    let ents = bsp::parse_entities(&bsp.entities);
    let origins = bsp::model_origins(&ents);
    let tex_dir = out.join("textures").join("map");
    let map_dir = out.join("maps");
    std::fs::create_dir_all(&map_dir).map_err(|e| e.to_string())?;

    // Materials, by the map's texture names.
    let mut materials: Vec<MapMaterial> = Vec::new();
    let mut by_name: HashMap<String, usize> = HashMap::new();
    let mut by_texdata: HashMap<usize, Option<usize>> = HashMap::new();
    let mut material_of = |texinfo: i32, materials: &mut Vec<MapMaterial>| -> Option<usize> {
        let td = bsp.texinfo.get(usize::try_from(texinfo).ok()?)?.texdata;
        let td = usize::try_from(td).ok()?;
        *by_texdata.entry(td).or_insert_with(|| {
            let raw = &bsp.texdata.get(td)?.name;
            if bsp::tool_texture(raw) {
                return None;
            }
            let base = bsp::base_material(raw);
            Some(*by_name.entry(base.clone()).or_insert_with(|| {
                materials.push(material(&base, read, remix, &tex_dir));
                materials.len() - 1
            }))
        })
    };

    // Faces: a group per (brush model, material).
    let mut groups: Vec<((usize, usize), Group)> = Vec::new();
    let mut group_at: HashMap<(usize, usize), usize> = HashMap::new();
    for (mi, model) in bsp.models.iter().enumerate() {
        let origin = if mi == 0 { Vec3::ZERO } else { origins.get(&mi).copied().unwrap_or(Vec3::ZERO) };
        for f in &bsp.faces[model.first_face..model.first_face + model.num_faces] {
            let Some((td, ti)) = bsp.texture(f.texinfo) else { continue };
            if ti.flags & bsp::SURF_HIDDEN != 0 {
                continue;
            }
            let Some(mat) = material_of(f.texinfo, &mut materials) else { continue };
            let (w, h) = (td.width.max(1) as f32, td.height.max(1) as f32);
            let pts = bsp.face_points(f);
            if pts.len() < 3 {
                continue;
            }
            // Source winds a face clockwise seen from its front (the plane's `side` isn't to be
            // trusted): its normal is the winding's, turned round.
            let newell = pts.iter().zip(pts.iter().cycle().skip(1)).fold(Vec3::ZERO, |n, (a, b)| n + Vec3::new((a.y - b.y) * (a.z + b.z), (a.z - b.z) * (a.x + b.x), (a.x - b.x) * (a.y + b.y)));
            let n = ours(-newell).normalize_or_zero();
            let gi = *group_at.entry((mi, mat)).or_insert_with(|| {
                groups.push(((mi, mat), Group::default()));
                groups.len() - 1
            });
            let g = &mut groups[gi].1;
            let base = g.pos.len() as u32;
            for p in &pts {
                let u = (ti.s[0] * p.x + ti.s[1] * p.y + ti.s[2] * p.z + ti.s[3]) / w;
                let v = (ti.t[0] * p.x + ti.t[1] * p.y + ti.t[2] * p.z + ti.t[3]) / h;
                g.pos.push(ours(*p + origin).to_array());
                g.nrm.push(n.to_array());
                g.uv.push([u, v]);
            }
            // A fan, wound to face the way the face does.
            let (a, b, c) = (ours(pts[0]), ours(pts[1]), ours(pts[2]));
            let flip = (b - a).cross(c - a).dot(n) < 0.0;
            for k in 1..pts.len() as u32 - 1 {
                if flip {
                    g.idx.extend_from_slice(&[base, base + k + 1, base + k]);
                } else {
                    g.idx.extend_from_slice(&[base, base + k, base + k + 1]);
                }
            }
        }
    }

    // Collision boxes; first, each brush model's bounds (triggers and the like are these).
    let mut solids = String::new();
    let mut nboxes = 0;
    for (mi, model) in bsp.models.iter().enumerate() {
        let origin = if mi == 0 { Vec3::ZERO } else { origins.get(&mi).copied().unwrap_or(Vec3::ZERO) };
        let (a, z) = (ours(model.mins + origin), ours(model.maxs + origin));
        let (a, z) = (a.min(z), a.max(z));
        solids.push_str(&format!("bounds {mi} {:.4} {:.4} {:.4} {:.4} {:.4} {:.4}\n", a.x, a.y, a.z, z.x, z.y, z.z));
    }
    for mi in 0..bsp.models.len() {
        let origin = if mi == 0 { Vec3::ZERO } else { origins.get(&mi).copied().unwrap_or(Vec3::ZERO) };
        for bi in bsp.model_brushes(mi) {
            let b = &bsp.brushes[bi];
            let c = b.contents;
            let kind = if c & bsp::CONTENTS_SLIME != 0 {
                "goo"
            } else if c & bsp::CONTENTS_WATER != 0 {
                "water"
            } else if c & bsp::CONTENTS_WINDOW != 0 {
                "glass"
            } else if c & bsp::CONTENTS_GRATE != 0 {
                "grate"
            } else if c & bsp::CONTENTS_SOLID != 0 {
                let mats: Vec<usize> = bsp.brush_textures(b).into_iter().filter_map(|t| material_of(t, &mut materials)).collect();
                if mats.iter().any(|&m| materials[m].name.starts_with("glass/")) {
                    "glass"
                } else {
                    let yes = mats.iter().filter(|&&m| materials[m].portalable).count();
                    if yes > 0 && yes * 2 >= mats.len() { "white" } else { "black" }
                }
            } else if c & bsp::CONTENTS_PLAYERCLIP != 0 {
                "clip"
            } else {
                continue;
            };
            for (lo, hi) in bsp::brush_boxes(&bsp, b) {
                let (a, z) = (ours(lo + origin), ours(hi + origin));
                let (a, z) = (a.min(z), a.max(z));
                solids.push_str(&format!("{kind} {mi} {:.4} {:.4} {:.4} {:.4} {:.4} {:.4}\n", a.x, a.y, a.z, z.x, z.y, z.z));
                nboxes += 1;
            }
        }
    }

    // The mesh file.
    let mut m = Vec::new();
    m.extend_from_slice(b"PRMESH1\0");
    m.extend_from_slice(&(materials.len() as u32).to_le_bytes());
    for mat in &materials {
        m.extend_from_slice(&(mat.name.len() as u16).to_le_bytes());
        m.extend_from_slice(mat.name.as_bytes());
        m.push(mat.flags());
    }
    m.extend_from_slice(&(groups.len() as u32).to_le_bytes());
    for ((model, mat), g) in &groups {
        m.extend_from_slice(&(*model as u32).to_le_bytes());
        m.extend_from_slice(&(*mat as u32).to_le_bytes());
        m.extend_from_slice(&(g.pos.len() as u32).to_le_bytes());
        for i in 0..g.pos.len() {
            for v in g.pos[i].iter().chain(&g.nrm[i]).chain(&g.uv[i]) {
                m.extend_from_slice(&v.to_le_bytes());
            }
        }
        m.extend_from_slice(&(g.idx.len() as u32).to_le_bytes());
        for i in &g.idx {
            m.extend_from_slice(&i.to_le_bytes());
        }
    }
    std::fs::write(map_dir.join(format!("{name}.mesh")), m).map_err(|e| e.to_string())?;
    std::fs::write(map_dir.join(format!("{name}.solids")), solids).map_err(|e| e.to_string())?;
    std::fs::write(map_dir.join(format!("{name}.ents")), &bsp.entities).map_err(|e| e.to_string())?;
    let used = props(&bsp, &ents, &map_dir.join(format!("{name}.props")))?;
    Ok((groups.len(), nboxes, materials.len(), used))
}
