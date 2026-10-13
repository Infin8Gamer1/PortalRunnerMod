//! Mirror's Edge's levels as the game's own (`--port-me`, issue #100): each map, with the
//! sublevels it streams in, read from the player's install (`me_assets::map`) and written as a
//! level file (`levels/me/`). Its meshes are Mirror's Edge props, drawn from the install, and
//! what each collides with is kept in the file; its blocking volumes are hulls; its start
//! is the level's.

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::rc::Rc;

use aperture::doc::{Kind, LevelDoc, Object};
use aperture::me::{plane, ue, ue_dir, MeShape};
use aperture::Mat;
use glam::{Mat3, Quat, Vec3, Vec4};
use me_assets::map;
use me_assets::{Package, Packages};

use aperture::me::MAPS;

/// An Unreal rotator as our turn.
pub fn turn(r: [i32; 3]) -> Quat {
    let a = map::axes(r);
    // Where it takes our X (Unreal's Y), Y (its Z) and Z (its -X).
    Quat::from_mat3(&Mat3::from_cols(ue_dir(a[1]), ue_dir(a[2]), -ue_dir(a[0]))).normalize()
}

/// Unreal's scale along its axes as ours.
pub fn scale(s: [f32; 3]) -> Vec3 {
    Vec3::new(s[1], s[2], s[0])
}

fn round(x: f32, k: f32) -> f32 {
    (x * k).round() / k
}

fn tidy(v: Vec3) -> Vec3 {
    Vec3::new(round(v.x, 1e4), round(v.y, 1e4), round(v.z, 1e4))
}

/// A placed mesh's materials in place of its own, a section each (their paths in the package;
/// empty: its own), none if it keeps them all.
fn overrides(pkg: &Package, materials: &[i32]) -> Vec<String> {
    if materials.iter().all(|&m| m == 0) {
        return Vec::new();
    }
    materials.iter().map(|&m| if m == 0 { String::new() } else { pkg.path(m) }).collect()
}

/// A mesh's box (ours, from its origin).
pub fn mesh_box(pkg: &Package, mesh: usize) -> Option<(Vec3, Vec3)> {
    let props = me_assets::props::Props::read(pkg, &pkg.exports[mesh]).ok()?;
    let mut c = me_assets::package::Cursor::new(&pkg.data, props.end);
    let (o, e) = (c.vec3().ok()?, c.vec3().ok()?);
    let (o, e) = (ue(o), Vec3::new(e[1], e[2], e[0]) * 0.01);
    Some((tidy(o - e), tidy(o + e)))
}

/// A volume's faces' planes (ours, from where it stands, before it's turned).
pub fn volume_planes(v: &map::Volume) -> Vec<Vec4> {
    let polys: Vec<Vec<Vec3>> = v
        .polys
        .iter()
        .map(|p| p.iter().map(|q| ue([(q[0] - v.pre_pivot[0]) * v.scale[0], (q[1] - v.pre_pivot[1]) * v.scale[1], (q[2] - v.pre_pivot[2]) * v.scale[2]])).collect())
        .collect();
    let all: Vec<Vec3> = polys.iter().flatten().copied().collect();
    if all.is_empty() {
        return vec![];
    }
    let mid = all.iter().copied().sum::<Vec3>() / all.len() as f32;
    let mut out: Vec<Vec4> = vec![];
    for p in &polys {
        // Newell's normal, turned to face out (away from the middle).
        let mut n = Vec3::ZERO;
        for k in 0..p.len() {
            let (a, b) = (p[k], p[(k + 1) % p.len()]);
            n += Vec3::new((a.y - b.y) * (a.z + b.z), (a.z - b.z) * (a.x + b.x), (a.x - b.x) * (a.y + b.y));
        }
        if n.length_squared() < 1e-12 {
            continue;
        }
        let n = n.normalize();
        let n = if n.dot(p[0] - mid) < 0.0 { -n } else { n };
        let pl = plane(n, n.dot(p[0]));
        let pl = Vec4::new(round(pl.x, 1e5), round(pl.y, 1e5), round(pl.z, 1e5), round(pl.w, 1e4));
        if !out.iter().any(|q| q.truncate().dot(pl.truncate()) > 0.9999 && (q.w - pl.w).abs() < 1e-3) {
            out.push(pl);
        }
    }
    out
}

/// What a map streams in: its sublevels' packages (not its sound's or text's).
fn streamed(pkg: &Package) -> Vec<String> {
    let mut out = vec![];
    for e in &pkg.exports {
        if !pkg.class_name(e).starts_with("LevelStreaming") {
            continue;
        }
        let Ok(p) = me_assets::props::Props::read_object(pkg, e) else { continue };
        let Some(name) = p.name(pkg, "PackageName") else { continue };
        let low = name.to_ascii_lowercase();
        if low.ends_with("_aud") || low.contains("_loc_") || out.contains(&name) {
            continue;
        }
        out.push(name);
    }
    out
}

