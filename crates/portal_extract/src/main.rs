//! portal-extract: copies what Portal Runner uses out of your own Portal and Portal 2
//! installs into `assets/extracted/` (which git ignores): sounds as they are, textures from
//! VTF to PNG, and models from MDL/VVD/VTX to glTF (.glb, in metres, Y up, facing -Z).
//!
//! ```text
//! cargo run --release -p portal_extract -- [--portal <dir>] [--portal2 <dir>] [--out <dir>]
//! ```
//!
//! Without arguments it looks in the usual Steam folders. Nothing it writes is part of the
//! project: run it on your own machine, against your own copies of the games.

mod anim;
mod glb;
mod vpk;

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use vpk::Vpk;

/// Everything readable from one game: its VPKs (in priority order) and loose folders.
struct Game {
    name: &'static str,
    vpks: Vec<Vpk>,
    loose: Vec<PathBuf>,
}

impl Game {
    fn open(name: &'static str, root: &Path, vpks: &[&str], loose: &[&str]) -> Option<Game> {
        if !root.is_dir() {
            return None;
        }
        let mut g = Game { name, vpks: Vec::new(), loose: Vec::new() };
        for v in vpks {
            let p = root.join(v);
            match Vpk::open(&p) {
                Ok(vpk) => g.vpks.push(vpk),
                Err(e) if p.exists() => eprintln!("  {}: {e}", p.display()),
                Err(_) => {}
            }
        }
        for l in loose {
            let p = root.join(l);
            if p.is_dir() {
                g.loose.push(p);
            }
        }
        (!g.vpks.is_empty()).then_some(g)
    }

    fn read(&self, path: &str) -> Option<Vec<u8>> {
        let path = path.to_ascii_lowercase().replace('\\', "/");
        for l in &self.loose {
            if let Ok(b) = std::fs::read(l.join(&path)) {
                return Some(b);
            }
        }
        self.vpks.iter().find_map(|v| v.read(&path))
    }
}

fn steam_dirs() -> Vec<PathBuf> {
    let mut v = Vec::new();
    if let Some(home) = std::env::var_os("HOME") {
        let home = PathBuf::from(home);
        for s in [".local/share/Steam", ".steam/steam", ".var/app/com.valvesoftware.Steam/.local/share/Steam"] {
            v.push(home.join(s).join("steamapps/common"));
        }
    }
    for s in [r"C:\Program Files (x86)\Steam\steamapps\common", r"C:\Program Files\Steam\steamapps\common"] {
        v.push(PathBuf::from(s));
    }
    v
}

fn arg(args: &[String], name: &str) -> Option<PathBuf> {
    args.iter().position(|a| a == name).and_then(|i| args.get(i + 1)).map(PathBuf::from)
}

