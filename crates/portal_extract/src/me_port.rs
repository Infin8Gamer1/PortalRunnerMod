//! Mirror's Edge's levels as the game's own (`--port-me`, issue #100): each map, with the
//! sublevels it streams in, read from the player's install (`me_assets::map`) and written as a
//! level file (`levels/me/`). Its meshes are Mirror's Edge props, drawn from the install, and
//! what each collides with is kept in the file; its blocking volumes are hulls; its start
//! is the level's.

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::rc::Rc;

use aperture::doc::{Kind, LevelDoc, Object};
use aperture::me::MeShape;
use aperture::Mat;
use glam::{Mat3, Quat, Vec3, Vec4};
use me_assets::map::{self, Shape};
use me_assets::{Package, Packages};

use aperture::me::MAPS;

/// Unreal's points (centimetres; X forward, Y right, Z up) as ours (metres; X right, Y up,
/// Z back).
fn ue(p: [f32; 3]) -> Vec3 {
    Vec3::new(p[1], p[2], -p[0]) * 0.01
}

fn ue_dir(p: [f32; 3]) -> Vec3 {
    Vec3::new(p[1], p[2], -p[0])
}

/// An Unreal rotator as our turn.
fn turn(r: [i32; 3]) -> Quat {
    let a = map::axes(r);
    // Where it takes our X (Unreal's Y), Y (its Z) and Z (its -X).
    Quat::from_mat3(&Mat3::from_cols(ue_dir(a[1]), ue_dir(a[2]), -ue_dir(a[0]))).normalize()
}

/// Unreal's scale along its axes as ours.
fn scale(s: [f32; 3]) -> Vec3 {
    Vec3::new(s[1], s[2], s[0])
}

fn round(x: f32, k: f32) -> f32 {
    (x * k).round() / k
}

fn tidy(v: Vec3) -> Vec3 {
    Vec3::new(round(v.x, 1e4), round(v.y, 1e4), round(v.z, 1e4))
}

fn plane(n: Vec3, d: f32) -> Vec4 {
    let len = n.length();
    let n = n / len;
    Vec4::new(round(n.x, 1e5), round(n.y, 1e5), round(n.z, 1e5), round(d / len, 1e4))
}

/// A box's planes: its centre `c`, unit axes and half sizes.
fn box_planes(c: Vec3, axes: [Vec3; 3], half: [f32; 3]) -> Vec<Vec4> {
    let mut out = vec![];
    for k in 0..3 {
        let a = axes[k].normalize();
        out.push(plane(a, a.dot(c) + half[k]));
        out.push(plane(-a, -a.dot(c) + half[k]));
    }
    out
}

/// A mesh's collision, in its own space as ours.
fn shape_of(c: &map::Collision) -> MeShape {
    let mut s = MeShape::default();
    for sh in &c.shapes {
        match sh {
            Shape::Hull { planes, .. } => {
                if planes.len() >= 4 {
                    s.hulls.push(planes.iter().map(|p| plane(ue_dir([p[0], p[1], p[2]]), p[3] * 0.01)).collect());
                }
            }
            Shape::Box { tm, size } => {
                let axes = [ue_dir(tm[0]), ue_dir(tm[1]), ue_dir(tm[2])];
                s.hulls.push(box_planes(ue(tm[3]), axes, [size[0] * 0.005, size[1] * 0.005, size[2] * 0.005]));
            }
            // (A capsule or a ball as the box round it.)
            Shape::Capsule { tm, radius, length } => {
                let axes = [ue_dir(tm[0]), ue_dir(tm[1]), ue_dir(tm[2])];
                let r = radius * 0.01;
                s.hulls.push(box_planes(ue(tm[3]), axes, [r, r, length * 0.005 + r]));
            }
            Shape::Sphere { center, radius } => {
                let r = radius * 0.01;
                s.hulls.push(box_planes(ue(*center), [Vec3::X, Vec3::Y, Vec3::Z], [r, r, r]));
            }
        }
    }
    s.tris = c.tris.iter().map(|t| t.map(|p| tidy(ue(p)))).collect();
    s
}

/// A mesh's box (ours, from its origin).
fn mesh_box(pkg: &Package, mesh: usize) -> Option<(Vec3, Vec3)> {
    let props = me_assets::props::Props::read(pkg, &pkg.exports[mesh]).ok()?;
    let mut c = me_assets::package::Cursor::new(&pkg.data, props.end);
    let (o, e) = (c.vec3().ok()?, c.vec3().ok()?);
    let (o, e) = (ue(o), Vec3::new(e[1], e[2], e[0]) * 0.01);
    Some((tidy(o - e), tidy(o + e)))
}