/// The packages a streamed sublevel's name is shipped as: itself; or, cut in parts, its
/// `_Pt1`, `_Pt2`...; or, named for its map's folder (Mall's `SP05_MallExterior`), the same
/// under its map's name (`Mall_MallExterior`).
fn files_of(pkgs: &Packages, name: &str, persistent: &str) -> Vec<String> {
    if pkgs.has(name) {
        return vec![name.to_string()];
    }
    let parts: Vec<String> = (1..10).map(|i| format!("{name}_Pt{i}")).take_while(|n| pkgs.has(n)).collect();
    if !parts.is_empty() {
        return parts;
    }
    let stem = persistent.strip_suffix("_p").unwrap_or(persistent);
    match name.split_once('_') {
        Some((_, rest)) if pkgs.has(&format!("{stem}_{rest}")) => vec![format!("{stem}_{rest}")],
        _ => vec![],
    }
}

/// Where Faith starts, of the starts a level has (its own start first).
// (Rank 0 is also a story level's default checkpoint.)
const STARTS: [&str; 4] = ["TdTimeTrialStart", "PlayerStart", "TdPlayerStart", "TdTutorialStart"];

/// A story checkpoint's box: what she walks into.
const STORY_CHECKPOINT: Vec3 = Vec3::new(6.0, 4.0, 6.0);

/// How bright a Mirror's Edge light of brightness 1 is, in lumens (its lightmaps' units aren't
/// ours: set by eye against the sun).
const LIGHT_LUMENS: f32 = 30_000.0;

/// Where she can be: the level's starts, checkpoints and paths (its AI's). A mesh that only
/// has triangles to collide with (a building's) only collides near these: the city's skyline
/// is all triangles, and nowhere she goes.
const PLACES: [&str; 9] = ["TdTimeTrialStart", "PlayerStart", "TdPlayerStart", "TdTutorialStart", "TdCheckpoint", "TdTutorialCheckpoint", "TdTimerCheckpoint", "PathNode", "TdReachSpec"];
const NEAR: f32 = 40.0;

/// How far a ladder volume's path starts above the floor, and ends above the top she climbs
/// out onto.
const LADDER_FOOT: f32 = 0.8;
const LADDER_TOP: f32 = 0.8;

/// A time trial checkpoint's box: what she runs through.
const CHECKPOINT: Vec3 = Vec3::new(4.0, 4.0, 4.0);

/// How far her middle is above the floor (Mirror's Edge's collision cylinder's half height).
const HALF_HEIGHT: f32 = 0.9;