/// (our name, candidate paths in order: the first one found is used).
const SOUNDS: &[(&str, &[&str])] = &[
    // The gun and portals (Portal).
    ("gun_blue", &["sound/weapons/portalgun/portalgun_shoot_blue1.wav"]),
    ("gun_orange", &["sound/weapons/portalgun/portalgun_shoot_red1.wav"]),
    ("portal_open1", &["sound/weapons/portalgun/portal_open1.wav"]),
    ("portal_open2", &["sound/weapons/portalgun/portal_open2.wav"]),
    ("portal_open3", &["sound/weapons/portalgun/portal_open3.wav"]),
    ("portal_invalid", &["sound/weapons/portalgun/portal_invalid_surface3.wav"]),
    ("portal_fizzle", &["sound/weapons/portalgun/portal_fizzle2.wav"]),
    ("portal_close1", &["sound/weapons/portalgun/portal_close1.wav"]),
    ("portal_close2", &["sound/weapons/portalgun/portal_close2.wav"]),
    ("portal_ambient", &["sound/weapons/portalgun/portal_ambient_loop1.wav"]),
    ("portal_enter1", &["sound/player/portal_enter1.wav"]),
    ("portal_enter2", &["sound/player/portal_enter2.wav"]),
    ("portal_exit1", &["sound/player/portal_exit1.wav"]),
    ("portal_exit2", &["sound/player/portal_exit2.wav"]),
    ("gun_powerup", &["sound/player/portalgun_powerup1.wav", "sound/weapons/portalgun/portalgun_powerup1.wav"]),
    // Chambers (Portal 2).
    ("button_down", &["sound/buttons/portal_button_down_01.wav"]),
    ("button_up", &["sound/buttons/portal_button_up_01.wav"]),
    ("switch_press", &["sound/buttons/og_switch_press_01.wav"]),
    ("tick", &["sound/ambient/machines/synth_tick_tock_01.wav", "sound/ambient/machines/ticktock1.wav"]),
    ("door_open", &["sound/plats/elevator_door_open_01.wav"]),
    ("door_close", &["sound/plats/elevator_door_close_01.wav"]),
    ("chamber_solved", &["sound/buttons/button_synth_positive_01.wav"]),
    ("chamber_fail", &["sound/buttons/button_synth_negative_01.wav"]),
    ("elevator_chime", &["sound/ambient/alarms/portal_elevator_chime.wav"]),
    ("dropper_open", &["sound/world/dropper_iris_open_01.wav"]),
    ("dropper_close", &["sound/world/dropper_iris_close_01.wav"]),
    ("cube_fizzle", &["sound/npc/turret_floor/turret_fizzler_1.wav"]),
    ("cube_hit1", &["sound/physics/cube/phys_cube_lg_01.wav"]),
    ("cube_hit2", &["sound/physics/cube/phys_cube_lg_02.wav"]),
    ("cube_hit3", &["sound/physics/cube/phys_cube_lg_03.wav"]),
    ("cube_hit4", &["sound/physics/cube/phys_cube_lg_04.wav"]),
    ("cube_bounce1", &["sound/physics/paint/phys_bouncy_cube_lg_01.wav"]),
    ("cube_bounce2", &["sound/physics/paint/phys_bouncy_cube_lg_02.wav"]),
    ("pickup", &["sound/player/object_use_01.wav"]),
    ("hold_loop", &["sound/player/object_use_lp_01.wav", "sound/player/hold_loop.wav"]),
    ("drop", &["sound/player/object_use_stop_01.wav"]),
    ("use_fail", &["sound/player/object_use_failure_01.wav"]),
    ("gel_splat1", &["sound/physics/paint/paint_blob_splat_01.wav"]),
    ("gel_splat2", &["sound/physics/paint/paint_blob_splat_02.wav"]),
    ("gel_splat3", &["sound/physics/paint/paint_blob_splat_03.wav"]),
    ("gel_splat4", &["sound/physics/paint/paint_blob_splat_04.wav"]),
    ("gel_bounce", &["sound/physics/paint/paint_jump_polarity_splat.wav"]),
    ("gel_pipe", &["sound/physics/paint/paint_nozzle_waterfall_short_lp.wav", "sound/physics/paint/paint_nozzle_waterfall_lp.wav"]),
    ("laser_on", &["sound/world/laser_activation_01.wav"]),
    ("laser_hum", &["sound/world/laser_node_lp_01.wav"]),
    ("catcher_on", &["sound/world/laser_node_power_on.wav"]),
    ("catcher_off", &["sound/world/laser_node_power_off.wav"]),
    ("laser_burn1", &["sound/player/pl_burnpain1_no_vo.wav"]),
    ("laser_burn2", &["sound/player/pl_burnpain2_no_vo.wav"]),
    ("bridge_on", &["sound/world/light_power_on_01.wav"]),
    ("bridge_hum", &["sound/ambient/machines/beam_platform_loop1.wav"]),
    ("plate_launch", &["sound/plats/paint_piston_start_01.wav"]),
    ("fling_whoosh", &["sound/player/fling_whoosh.wav"]),
    ("fall_wind", &["sound/player/player_fall_whoosh_lp_01.wav"]),
    ("hard_impact1", &["sound/player/high_velocity_impact_01.wav"]),
    ("hard_impact2", &["sound/player/high_velocity_impact_02.wav"]),
    ("room_hum", &["sound/ambient/machines/fluorescent_hum_1.wav"]),
    // Music (Portal): quiet, in the chambers.
    ("music_chamber1", &["sound/music/portal_procedural_jiggle_bone.mp3"]),
    ("music_chamber2", &["sound/music/portal_subject_name_here.mp3"]),
    ("music_chamber3", &["sound/music/portal_android_hell.mp3"]),
    ("music_chamber4", &["sound/music/portal_taste_of_blood.mp3"]),
];

