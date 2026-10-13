//! A map package's Kismet (#109): its sequence ops, how their outputs lead into other ops'
//! inputs, what their variables hold, and its matinees' moves (`InterpData` → groups → a move
//! track's keys).

use crate::package::Package;
use crate::props::Props;

/// One sequence op (an event, an action, a condition, a matinee).
#[derive(Clone, Debug)]
pub struct Op {
    /// Its export (0-based).
    pub export: usize,
    pub class: String,
    pub name: String,
    /// Each output: the ops (exports) and inputs it leads to, and its delay (seconds).
    pub outputs: Vec<Vec<(usize, usize, f32)>>,
    /// Each variable link: its description and the objects its variables hold (exports, or
    /// imports as negatives; a named variable's or the player's as 0).
    pub vars: Vec<(String, Vec<i32>)>,
    /// Each input: its description, and (a sub-sequence's) the op inside it that it sets off.
    pub inputs: Vec<(String, Option<usize>)>,
    /// Each output's description.
    pub output_names: Vec<String>,
    /// The sequence it's in (its export).
    pub parent: Option<usize>,
    /// An event's actor (its export): what it happens to.
    pub originator: Option<usize>,
}

/// A link's description (a StrProperty).
fn desc(pkg: &Package, p: &Props, name: &str) -> String {
    p.get(name).and_then(|t| crate::package::Cursor::new(&pkg.data, t.at).fstring().ok()).unwrap_or_default()
}

/// A key of a move track: when, where (Unreal units), and turned how (Unreal's euler: roll,
/// pitch, yaw in degrees), and how it's eased to the next (`linear`: straight; else curved).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Key {
    pub t: f32,
    pub pos: [f32; 3],
    pub euler: [f32; 3],
    pub linear: bool,
}

/// A matinee's move: the actors it moves, its keys, whether they're relative to where each
/// actor starts, how long it is and how fast it plays.
#[derive(Clone, Debug)]
pub struct Move {
    /// The SeqAct_Interp's export.
    pub op: usize,
    pub group: String,
    pub actors: Vec<usize>,
    pub keys: Vec<Key>,
    pub relative: bool,
    pub length: f32,
    pub rate: f32,
    pub looping: bool,
}

fn vec3(p: &Props, pkg: &Package, name: &str) -> [f32; 3] {
    p.vec3(pkg, name).unwrap_or_default()
}

/// What a variable link's variable holds: an object variable's object (an export, or an import
/// as a negative; 0 for a player or named variable, or one that's empty); anything else linked
/// (a matinee's data) itself.
fn var_value(pkg: &Package, v: i32) -> i32 {
    if v <= 0 {
        return 0;
    }
    let e = &pkg.exports[v as usize - 1];
    if !pkg.class_name(e).starts_with("SeqVar") {
        return v;
    }
    Props::read(pkg, e).ok().and_then(|p| p.object(pkg, "ObjValue")).or_else(|| Props::read_object(pkg, e).ok().and_then(|p| p.object(pkg, "ObjValue"))).unwrap_or(0)
}

/// Every op in the package.
pub fn ops(pkg: &Package) -> Vec<Op> {
    let mut out = vec![];
    for (i, e) in pkg.exports.iter().enumerate() {
        let class = pkg.class_name(e);
        if !(class.starts_with("Seq") || class.starts_with("TdSeq")) || class.starts_with("SeqVar") {
            continue;
        }
        let Ok(p) = Props::read(pkg, e) else { continue };
        let outputs = p
            .struct_array(pkg, "OutputLinks")
            .iter()
            .map(|l| {
                let delay = l.f32(pkg, "ActivateDelay").unwrap_or(0.0);
                l.struct_array(pkg, "Links")
                    .iter()
                    .filter_map(|x| {
                        let op = x.object(pkg, "LinkedOp").filter(|&o| o > 0)? as usize - 1;
                        Some((op, x.i32(pkg, "InputLinkIdx").unwrap_or(0).max(0) as usize, delay))
                    })
                    .collect()
            })
            .collect();
        let vars = p.struct_array(pkg, "VariableLinks").iter().map(|l| (desc(pkg, l, "LinkDesc"), l.object_array(pkg, "LinkedVariables").iter().map(|&v| var_value(pkg, v)).collect())).collect();
        let inputs = p.struct_array(pkg, "InputLinks").iter().map(|l| (desc(pkg, l, "LinkDesc"), l.object(pkg, "LinkedOp").filter(|&o| o > 0).map(|o| o as usize - 1))).collect();
        let output_names = p.struct_array(pkg, "OutputLinks").iter().map(|l| desc(pkg, l, "LinkDesc")).collect();
        let parent = p.object(pkg, "ParentSequence").filter(|&o| o > 0).map(|o| o as usize - 1);
        let originator = p.object(pkg, "Originator").filter(|&o| o > 0).map(|o| o as usize - 1);
        out.push(Op { export: i, class: class.to_string(), name: e.name.clone(), outputs, vars, inputs, output_names, parent, originator });
    }
    out
}