pub fn port_level(pkgs: &mut Packages, persistent: &str, title: &str, report: &mut String) -> Option<LevelDoc> {
    let top = pkgs.get(persistent).ok()?;
    let mut names = vec![persistent.to_string()];
    names.extend(streamed(&top).into_iter().filter(|n| !n.eq_ignore_ascii_case(persistent)));
    let mut doc = LevelDoc::new(title);
    doc.indoor = false;
    let mut starts: Vec<(usize, Vec3, f32)> = vec![];
    let mut low = f32::MAX;
    let mut counts: BTreeMap<String, usize> = BTreeMap::new();
    let mut read: Vec<(String, Rc<Package>, map::Map)> = vec![];
    for name in &names {
        let files = files_of(pkgs, name, persistent);
        if files.is_empty() {
            report.push_str(&format!("  {name}: not found\n"));
        }
        for file in files {
            if read.iter().any(|(n, _, _)| n.eq_ignore_ascii_case(&file)) {
                continue;
            }
            let Ok(pkg) = pkgs.get(&file) else { continue };
            let m = map::read(&pkg);
            read.push((file, pkg, m));
        }
    }
    // (`PORT_ME_NEAR=x,y,z`: what's round a point, ours, in every package: why she falls
    // somewhere, say.)
    if let Some(at) = std::env::var("PORT_ME_NEAR").ok().and_then(|s| { let v: Vec<f32> = s.split(',').filter_map(|x| x.parse().ok()).collect(); (v.len() == 3).then(|| Vec3::new(v[0], v[1], v[2])) }) {
        for (name, _pkg, m) in &read {
            for p in &m.meshes {
                let d = ue(p.location) - at;
                if Vec3::new(d.x, 0.0, d.z).length() < 12.0 && d.y.abs() < 15.0 {
                    eprintln!("NEAR {name} mesh {} {} {} at {} hidden {} blocks {}", p.class, p.name, p.mesh_name, ue(p.location), p.hidden, p.blocks);
                }
            }
            for v in &m.volumes {
                let d = ue(v.location) - at;
                if Vec3::new(d.x, 0.0, d.z).length() < 30.0 && d.y.abs() < 30.0 {
                    eprintln!("NEAR {name} vol {} {} at {} polys {}", v.class, v.name, ue(v.location), v.polys.len());
                }
            }
            for o in &m.others {
                let d = ue(o.location) - at;
                if Vec3::new(d.x, 0.0, d.z).length() < 12.0 && d.y.abs() < 15.0 {
                    eprintln!("NEAR {name} other {} {} at {}", o.class, o.name, ue(o.location));
                }
            }
        }
    }
    let places: Vec<Vec3> = read.iter().flat_map(|(_, _, m)| m.others.iter().filter(|o| PLACES.contains(&o.class.as_str())).map(|o| ue(o.location))).collect();
    let near = |lo: Vec3, hi: Vec3| places.iter().any(|p| p.clamp(lo, hi).distance(*p) < NEAR);
    let mut far = 0;
    let mut sun: Option<(f32, Vec3, Vec3)> = None;
    let mut course: Vec<(bool, String, Vec3, Option<String>)> = vec![];
    let mut story: Vec<(String, String, Vec3, f32)> = vec![];
    let mut lights = 0;
    for (name, pkg, m) in &read {
        let pkg: &Package = pkg;
        let mut seen: HashMap<(String, usize), (bool, bool, Option<(Vec3, Vec3)>)> = HashMap::new();
        let (mut props, mut solid) = (0, 0);
        for p in &m.meshes {
            // Its mesh: in this package, or (an import) its own.
            let (home_name, home, mesh) = match &p.import {
                None => (name.clone(), read.iter().find(|r| r.0 == *name).map(|r| r.1.clone()), p.mesh),
                Some(other) => {
                    let Ok(h) = pkgs.get(other) else { continue };
                    let Some(i) = h.exports.iter().position(|e| e.name == p.mesh_name && h.class_name(e) == "StaticMesh") else { continue };
                    (other.clone(), Some(h), i)
                }
            };
            let Some(home) = home else { continue };
            let home: &Package = &home;
            // (The sky is a dome round the city, the sun's flare a card drawn over the sun:
            // ray traced, the dome would shut the sun out. The game's own sky stands in.)
            let path = home.path(mesh as i32 + 1);
            // (Nor are its effects' meshes, light shafts and the like: glows the game blends
            // over the picture, nothing to stand on.)
            if p.hidden || path.starts_with("L_Sun") || path.starts_with("L_Sky") || path.starts_with("FX_") {
                continue;
            }
            // (What it collides with isn't kept: the game reads it with the mesh. Only whether
            // it has anything, and whether that's only triangles.)
            let (shaped, tris_only, bounds) = *seen.entry((home_name.clone(), mesh)).or_insert_with(|| {
                let shape = MeShape::from_game(&map::mesh_collision(home, mesh));
                (!shape.is_empty(), shape.hulls.is_empty(), mesh_box(home, mesh))
            });
            let at = tidy(ue(p.location));
            // (Where it stands, roughly: its box's size round its point.)
            let reach = bounds.map_or(1.0, |(lo, hi)| (hi - lo).length() * p.scale.iter().fold(0.0f32, |m, s| m.max(s.abs())));
            let shaped = shaped && !(tris_only && !places.is_empty() && !near(at - reach, at + reach) && {
                far += 1;
                true
            });
            let mut o = Object::new(
                Kind::MeProp { package: home_name.clone(), mesh: p.mesh_name.clone(), scale: tidy(scale(p.scale)), bounds, solid: p.blocks && shaped, own_shape: true, materials: if p.import.is_none() { overrides(pkg, &p.materials) } else { Vec::new() } },
                at,
            );
            o.set_rotation(turn(p.rotation));
            o.name = p.name.clone();
            if p.blocks && shaped {
                solid += 1;
                if let Some((lo, hi)) = bounds {
                    low = low.min(at.y + lo.y.min(hi.y) * p.scale[2].abs().max(0.01));
                }
            }
            props += 1;
            doc.objects.push(o);
        }
        // Its BSP: the rooms, floors and walls built of brushes (one mesh, where it is).
        let mut bsp_faces = 0;
        if let Ok(b) = me_assets::bsp::Bsp::read(pkg) {
            let mesh = b.mesh(pkg);
            if !mesh.positions.is_empty() || !b.collision_tris().is_empty() {
                bsp_faces = b.faces.len();
                let bounds = (!mesh.positions.is_empty()).then(|| {
                    let (o, e) = (ue(mesh.origin), Vec3::new(mesh.extent[1], mesh.extent[2], mesh.extent[0]) * 0.01);
                    (tidy(o - e), tidy(o + e))
                });
                let mut o = Object::new(
                    Kind::MeProp { package: name.clone(), mesh: me_assets::bsp::BSP.into(), scale: Vec3::ONE, bounds, solid: true, own_shape: true, materials: vec![] },
                    Vec3::ZERO,
                );
                o.name = format!("{name} BSP");
                doc.objects.push(o);
            }
        }
        for v in &m.volumes {
            *counts.entry(v.class.clone()).or_default() += 1;
            let planes = volume_planes(v);
            if planes.len() < 4 {
                continue;
            }
            let at = tidy(ue(v.location));
            let q = turn(v.rotation);
            match v.class.as_str() {
                "BlockingVolume" => {
                    let mut o = Object::new(Kind::Hull { planes, mat: Mat::Clip }, at);
                    o.set_rotation(q);
                    o.name = v.name.clone();
                    doc.objects.push(o);
                }
                "TdBarbedWireVolume" => {
                    let Some(b) = aperture::me::hull_brush(&planes).turned(q).bounds() else { continue };
                    let mut o = Object::sized(Kind::BarbedWire, tidy(at + b.center()), tidy(b.size()));
                    o.name = v.name.clone();
                    doc.objects.push(o);
                }
                // Mirror's Edge's moves: each volume's path (its start and end, the wall it's on).
                "TdLadderVolume" | "TdZiplineVolume" | "TdSwingVolume" | "TdBalanceWalkVolume" => {
                    let a = map::Chain::new(pkg, v.export);
                    let (Some(start), Some(end)) = (a.vec3("Start"), a.vec3("End")) else { continue };
                    let (mut start, mut end) = (ue(start), ue(end));
                    // (Its path is in the volume, or near it: one that isn't (a volume whose
                    // path was never set, or was left behind when it was moved) runs up its
                    // middle, bottom to top.)
                    if v.class == "TdLadderVolume" {
                        if let Some(b) = aperture::me::hull_brush(&planes).turned(q).bounds() {
                            // (Its top can be out over what she climbs onto.)
                            let (lo, hi) = (at + b.min - Vec3::new(2.0, 3.0, 2.0), at + b.max + Vec3::new(2.0, 8.0, 2.0));
                            let inside = |p: Vec3| p.cmpge(lo).all() && p.cmple(hi).all();
                            if !inside(start) || !inside(end) || end.y - start.y < 0.5 {
                                let c = at + b.center();
                                report.push_str(&format!("  {}: its path {start} -> {end} isn't in it ({lo} - {hi}): up its middle\n", v.name));
                                start = Vec3::new(c.x, at.y + b.min.y + LADDER_FOOT, c.z);
                                end = Vec3::new(c.x, at.y + b.max.y - LADDER_TOP, c.z);
                            }
                        }
                    }
                    let wall = a.vec3("WallNormal").map_or(Vec3::Z, |n| ue_dir(n).normalize_or(Vec3::Z));
                    let mut o = match v.class.as_str() {
                        // (Where its foot and top are is worked out once the level's built:
                        // `place_ladders`.)
                        "TdLadderVolume" => {
                            let pipe = a.name("LadderType").as_deref() == Some("LT_Pipe");
                            let exit = a.bool("bCanExitAtTop").unwrap_or(true);
                            let mut o = Object::new(Kind::Ladder { height: round(end.y - start.y, 1e3), pipe, exit }, tidy(start));
                            o.face(wall);
                            o
                        }
                        // Its cable sags: its spline's middle point hangs below the straight line
                        // (at its way along it, as a parabola's would).
                        "TdZiplineVolume" => {
                            let (hi, lo) = if start.y >= end.y { (start, end) } else { (end, start) };
                            let flat = Vec3::new(lo.x - hi.x, 0.0, lo.z - hi.z);
                            let sag = a.vec3("Middle").map(ue).map_or(0.0, |m| {
                                let t = (Vec3::new(m.x - hi.x, 0.0, m.z - hi.z).dot(flat) / flat.length_squared().max(1e-6)).clamp(0.0, 1.0);
                                let below = hi.y + (lo.y - hi.y) * t - m.y;
                                if (0.1..0.9).contains(&t) { (below / (4.0 * t * (1.0 - t))).max(0.0) } else { 0.0 }
                            });
                            Object::new(Kind::ZipLine { to: tidy(lo - hi), sag: round(sag, 1e3) }, tidy(hi))
                        }
                        // A swing pole: across the volume (its own Y), at its height.
                        "TdSwingVolume" => {
                            let along = q * Vec3::X;
                            let half = planes.iter().filter(|p| p.truncate().dot(Vec3::X) > 0.9).map(|p| p.w).fold(0.5, f32::max).min(1.5);
                            let mid = ue(v.location);
                            Object::new(Kind::SwingPole { to: tidy(along * half * 2.0) }, tidy(mid - along * half))
                        }
                        _ => Object::new(Kind::Beam { to: tidy(end - start), solid: false }, tidy(start)),
                    };
                    o.name = v.name.clone();
                    doc.objects.push(o);
                }
                _ => {}
            }
        }
        for o in &m.others {
            *counts.entry(o.class.clone()).or_default() += 1;
            // The sun: its directional light baked into the lightmaps (the one the level's
            // shadows were made with; a brighter one only lights what moves, from wherever
            // suits it), else its brightest.
            if o.class == "DirectionalLight" {
                let a = map::Chain::new(pkg, o.export);
                let Some(c) = a.object("LightComponent").filter(|&c| c > 0) else { continue };
                let c = map::Chain::new(pkg, c as usize - 1);
                if c.bool("bEnabled") == Some(false) || a.bool("bHidden") == Some(true) {
                    continue;
                }
                let brightness = c.f32("Brightness").unwrap_or(1.0);
                let baked = c.bool("bHasLightEverBeenBuiltIntoLightMap") == Some(true);
                // (A Color: B, G, R, A.)
                let color = c.bytes("LightColor").filter(|b| b.len() == 4).map_or(Vec3::ONE, |b| Vec3::new(b[2] as f32, b[1] as f32, b[0] as f32) / 255.0);
                let dir = ue_dir(map::rotate(o.rotation, [1.0, 0.0, 0.0])).normalize();
                // (Ranked baked first, then by brightness.)
                let rank = brightness + if baked { 1000.0 } else { 0.0 };
                if sun.as_ref().is_none_or(|s: &(f32, Vec3, Vec3)| rank > s.0) {
                    sun = Some((rank, dir, color));
                }
            }
            // A time trial's course: its start, then each checkpoint the one before leads to.
            if o.class == "TdTimeTrialStart" || o.class == "TdTimerCheckpoint" {
                let next = map::Chain::new(pkg, o.export).object("nextNavigationPoint").filter(|&n| n > 0).map(|n| pkg.exports[n as usize - 1].name.clone());
                course.push((o.class == "TdTimeTrialStart", o.name.clone(), ue(o.location), next));
            }
            if let Some(rank) = STARTS.iter().position(|s| *s == o.class) {
                starts.push((rank, ue(o.location), -(o.rotation[1] as f32) * 360.0 / 65536.0));
            }
            // A story level's checkpoints: where she comes back to (its default one is where
            // the level starts, before any player start).
            if o.class == "TdCheckpoint" {
                let a = map::Chain::new(pkg, o.export);
                let yaw = -(o.rotation[1] as f32) * 360.0 / 65536.0;
                // (Not the start's own: she'd start in it.)
                if a.bool("DefaultCheckpoint") == Some(true) {
                    starts.push((0, ue(o.location), yaw));
                    continue;
                }
                // (Its name as the game shows it, if it has one: not its actor's.)
                story.push((a.name("CheckpointName").filter(|n| n != "None").unwrap_or_default(), o.name.clone(), ue(o.location), yaw));
            }
            // Its lights: a point light (an area light as one, a bit bigger), a spot light.
            if o.class.contains("PointLight") || o.class.contains("SpotLight") || o.class == "TdAreaLight" {
                let a = map::Chain::new(pkg, o.export);
                let Some(c) = a.object("LightComponent").filter(|&c| c > 0) else { continue };
                let c = map::Chain::new(pkg, c as usize - 1);
                if c.bool("bEnabled") == Some(false) {
                    continue;
                }
                let brightness = c.f32("Brightness").unwrap_or(1.0);
                if brightness <= 0.0 {
                    continue;
                }
                let color = c.bytes("LightColor").filter(|b| b.len() == 4).map_or(Vec3::ONE, |b| Vec3::new(b[2] as f32, b[1] as f32, b[0] as f32) / 255.0);
                let color = Vec3::new(round(color.x, 1e3), round(color.y, 1e3), round(color.z, 1e3));
                let range = round(c.f32("Radius").unwrap_or(1024.0) * 0.01, 1e2);
                let intensity = round(brightness * LIGHT_LUMENS, 1.0);
                let spot = o.class.contains("SpotLight");
                let kind = if spot {
                    let outer = c.f32("OuterConeAngle").unwrap_or(44.0);
                    let inner = c.f32("InnerConeAngle").unwrap_or(0.0);
                    Kind::SpotLight { color, intensity, range, radius: 0.1, angle: round(outer.clamp(1.0, 89.0), 1e2), soft: round((1.0 - inner / outer.max(1.0)).clamp(0.0, 1.0), 1e2), shadows: false }
                } else {
                    Kind::PointLight { color, intensity, range, radius: if o.class == "TdAreaLight" { 0.4 } else { 0.1 }, shadows: false }
                };
                let mut l = Object::new(kind, tidy(ue(o.location)));
                if spot {
                    l.face(ue_dir(map::rotate(o.rotation, [1.0, 0.0, 0.0])).normalize());
                }
                l.name = o.name.clone();
                doc.objects.push(l);
                lights += 1;
            }
        }
        report.push_str(&format!("  {name}: {props} meshes ({solid} solid), {} volumes, {bsp_faces} BSP faces\n", m.volumes.len()));
    }
    report.push_str(&format!("  {far} meshes with only triangles to collide with are out of her way: not solid\n"));
    // (As bright as our outdoor sun: Mirror's Edge's brightness is its lightmaps'.)
    if let Some((_, dir, color)) = sun {
        let c = Vec3::new(round(color.x, 1e3), round(color.y, 1e3), round(color.z, 1e3));
        let mut o = Object::new(Kind::Sun { color: c, illuminance: 11_000.0 }, Vec3::new(0.0, 100.0, 0.0));
        o.face(dir);
        doc.objects.push(o);
    }
    // The course's checkpoints in order (a box round each, her feet a little under its
    // point), the last its finish line.
    if let Some(start) = course.iter().find(|c| c.0) {
        let mut order = vec![];
        let mut next = start.3.clone();
        while let Some(n) = next.take() {
            let Some(c) = course.iter().find(|c| c.1 == n && !c.0) else { break };
            if order.iter().any(|o: &&(bool, String, Vec3, Option<String>)| o.1 == c.1) {
                break;
            }
            order.push(c);
            next = c.3.clone();
        }
        for (k, c) in order.iter().enumerate() {
            let feet = tidy(c.2 - Vec3::Y * HALF_HEIGHT);
            let kind = if k + 1 == order.len() { Kind::Exit } else { Kind::Checkpoint { name: format!("{}", k + 1), spawn: Vec3::new(0.0, -CHECKPOINT.y * 0.5, 0.0) } };
            let mut o = Object::sized(kind, tidy(feet + Vec3::Y * CHECKPOINT.y * 0.5), CHECKPOINT);
            o.name = c.1.clone();
            // (Facing the next, to start again from.)
            if let Some(n) = order.get(k + 1) {
                let d = n.2 - c.2;
                o.yaw = round((-d.x).atan2(-d.z).to_degrees(), 1e3);
            }
            doc.objects.push(o);
        }
        // (Passed in order, as the game's are: #104.)
        doc.course = true;
        report.push_str(&format!("  a time trial: {} checkpoints, the last its finish\n", order.len()));
    }
    // A story level's checkpoints (not a time trial's: it has its course), in any order.
    if !doc.course && !story.is_empty() {
        // (Not one round where she starts: she'd reach it as she starts.)
        let start = starts.iter().min_by_key(|s| s.0).map(|s| s.1);
        for (name, actor, at, yaw) in &story {
            if start.is_some_and(|s| (s - *at).abs().cmplt(STORY_CHECKPOINT).all()) {
                continue;
            }
            let feet = tidy(*at - Vec3::Y * HALF_HEIGHT);
            let mut o = Object::sized(Kind::Checkpoint { name: name.clone(), spawn: Vec3::new(0.0, -STORY_CHECKPOINT.y * 0.5, 0.0) }, tidy(feet + Vec3::Y * STORY_CHECKPOINT.y * 0.5), STORY_CHECKPOINT);
            o.yaw = round(yaw.rem_euclid(360.0), 1e3);
            o.name = actor.clone();
            doc.objects.push(o);
        }
        doc.free_checkpoints = true;
        report.push_str(&format!("  {} checkpoints\n", story.len()));
    }
    report.push_str(&format!("  {lights} lights\n"));
    starts.sort_by_key(|s| s.0);
    if let Some(&(_, at, yaw)) = starts.first() {
        let mut o = Object::new(Kind::Spawn { gravity: None }, tidy(at - Vec3::Y * HALF_HEIGHT));
        o.yaw = round(yaw.rem_euclid(360.0), 1e3);
        doc.objects.push(o);
    } else {
        report.push_str("  no start found\n");
    }
    if low < f32::MAX {
        doc.kill_y = round(low - 10.0, 10.0);
    }
    // Its movers and the Kismet that drives them (#109).
    super::me_kismet::port(&read, &mut doc, report);
    free_lights(&mut doc, report);
    place_ladders(&mut doc, report);
    let others: Vec<String> = counts.iter().map(|(c, n)| format!("{c} {n}")).collect();
    report.push_str(&format!("  actors: {}\n", others.join(", ")));
    Some(doc)
}