/// (our name, VTF candidates).
const TEXTURES: &[(&str, &[&str])] = &[
    ("white_panel", &["materials/concrete/concrete_modular_wall001a.vtf"]),
    ("white_floor", &["materials/concrete/concrete_modular_floor001a.vtf"]),
    ("white_ceiling", &["materials/concrete/concrete_modular_ceiling001a.vtf"]),
    ("black_panel", &["materials/metal/metalwall048b.vtf", "materials/metal/black_wall_metal_002a.vtf"]),
    ("black_floor", &["materials/metal/metal_modular_floor001.vtf", "materials/metal/black_floor_metal_001c.vtf"]),
    ("grate", &["materials/metal/metalgrate018.vtf"]),
    ("metal_box", &["materials/models/props/metal_box.vtf"]),
];

/// A model to convert: (our name, which game, MDL path, skin, and a turn about our Z axis in
/// degrees for the few whose upright turn lives somewhere `vmdl` doesn't look).
const MODELS: &[(&str, &str, &str, usize, f32)] = &[
    ("w_portalgun", "portal", "models/weapons/w_portalgun.mdl", 0, 0.0),
    ("v_portalgun", "portal", "models/weapons/v_portalgun.mdl", 0, 0.0),
    // The puzzle pieces.
    ("metal_box", "portal2", "models/props/metal_box.mdl", 0, 0.0),
    ("companion_cube", "portal2", "models/props/metal_box.mdl", 1, 0.0),
    ("reflection_cube", "portal2", "models/props/reflection_cube.mdl", 0, 0.0),
    ("floor_button", "portal2", "models/props/portal_button.mdl", 0, -90.0),
    ("floor_button_on", "portal2", "models/props/portal_button.mdl", 1, -90.0),
    ("cube_button", "portal2", "models/props/box_socket.mdl", 0, 0.0),
    ("cube_button_on", "portal2", "models/props/box_socket.mdl", 1, 0.0),
    ("pedestal_button", "portal2", "models/props/switch001.mdl", 0, 0.0),
    ("box_dropper", "portal2", "models/props/box_dropper.mdl", 0, 0.0),
    ("box_dropper_cover", "portal2", "models/props/box_dropper_cover.mdl", 0, 0.0),
    ("faith_plate", "portal2", "models/props/faith_plate.mdl", 0, 0.0),
    ("bridge_emitter", "portal2", "models/props/wall_emitter.mdl", 0, 0.0),
    ("laser_emitter", "portal2", "models/props/laser_emitter_center.mdl", 0, 0.0),
    ("laser_catcher", "portal2", "models/props/laser_catcher_center.mdl", 0, 0.0),
    ("laser_catcher_on", "portal2", "models/props/laser_catcher_center.mdl", 1, 0.0),
    ("laser_relay", "portal2", "models/props/laser_receptacle.mdl", 0, 0.0),
    ("fizzler_emitter", "portal2", "models/props/fizzler_dynamic.mdl", 0, 0.0),
    ("door_upper", "portal2", "models/props/vert_door/vert_door_upper.mdl", 0, 0.0),
    ("door_lower", "portal2", "models/props/vert_door/vert_door_lower.mdl", 0, 0.0),
    ("door_frame", "portal2", "models/props/vert_door/vert_door_frame.mdl", 0, 0.0),
    ("elevator", "portal", "models/props/round_elevator_body.mdl", 0, 0.0),
    ("sign_frame", "portal2", "models/props/sign_frame02/sign_frame02.mdl", 0, 0.0),
];

