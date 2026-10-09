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

/// (our name, which game, MDL path).
const MODELS: &[(&str, &str, &str)] = &[
    ("w_portalgun", "portal", "models/weapons/w_portalgun.mdl"),
    ("v_portalgun", "portal", "models/weapons/v_portalgun.mdl"),
    ("metal_box", "portal", "models/props/metal_box.mdl"),
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
    for (name, game, path) in MODELS {
        let Some(g) = games.iter().find(|g| g.name == *game) else {
            missing.push(*name);
            continue;
        };
        match convert_model(g, &games, path) {
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

fn convert_dir(p: [f32; 3]) -> [f32; 3] {
    [-p[1], p[2], -p[0]]
}

fn convert_model(game: &Game, all: &[Game], mdl_path: &str) -> Result<Vec<u8>, String> {
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
    let verts: Vec<([f32; 3], [f32; 3], [f32; 2])> = model
        .vertices()
        .iter()
        .map(|v| (convert_point(v.position.into()), convert_dir(v.normal.into()), v.texture_coordinates))
        .collect();

    // One primitive per material.
    let skin = model.skin_tables().next();
    let mut by_material: HashMap<i32, Vec<u32>> = HashMap::new();
    for mesh in model.meshes() {
        let entry = by_material.entry(mesh.material_index()).or_default();
        for strip in mesh.vertex_strip_indices() {
            let idx: Vec<u32> = strip.map(|i| i as u32).collect();
            for t in idx.chunks_exact(3) {
                // Source winds clockwise.
                entry.extend_from_slice(&[t[0], t[2], t[1]]);
            }
        }
    }
    let mut prims = Vec::new();
    let mut mats: Vec<i32> = by_material.keys().copied().collect();
    mats.sort();
    for m in mats {
        let tex_info = skin.as_ref().and_then(|s| s.texture_info(m));
        let mut texture = None;
        if let Some(info) = tex_info {
            let name = info.name.replace('\\', "/").to_ascii_lowercase();
            for sp in info.search_paths.iter().map(|s| s.replace('\\', "/").to_ascii_lowercase()).chain(std::iter::once(String::new())) {
                let vmt_path = format!("materials/{}{}.vmt", sp, name).replace("//", "/");
                if let Some(vmt) = read(&vmt_path) {
                    let text = String::from_utf8_lossy(&vmt);
                    if let Some(base) = vmt_base_texture(&text) {
                        if let Some(vtf) = read(&format!("materials/{base}.vtf")) {
                            if let Ok(img) = decode_vtf(&vtf) {
                                let mut png = Vec::new();
                                image::DynamicImage::ImageRgba8(img)
                                    .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
                                    .map_err(|e| e.to_string())?;
                                texture = Some(png);
                            }
                        }
                    }
                    break;
                }
            }
        }
        prims.push(glb::Primitive { indices: by_material.remove(&m).unwrap(), texture_png: texture });
    }
    Ok(glb::write(&verts, &prims))
}