/// Where a ray from `o` along `d` first meets a box, within `len`.
fn ray(boxes: &[aperture::level::Solid], o: Vec3, d: Vec3, len: f32) -> Option<f32> {
    let mut best: Option<f32> = None;
    for s in boxes {
        let (mut t0, mut t1) = (0.0f32, len);
        let mut hit = true;
        for k in 0..3 {
            if d[k].abs() < 1e-9 {
                if o[k] < s.b.min[k] || o[k] > s.b.max[k] {
                    hit = false;
                    break;
                }
                continue;
            }
            let (a, b) = ((s.b.min[k] - o[k]) / d[k], (s.b.max[k] - o[k]) / d[k]);
            t0 = t0.max(a.min(b));
            t1 = t1.min(a.max(b));
        }
        if hit && t0 <= t1 && best.is_none_or(|b| t0 < b) {
            best = Some(t0);
        }
    }
    best
}

/// Mirror's Edge's ladder volumes hold the path her middle takes, not how far it is from the
/// wall or the floor: ours have their foot on the wall where it meets the floor, and the top
/// she climbs out onto. Found in the level as it's built: the wall behind the path (a pipe, or
/// the ladder's own rungs, if they stand out from it), the floor under its start (unless it
/// starts high up the wall), the top past its end.
/// The most a light's moved out of the lamp it's in.
const LIGHT_FREE: f32 = 1.0;

