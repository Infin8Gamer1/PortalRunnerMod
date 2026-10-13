//! A Mirror's Edge map's movers and the Kismet that drives them, as level objects (#109).
//!
//! Each matinee's move of an actor is a `MeMover` (standing at the actor, carrying its prop);
//! each op that can lead to one is a `MeLogic`, its outputs leading to the others'. What's
//! between packages and sequences is joined up here: a remote event's call leads to the events
//! of its name (in any of the map's packages), a sub-sequence's input to the op inside it, a
//! sequence's finish to what its sequence's output leads to.

use std::collections::HashMap;
use std::rc::Rc;

use aperture::doc::{Kind, LevelDoc, Object};
use aperture::io::Output;
use aperture::level::MoverKey;
use aperture::me::ue;
use glam::{EulerRot, Quat, Vec3};
use me_assets::kismet::{self, Op};
use me_assets::map;
use me_assets::props::Props;
use me_assets::Package;

fn turn(r: [i32; 3]) -> Quat {
    super::me_port::turn(r)
}

fn round(x: f32, k: f32) -> f32 {
    (x * k).round() / k
}

fn tidy(v: Vec3) -> Vec3 {
    Vec3::new(round(v.x, 1e4), round(v.y, 1e4), round(v.z, 1e4))
}

/// Unreal's euler degrees (roll, pitch, yaw) as a rotator.
fn rotator(e: [f32; 3]) -> [i32; 3] {
    let u = |d: f32| (d * 65536.0 / 360.0).round() as i32;
    [u(e[1]), u(e[2]), u(e[0])]
}

/// A node: an op of one of the packages.
struct Node<'a> {
    pkg: usize,
    op: &'a Op,
    name: String,
    regions: Vec<faith_move::Aabb>,
}

/// An op's properties: a bool's, a float's, a name's.
fn flag(pkg: &Package, op: &Op, name: &str) -> Option<bool> {
    Props::read(pkg, &pkg.exports[op.export]).ok()?.get(name).map(|t| t.bool_value)
}

fn num(pkg: &Package, op: &Op, name: &str) -> Option<f32> {
    let p = Props::read(pkg, &pkg.exports[op.export]).ok()?;
    p.f32(pkg, name).or_else(|| p.i32(pkg, name).map(|i| i as f32))
}

fn name_of(pkg: &Package, op: &Op, name: &str) -> Option<String> {
    Props::read(pkg, &pkg.exports[op.export]).ok()?.name(pkg, name)
}

fn string_of(pkg: &Package, op: &Op, name: &str) -> Option<String> {
    let p = Props::read(pkg, &pkg.exports[op.export]).ok()?;
    let t = p.get(name)?;
    me_assets::package::Cursor::new(&pkg.data, t.at).fstring().ok()
}

/// The box round a placed mesh (ours): its mesh's box, placed.
fn mesh_region(doc: &LevelDoc, pkg: &str, actor: &str, at: Vec3) -> Option<faith_move::Aabb> {
    // (Its prop: by its name where it stands; its package may be its mesh's, not the map's.)
    let o = doc.objects.iter().find(|o| (o.name == actor || o.name == format!("{pkg}.{actor}")) && o.at.distance(at) < 0.01 && matches!(o.kind, Kind::MeProp { .. }))?;
    let Kind::MeProp { bounds: Some((lo, hi)), scale, .. } = &o.kind else { return None };
    let q = o.rotation();
    let (mut a, mut b) = (Vec3::MAX, Vec3::MIN);
    for k in 0..8 {
        let c = Vec3::new(if k & 1 == 0 { lo.x } else { hi.x }, if k & 2 == 0 { lo.y } else { hi.y }, if k & 4 == 0 { lo.z } else { hi.z });
        let p = o.at + q * (c * *scale);
        a = a.min(p);
        b = b.max(p);
    }
    Some(faith_move::Aabb::new(a, b))
}