/// Animations to bring along: (our model name, [(the MDL's animation, our name for it)]). A
/// model with any is drawn as its `@idle` animation poses it (its bind pose if it has none).
const ANIMATIONS: &[(&str, &[(&str, &str)])] = &[
    // The paddle snapping up to throw, holding, then settling back into its pit.
    ("faith_plate", &[("@angled", "angled"), ("@straightup", "straightup")]),
];

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let out = arg(&args, "--out").unwrap_or_else(|| Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/extracted"));
    let find = |flag: &str, folder: &str| -> Option<PathBuf> {
        arg(&args, flag).or_else(|| steam_dirs().into_iter().map(|d| d.join(folder)).find(|p| p.is_dir()))
    };
    let mut games = Vec::new();
    if let Some(p1) = find("--portal", "Portal") {
        println!("Portal:   {}", p1.display());
        if let Some(g) = Game::open("portal", &p1, &["portal/portal_pak_dir.vpk", "hl2/hl2_sound_misc_dir.vpk", "hl2/hl2_misc_dir.vpk", "hl2/hl2_textures_dir.vpk"], &["portal"]) {
            games.push(g);
        }
    } else {
        println!("Portal not found (pass --portal <dir>)");
    }
    if let Some(p2) = find("--portal2", "Portal 2") {
        println!("Portal 2: {}", p2.display());
        if let Some(g) = Game::open(
            "portal2",
            &p2,
            &["update/pak01_dir.vpk", "portal2_dlc2/pak01_dir.vpk", "portal2_dlc1/pak01_dir.vpk", "portal2/pak01_dir.vpk"],
            &["update", "portal2"],
        ) {
            games.push(g);
        }
    } else {
        println!("Portal 2 not found (pass --portal2 <dir>)");
    }
    if games.is_empty() {
        eprintln!("Nothing to extract from.");
        std::process::exit(1);
    }
    let read_any = |path: &str| games.iter().find_map(|g| g.read(path));
    // `--inspect <mdl path>...`: describe models (parts, bones, size) instead of extracting.
    if let Some(i) = args.iter().position(|a| a == "--inspect") {
        for path in &args[i + 1..] {
            inspect(&games, path);
        }
        return;
    }

    for sub in ["sounds", "textures", "models"] {
        std::fs::create_dir_all(out.join(sub)).expect("create output folder");
    }
    println!("Writing to {}", out.display());

    let mut missing = Vec::new();
    let mut n = 0;
    for (name, candidates) in SOUNDS {
        match candidates.iter().find_map(|c| read_any(c).map(|b| (b, *c))) {
            Some((bytes, path)) => {
                let ext = Path::new(path).extension().and_then(|e| e.to_str()).unwrap_or("wav");
                std::fs::write(out.join("sounds").join(format!("{name}.{ext}")), bytes).expect("write sound");
                n += 1;
            }
            None => missing.push(*name),
        }
    }
    println!("  {n} sounds");

    let mut n = 0;
    for (name, candidates) in TEXTURES {
        let mut done = false;
        for c in *candidates {
            let Some(bytes) = read_any(c) else { continue };
            match decode_vtf(&bytes) {
                Ok(img) => {
                    img.save(out.join("textures").join(format!("{name}.png"))).expect("write texture");
                    done = true;
                    n += 1;
                    break;
                }
                Err(e) => eprintln!("  {c}: {e}"),
            }
        }
        if !done {
            missing.push(*name);
        }
    }
    println!("  {n} textures");

    let mut n = 0;
    for (name, game, path, skin, turn) in MODELS {
        let Some(g) = games.iter().find(|g| g.name == *game).or_else(|| games.first()) else {
            missing.push(*name);
            continue;
        };
        let anims = ANIMATIONS.iter().find(|(n, _)| n == name).map(|(_, a)| *a).unwrap_or_default();
        match convert_model(g, &games, path, *skin, *turn, anims) {
            Ok(bytes) => {
                std::fs::write(out.join("models").join(format!("{name}.glb")), bytes).expect("write model");
                n += 1;
            }
            Err(e) => {
                eprintln!("  {path}: {e}");
                missing.push(*name);
            }
        }
    }
    println!("  {n} models");
    if !missing.is_empty() {
        println!("Not found (the game falls back to stand-ins): {}", missing.join(", "));
    }
}