/// Mirror's Edge puts many of its lights inside their lamps (a fluorescent tube's housing, a
/// caged bulb): its lightmaps were baked without the lamps in the way. Traced, the lamp would
/// shut its own light in: a light inside a small prop's box is moved out of it, the nearest way
/// (a ceiling lamp's, out of its bottom), a little past it.
fn free_lights(doc: &mut LevelDoc, report: &mut String) {
    // Small props' boxes: where, turned, scaled, and their box.
    let boxes: Vec<(Vec3, Quat, Vec3, Vec3, Vec3)> = doc
        .objects
        .iter()
        .filter_map(|o| match &o.kind {
            Kind::MeProp { mesh, scale, bounds: Some((lo, hi)), .. } if mesh != me_assets::bsp::BSP => {
                let size = (*hi - *lo) * scale.abs();
                (size.max_element() < 4.0 && size.min_element() > 0.01).then(|| (o.at, o.rotation(), *scale, *lo, *hi))
            }
            _ => None,
        })
        .collect();
    let mut moved = 0;
    for o in doc.objects.iter_mut() {
        if !matches!(o.kind, Kind::PointLight { .. } | Kind::SpotLight { .. }) {
            continue;
        }
        let mut p = o.at;
        for &(at, turn, scale, lo, hi) in &boxes {
            if (p - at).length_squared() > 4.0 * 4.0 * 3.0 {
                continue;
            }
            // (In its own space, scaled: its box's size as it's drawn.)
            let s = scale.abs();
            let local = (turn.inverse() * (p - at)) * scale.signum();
            let (lo, hi) = (lo * s, hi * s);
            if !(local.cmpgt(lo).all() && local.cmplt(hi).all()) {
                continue;
            }
            let (to_lo, to_hi) = (local - lo, hi - local);
            let (mut k, mut d, mut out) = (0, f32::MAX, Vec3::ZERO);
            for i in 0..3 {
                if to_lo[i] < d {
                    (k, d, out) = (i, to_lo[i], -Vec3::AXES[i]);
                }
                if to_hi[i] < d {
                    (k, d, out) = (i, to_hi[i], Vec3::AXES[i]);
                }
            }
            let _ = k;
            if d + 0.05 > LIGHT_FREE {
                continue;
            }
            p += turn * (out * scale.signum()) * (d + 0.05);
        }
        if p != o.at {
            o.at = tidy(p);
            moved += 1;
        }
    }
    report.push_str(&format!("  {moved} lights moved out of their lamps\n"));
}