/// Into `doc`: the movers and Kismet of the map's packages (`read`: each package's name,
/// itself, and what's placed in it). The props they carry are named `package.actor` (unique
/// across the packages).
pub fn port(read: &[(String, Rc<Package>, map::Map)], doc: &mut LevelDoc, report: &mut String) {
    let ops: Vec<Vec<Op>> = read.iter().map(|(_, p, _)| kismet::ops(p)).collect();
    // Every op a node.
    let mut nodes: Vec<Node> = Vec::new();
    let mut id: HashMap<(usize, usize), usize> = HashMap::new();
    for (pi, list) in ops.iter().enumerate() {
        for op in list {
            id.insert((pi, op.export), nodes.len());
            // (By its path: ops in different sequences can have the same name.)
            let path = read[pi].1.path(op.export as i32 + 1);
            let path = path.strip_prefix("TheWorld.PersistentLevel.Main_Sequence.").unwrap_or(&path);
            nodes.push(Node { pkg: pi, op, name: format!("{}.{path}", read[pi].0), regions: Vec::new() });
        }
    }
    // Where each output leads (a sub-sequence's input: the op inside it).
    let target = |pi: usize, export: usize, input: usize| -> Option<(usize, usize)> {
        let k = *id.get(&(pi, export))?;
        let op = nodes[k].op;
        if op.class == "Sequence" {
            let inner = op.inputs.get(input)?.1?;
            return Some((*id.get(&(pi, inner))?, 0));
        }
        Some((k, input))
    };
    let mut links: Vec<Vec<(usize, usize, usize, f32)>> = vec![Vec::new(); nodes.len()];
    for (k, n) in nodes.iter().enumerate() {
        for (j, out) in n.op.outputs.iter().enumerate() {
            for &(to, input, delay) in out {
                if let Some((t, i)) = target(n.pkg, to, input) {
                    links[k].push((j, t, i, delay));
                }
            }
        }
    }
    // A sequence's finish: what its sequence's output of that name leads to.
    for k in 0..nodes.len() {
        if nodes[k].op.class != "SeqAct_FinishSequence" {
            continue;
        }
        let pi = nodes[k].pkg;
        let pkg = &read[pi].1;
        let label = string_of(pkg, nodes[k].op, "OutputLabel").unwrap_or_default();
        let Some(seq) = nodes[k].op.parent.and_then(|p| id.get(&(pi, p)).copied()) else { continue };
        let Some(j) = nodes[seq].op.output_names.iter().position(|n| n.eq_ignore_ascii_case(&label)) else { continue };
        let onward: Vec<_> = links[seq].iter().filter(|l| l.0 == j).map(|&(_, t, i, d)| (0, t, i, d)).collect();
        links[k].extend(onward);
    }
    // A remote event's call: the events of its name.
    let mut remote: HashMap<String, Vec<usize>> = HashMap::new();
    for (k, n) in nodes.iter().enumerate() {
        if n.op.class == "SeqEvent_RemoteEvent" {
            if let Some(e) = name_of(&read[n.pkg].1, n.op, "EventName") {
                remote.entry(e.to_ascii_lowercase()).or_default().push(k);
            }
        }
    }
    for k in 0..nodes.len() {
        if nodes[k].op.class == "SeqAct_ActivateRemoteEvent" {
            if let Some(e) = name_of(&read[nodes[k].pkg].1, nodes[k].op, "EventName") {
                for &t in remote.get(&e.to_ascii_lowercase()).into_iter().flatten() {
                    links[k].push((0, t, 0, 0.0));
                }
            }
        }
    }
    // The movers: each matinee's move of each actor there is to move.
    let mut movers: Vec<Object> = Vec::new();
    // (Each carried prop: its new name, by its actor's name and where it stands.)
    let mut carried: Vec<(String, Vec3, String)> = Vec::new();
    let mut interp_movers: HashMap<usize, Vec<String>> = HashMap::new();
    for (pi, (pname, pkg, m)) in read.iter().enumerate() {
        for mv in kismet::moves(pkg, &ops[pi]) {
            if mv.keys.is_empty() {
                continue;
            }
            for &actor in &mv.actors {
                let Some(p) = m.meshes.iter().find(|p| p.export == actor) else { continue };
                let (p0, q0) = (ue(p.location), turn(p.rotation));
                let keys: Vec<MoverKey> = mv
                    .keys
                    .iter()
                    .map(|k| {
                        let (pos, q) = if mv.relative { (p0 + q0 * ue(k.pos), q0 * turn(rotator(k.euler))) } else { (ue(k.pos), turn(rotator(k.euler))) };
                        let dq = (q * q0.inverse()).normalize();
                        let (y, x, z) = dq.to_euler(EulerRot::YXZ);
                        let deg = |a: f32| round(a.to_degrees(), 1e3);
                        MoverKey { t: round(k.t, 1e4), offset: tidy(pos - p0), turn: Vec3::new(deg(y), deg(x), deg(z)), linear: k.linear }
                    })
                    .collect();
                // (One that doesn't move isn't a mover.)
                if keys.iter().all(|k| k.offset.length() < 0.01 && k.turn.abs().max_element() < 0.5) {
                    continue;
                }
                let name = format!("{pname}.{}.{}", pkg.exports[mv.op].name, p.name);
                // (And what's attached to it: a camera's head on its turning base.)
                let mut carries = vec![p];
                for q in &m.meshes {
                    if map::Chain::new(pkg, q.export).object("Base").filter(|&b| b > 0).is_some_and(|b| b as usize - 1 == actor) && !carries.iter().any(|c| c.export == q.export) {
                        carries.push(q);
                    }
                }
                for c in &carries {
                    let entry = (c.name.clone(), tidy(ue(c.location)), format!("{pname}.{}", c.name));
                    if !carried.contains(&entry) {
                        carried.push(entry);
                    }
                }
                let carries = carries.iter().map(|c| format!("{pname}.{}", c.name)).collect();
                let mut o = Object::new(Kind::MeMover { keys, length: round(mv.length, 1e4), rate: mv.rate, looping: mv.looping, carries }, tidy(p0));
                o.name = name.clone();
                movers.push(o);
                if let Some(&k) = id.get(&(pi, mv.op)) {
                    interp_movers.entry(k).or_default().push(name);
                }
            }
        }
    }
    // What ops that switch things (collision, visibility) switch: the props (each its own
    // body, to switch) and volumes (whose events are then on or off) they name (#111).
    let mut targets: HashMap<usize, Vec<String>> = HashMap::new();
    let mut bodies: Vec<(String, Vec3)> = Vec::new();
    for (k, n) in nodes.iter().enumerate() {
        if !matches!(n.op.class.as_str(), "SeqAct_ChangeCollision" | "SeqAct_ToggleHidden") {
            continue;
        }
        let (pname, _, m) = &read[n.pkg];
        for a in n.op.vars.iter().flat_map(|(_, v)| v.iter()).filter(|&&a| a > 0).map(|&a| a as usize - 1) {
            if let Some(p) = m.meshes.iter().find(|p| p.export == a) {
                let name = format!("{pname}.{}", p.name);
                let entry = (p.name.clone(), tidy(ue(p.location)), name.clone());
                if !carried.contains(&entry) {
                    carried.push(entry);
                }
                if !bodies.iter().any(|b| b.0 == name) {
                    bodies.push((name.clone(), tidy(ue(p.location))));
                }
                targets.entry(k).or_default().push(name);
            } else if m.volumes.iter().any(|v| v.export == a) || m.others.iter().any(|o| o.export == a) {
                targets.entry(k).or_default().push(format!("{pname}.{}", read[n.pkg].1.exports[a].name));
            }
        }
    }
    // The ops that matter: those that lead (in any number of steps) to a matinee with a mover,
    // or to switching something.
    let mut keep = vec![false; nodes.len()];
    let mut stack: Vec<usize> = interp_movers.keys().chain(targets.keys()).copied().collect();
    let mut into: Vec<Vec<usize>> = vec![Vec::new(); nodes.len()];
    for (k, l) in links.iter().enumerate() {
        for &(_, t, _, _) in l {
            into[t].push(k);
        }
    }
    while let Some(k) = stack.pop() {
        if keep[k] {
            continue;
        }
        keep[k] = true;
        stack.extend(into[k].iter().copied());
    }
    // What sets each event off: the actors that generate it (their GeneratedEvents).
    let mut originators: HashMap<usize, Vec<faith_move::Aabb>> = HashMap::new();
    let mut origin_names: HashMap<usize, String> = HashMap::new();
    for (pi, (pname, pkg, m)) in read.iter().enumerate() {
        let mut region_of: Vec<(usize, Option<faith_move::Aabb>)> = Vec::new();
        for v in &m.volumes {
            let planes = super::me_port::volume_planes(v);
            let b = aperture::me::hull_brush(&planes).turned(turn(v.rotation)).bounds().map(|b| b.translated(ue(v.location)));
            region_of.push((v.export, b));
        }
        for p in &m.meshes {
            // (From its mesh's box where it's placed: a hidden one, not drawn, too.)
            let placed = (p.import.is_none()).then(|| super::me_port::mesh_box(pkg, p.mesh)).flatten().map(|(lo, hi)| {
                let (q, s) = (turn(p.rotation), super::me_port::scale(p.scale));
                let (mut a, mut b) = (Vec3::MAX, Vec3::MIN);
                for k in 0..8 {
                    let c = Vec3::new(if k & 1 == 0 { lo.x } else { hi.x }, if k & 2 == 0 { lo.y } else { hi.y }, if k & 4 == 0 { lo.z } else { hi.z });
                    let w = ue(p.location) + q * (c * s);
                    a = a.min(w);
                    b = b.max(w);
                }
                faith_move::Aabb::new(a, b)
            });
            region_of.push((p.export, placed.or_else(|| mesh_region(doc, pname, &p.name, tidy(ue(p.location))))));
        }
        for o in &m.others {
            // (A trigger: its cylinder, as a box.)
            let c = map::Chain::new(pkg, o.export);
            let comp = c.object("CylinderComponent").filter(|&x| x > 0).map(|x| map::Chain::new(pkg, x as usize - 1));
            let r = comp.as_ref().and_then(|c| c.f32("CollisionRadius")).unwrap_or(40.0) * 0.01;
            let h = comp.as_ref().and_then(|c| c.f32("CollisionHeight")).unwrap_or(40.0) * 0.01;
            region_of.push((o.export, Some(faith_move::Aabb::from_center_size(ue(o.location), Vec3::new(r * 2.0, h * 2.0, r * 2.0)))));
        }
        let regions: HashMap<usize, faith_move::Aabb> = region_of.into_iter().filter_map(|(e, b)| Some((e, b?))).collect();
        for (&export, &b) in &regions {
            for e in kismet::events_of(pkg, export) {
                if let Some(&k) = id.get(&(pi, e)) {
                    originators.entry(k).or_default().push(b);
                }
            }
        }
        // (Each event's actor, by name: switched off, its events don't fire.)
        for (&export, _) in &regions {
            for e in kismet::events_of(pkg, export) {
                if let Some(&k) = id.get(&(pi, e)) {
                    origin_names.entry(k).or_insert_with(|| format!("{pname}.{}", pkg.exports[export].name));
                }
            }
        }
        for op in &ops[pi] {
            if let (Some(a), Some(&k)) = (op.originator, id.get(&(pi, op.export))) {
                origin_names.entry(k).or_insert_with(|| format!("{pname}.{}", pkg.exports[a].name));
            }
        }
        // (And the actor an event names as its own.)
        for op in &ops[pi] {
            let Some(b) = op.originator.and_then(|a| regions.get(&a)) else { continue };
            let Some(&k) = id.get(&(pi, op.export)) else { continue };
            let list = originators.entry(k).or_default();
            if !list.contains(b) {
                list.push(*b);
            }
        }
    }
    // The kept ops, as objects.
    let names: Vec<String> = nodes.iter().map(|n| n.name.clone()).collect();
    let mut kept = 0;
    for (k, n) in nodes.iter_mut().enumerate() {
        if !keep[k] {
            continue;
        }
        let pkg = &read[n.pkg].1;
        let c = n.op.class.as_str();
        let mut params: Vec<(String, String)> = Vec::new();
        let mut set = |k: &str, v: String| params.push((k.to_string(), v));
        if n.op.output_names.iter().any(|o| !o.is_empty()) {
            set("outputs", n.op.output_names.join("|"));
        }
        if c.starts_with("SeqEvent") || c.starts_with("SeqEvt") {
            set("max", format!("{}", num(pkg, n.op, "MaxTriggerCount").unwrap_or(1.0)));
            set("retrigger", format!("{}", num(pkg, n.op, "ReTriggerDelay").unwrap_or(0.5)));
        }
        if let Some(o) = origin_names.get(&k) {
            set("originator", o.clone());
        }
        for t in targets.get(&k).into_iter().flatten() {
            set("target", t.clone());
        }
        if n.op.inputs.iter().any(|i| !i.0.is_empty()) {
            set("inputs", n.op.inputs.iter().map(|i| i.0.clone()).collect::<Vec<_>>().join("|"));
        }
        match c {
            // (Whether she bumps into it after: only what blocks all, or all but weapons.)
            "SeqAct_ChangeCollision" => {
                let t = name_of(pkg, n.op, "CollisionType").unwrap_or_default();
                let blocks = matches!(t.as_str(), "COLLIDE_BlockAll" | "COLLIDE_BlockAllButWeapons") || t.is_empty() && flag(pkg, n.op, "bBlockActors") == Some(true);
                let touches = blocks || t.starts_with("COLLIDE_Touch");
                set("blocks", blocks.to_string());
                set("touches", touches.to_string());
            }
            "SeqAct_Interp" => {
                for m in interp_movers.get(&k).into_iter().flatten() {
                    set("mover", m.clone());
                }
                if flag(pkg, n.op, "bRewindOnPlay") == Some(true) {
                    set("rewind_on_play", "true".into());
                }
            }
            // (How long its mover stays open before it's "Open Finished": UE3's default 1.5 s.)
            "SeqEvent_Mover" => set("stay_open", format!("{}", num(pkg, n.op, "StayOpenTime").unwrap_or(1.5))),
            "SeqAct_Delay" => set("duration", format!("{}", num(pkg, n.op, "Duration").or_else(|| num(pkg, n.op, "DefaultDuration")).unwrap_or(1.0))),
            "SeqAct_Gate" => {
                if flag(pkg, n.op, "bOpen") == Some(false) {
                    set("open", "false".into());
                }
            }
            "SeqAct_Switch" | "SeqAct_DelaySwitch" | "SeqAct_RandomSwitch" => {
                set("links", format!("{}", num(pkg, n.op, "LinkCount").unwrap_or(n.op.outputs.len().max(1) as f32)));
                if let Some(i) = num(pkg, n.op, "IncrementAmount") {
                    set("increment", format!("{i}"));
                }
                if flag(pkg, n.op, "bLooping") == Some(true) {
                    set("looping", "true".into());
                }
                if let Some(d) = num(pkg, n.op, "Delay") {
                    set("delay", format!("{d}"));
                }
            }
            _ => {}
        }
        n.regions = originators.get(&k).cloned().unwrap_or_default();
        // (A mover event's region: standing on its mover.)
        if c == "SeqEvent_Mover" {
            for r in n.regions.iter_mut() {
                *r = faith_move::Aabb::new(Vec3::new(r.min.x, r.max.y - 0.1, r.min.z), Vec3::new(r.max.x, r.max.y + 0.4, r.max.z));
            }
        }
        let mut o = match n.regions.first() {
            Some(r) => Object::sized(Kind::MeLogic { op: c.to_string(), params: Vec::new() }, tidy(r.center()), tidy(r.size())),
            None => Object::new(Kind::MeLogic { op: c.to_string(), params: Vec::new() }, Vec3::ZERO),
        };
        for r in n.regions.iter().skip(1) {
            params.push(("region".into(), format!("{} {} {} {} {} {}", round(r.min.x, 1e3), round(r.min.y, 1e3), round(r.min.z, 1e3), round(r.max.x, 1e3), round(r.max.y, 1e3), round(r.max.z, 1e3))));
        }
        if let Kind::MeLogic { params: p, .. } = &mut o.kind {
            *p = params;
        }
        o.name = n.name.clone();
        o.outputs = links[k]
            .iter()
            .filter(|l| keep[l.1])
            .map(|&(j, t, i, d)| Output { event: j.to_string(), target: names[t].clone(), input: i.to_string(), param: String::new(), delay: d, times: -1 })
            .collect();
        doc.objects.push(o);
        kept += 1;
    }
    // The props they carry, named so they're found (`package.actor`).
    for o in doc.objects.iter_mut() {
        if matches!(o.kind, Kind::MeProp { .. }) {
            if let Some((_, _, new)) = carried.iter().find(|(n, at, _)| *n == o.name && at.distance(o.at) < 0.01) {
                o.name = new.clone();
            }
        }
    }
    // (A switched prop that no matinee moves: a body of its own, still.)
    for (name, at) in bodies {
        let moved = movers.iter().any(|o| matches!(&o.kind, Kind::MeMover { carries, .. } if carries.contains(&name)));
        if !moved {
            let mut o = Object::new(Kind::MeMover { keys: Vec::new(), length: 0.0, rate: 1.0, looping: false, carries: vec![name.clone()] }, at);
            o.name = format!("{name}.body");
            movers.push(o);
        }
    }
    report.push_str(&format!("  {} movers, {kept} Kismet ops driving them\n", movers.len()));
    doc.objects.extend(movers);
}