fn inspect(games: &[Game], mdl_path: &str) {
    let read = |p: &str| games.iter().find_map(|g| g.read(p));
    let stem = mdl_path.trim_end_matches(".mdl");
    let (Some(mdl), Some(vvd), Some(vtx)) = (read(mdl_path), read(&format!("{stem}.vvd")), read(&format!("{stem}.dx90.vtx"))) else {
        println!("{mdl_path}: not found");
        return;
    };
    let model = match (vmdl::Mdl::read(&mdl), vmdl::Vtx::read(&vtx), vmdl::Vvd::read(&vvd)) {
        (Ok(m), Ok(t), Ok(v)) => vmdl::Model::from_parts(m, t, v),
        (m, t, v) => {
            println!("{mdl_path}: unreadable: {:?} {:?} {:?}", m.err(), t.err(), v.err());
            return;
        }
    };
    let (lo, hi) = model.bounding_box();
    let (lo, hi) = (convert_point(lo.into()), convert_point(hi.into()));
    println!("{mdl_path}: {} vertices, bounds {:?} .. {:?} m", model.vertices().len(), lo, hi);
    let mut uses = vec![0usize; model.bones().count()];
    for v in model.vertices() {
        if let Some(b) = v.bone_weights.weights().max_by(|a, b| a.weight.total_cmp(&b.weight)) {
            if let Some(u) = uses.get_mut(b.bone_id as usize) {
                *u += 1;
            }
        }
    }
    let world = bone_transforms(&model);
    let mut lo = glam::Vec3::splat(f32::INFINITY);
    let mut hi = glam::Vec3::splat(f32::NEG_INFINITY);
    for v in model.vertices() {
        let p = glam::Vec3::from(convert_point(root_turn(&model, v.position.into())));
        lo = lo.min(p);
        hi = hi.max(p);
    }
    println!("  posed vertices: {:.3?} .. {:.3?}", lo, hi);
    for (i, b) in model.bones().enumerate() {
        let p = convert_point(root_turn(&model, world[i].to_array()));
        println!("  bone {i} {:?} parent {} at {:.3?} ({} vertices)", b.name, b.parent, p, uses[i]);
    }
    for (i, t) in model.textures().iter().enumerate() {
        println!("  material {i}: {}", t.name);
    }
    println!("  skins: {}", model.skin_tables().count());
    let bones: Vec<&vmdl::mdl::Bone> = model.bones().collect();
    match anim::read(&mdl, &bones) {
        Ok(anims) => {
            for a in &anims {
                println!("  animation {:?}: {} frames at {} fps", a.name, a.frames.len(), a.fps);
            }
        }
        Err(e) => println!("  animations unreadable: {e}"),
    }
}

fn decode_vtf(bytes: &[u8]) -> Result<image::RgbaImage, String> {
    let mut data = bytes.to_vec();
    let vtf = vtf::from_bytes(&mut data).map_err(|e| e.to_string())?;
    let img = vtf.highres_image.decode(0).map_err(|e| e.to_string())?;
    Ok(img.to_rgba8())
}

/// A VMT's `$basetexture` (and whether it's translucent).
fn vmt_base_texture(text: &str) -> Option<String> {
    for line in text.lines() {
        let l = line.trim();
        let lower = l.to_ascii_lowercase();
        if lower.starts_with("\"$basetexture\"") || lower.starts_with("$basetexture") {
            let rest = l[l.to_ascii_lowercase().find("$basetexture").unwrap() + "$basetexture".len()..].trim().trim_start_matches('"').trim();
            let v = rest.trim_matches('"').trim();
            if !v.is_empty() {
                return Some(v.replace('\\', "/").to_ascii_lowercase());
            }
        }
    }
    None
}

