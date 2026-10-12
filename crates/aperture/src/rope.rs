//! Portal's cables (move_rope and keyframe_rope): each hangs between its key and the next as
//! Source's ropes do, so what the maps hang on them (Chamber 16's cage lights) hangs from them.
//!
//! Source simulates a rope as a chain of nodes (`C_RopeKeyframe`, `CBaseRopePhysics`): Verlet
//! at 50 steps a second, gravity of 1500 units/s², each step's motion kept at 98%, then three
//! passes pulling stretched links back to their length, the ends held to their keys. A link's
//! length is the keys' distance plus the rope's `Slack`, less a fudge of 100 units, shared
//! among the links. Three passes don't undo all that gravity stretches, so ropes hang a little
//! lower than their length alone would have them: running the same steps puts them where
//! Portal does.

use glam::Vec3;

use crate::source::{key, to_ours, Entity};

/// A cable: points along it (ours), and its radius (m).
#[derive(Clone, Debug, PartialEq)]
pub struct Rope {
    pub points: Vec<Vec3>,
    pub radius: f32,
}

const GRAVITY: f32 = 1500.0;
const STEP: f32 = 1.0 / 50.0;
const DAMP: f32 = 0.98;
const PASSES: usize = 3;
const SLACK_FUDGE: f32 = -100.0;
/// How long a rope's left to hang before it's drawn (seconds).
const HANG: f32 = 10.0;
/// Points drawn between each two of its nodes, at the least.
const MIN_SUBDIV: usize = 4;

/// The map's cables, hung. (Those parented to something that moves are left out: they'd hang
/// where it started.)
pub fn hang(ents: &[Entity]) -> Vec<Rope> {
    let origin = |e: &Entity| -> Option<Vec3> {
        let v: Vec<f32> = key(e, "origin")?.split_whitespace().filter_map(|s| s.parse().ok()).collect();
        (v.len() == 3).then(|| Vec3::new(v[0], v[1], v[2]))
    };
    let named = |n: &str| ents.iter().find(|e| key(e, "classname") == Some("keyframe_rope") && key(e, "targetname").is_some_and(|t| t.eq_ignore_ascii_case(n)));
    let mut out = Vec::new();
    for e in ents {
        if !matches!(key(e, "classname"), Some("move_rope" | "keyframe_rope")) {
            continue;
        }
        let Some(next) = key(e, "NextKey").filter(|n| !n.is_empty()).and_then(named) else { continue };
        if key(e, "parentname").is_some_and(|p| !p.is_empty()) || key(next, "parentname").is_some_and(|p| !p.is_empty()) {
            continue;
        }
        let (Some(a), Some(b)) = (origin(e), origin(next)) else { continue };
        let num = |k: &str, d: f32| key(e, k).and_then(|v| v.trim().parse::<f32>().ok()).unwrap_or(d);
        // (Its `Type`: 0 is ten nodes, 1 four, 2 two; without one, Source's default five.)
        let nodes = match key(e, "Type").map(str::trim) {
            Some("0") => 10,
            Some("1") => 4,
            Some(_) => 2,
            None => 5,
        };
        let dangling = key(e, "Dangling") == Some("1");
        let length = ((b - a).length().floor() + num("Slack", 0.0) + SLACK_FUDGE).max(0.0) / (nodes - 1) as f32;
        let chain = simulate(a, b, nodes, length, dangling);
        let subdiv = (num("Subdiv", 2.0) as usize).max(MIN_SUBDIV);
        out.push(Rope { points: smooth(&chain, subdiv).into_iter().map(to_ours).collect(), radius: num("Width", 2.0) * 0.5 * 0.0254 });
    }
    out
}

/// A rope's nodes after it's hung (Source's space): `nodes` of them from `a` to `b`, links
/// `length` long, its end loose if it's `dangling`.
fn simulate(a: Vec3, b: Vec3, nodes: usize, length: f32, dangling: bool) -> Vec<Vec3> {
    let mut pos: Vec<Vec3> = (0..nodes).map(|i| a.lerp(b, i as f32 / (nodes - 1) as f32)).collect();
    let mut prev = pos.clone();
    let fall = Vec3::new(0.0, 0.0, -GRAVITY) * STEP * STEP * 0.5;
    for _ in 0..(HANG / STEP) as usize {
        for (p, q) in pos.iter_mut().zip(prev.iter_mut()) {
            let was = *p;
            *p += (*p - *q) * DAMP + fall;
            *q = was;
        }
        for _ in 0..PASSES {
            for i in 0..nodes - 1 {
                let to = pos[i] - pos[i + 1];
                let d = to.length();
                if d > length {
                    let pull = to * (1.0 - length / d) * 0.5;
                    pos[i] -= pull;
                    pos[i + 1] += pull;
                }
            }
            pos[0] = a;
            if !dangling {
                pos[nodes - 1] = b;
            }
        }
    }
    pos
}

/// The curve Source draws through a rope's nodes: Catmull-Rom, `subdiv` points between each
/// two (the end nodes standing in for the ones beyond them).
fn smooth(nodes: &[Vec3], subdiv: usize) -> Vec<Vec3> {
    let n = nodes.len();
    let at = |i: isize| nodes[i.clamp(0, n as isize - 1) as usize];
    let mut out = vec![nodes[0]];
    for i in 0..n as isize - 1 {
        let (p0, p1, p2, p3) = (at(i - 1), at(i), at(i + 1), at(i + 2));
        for k in 1..=subdiv + 1 {
            let t = k as f32 / (subdiv + 1) as f32;
            let (t2, t3) = (t * t, t * t * t);
            out.push(0.5 * (2.0 * p1 + (p2 - p0) * t + (2.0 * p0 - 5.0 * p1 + 4.0 * p2 - p3) * t2 + (3.0 * p1 - p0 - 3.0 * p2 + p3) * t3));
        }
    }
    out
}
