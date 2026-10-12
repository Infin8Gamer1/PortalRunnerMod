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
    /// A decal that darkens what it's on (DecalModulate: bullet holes, scorches).
    pub modulate: bool,
}

impl MapMaterial {
    fn flags(&self) -> u8 {
        (self.translucent as u8) | (self.alphatest as u8) << 1 | (self.emissive as u8) << 2 | (self.additive as u8) << 3 | (self.portalable as u8) << 4 | (self.textured as u8) << 5 | (self.modulate as u8) << 6
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
        modulate: shader.starts_with("decalmodulate"),
    };
    let png = tex_dir.join(format!("{name}.png"));
    let base = crate::vmt_base_texture(&text);
    let vtf = base.as_ref().and_then(|b| read(&format!("materials/{b}.vtf")));
    // A texture with frames (a door's state panel: X, then the check) has each one written,
    // `_frame1` on: the map's logic picks which is shown.
    let frames = vtf.as_deref().map_or(1, crate::vtf_frames);
    if png.exists() && (frames == 1 || tex_dir.join(format!("{name}_frame1.png")).exists()) {
        m.textured = true;
        return m;
    }
    let Some(vtf) = vtf else { return m };
    let Ok(img) = crate::decode_vtf(&vtf) else { return m };
    let _ = std::fs::create_dir_all(png.parent().unwrap());
    let img = crate::remix::shrink(img, MAP_TEXTURE_MAX);
    if img.save(&png).is_err() {
        return m;
    }
    m.textured = true;
    if frames > 1 {
        for k in 1..frames {
            if let Ok(img) = crate::decode_vtf_frame(&vtf, k) {
                let _ = crate::remix::shrink(img, MAP_TEXTURE_MAX).save(tex_dir.join(format!("{name}_frame{k}.png")));
            }
        }
        // Portal with RTX remasters one frame only (it's found by the last one's pixels): the
        // frames stay Portal's, so they match.
        for suffix in ["_normal", "_mr", "_emissive"] {
            let _ = std::fs::remove_file(tex_dir.join(format!("{name}{suffix}.png")));
        }
        return m;
    }
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

/// A face's triangles (corner indices), a fan wound to face `n`. Which way the corners go
/// round is the whole polygon's (Newell's normal): a face's first three corners can lie on a
/// line (Source adds corners along an edge where faces meet), and judged by those alone such
/// faces came out wound either way, half of them backwards and culled from the room's side (a
/// hole in the wall you could walk into). The slivers between corners on one line are left out.
fn fan(q: &[Vec3], n: Vec3) -> Vec<u32> {
    let winding = q.iter().zip(q.iter().cycle().skip(1)).fold(Vec3::ZERO, |w, (a, b)| w + a.cross(*b));
    let flip = winding.dot(n) < 0.0;
    let mut out = Vec::new();
    for k in 1..q.len().saturating_sub(1) {
        let (a, b, c) = (q[0], q[k], q[k + 1]);
        if (b - a).cross(c - a).length_squared() < 1e-10 {
            continue;
        }
        let k = k as u32;
        out.extend(if flip { [0, k + 1, k] } else { [0, k, k + 1] });
    }
    out
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

/// Write a map's overlays out (`maps/<name>.overlays`, a line each: `material|flags|name|
/// x y z u v (each corner, ours, four times)|nx ny nz`), their materials' textures once. `name`
/// is its info_overlay_accessor's, where the logic can switch its texture's frame (a door's
/// indicator strip, blue to orange).
fn overlays(bsp: &Bsp, ents: &[bsp::Entity], path: &Path, read: &dyn Fn(&str) -> Option<Vec<u8>>, remix: Option<&crate::remix::Remix>, tex_dir: &Path) -> Result<(), String> {
    let names: HashMap<i32, &str> = ents
        .iter()
        .filter(|e| bsp::key(e, "classname") == Some("info_overlay_accessor"))
        .filter_map(|e| Some((bsp::key(e, "OverlayID")?.trim().parse().ok()?, bsp::key(e, "targetname")?)))
        .collect();
    let mut mats: HashMap<String, MapMaterial> = HashMap::new();
    let mut text = String::new();
    for o in &bsp.overlays {
        let Some((td, _)) = bsp.texture(o.texinfo) else { continue };
        if bsp::tool_texture(&td.name) {
            continue;
        }
        let base = bsp::base_material(&td.name);
        let m = mats.entry(base.clone()).or_insert_with(|| material(&base, read, remix, tex_dir));
        if !m.textured {
            continue;
        }
        let corners: Vec<String> = o.quad().iter().map(|(p, uv)| {
            let q = ours(*p);
            format!("{} {} {} {} {}", q.x, q.y, q.z, uv[0], uv[1])
        }).collect();
        let n = ours(o.normal).normalize_or_zero();
        text.push_str(&format!("{base}|{}|{}|{}|{} {} {}\n", m.flags(), names.get(&o.id).copied().unwrap_or(""), corners.join(" "), n.x, n.y, n.z));
    }
    std::fs::write(path, text).map_err(|e| e.to_string())
}

/// A prop model the map uses: (model, skin, the animations the map plays on it).
pub type UsedModel = (String, i32, Vec<String>);

/// Write a map's props out (`maps/<name>.props`, a line each: `kind|model file|x y z|pitch
/// yaw roll|parent|name`, Source space; a physics prop's line goes on `|bottom`, how far its
/// lowest point is below its origin as it's turned, so the game can stand it on what's under
/// it), and say which models they use.
fn props(bsp: &Bsp, ents: &[bsp::Entity], path: &Path, read: &dyn Fn(&str) -> Option<Vec<u8>>) -> Result<Vec<UsedModel>, String> {
    // Props the logic puts on an attachment of the prop they're parented to (a catcher's ball
    // on `ball_attachment`): they're written where that is (#52).
    let mut on_attachment: HashMap<String, String> = HashMap::new();
    for e in ents {
        for (k, v) in e {
            let f: Vec<&str> = v.split([',', '\x1b']).collect();
            if (k.starts_with("On") || k.starts_with("Out")) && f.len() >= 3 && f[1].eq_ignore_ascii_case("SetParentAttachment") {
                on_attachment.insert(f[0].trim().to_ascii_lowercase(), f[2].trim().to_string());
            }
        }
    }
    let attachment_at = |e: &bsp::Entity| -> Option<Vec3> {
        let name = on_attachment.get(&bsp::key(e, "targetname")?.to_ascii_lowercase())?;
        let parent_name = bsp::key(e, "parentname")?;
        let parent = ents.iter().find(|p| bsp::key(p, "targetname").is_some_and(|n| n.eq_ignore_ascii_case(parent_name)))?;
        // (Posed as the catcher closes on it: the ball only shows once it's caught one. The
        // bind pose leaves out the model's turn upright, so it's only the last resort.)
        let mdl = read(&bsp::key(parent, "model")?.to_ascii_lowercase())?;
        let local = ["closed", "close", "idle"].iter().find_map(|a| crate::attachment_point_posed(&mdl, name, a)).or_else(|| crate::attachment_point(&mdl, name))?;
        let a = bsp::vec3_key(parent, "angles").unwrap_or(Vec3::ZERO);
        let turn = glam::Quat::from_rotation_z(a.y.to_radians()) * glam::Quat::from_rotation_y(a.x.to_radians()) * glam::Quat::from_rotation_x(a.z.to_radians());
        Some(bsp::vec3_key(parent, "origin")? + turn * local)
    };
    // The animations the map's logic plays on each named prop.
    let mut anims: HashMap<String, Vec<String>> = HashMap::new();
    // And the skins it switches each to (a chamber sign flickering on: 2, 3, 4... 6).
    let mut skins: HashMap<String, Vec<i32>> = HashMap::new();
    for e in ents {
        for (k, v) in e {
            if !(k.starts_with("On") || k.starts_with("Out")) {
                continue;
            }
            let f: Vec<&str> = v.split([',', '\x1b']).collect();
            if f.len() >= 3
                && f[1].eq_ignore_ascii_case("Skin")
                && let Ok(k) = f[2].trim().parse::<i32>()
            {
                let list = skins.entry(f[0].trim().to_ascii_lowercase()).or_default();
                if !list.contains(&k) {
                    list.push(k);
                }
            }
            if f.len() >= 3 && (f[1].eq_ignore_ascii_case("SetAnimation") || f[1].eq_ignore_ascii_case("SetDefaultAnimation")) && !f[2].trim().is_empty() {
                let list = anims.entry(f[0].trim().to_ascii_lowercase()).or_default();
                let a = f[2].trim().to_ascii_lowercase();
                if !list.contains(&a) {
                    list.push(a);
                }
            }
        }
    }
    let mut text = String::new();
    let mut used: Vec<UsedModel> = Vec::new();
    let mut add = |kind: &str, model: &str, skin: i32, o: Vec3, a: Vec3, parent: &str, name: &str, default_anim: Option<&str>| {
        let model = model.to_ascii_lowercase().replace('\\', "/");
        if OWN_MODELS.contains(&model.as_str()) || model.contains("portal_cleanser") || !model.ends_with(".mdl") {
            return;
        }
        let bottom = if kind == "physics" { read(&model).and_then(|m| hull_bottom(&m, a)).map(|b| format!("|{b}")).unwrap_or_default() } else { String::new() };
        text.push_str(&format!("{kind}|{}|{} {} {}|{} {} {}|{parent}|{name}{bottom}\n", prop_glb(&model, skin), o.x, o.y, o.z, a.x, a.y, a.z));
        let mut its: Vec<String> = anims.get(&name.to_ascii_lowercase()).cloned().unwrap_or_default();
        its.extend(default_anim.map(|d| d.to_ascii_lowercase()));
        let switched = if name.is_empty() { Vec::new() } else { skins.get(&name.to_ascii_lowercase()).cloned().unwrap_or_default() };
        for skin in std::iter::once(skin).chain(switched) {
            match used.iter_mut().find(|(m, s, _)| *m == model && *s == skin) {
                Some((_, _, list)) => {
                    for a in &its {
                        if !list.contains(a) {
                            list.push(a.clone());
                        }
                    }
                }
                None => used.push((model.clone(), skin, its.clone())),
            }
        }
    };
    for p in &bsp.static_props {
        add("static", &p.model, p.skin, p.origin, p.angles, "", "", None);
    }
    // Its glow sprites (env_sprite: a catcher's halo and ring, the lamps' glows): `sprite|
    // <material>|x y z|<size> <r> <g> <b> <alpha> <rendermode> <on>|parent|name`, their textures
    // written once. Size is Source's (units across: texture width times scale). One's on from
    // the start if it says so, or if nothing could turn it on (it's unnamed). The escapes'
    // near-black sphere silhouettes are left out.
    let tex_dir = path.parent().and_then(|m| m.parent()).map(|o| o.join("textures").join("map")).unwrap_or_default();
    let mut sprites = String::new();
    for e in ents {
        if bsp::key(e, "classname") != Some("env_sprite") {
            continue;
        }
        let (Some(model), Some(o)) = (bsp::key(e, "model"), bsp::vec3_key(e, "origin")) else { continue };
        let name = model.to_ascii_lowercase().replace('\\', "/");
        let name = name.trim_start_matches("materials/").trim_end_matches(".vmt").trim_end_matches(".spr").to_string();
        if name.contains("sphere_silhouette") {
            continue;
        }
        let mat = material(&name, read, None, &tex_dir);
        if !mat.textured {
            continue;
        }
        let vmt = read(&format!("materials/{name}.vmt")).map(|b| String::from_utf8_lossy(&b).into_owned()).unwrap_or_default();
        let width = crate::vmt_base_texture(&vmt).and_then(|b| read(&format!("materials/{b}.vtf"))).and_then(|v| v.get(16..18).map(|w| u16::from_le_bytes([w[0], w[1]]))).unwrap_or(64);
        let scale = bsp::key(e, "scale").and_then(|v| v.trim().parse::<f32>().ok()).filter(|s| *s > 0.0).unwrap_or(1.0);
        let color: Vec<f32> = bsp::key(e, "rendercolor").unwrap_or("255 255 255").split_whitespace().filter_map(|c| c.parse().ok()).collect();
        let alpha = bsp::key(e, "renderamt").and_then(|v| v.trim().parse::<f32>().ok()).unwrap_or(255.0);
        let mode = bsp::key(e, "rendermode").and_then(|v| v.trim().parse::<i32>().ok()).unwrap_or(0);
        let target = bsp::key(e, "targetname").unwrap_or("");
        let on = target.is_empty() || bsp::key(e, "spawnflags").and_then(|v| v.trim().parse::<i32>().ok()).unwrap_or(0) & 1 != 0;
        let o = attachment_at(e).unwrap_or(o);
        let c = |k: usize| color.get(k).copied().unwrap_or(255.0);
        sprites.push_str(&format!(
            "sprite|{name}|{} {} {}|{} {} {} {} {} {} {}|{}|{target}\n",
            o.x, o.y, o.z, width as f32 * scale, c(0), c(1), c(2), alpha, mode, u8::from(on), bsp::key(e, "parentname").unwrap_or("")
        ));
    }
    // An emancipation grill's particle systems (info_particle_system: the field's streaks,
    // the sparks off its edges): `particles|<effect>|x y z|pitch yaw roll|parent|name`.
    for e in ents {
        if bsp::key(e, "classname") != Some("info_particle_system") {
            continue;
        }
        let Some(effect) = bsp::key(e, "effect_name").map(str::to_ascii_lowercase).filter(|n| n == "portal_cleanser" || n == "cleanser_edge_1") else { continue };
        let (Some(o), a) = (bsp::vec3_key(e, "origin"), bsp::vec3_key(e, "angles").unwrap_or(Vec3::ZERO)) else { continue };
        sprites.push_str(&format!(
            "particles|{effect}|{} {} {}|{} {} {}|{}|{}\n",
            o.x, o.y, o.z, a.x, a.y, a.z, bsp::key(e, "parentname").unwrap_or(""), bsp::key(e, "targetname").unwrap_or("")
        ));
    }
    for e in ents {
        let class = bsp::key(e, "classname").unwrap_or("");
        if !matches!(class, "prop_dynamic" | "prop_dynamic_override" | "prop_physics" | "prop_physics_override" | "prop_physics_multiplayer" | "prop_portal_stats_display") {
            continue;
        }
        // (The elevator car with the stats screen in it, from Chamber 13 on, has no model key:
        // Portal's server always gives it the round elevator's body.)
        let model = if class == "prop_portal_stats_display" { Some("models/props/round_elevator_body.mdl") } else { bsp::key(e, "model") };
        let (Some(model), Some(o)) = (model, bsp::vec3_key(e, "origin")) else { continue };
        let o = attachment_at(e).unwrap_or(o);
        let a = bsp::vec3_key(e, "angles").unwrap_or(Vec3::ZERO);
        let skin = bsp::key(e, "skin").and_then(|s| s.parse().ok()).unwrap_or(0);
        let kind = if class.starts_with("prop_physics") { "physics" } else { "dynamic" };
        let default_anim = bsp::key(e, "DefaultAnim").filter(|d| !d.trim().is_empty());
        add(kind, model, skin, o, a, bsp::key(e, "parentname").unwrap_or(""), bsp::key(e, "targetname").unwrap_or(""), default_anim);
    }
    text.push_str(&sprites);
    std::fs::write(path, text).map_err(|e| e.to_string())?;
    Ok(used)
}

/// How far a model's lowest point is below its origin, turned by Source's angles (pitch yaw
/// roll): the corners of its hull (the MDL header's `hull_min`/`hull_max`).
fn hull_bottom(mdl: &[u8], a: Vec3) -> Option<f32> {
    let f = |o: usize| -> Option<f32> { Some(f32::from_le_bytes(mdl.get(o..o + 4)?.try_into().ok()?)) };
    let (lo, hi) = (Vec3::new(f(104)?, f(108)?, f(112)?), Vec3::new(f(116)?, f(120)?, f(124)?));
    if lo == hi {
        return None;
    }
    let turn = glam::Quat::from_rotation_z(a.y.to_radians()) * glam::Quat::from_rotation_y(a.x.to_radians()) * glam::Quat::from_rotation_x(a.z.to_radians());
    let low = (0..8).map(|k| (turn * Vec3::new(if k & 1 == 0 { lo.x } else { hi.x }, if k & 2 == 0 { lo.y } else { hi.y }, if k & 4 == 0 { lo.z } else { hi.z })).z).fold(f32::MAX, f32::min);
    Some(-low)
}

/// Write a map out (see the module notes). Returns (groups, boxes, materials) written, and
/// the prop models it uses.
pub fn extract(name: &str, bytes: &[u8], read: &dyn Fn(&str) -> Option<Vec<u8>>, remix: Option<&crate::remix::Remix>, out: &Path) -> Result<(usize, usize, usize, Vec<UsedModel>), String> {
    let bsp = Bsp::parse(bytes)?;
    let mut ents = bsp::parse_entities(&bsp.entities);
    let origins = bsp::model_origins(&ents);
    // Templates made as the map starts, moved to where they're made (their brushes too).
    let placed = crate::templates::place(&mut ents);
    let place = |mi: usize, p: Vec3| placed.get(&mi).map_or(p, |t| t.point(p));
    let turn = |mi: usize, d: Vec3| placed.get(&mi).map_or(d, |t| t.turn * d);
    let place_box = |mi: usize, lo: Vec3, hi: Vec3| -> (Vec3, Vec3) {
        let corners = [0, 1, 2, 3, 4, 5, 6, 7].map(|k| ours(place(mi, Vec3::new(if k & 1 == 0 { lo.x } else { hi.x }, if k & 2 == 0 { lo.y } else { hi.y }, if k & 4 == 0 { lo.z } else { hi.z }))));
        (corners.iter().fold(Vec3::splat(f32::MAX), |a, c| a.min(*c)), corners.iter().fold(Vec3::splat(f32::MIN), |a, c| a.max(*c)))
    };
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
            let n = ours(turn(mi, -newell)).normalize_or_zero();
            let gi = *group_at.entry((mi, mat)).or_insert_with(|| {
                groups.push(((mi, mat), Group::default()));
                groups.len() - 1
            });
            let g = &mut groups[gi].1;
            let base = g.pos.len() as u32;
            // A displacement: its grid, textured as its flat face is, each triangle wound to
            // face the way the face does, normals from the grid.
            if let Some((grid, side)) = bsp.disp_grid(f) {
                let q: Vec<Vec3> = grid.iter().map(|(p, _)| ours(place(mi, *p + origin))).collect();
                let mut nrm = vec![Vec3::ZERO; q.len()];
                let mut tris = Vec::new();
                for i in 0..side - 1 {
                    for j in 0..side - 1 {
                        let (a, b, c, d) = (i * side + j, (i + 1) * side + j, (i + 1) * side + j + 1, i * side + j + 1);
                        for t in [[a, b, c], [a, c, d]] {
                            let w = (q[t[1]] - q[t[0]]).cross(q[t[2]] - q[t[0]]);
                            let t = if w.dot(n) < 0.0 { [t[0], t[2], t[1]] } else { t };
                            let w = w.length() * if w.dot(n) < 0.0 { -1.0 } else { 1.0 };
                            let face_n = (q[t[1]] - q[t[0]]).cross(q[t[2]] - q[t[0]]).normalize_or_zero() * w.abs();
                            for k in t {
                                nrm[k] += face_n;
                            }
                            tris.extend(t.map(|k| base + k as u32));
                        }
                    }
                }
                for (k, (_, flat)) in grid.iter().enumerate() {
                    let u = (ti.s[0] * flat.x + ti.s[1] * flat.y + ti.s[2] * flat.z + ti.s[3]) / w;
                    let v = (ti.t[0] * flat.x + ti.t[1] * flat.y + ti.t[2] * flat.z + ti.t[3]) / h;
                    g.pos.push(q[k].to_array());
                    g.nrm.push(nrm[k].normalize_or(n).to_array());
                    g.uv.push([u, v]);
                }
                g.idx.extend(tris);
                continue;
            }
            for p in &pts {
                let u = (ti.s[0] * p.x + ti.s[1] * p.y + ti.s[2] * p.z + ti.s[3]) / w;
                let v = (ti.t[0] * p.x + ti.t[1] * p.y + ti.t[2] * p.z + ti.t[3]) / h;
                g.pos.push(ours(place(mi, *p + origin)).to_array());
                g.nrm.push(n.to_array());
                g.uv.push([u, v]);
            }
            let q: Vec<Vec3> = pts.iter().map(|p| ours(turn(mi, *p))).collect();
            g.idx.extend(fan(&q, n).into_iter().map(|k| base + k));
        }
    }

