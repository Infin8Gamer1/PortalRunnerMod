//! Training's movement challenges played in the sim, a checkpoint at a time: `me_challenges
//! [level file]`. From her start, each challenge under way has her put through its checkpoints
//! in order, doing what each wants (a jump or crouch for real, else her state set for a frame);
//! prints what starts, what's passed, and which props runner vision lights up or puts out on
//! the way (#118). It stops where a checkpoint wants what it can't fake (a wallrun jump).
use aperture::doc::LevelDoc;
use aperture::sim::Sim;
use faith_move::{Controller, Input, State};
use glam::Vec3;

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let file = a.get(1).cloned().unwrap_or_else(|| "levels/me/tutorial.ron".into());
    let doc = LevelDoc::from_ron(&std::fs::read_to_string(&file).expect("level")).expect("doc");
    let l = doc.build();
    let cp = l.checkpoints[0].clone();
    let mut sim = Sim::new(l.clone());
    let mut c = Controller::new(aperture::tuning(5.3, 15.0), cp.spawn, cp.yaw);
    c.state = State::Ground;
    let dt = 1.0 / 60.0;
    let mut lit_before: Vec<usize> = Vec::new();
    // (Crouched: through a checkpoint under a duct, as she'd slide.)
    let step = |sim: &mut Sim, c: &mut Controller, frames: u32| {
        for _ in 0..frames {
            sim.step_player(c, &Input { crouch_held: true, ..Default::default() }, dt);
            sim.step(dt, c);
        }
    };
    let report = |sim: &Sim, lit_before: &mut Vec<usize>| {
        let me = sim.me.as_ref().unwrap();
        let mut lit: Vec<usize> = me.lit.iter().filter(|(_, l)| l.level > 0.5).map(|(&p, _)| p).collect();
        lit.sort();
        let on: Vec<String> = lit.iter().filter(|p| !lit_before.contains(p)).map(|&p| format!("{}.{}", sim.level.me_props[p].package, sim.level.me_props[p].mesh)).collect();
        let off: Vec<String> = lit_before.iter().filter(|p| !lit.contains(p)).map(|&p| format!("{}.{}", sim.level.me_props[p].package, sim.level.me_props[p].mesh)).collect();
        if !on.is_empty() || !off.is_empty() {
            println!("    lit {on:?}  out {off:?}");
        }
        *lit_before = lit;
    };
    step(&mut sim, &mut c, 1200);
    report(&sim, &mut lit_before);
    let mut done = Vec::new();
    for _ in 0..200 {
        let Some((name, passed)) = sim.me.as_ref().unwrap().challenge.clone() else {
            println!("no challenge under way");
            break;
        };
        if done.last() != Some(&name) {
            println!("challenge {name}");
            done.push(name.clone());
        }
        // (Its next place: the first she hasn't passed, any of its checkpoints, and what it
        // wants done there.)
        let mut places: Vec<(i32, usize, Vec<String>)> = sim
            .level
            .me_logic
            .iter()
            .enumerate()
            .filter(|(_, o)| o.op == "TdTutorialCheckpoint")
            .filter_map(|(k, o)| {
                o.params.iter().filter(|(n, _)| n == "track").find_map(|(_, t)| {
                    let w: Vec<&str> = t.split_whitespace().collect();
                    let needs = w.iter().position(|x| *x == "needs").and_then(|i| w.get(i + 1)).map_or(Vec::new(), |n| n.split(',').map(str::to_string).collect());
                    (w.first() == Some(&name.as_str())).then(|| (w.get(1).and_then(|x| x.parse::<i32>().ok()).unwrap_or(0), k, needs))
                })
            })
            .collect();
        places.sort_by_key(|p| (p.0, p.1));
        places.dedup_by_key(|p| p.0);
        let Some((order, k, needs)) = places.get(passed).cloned() else { break };
        let r = sim.level.me_logic[k].regions[0];
        println!("  through {} (place {order}{})", sim.level.me_logic[k].name, if needs.is_empty() { String::new() } else { format!(", {}", needs.join(" or ")) });
        c.teleport(Vec3::new(r.center().x, r.min.y + 0.05, r.center().z), Vec3::ZERO, c.yaw, 0.0, false);
        c.crouched = true;
        step(&mut sim, &mut c, 1);
        // (Pushed out, as from under a duct: crouched, she goes for it from there.)
        if !r.overlaps(&c.body().aabb(c.feet)) {
            let to = r.center() - c.feet;
            c.yaw = (-to.x).atan2(-to.z);
            for _ in 0..90 {
                sim.step_player(&mut c, &Input { move_axis: glam::Vec2::new(0.0, 1.0), crouch_held: true, ..Default::default() }, dt);
                sim.step(dt, &mut c);
                if r.overlaps(&c.body().aabb(c.feet)) {
                    break;
                }
            }
        }
        // (What it wants: done for real where it's a press, else her state set for a frame.)
        let want = needs.first().cloned().unwrap_or_default();
        let at = c.feet;
        match want.as_str() {
            "MOVE_Jump" => {
                c.crouched = false;
                c.state = State::Ground;
                sim.step_player(&mut c, &Input { jump_pressed: true, jump_held: true, ..Default::default() }, dt);
                sim.step(dt, &mut c);
            }
            "" | "MOVE_Walking" | "MOVE_Landing" | "MOVE_LedgeWalk" => {
                c.crouched = false;
                for _ in 0..3 {
                    c.state = State::Ground;
                    sim.step(dt, &mut c);
                }
            }
            "MOVE_Crouch" => {
                c.crouched = true;
                c.state = State::Ground;
                sim.step(dt, &mut c);
            }
            w => {
                c.state = match w {
                    "MOVE_Slide" => State::Slide { t: 0.1 },
                    "ETE_SkillRoll" => State::Roll { t: 0.1 },
                    "MOVE_WallRun" | "MOVE_WallRunningLeft" | "MOVE_WallRunningRight" => State::WallRun { normal: Vec3::X, t: 0.1 },
                    "MOVE_Grabbing" => State::LedgeHang { normal: Vec3::X, ledge_y: at.y + 2.0, turned: false },
                    "MOVE_ZipLine" => State::ZipLine { a: at, b: at + Vec3::X, sag: 0.0, s: 0.0, speed: 1.0 },
                    "MOVE_Swing" => State::Swing { a: at, b: at + Vec3::X, at, dir: Vec3::Z, angle: 0.0, rate: 0.0 },
                    "MOVE_GrabPullUp" | "MOVE_SpringBoarding" | "MOVE_VaultOver" | "MOVE_SpeedVaulting" | "ETE_SpeedVault" => {
                        let kind = match w {
                            "MOVE_GrabPullUp" => faith_move::TraverseKind::PullUp,
                            "MOVE_SpringBoarding" => faith_move::TraverseKind::SpringBoard,
                            _ => faith_move::TraverseKind::Vault,
                        };
                        State::Traverse(faith_move::Traverse { kind, from: at, to: at, t: 0.0, dur: 1.0, arc: 0.0, exit_vel: Vec3::ZERO })
                    }
                    _ => State::Ground,
                };
                sim.step(dt, &mut c);
                c.state = State::Ground;
            }
        }
        step(&mut sim, &mut c, 29);
        c.teleport(Vec3::new(r.center().x, r.max.y + 20.0, r.center().z), Vec3::ZERO, c.yaw, 0.0, false);
        step(&mut sim, &mut c, 2);
        report(&sim, &mut lit_before);
        step(&mut sim, &mut c, 90);
        report(&sim, &mut lit_before);
        if sim.me.as_ref().unwrap().challenge == Some((name.clone(), passed)) {
            println!("  (stuck: {want} isn't something this can fake)");
            break;
        }
    }
}