/// Source units are inches, +X forward, +Y left, +Z up; ours are metres, +X right, +Y up,
/// -Z forward.
fn convert_point(p: [f32; 3]) -> [f32; 3] {
    const M: f32 = 0.0254;
    [-p[1] * M, p[2] * M, -p[0] * M]
}

/// A material's look, from its VMT: the base texture (following `include`s, for Portal 2's
/// patch materials) and whether it's see-through.
fn read_material(read: &dyn Fn(&str) -> Option<Vec<u8>>, name: &str, search_paths: &[String]) -> glb::Material {
    let mut vmt_text = None;
    for sp in search_paths.iter().map(|s| s.replace('\\', "/").to_ascii_lowercase()).chain(std::iter::once(String::new())) {
        let vmt_path = format!("materials/{}{}.vmt", sp, name).replace("//", "/");
        if let Some(vmt) = read(&vmt_path) {
            vmt_text = Some(String::from_utf8_lossy(&vmt).into_owned());
            break;
        }
    }
    let mut text = vmt_text.unwrap_or_default();
    // A patch material: the look is in the file it includes.
    for _ in 0..3 {
        if vmt_base_texture(&text).is_some() {
            break;
        }
        let Some(inc) = vmt_value(&text, "include") else { break };
        match read(&inc.replace('\\', "/").to_ascii_lowercase()) {
            Some(b) => text = format!("{}\n{}", String::from_utf8_lossy(&b), text),
            None => break,
        }
    }
    let flag = |k: &str| vmt_value(&text, k).is_some_and(|v| v.trim() == "1");
    let mut texture_png = None;
    if let Some(base) = vmt_base_texture(&text) {
        if let Some(vtf) = read(&format!("materials/{base}.vtf")) {
            if let Ok(img) = decode_vtf(&vtf) {
                let mut png = Vec::new();
                if image::DynamicImage::ImageRgba8(img).write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png).is_ok() {
                    texture_png = Some(png);
                }
            }
        }
    }
    glb::Material { texture_png, blend: flag("$translucent"), mask: flag("$alphatest") }
}

/// A VMT key's value (case-insensitive, quoted or not), anywhere in the file.
fn vmt_value(text: &str, key: &str) -> Option<String> {
    for line in text.lines() {
        let l = line.trim();
        let lower = l.to_ascii_lowercase();
        let k = lower.trim_start_matches('"');
        if let Some(rest) = k.strip_prefix(key) {
            let rest = rest.trim_start_matches('"');
            if !rest.starts_with(|c: char| c.is_whitespace()) {
                continue;
            }
            let v = l[l.len() - rest.len()..].trim().trim_matches('"').trim();
            if !v.is_empty() {
                return Some(v.to_string());
            }
        }
    }
    None
}

/// Each bone's place in the model's bind pose (Source space), from its pose-to-bone matrix.
fn bone_transforms(model: &vmdl::Model) -> Vec<glam::Vec3> {
    pose_to_bone(model).iter().map(|m| m.inverse().translation.into()).collect()
}

/// Each bone's pose-to-bone matrix: from the model's bind pose into the bone's own space.
fn pose_to_bone(model: &vmdl::Model) -> Vec<glam::Affine3A> {
    model
        .bones()
        .map(|b| {
            let m: [[f32; 4]; 3] = bytemuck::cast(b.pose_to_bone);
            glam::Affine3A::from_cols_array(&[m[0][0], m[1][0], m[2][0], m[0][1], m[1][1], m[2][1], m[0][2], m[1][2], m[2][2], m[0][3], m[1][3], m[2][3]])
        })
        .collect()
}

/// The model's own turn (a model built Y-up is turned upright by its root bone and its idle
/// animation; Source draws it that way).
fn root_turn(model: &vmdl::Model, p: [f32; 3]) -> [f32; 3] {
    let v = model.apply_root_transform(vmdl::Vector { x: p[0], y: p[1], z: p[2] });
    [v.x, v.y, v.z]
}