fn place_ladders(doc: &mut LevelDoc, report: &mut String) {
    let l = doc.build();
    let boxes = &l.solids[..l.map_solids];
    let free = doc.free_checkpoints;
    for o in doc.objects.iter_mut() {
        // Her start, and where a story level's checkpoints start her again: on the floor under
        // where the game stands her (its point is her middle, not always as high over it).
        let feet = match &o.kind {
            Kind::Spawn { .. } => Some(o.at),
            Kind::Checkpoint { spawn, .. } if free => Some(o.at + *spawn),
            _ => None,
        };
        if let Some(feet) = feet {
            let middle = feet + Vec3::Y * HALF_HEIGHT;
            if let Some(t) = ray(boxes, middle, Vec3::NEG_Y, HALF_HEIGHT + 2.0) {
                o.at.y = round(o.at.y + (middle.y - t) - feet.y, 1e4);
            }
            continue;
        }
        // A beam's path runs through it: ours runs along its top.
        if let Kind::Beam { to, .. } = &o.kind {
            let mid = o.at + *to * 0.5;
            if let Some(t) = ray(boxes, mid + Vec3::Y * 0.5, Vec3::NEG_Y, 1.0) {
                o.at.y = round(o.at.y + 0.5 - t, 1e4);
            }
            continue;
        }
        let (start, n) = (o.at, o.dir());
        let Kind::Ladder { height, .. } = &mut o.kind else { continue };
        let end = start + Vec3::Y * *height;
        let probe = start + Vec3::Y * 1.0;
        let back = ray(boxes, probe + n * 1.5, -n, 3.0).map_or(0.0, |t| 1.5 - t);
        let wall = start + n * back;
        let floor = ray(boxes, wall + n * 0.3 + Vec3::Y * 0.5, Vec3::NEG_Y, 1.6).map(|t| wall.y + 0.5 - t);
        let foot_y = floor.unwrap_or(start.y - LADDER_FOOT);
        let top = ray(boxes, Vec3::new(wall.x, end.y + 1.0, wall.z) - n * 0.4, Vec3::NEG_Y, 2.5).map_or(end.y - LADDER_TOP, |t| end.y + 1.0 - t);
        o.at = tidy(Vec3::new(wall.x, foot_y, wall.z));
        *height = round(top - foot_y, 1e3);
        report.push_str(&format!("  {}: foot {} ({}), {:.2} m up\n", o.name, o.at, if floor.is_some() { "on the floor" } else { "up the wall" }, *height));
    }
}