/// Every matinee's moves: for each SeqAct_Interp, each of its groups with a move track and
/// actors to move.
pub fn moves(pkg: &Package, ops: &[Op]) -> Vec<Move> {
    let mut out = vec![];
    for op in ops.iter().filter(|o| o.class == "SeqAct_Interp") {
        let Ok(p) = Props::read(pkg, &pkg.exports[op.export]) else { continue };
        // (Its data: the variable linked as "Data".)
        let Some(data) = op.vars.iter().flat_map(|(_, v)| v.iter()).copied().filter(|&d| d > 0).map(|d| d as usize - 1).find(|&d| pkg.class_name(&pkg.exports[d]) == "InterpData") else {
            continue;
        };
        let Ok(dp) = Props::read(pkg, &pkg.exports[data]) else { continue };
        let length = dp.f32(pkg, "InterpLength").unwrap_or(5.0);
        let rate = p.f32(pkg, "PlayRate").unwrap_or(1.0);
        let looping = p.get("bLooping").is_some_and(|t| t.bool_value);
        for g in dp.object_array(pkg, "InterpGroups").into_iter().filter(|&g| g > 0) {
            let ge = &pkg.exports[g as usize - 1];
            let Ok(gp) = Props::read(pkg, ge) else { continue };
            let group = gp.name(pkg, "GroupName").unwrap_or_default();
            // The group's actors: the variables linked under its name.
            let actors: Vec<usize> = op.vars.iter().filter(|(d, _)| d.eq_ignore_ascii_case(&group)).flat_map(|(_, v)| v.iter().filter(|&&a| a > 0).map(|&a| a as usize - 1)).collect();
            for t in gp.object_array(pkg, "InterpTracks").into_iter().filter(|&t| t > 0) {
                let te = &pkg.exports[t as usize - 1];
                if pkg.class_name(te) != "InterpTrackMove" {
                    continue;
                }
                let Ok(tp) = Props::read(pkg, te) else { continue };
                let curve = |name: &str| -> Vec<(f32, [f32; 3], bool)> {
                    let Some(tag) = tp.get(name) else { return vec![] };
                    let Ok(c) = Props::read_list(pkg, tag.at, tag.at + tag.size, name) else { return vec![] };
                    c.struct_array(pkg, "Points")
                        .iter()
                        .map(|k| (k.f32(pkg, "InVal").unwrap_or(0.0), vec3(k, pkg, "OutVal"), k.name(pkg, "InterpMode").is_some_and(|m| m == "CIM_Linear" || m == "CIM_Constant")))
                        .collect()
                };
                let pos = curve("PosTrack");
                let rot = curve("EulerTrack");
                let keys: Vec<Key> = pos
                    .iter()
                    .enumerate()
                    .map(|(k, &(t, p, linear))| Key { t, pos: p, euler: rot.get(k).map_or([0.0; 3], |r| r.1), linear })
                    .collect();
                let relative = tp.name(pkg, "MoveFrame").is_some_and(|m| m == "IMF_RelativeToInitial");
                out.push(Move { op: op.export, group: group.clone(), actors: actors.clone(), keys, relative, length, rate, looping });
            }
        }
    }
    out
}

/// The events an actor sets off (its `GeneratedEvents`): ops' exports.
pub fn events_of(pkg: &Package, actor: usize) -> Vec<usize> {
    crate::map::Chain::new(pkg, actor).objects("GeneratedEvents").into_iter().filter(|&e| e > 0).map(|e| e as usize - 1).collect()
}