    // Collision boxes; first, each brush model's bounds (triggers and the like are these).
    let mut solids = String::new();
    let mut nboxes = 0;
    for (mi, model) in bsp.models.iter().enumerate() {
        let origin = if mi == 0 { Vec3::ZERO } else { origins.get(&mi).copied().unwrap_or(Vec3::ZERO) };
        let (a, z) = place_box(mi, model.mins + origin, model.maxs + origin);
        solids.push_str(&format!("bounds {mi} {:.4} {:.4} {:.4} {:.4} {:.4} {:.4}\n", a.x, a.y, a.z, z.x, z.y, z.z));
    }
    // A floor displacement stands on its brush: a box a grid cell, up to the cell's highest
    // corner, so the walk follows its mounds (steps no taller than the slope's).
    for f in &bsp.faces[bsp.models[0].first_face..bsp.models[0].first_face + bsp.models[0].num_faces] {
        let Some((grid, side)) = bsp.disp_grid(f) else { continue };
        let q: Vec<(Vec3, Vec3)> = grid.iter().map(|(p, flat)| (ours(*p), ours(*flat))).collect();
        let up = (q[side].1 - q[0].1).cross(q[1].1 - q[0].1).normalize_or_zero();
        if up.y.abs() < 0.7 {
            continue;
        }
        let kind = if material_of(f.texinfo, &mut materials).is_some_and(|m| materials[m].portalable) { "white" } else { "black" };
        for i in 0..side - 1 {
            for j in 0..side - 1 {
                let cell = [q[i * side + j], q[(i + 1) * side + j], q[(i + 1) * side + j + 1], q[i * side + j + 1]];
                let lo = cell.iter().fold(Vec3::splat(f32::MAX), |m, (p, flat)| m.min(*p).min(*flat));
                let hi = cell.iter().fold(Vec3::splat(f32::MIN), |m, (p, _)| m.max(*p));
                if hi.y - lo.y < 0.01 {
                    continue;
                }
                solids.push_str(&format!("{kind} 0 {:.4} {:.4} {:.4} {:.4} {:.4} {:.4}\n", lo.x, lo.y, lo.z, hi.x, hi.y, hi.z));
                nboxes += 1;
            }
        }
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
                let textures = bsp.brush_textures(b);
                let mats: Vec<usize> = textures.iter().filter_map(|&t| material_of(t, &mut materials)).collect();
                // All nodraw: invisible, but portals stick to it as to anything not marked
                // otherwise (Portal's Chamber 14 floors its sunken stairs with one).
                let nodraw = !textures.is_empty() && textures.iter().all(|&t| bsp.texture(t).is_some_and(|(d, _)| d.name.eq_ignore_ascii_case("tools/toolsnodraw")));
                if nodraw {
                    "white"
                } else if mats.iter().any(|&m| materials[m].name.starts_with("glass/")) {
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
            // A tilted side that takes portals: its outline too, for a portal to go on its true
            // plane (its boxes only step along it). A brush entity's, any way it faces: it may
            // swing round (a panel on a rotating door).
            if kind == "white" || kind == "black" {
                let corners = bsp.brush_corners(b);
                let sides: Vec<&bsp::BrushSide> = (0..b.num_sides).map(|k| &bsp.sides[b.first_side + k]).filter(|s| !s.bevel).collect();
                for (k, side) in sides.iter().enumerate() {
                    let plane = &bsp.planes[side.plane];
                    if (mi == 0 && (plane.axial || plane.normal.abs().max_element() > 0.999)) || !material_of(side.texinfo, &mut materials).is_some_and(|m| materials[m].portalable) {
                        continue;
                    }
                    let mut pts: Vec<Vec3> = corners.iter().filter(|(_, on)| on.contains(&k)).map(|(p, _)| ours(place(mi, *p + origin))).collect();
                    pts.dedup_by(|a, b| a.distance(*b) < 1e-3);
                    if pts.len() < 3 {
                        continue;
                    }
                    let n = ours(turn(mi, plane.normal)).normalize_or_zero();
                    let c = pts.iter().copied().sum::<Vec3>() / pts.len() as f32;
                    let e1 = (pts[0] - c).normalize_or_zero();
                    let e2 = n.cross(e1);
                    pts.sort_by(|a, b| (*a - c).dot(e2).atan2((*a - c).dot(e1)).total_cmp(&(*b - c).dot(e2).atan2((*b - c).dot(e1))));
                    solids.push_str(&format!("slope {mi} {:.5} {:.5} {:.5}", n.x, n.y, n.z));
                    for p in &pts {
                        solids.push_str(&format!(" {:.4} {:.4} {:.4}", p.x, p.y, p.z));
                    }
                    solids.push('\n');
                }
            }
            for (lo, hi) in bsp::brush_boxes(&bsp, b) {
                let (a, z) = place_box(mi, lo + origin, hi + origin);
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
    let lump = if placed.is_empty() { bsp.entities.clone() } else { crate::templates::write(&ents) };
    std::fs::write(map_dir.join(format!("{name}.ents")), lump).map_err(|e| e.to_string())?;
    let used = props(&bsp, &ents, &map_dir.join(format!("{name}.props")), read)?;
    overlays(&bsp, &ents, &map_dir.join(format!("{name}.overlays")), read, remix, &tex_dir)?;
    Ok((groups.len(), nboxes, materials.len(), used))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A wall whose first three corners are on one line (an extra corner where another face
    /// meets its edge) is still wound to face the way it faces, every triangle of it.
    #[test]
    fn faces_are_wound_by_the_whole_polygon() {
        // A 2 x 1 rectangle in the XY plane, corners going round counter-clockwise seen from
        // +Z, starting along its bottom edge with a corner in the middle of it.
        let q = [Vec3::new(0.0, 0.0, 0.0), Vec3::new(1.0, 0.0, 0.0), Vec3::new(2.0, 0.0, 0.0), Vec3::new(2.0, 1.0, 0.0), Vec3::new(0.0, 1.0, 0.0)];
        for n in [Vec3::Z, Vec3::NEG_Z] {
            let idx = fan(&q, n);
            assert!(!idx.is_empty());
            let mut area = 0.0;
            for t in idx.chunks(3) {
                let (a, b, c) = (q[t[0] as usize], q[t[1] as usize], q[t[2] as usize]);
                let w = (b - a).cross(c - a);
                assert!(w.dot(n) > 0.0, "a triangle faces away from {n}: {t:?}");
                area += w.length() * 0.5;
            }
            assert!((area - 2.0).abs() < 1e-5, "covers {area} of 2");
        }
    }
}