/// Convert a model (with skin `skin`) to glTF: a part per bone that moves any of it, and the
/// animations `anims` names ((the MDL's name, ours)) as keyframes on those parts.
fn convert_model(game: &Game, all: &[Game], mdl_path: &str, skin: usize, turn_deg: f32, anims: &[(&str, &str)]) -> Result<Vec<u8>, String> {
    // Source space to ours (see `convert_point`), then the extra turn.
    let to_ours = glam::Quat::from_rotation_z(turn_deg.to_radians()) * glam::Quat::from_mat3(&glam::Mat3::from_cols(glam::Vec3::NEG_Z, glam::Vec3::NEG_X, glam::Vec3::Y));
    let convert = |p: glam::Vec3| to_ours * (p * 0.0254);
    let read = |p: &str| game.read(p).or_else(|| all.iter().find_map(|g| g.read(p)));
    let mdl = read(mdl_path).ok_or("no .mdl")?;
    let stem = mdl_path.trim_end_matches(".mdl");
    let vvd = read(&format!("{stem}.vvd")).ok_or("no .vvd")?;
    let vtx = read(&format!("{stem}.dx90.vtx")).ok_or("no .dx90.vtx")?;
    let model = vmdl::Model::from_parts(
        vmdl::Mdl::read(&mdl).map_err(|e| e.to_string())?,
        vmdl::Vtx::read(&vtx).map_err(|e| e.to_string())?,
        vmdl::Vvd::read(&vvd).map_err(|e| e.to_string())?,
    );
    let names: Vec<String> = model.bones().map(|b| b.name.clone()).collect();
    let bone_count = names.len();
    let vertices = model.vertices();
    let bone_of = |i: usize| -> usize {
        vertices[i].bone_weights.weights().max_by(|a, b| a.weight.total_cmp(&b.weight)).map(|w| w.bone_id as usize).unwrap_or(0).min(bone_count.saturating_sub(1))
    };

    // Where each bone rests, and what takes a bind-pose vertex of its to where it rests. A
    // still model is as its bind pose has it (turned upright the way `vmdl` sees it); one that
    // moves, as its own animations pose it.
    let parents: Vec<i32> = model.bones().map(|b| b.parent).collect();
    let (rest, skinning, clips_src) = if anims.is_empty() {
        let root = |p: glam::Vec3| glam::Vec3::from(root_turn(&model, p.to_array()));
        let rest: Vec<glam::Affine3A> = bone_transforms(&model).into_iter().map(|t| glam::Affine3A::from_translation(root(t))).collect();
        (rest, None, Vec::new())
    } else {
        let bones: Vec<&vmdl::mdl::Bone> = model.bones().collect();
        let mut decoded = anim::read(&mdl, &bones)?;
        let to_bone = pose_to_bone(&model);
        let rest = match decoded.iter().find(|a| a.name.eq_ignore_ascii_case("@idle")) {
            Some(idle) => idle.world(0, &parents),
            None => to_bone.iter().map(|m| m.inverse()).collect(),
        };
        let skinning: Vec<glam::Affine3A> = rest.iter().zip(&to_bone).map(|(r, m)| *r * *m).collect();
        let mut clips = Vec::new();
        for (theirs, ours) in anims {
            let i = decoded.iter().position(|a| a.name.eq_ignore_ascii_case(theirs)).ok_or_else(|| format!("no animation {theirs}"))?;
            clips.push((decoded.swap_remove(i), *ours));
        }
        (rest, Some(skinning), clips)
    };
    let place = |b: usize, p: [f32; 3]| -> glam::Vec3 {
        match &skinning {
            Some(s) => convert(s[b].transform_point3(p.into())),
            None => convert(root_turn(&model, p).into()),
        }
    };
    let place_dir = |b: usize, n: [f32; 3]| -> glam::Vec3 {
        match &skinning {
            Some(s) => to_ours * s[b].transform_vector3(n.into()).normalize_or_zero(),
            None => to_ours * glam::Vec3::from(root_turn(&model, n)),
        }
    };
    let pivot_of = |b: usize| -> glam::Vec3 { convert(rest.get(b).map(|t| t.translation.into()).unwrap_or_default()) };

    let skin_table = model.skin_tables().nth(skin).or_else(|| model.skin_tables().next());
    let mut materials: Vec<glb::Material> = Vec::new();
    let mut material_of: HashMap<i32, usize> = HashMap::new();
    // Parts by bone: (local vertices, global -> local, triangles by material).
    let mut parts: HashMap<usize, (Vec<([f32; 3], [f32; 3], [f32; 2])>, HashMap<usize, u32>, HashMap<usize, Vec<u32>>)> = HashMap::new();
    for mesh in model.meshes() {
        let m = mesh.material_index();
        let mat = match material_of.get(&m) {
            Some(&k) => k,
            None => {
                let info = skin_table.as_ref().and_then(|s| s.texture_info(m));
                let material = match info {
                    Some(info) => read_material(&read, &info.name.replace('\\', "/").to_ascii_lowercase(), &info.search_paths),
                    None => glb::Material { texture_png: None, blend: false, mask: false },
                };
                materials.push(material);
                material_of.insert(m, materials.len() - 1);
                materials.len() - 1
            }
        };
        for strip in mesh.vertex_strip_indices() {
            let idx: Vec<usize> = strip.collect();
            for t in idx.chunks_exact(3) {
                let b = bone_of(t[0]);
                let pivot = pivot_of(b);
                let (verts, remap, tris) = parts.entry(b).or_default();
                // Source winds clockwise.
                for &g in &[t[0], t[2], t[1]] {
                    let local = *remap.entry(g).or_insert_with(|| {
                        let v = &vertices[g];
                        let p = place(b, v.position.into()) - pivot;
                        let n = place_dir(b, v.normal.into());
                        verts.push((p.to_array(), n.to_array(), v.texture_coordinates));
                        (verts.len() - 1) as u32
                    });
                    tris.entry(mat).or_default().push(local);
                }
            }
        }
    }
    let mut keys: Vec<usize> = parts.keys().copied().collect();
    keys.sort();

    // Each clip: every part that moves, placed (relative to the root) at each frame. A part
    // goes wherever its bone goes from where it rests.
    let mut clips = Vec::new();
    for (a, name) in &clips_src {
        let mut tracks: Vec<(usize, Vec<[f32; 3]>, Vec<[f32; 4]>)> = keys.iter().enumerate().map(|(i, _)| (i, Vec::new(), Vec::new())).collect();
        for f in 0..a.frames.len() {
            let world = a.world(f, &parents);
            for (track, &b) in tracks.iter_mut().zip(&keys) {
                let moved = world[b] * rest[b].inverse();
                let (_, r, t) = moved.to_scale_rotation_translation();
                let r = (to_ours * r * to_ours.inverse()).normalize();
                track.1.push((r * pivot_of(b) + convert(t)).to_array());
                track.2.push(r.to_array());
            }
        }
        // Leave out the parts that never move.
        tracks.retain(|(i, ts, rs)| {
            let pivot = pivot_of(keys[*i]);
            ts.iter().any(|t| glam::Vec3::from(*t).distance(pivot) > 1e-4) || rs.iter().any(|r| glam::Quat::from_array(*r).angle_between(glam::Quat::IDENTITY) > 1e-4)
        });
        let times = (0..a.frames.len()).map(|f| f as f32 / a.fps.max(1.0)).collect();
        clips.push(glb::Clip { name: name.to_string(), times, tracks });
    }

    let out: Vec<glb::Part> = keys
        .into_iter()
        .map(|b| {
            let (verts, _, tris) = parts.remove(&b).unwrap();
            let mut prims: Vec<(Vec<u32>, usize)> = tris.into_iter().map(|(m, t)| (t, m)).collect();
            prims.sort_by_key(|p| p.1);
            glb::Part { name: names.get(b).cloned().unwrap_or_else(|| format!("bone{b}")), pivot: pivot_of(b).to_array(), verts, prims }
        })
        .collect();
    Ok(glb::write(&out, &materials, &clips))
}