/// Mirror's Edge's install's CookedPC: `--me <dir>`, `FAITH_ME_DIR`, `me_path.txt`, or Steam's.
pub fn find_cooked(arg: Option<PathBuf>, steam: &[PathBuf]) -> Option<PathBuf> {
    let mut tries: Vec<PathBuf> = arg.into_iter().collect();
    if let Some(p) = std::env::var_os("FAITH_ME_DIR") {
        tries.push(PathBuf::from(p));
    }
    if let Ok(s) = std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../me_path.txt")) {
        tries.push(PathBuf::from(s.trim()));
    }
    tries.extend(steam.iter().map(|d| d.join("mirrors edge")));
    tries.into_iter().find_map(|p| me_assets::cooked_pc(&p))
}

/// Write every map's level file (`levels/me/`), keeping any that were edited since they were
/// written (as `--port-maps` does).
pub fn port_all(cooked: &Path, only: Option<&str>) {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../levels/me");
    if std::fs::create_dir_all(&dir).is_err() {
        return;
    }
    let ledger = dir.join("written.txt");
    let mut written: BTreeMap<String, u64> = std::fs::read_to_string(&ledger)
        .unwrap_or_default()
        .lines()
        .filter_map(|l| l.split_once(' ').and_then(|(n, f)| Some((n.to_string(), f.parse().ok()?))))
        .collect();
    let mut pkgs = Packages::new(cooked);
    // (The levels' props collide with their meshes' shapes, read from here as the game does.)
    aperture::me::set_install(cooked);
    let (mut made, mut kept) = (0, Vec::new());
    for (map, file, title) in MAPS {
        if only.is_some_and(|o| !o.eq_ignore_ascii_case(file) && !o.eq_ignore_ascii_case(map)) {
            continue;
        }
        let path = dir.join(format!("{file}.ron"));
        if let Ok(old) = std::fs::read(&path) {
            if written.get(*file) != Some(&crate::fingerprint(&old)) {
                kept.push(*file);
                continue;
            }
        }
        let mut report = String::new();
        let Some(doc) = port_level(&mut pkgs, map, title, &mut report) else {
            println!("  {map}: not found");
            continue;
        };
        print!("{map} -> levels/me/{file}.ron\n{report}");
        let text = doc.to_ron();
        if std::fs::write(&path, &text).is_ok() {
            written.insert(file.to_string(), crate::fingerprint(text.as_bytes()));
            made += 1;
        }
    }
    let list: String = written.iter().map(|(n, f)| format!("{n} {f}\n")).collect();
    let _ = std::fs::write(&ledger, list);
    println!("{made} of Mirror's Edge's maps as the game's own levels (levels/me/)");
    if !kept.is_empty() {
        println!("kept as edited (delete them to have them made again): {}", kept.join(", "));
    }
}