/// A volume's faces' planes (ours, from where it stands, before it's turned).
fn volume_planes(v: &map::Volume) -> Vec<Vec4> {
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

/// Where Faith starts, of the starts a level has (its own start first).
const STARTS: [&str; 4] = ["TdTimeTrialStart", "PlayerStart", "TdPlayerStart", "TdTutorialStart"];

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
    let mut shapes: BTreeMap<String, MeShape> = BTreeMap::new();
    let mut starts: Vec<(usize, Vec3, f32)> = vec![];
    let mut low = f32::MAX;
    let mut counts: BTreeMap<String, usize> = BTreeMap::new();
    let mut read: Vec<(String, Rc<Package>, map::Map)> = vec![];
    for name in &names {
        match pkgs.get(name) {
            Ok(pkg) => {
                let m = map::read(&pkg);
                read.push((name.clone(), pkg, m));
            }
            Err(_) => report.push_str(&format!("  {name}: not found\n")),
        }
    }
    let places: Vec<Vec3> = read.iter().flat_map(|(_, _, m)| m.others.iter().filter(|o| PLACES.contains(&o.class.as_str())).map(|o| ue(o.location))).collect();
    let near = |lo: Vec3, hi: Vec3| places.iter().any(|p| p.clamp(lo, hi).distance(*p) < NEAR);
    let mut far = 0;
    let mut sun: Option<(f32, Vec3, Vec3)> = None;
    let mut course: Vec<(bool, String, Vec3, Option<String>)> = vec![];
    for (name, pkg, m) in &read {
        let pkg: &Package = pkg;
        let mut seen: HashMap<usize, (bool, bool, Option<(Vec3, Vec3)>)> = HashMap::new();
        let (mut props, mut solid) = (0, 0);
        for p in &m.meshes {
            // (The sky is a dome round the city, the sun's flare a card drawn over the sun:
            // ray traced, the dome would shut the sun out. The game's own sky stands in.)
            let path = pkg.path(p.mesh as i32 + 1);
            // (Nor are its effects' meshes, light shafts and the like: glows the game blends
            // over the picture, nothing to stand on.)
            if p.hidden || path.starts_with("L_Sun") || path.starts_with("L_Sky") || path.starts_with("FX_") {
                continue;
            }
            let key = format!("{name}.{}", p.mesh_name);
            let (shaped, tris_only, bounds) = *seen.entry(p.mesh).or_insert_with(|| {
                let path = pkg.path(p.mesh as i32 + 1);
                // (The sky is a mesh too: nothing to bump into.)
                let sky = path.starts_with("L_Sky") || path.starts_with("L_Sun");
                let shape = if sky { MeShape::default() } else { shape_of(&map::mesh_collision(&pkg, p.mesh)) };
                let shaped = !shape.hulls.is_empty() || !shape.tris.is_empty();
                let tris_only = shape.hulls.is_empty();
                if shaped {
                    shapes.insert(key.clone(), shape);
                }
                (shaped, tris_only, mesh_box(pkg, p.mesh))
            });
            let at = tidy(ue(p.location));
            // (Where it stands, roughly: its box's size round its point.)
            let reach = bounds.map_or(1.0, |(lo, hi)| (hi - lo).length() * p.scale.iter().fold(0.0f32, |m, s| m.max(s.abs())));
            let shaped = shaped && !(tris_only && !places.is_empty() && !near(at - reach, at + reach) && {
                far += 1;
                true
            });
            let mut o = Object::new(
                Kind::MeProp { package: name.clone(), mesh: p.mesh_name.clone(), scale: tidy(scale(p.scale)), bounds, solid: p.blocks && shaped },
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
                    let (start, end) = (ue(start), ue(end));
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
                        "TdZiplineVolume" => {
                            let (hi, lo) = if start.y >= end.y { (start, end) } else { (end, start) };
                            Object::new(Kind::ZipLine { to: tidy(lo - hi) }, tidy(hi))
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
            // The sun: its brightest directional light.
            if o.class == "DirectionalLight" {
                let a = map::Chain::new(pkg, o.export);
                let Some(c) = a.object("LightComponent").filter(|&c| c > 0) else { continue };
                let c = map::Chain::new(pkg, c as usize - 1);
                if c.bool("bEnabled") == Some(false) || a.bool("bHidden") == Some(true) {
                    continue;
                }
                let brightness = c.f32("Brightness").unwrap_or(1.0);
                // (A Color: B, G, R, A.)
                let color = c.bytes("LightColor").filter(|b| b.len() == 4).map_or(Vec3::ONE, |b| Vec3::new(b[2] as f32, b[1] as f32, b[0] as f32) / 255.0);
                let dir = ue_dir(map::rotate(o.rotation, [1.0, 0.0, 0.0])).normalize();
                if sun.as_ref().is_none_or(|s: &(f32, Vec3, Vec3)| brightness > s.0) {
                    sun = Some((brightness, dir, color));
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
        }
        report.push_str(&format!("  {name}: {props} meshes ({solid} solid), {} volumes\n", m.volumes.len()));
    }
    report.push_str(&format!("  {far} meshes with only triangles to collide with are out of her way: not solid\n"));
    // (Mirror's Edge's brightest sun is 5: our outdoor sun's 11,000 lux.)
    if let Some((brightness, dir, color)) = sun {
        let c = Vec3::new(round(color.x, 1e3), round(color.y, 1e3), round(color.z, 1e3));
        let mut o = Object::new(Kind::Sun { color: c, illuminance: round(brightness * 2200.0, 1.0) }, Vec3::new(0.0, 100.0, 0.0));
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
        report.push_str(&format!("  a time trial: {} checkpoints, the last its finish\n", order.len()));
    }
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
    // (Only the shapes of meshes that are solid somewhere.)
    let used: std::collections::HashSet<String> = doc
        .objects
        .iter()
        .filter_map(|o| match &o.kind {
            Kind::MeProp { package, mesh, solid: true, .. } => Some(format!("{package}.{mesh}")),
            _ => None,
        })
        .collect();
    shapes.retain(|k, _| used.contains(k));
    doc.me_shapes = shapes;
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
fn place_ladders(doc: &mut LevelDoc, report: &mut String) {
    let l = doc.build();
    let boxes = &l.solids[..l.map_solids];
    for o in doc.objects.iter_mut() {
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
