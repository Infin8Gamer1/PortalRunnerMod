//! Which of the Mirror's Edge levels' movers the Kismet can ever play (#109): `me_movers_audit
//! [level files...]` (default: all of `levels/me`). From the events that can happen in play (the
//! level loading, her touching, using or kicking something, standing on a mover, reaching or
//! coming back to a checkpoint, Training's challenges), through every output, to the matinees; for each level, how many movers that
//! reaches, and for the rest, the events they wait on instead (by kind). `-v` lists those movers.
use std::collections::{BTreeMap, HashSet};

use aperture::doc::LevelDoc;

/// Events the VM sets off from what happens (the rest only fire when an op calls them).
const HAPPEN: [&str; 7] = ["SeqEvent_LevelLoaded", "SeqEvent_LevelStartup", "SeqEvt_TTRaceStarted", "SeqEvent_Touch", "SeqEvent_TdTouch", "SeqEvent_TdUsed", "SeqEvent_Used"];

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let verbose = args.iter().any(|a| a == "-v");
    let mut files: Vec<String> = args.into_iter().filter(|a| a != "-v").collect();
    if files.is_empty() {
        let mut all: Vec<String> = std::fs::read_dir("levels/me").expect("levels/me").flatten().map(|e| e.path().display().to_string()).filter(|p| p.ends_with(".ron")).collect();
        all.sort();
        files = all;
    }
    let mut waits_all: BTreeMap<String, usize> = BTreeMap::new();
    for f in files {
        let Ok(doc) = LevelDoc::from_ron(&std::fs::read_to_string(&f).unwrap_or_default()) else { continue };
        let l = doc.build();
        let ops = &l.me_logic;
        if ops.is_empty() {
            continue;
        }
        // (What can happen: events of those kinds with a region to happen in, or none needed;
        // a kick or a mover's, with a region; Training's challenges.)
        let can = |k: usize| {
            let o = &ops[k];
            let needs_region = !matches!(o.op.as_str(), "SeqEvent_LevelLoaded" | "SeqEvent_LevelStartup" | "SeqEvt_TTRaceStarted");
            (HAPPEN.contains(&o.op.as_str()) || matches!(o.op.as_str(), "SeqEvent_TakeDamage" | "SeqEvent_Mover")) && (!needs_region || !o.regions.is_empty())
                || o.op.starts_with("SeqEvt_TdMovementChallenge") && ops.iter().any(|c| c.op == "TdTutorialCheckpoint")
                || o.op == "SeqEvt_TdTutorialCheckpointCompleted" && ops.iter().any(|c| c.op == "TdTutorialCheckpoint")
                || matches!(o.op.as_str(), "SeqEvt_TdCheckpointLoaded" | "SeqEvt_TdCheckpointActivated") && !o.regions.is_empty()
        };
        let mut reached = vec![false; ops.len()];
        let mut stack: Vec<usize> = (0..ops.len()).filter(|&k| can(k)).collect();
        while let Some(k) = stack.pop() {
            if std::mem::replace(&mut reached[k], true) {
                continue;
            }
            stack.extend(ops[k].outputs.iter().map(|o| o.1));
        }
        // (Into each op: what leads to it.)
        let mut into: Vec<Vec<usize>> = vec![Vec::new(); ops.len()];
        for (k, o) in ops.iter().enumerate() {
            for out in &o.outputs {
                into[out.1].push(k);
            }
        }
        let mut movers = 0;
        let mut moved = HashSet::new();
        let mut stuck = Vec::new();
        for (k, o) in ops.iter().enumerate().filter(|(_, o)| o.op == "SeqAct_Interp" && !o.movers.is_empty()) {
            movers += o.movers.len();
            if reached[k] {
                moved.extend(o.movers.iter().copied());
                continue;
            }
            // (What it waits on: the events upstream of it.)
            let mut seen = HashSet::new();
            let mut back = vec![k];
            let mut events = Vec::new();
            while let Some(j) = back.pop() {
                if !seen.insert(j) {
                    continue;
                }
                if (ops[j].op.starts_with("SeqEvent") || ops[j].op.starts_with("SeqEvt")) && !matches!(ops[j].op.as_str(), "SeqEvent_RemoteEvent" | "SeqEvent_SequenceActivated") || into[j].is_empty() {
                    events.push(if into[j].is_empty() && !ops[j].op.starts_with("SeqEv") { format!("(nothing: {})", ops[j].op) } else { ops[j].op.clone() });
                }
                back.extend(into[j].iter().copied());
            }
            events.sort();
            events.dedup();
            for e in &events {
                *waits_all.entry(e.clone()).or_default() += o.movers.len();
            }
            stuck.push((o.name.clone(), o.movers.len(), events));
        }
        let name = f.rsplit('/').next().unwrap_or(&f);
        let unmoved: usize = stuck.iter().map(|s| s.1).sum();
        println!("{name}: {movers} mover tracks, {unmoved} never played");
        if verbose {
            for (op, n, events) in &stuck {
                println!("    {op} ({n}): waits on {}", events.join(", "));
            }
        }
    }
    println!("waiting on: {waits_all:?}");
}