/// `--audit-me <level file>`: a level's materials by how much of it they cover (m², its
/// props' sections' areas as placed), and what was found for each: its colour map (and how
/// it's tiled), or none (drawn plain), and its blend: to find what's drawn wrong.
pub fn audit(cooked: &Path, file: &str) {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../levels/me").join(format!("{file}.ron"));
    let Some(doc) = std::fs::read_to_string(&path).ok().and_then(|t| LevelDoc::from_ron(&t).ok()) else {
        eprintln!("can't read {}", path.display());
        return;
    };
    let mut pkgs = Packages::new(cooked);
    // (package, mesh, overrides) -> (instances, area scale sum)
    let mut uses: BTreeMap<(String, String, Vec<String>), (usize, f32)> = BTreeMap::new();
    for o in &doc.objects {
        if let Kind::MeProp { package, mesh, scale, materials, .. } = &o.kind {
            let s = scale.abs();
            let e = uses.entry((package.clone(), mesh.clone(), materials.clone())).or_default();
            e.0 += 1;
            e.1 += (s.x * s.y + s.y * s.z + s.x * s.z) / 3.0;
        }
    }
    // material path -> (area, instances, what was found)
    let mut mats: BTreeMap<String, (f32, usize, String)> = BTreeMap::new();
    for ((package, mesh, overrides), (n, scale2)) in &uses {
        let Ok((m, slots)) = me_assets::load_static_mesh_with(&mut pkgs, package, mesh, overrides, 64) else { continue };
        for (s, slot) in m.sections.iter().zip(&slots) {
            let mut area = 0.0f32;
            for t in m.indices[s.first_index as usize..(s.first_index + 3 * s.triangles) as usize].chunks(3) {
                let p = |k: u32| Vec3::from(m.positions[k as usize]) * 0.01;
                area += (p(t[1]) - p(t[0])).cross(p(t[2]) - p(t[0])).length() * 0.5;
            }
            let found = match &slot.diffuse {
                Some(d) => format!("colour map {}x{} x{:?}", d.width, d.height, slot.uv_scale),
                None => format!("no colour map: {:?}", slot.color),
            };
            let e = mats.entry(if slot.path.is_empty() { slot.material.clone() } else { slot.path.clone() }).or_insert((0.0, 0, String::new()));
            e.0 += area * scale2;
            e.1 += n;
            let glow = slot.glow.as_ref().map_or(String::new(), |(m, c)| format!(" glows {c:?}{}", if m.is_some() { " (mapped)" } else { "" }));
            e.2 = format!("{found} {:?}{glow}", slot.blend);
        }
    }
    let mut list: Vec<_> = mats.into_iter().collect();
    list.sort_by(|a, b| b.1 .0.total_cmp(&a.1 .0));
    for (m, (area, n, found)) in list.iter().take(60) {
        println!("{area:>10.0} m² {n:>5}  {m}: {found}");
    }
    println!("-- plain or tiled under 1:");
    for (m, (area, n, found)) in &list {
        if found.contains("no colour") || found.contains("x[0.") {
            println!("{area:>10.0} m² {n:>5}  {m}: {found}");
        }
    }
}
