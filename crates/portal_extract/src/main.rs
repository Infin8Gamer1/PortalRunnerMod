//! portal-extract: copies what Portal Runner uses out of your own Portal and Portal 2
//! installs into `assets/extracted/` (which git ignores): sounds as they are, textures from
//! VTF to PNG, and models from MDL/VVD/VTX to glTF (.glb, in metres, Y up, facing -Z). With
//! Portal with RTX installed too, textures it remastered come with its physically based maps
//! (`<name>_normal.png`, `<name>_mr.png`, `<name>_emissive.png`) and its base colour.
//!
//! ```text
//! cargo run --release -p portal_extract -- [--portal <dir>] [--portal2 <dir>] [--portal-rtx <dir>] [--out <dir>]
//! ```
//!
//! Without arguments it looks in the usual Steam folders. Nothing it writes is part of the
//! project: run it on your own machine, against your own copies of the games.

mod anim;
mod bsp;
mod glb;
mod maps;
mod remix;
mod scenes;
mod usd;
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
        // (An empty loose file is a placeholder some installs have: the archives have the data.)
        for l in &self.loose {
            if let Ok(b) = std::fs::read(l.join(&path)) {
                if !b.is_empty() {
                    return Some(b);
                }
            }
        }
        self.vpks.iter().find_map(|v| v.read(&path)).filter(|b| !b.is_empty())
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
    // Sentry turrets: their voice, and their guns.
    ("turret_deploy_1", &["sound/npc/turret_floor/turret_deploy_1.wav"]),
    ("turret_deploy_2", &["sound/npc/turret_floor/turret_deploy_2.wav"]),
    ("turret_deploy_3", &["sound/npc/turret_floor/turret_deploy_3.wav"]),
    ("turret_deploy_4", &["sound/npc/turret_floor/turret_deploy_4.wav"]),
    ("turret_deploy_5", &["sound/npc/turret_floor/turret_deploy_5.wav"]),
    ("turret_deploy_6", &["sound/npc/turret_floor/turret_deploy_6.wav"]),
    ("turret_active_1", &["sound/npc/turret_floor/turret_active_1.wav"]),
    ("turret_active_2", &["sound/npc/turret_floor/turret_active_2.wav"]),
    ("turret_active_3", &["sound/npc/turret_floor/turret_active_3.wav"]),
    ("turret_active_4", &["sound/npc/turret_floor/turret_active_4.wav"]),
    ("turret_active_5", &["sound/npc/turret_floor/turret_active_5.wav"]),
    ("turret_active_6", &["sound/npc/turret_floor/turret_active_6.wav"]),
    ("turret_active_7", &["sound/npc/turret_floor/turret_active_7.wav"]),
    ("turret_active_8", &["sound/npc/turret_floor/turret_active_8.wav"]),
    ("turret_search_1", &["sound/npc/turret_floor/turret_search_1.wav"]),
    ("turret_search_2", &["sound/npc/turret_floor/turret_search_2.wav"]),
    ("turret_search_3", &["sound/npc/turret_floor/turret_search_3.wav"]),
    ("turret_search_4", &["sound/npc/turret_floor/turret_search_4.wav"]),
    ("turret_autosearch_1", &["sound/npc/turret_floor/turret_autosearch_1.wav"]),
    ("turret_autosearch_2", &["sound/npc/turret_floor/turret_autosearch_2.wav"]),
    ("turret_autosearch_3", &["sound/npc/turret_floor/turret_autosearch_3.wav"]),
    ("turret_autosearch_4", &["sound/npc/turret_floor/turret_autosearch_4.wav"]),
    ("turret_autosearch_5", &["sound/npc/turret_floor/turret_autosearch_5.wav"]),
    ("turret_autosearch_6", &["sound/npc/turret_floor/turret_autosearch_6.wav"]),
    ("turret_tipped_1", &["sound/npc/turret_floor/turret_tipped_1.wav"]),
    ("turret_tipped_2", &["sound/npc/turret_floor/turret_tipped_2.wav"]),
    ("turret_tipped_3", &["sound/npc/turret_floor/turret_tipped_3.wav"]),
    ("turret_tipped_4", &["sound/npc/turret_floor/turret_tipped_4.wav"]),
    ("turret_tipped_5", &["sound/npc/turret_floor/turret_tipped_5.wav"]),
    ("turret_tipped_6", &["sound/npc/turret_floor/turret_tipped_6.wav"]),
    ("turret_retire_1", &["sound/npc/turret_floor/turret_retire_1.wav"]),
    ("turret_retire_2", &["sound/npc/turret_floor/turret_retire_2.wav"]),
    ("turret_retire_3", &["sound/npc/turret_floor/turret_retire_3.wav"]),
    ("turret_retire_4", &["sound/npc/turret_floor/turret_retire_4.wav"]),
    ("turret_retire_5", &["sound/npc/turret_floor/turret_retire_5.wav"]),
    ("turret_retire_6", &["sound/npc/turret_floor/turret_retire_6.wav"]),
    ("turret_retire_7", &["sound/npc/turret_floor/turret_retire_7.wav"]),
    ("turret_disabled_1", &["sound/npc/turret_floor/turret_disabled_1.wav"]),
    ("turret_disabled_2", &["sound/npc/turret_floor/turret_disabled_2.wav"]),
    ("turret_disabled_3", &["sound/npc/turret_floor/turret_disabled_3.wav"]),
    ("turret_disabled_4", &["sound/npc/turret_floor/turret_disabled_4.wav"]),
    ("turret_disabled_5", &["sound/npc/turret_floor/turret_disabled_5.wav"]),
    ("turret_disabled_6", &["sound/npc/turret_floor/turret_disabled_6.wav"]),
    ("turret_disabled_7", &["sound/npc/turret_floor/turret_disabled_7.wav"]),
    ("turret_disabled_8", &["sound/npc/turret_floor/turret_disabled_8.wav"]),
    ("turret_pickup_1", &["sound/npc/turret_floor/turret_pickup_1.wav"]),
    ("turret_pickup_2", &["sound/npc/turret_floor/turret_pickup_2.wav"]),
    ("turret_pickup_3", &["sound/npc/turret_floor/turret_pickup_3.wav"]),
    ("turret_pickup_4", &["sound/npc/turret_floor/turret_pickup_4.wav"]),
    ("turret_pickup_5", &["sound/npc/turret_floor/turret_pickup_5.wav"]),
    ("turret_pickup_6", &["sound/npc/turret_floor/turret_pickup_6.wav"]),
    ("turret_pickup_7", &["sound/npc/turret_floor/turret_pickup_7.wav"]),
    ("turret_pickup_8", &["sound/npc/turret_floor/turret_pickup_8.wav"]),
    ("turret_pickup_9", &["sound/npc/turret_floor/turret_pickup_9.wav"]),
    ("turret_pickup_10", &["sound/npc/turret_floor/turret_pickup_10.wav"]),
    ("turret_shotat_1", &["sound/npc/turret_floor/turret_shotat_1.wav"]),
    ("turret_shotat_2", &["sound/npc/turret_floor/turret_shotat_2.wav"]),
    ("turret_shotat_3", &["sound/npc/turret_floor/turret_shotat_3.wav"]),
    ("turret_collide_1", &["sound/npc/turret_floor/turret_collide_1.wav"]),
    ("turret_collide_2", &["sound/npc/turret_floor/turret_collide_2.wav"]),
    ("turret_collide_3", &["sound/npc/turret_floor/turret_collide_3.wav"]),
    ("turret_collide_4", &["sound/npc/turret_floor/turret_collide_4.wav"]),
    ("turret_collide_5", &["sound/npc/turret_floor/turret_collide_5.wav"]),
    ("turret_shoot_1", &["sound/npc/turret_floor/shoot1.wav"]),
    ("turret_shoot_2", &["sound/npc/turret_floor/shoot2.wav"]),
    ("turret_shoot_3", &["sound/npc/turret_floor/shoot3.wav"]),
    ("turret_die", &["sound/npc/turret_floor/die.wav"]),
    ("turret_ping", &["sound/npc/turret_floor/ping.wav"]),
    // High energy pellets (Portal's are Half-Life 2's energy balls).
    ("pellet_bounce", &["sound/weapons/physcannon/energy_bounce1.wav"]),
    ("pellet_bounce2", &["sound/weapons/physcannon/energy_bounce2.wav"]),
    ("pellet_gone", &["sound/weapons/physcannon/energy_disintegrate4.wav"]),
    ("pellet_launch", &["sound/weapons/physcannon/energy_sing_flyby1.wav"]),
    ("pellet_caught", &["sound/weapons/physcannon/energy_sing_explosion2.wav"]),
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
    // Deadly goo (Portal's `nature/hazard_liquid`; Portal with RTX's toxic water).
    ("goo", &["materials/nature/hazard_liquid.vtf"]),
    // Observation windows: frosted glass, and clear glass's ripples.
    ("glass_frosted", &["materials/glass/glasswindow_frosted.vtf"]),
    ("glass_frosted_002", &["materials/glass/glasswindow_frosted_002.vtf"]),
    ("glass_frosted_003", &["materials/glass/glasswindow_frosted_003.vtf"]),
    ("glass_normal", &["materials/glass/glass_normal.vtf"]),
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
    // High energy pellets' launcher and catcher (Portal's).
    ("pellet_launcher", "portal", "models/props/combine_ball_launcher.mdl", 0, 0.0),
    ("pellet_catcher", "portal", "models/props/combine_ball_catcher.mdl", 0, 0.0),
    // A sentry turret (Portal's).
    ("turret", "portal", "models/props/turret_01.mdl", 0, 0.0),
    ("door_upper", "portal2", "models/props/vert_door/vert_door_upper.mdl", 0, 0.0),
    ("door_lower", "portal2", "models/props/vert_door/vert_door_lower.mdl", 0, 0.0),
    ("door_frame", "portal2", "models/props/vert_door/vert_door_frame.mdl", 0, 0.0),
    ("elevator", "portal", "models/props/round_elevator_body.mdl", 0, 0.0),
    ("sign_frame", "portal2", "models/props/sign_frame02/sign_frame02.mdl", 0, 0.0),
];

/// Portal with RTX's remastered models: (our name, USD files under `gameReadyAssets` (each with
/// turns in degrees about X then Z, in Source space, lining it up with the model it replaces),
/// and what to call each part by the joint or mesh it comes from; parts not listed are left
/// out). Each is written as `<name>.glb` beside the Portal and Portal 2 models; the game
/// prefers it. (Remix's captures of skinned models come a quarter turn round about Z, and some
/// meshes were built lying on their side.)
type RtxModel = (&'static str, &'static [(&'static str, f32, f32)], &'static [(&'static str, &'static str)]);
const RTX_MODELS: &[RtxModel] = &[
    // The Weighted Storage Cube.
    ("rtx_storage_cube", &[("SubUSDs/SM_Prop_CompanionCube_A1_01.usd", 0.0, 0.0)], &[("SM_Prop_CompanionCube_A1_01", "body")]),
    // The pedestal button: its base, and the button that goes down.
    ("rtx_pedestal_button", &[("skinned/switch001.usd", 0.0, 90.0)], &[("Root", "Root"), ("Button", "Button")]),
    // The floor button: its base, and the pad a cube or Faith presses down.
    (
        "rtx_floor_button",
        &[("SubUSDs/SM_Fixture_Superbutton_Base.usd", 0.0, 0.0), ("SubUSDs/SM_Fixture_Superbutton_Pad.usd", 90.0, 0.0)],
        &[("SM_Fixture_Superbutton_Base", "base"), ("SM_Fixture_Superbutton_Pad", "button_top")],
    ),
    // The dropper's iris: four leaves that slide apart, and the arms that work them.
    (
        "rtx_dropper_cover",
        &[("skinned/box_dropper_cover.usd", 0.0, 90.0)],
        &[
            ("Cover_1", "Cover_1"),
            ("Cover_2", "Cover_2"),
            ("Cover_3", "Cover_3"),
            ("Cover_4", "Cover_4"),
            ("Scene_Root", "arms"),
            ("Root", "arms"),
            ("Arm_1_Slider", "arms"),
            ("Arm_1_Rot1", "arms"),
            ("Arm_1_Rot2", "arms"),
            ("Arm_1_Rot3", "arms"),
            ("Arm_2_Slider", "arms"),
            ("Arm_2_Rot1", "arms"),
            ("Arm_2_Rot2", "arms"),
            ("Arm_2_Rot3", "arms"),
            ("Arm_3_Slider", "arms"),
            ("Arm_3_Rot1", "arms"),
            ("Arm_3_Rot2", "arms"),
            ("Arm_3_Rot3", "arms"),
            ("Arm_4_Slider", "arms"),
            ("Arm_4_Rot1", "arms"),
            ("Arm_4_Rot2", "arms"),
            ("Arm_4_Rot3", "arms"),
        ],
    ),
    // An emancipation grill's emitter post (Portal's grills are level geometry; this is the
    // remaster of their edge), with its glass strip. (Its lip, a separate file, is left out.)
    (
        "rtx_fizzler_emitter",
        &[("SubUSDs/SM_Fixture_FizzleGates_A1_01.usd", 0.0, 0.0)],
        &[("SM_Fixture_FizzleGates_A1", "post"), ("SM_Fixture_FizzleGates_A1_Glass", "post")],
    ),
    // Portal's round chamber door: the frame round the doorway (built lying down), and two
    // leaves that slide apart.
    (
        "rtx_door",
        &[
            ("skinned/door_01_frame_reference.usd", -90.0, 0.0),
            ("skinned/door_01_lftdoor_reference.usd", 0.0, 90.0),
            ("skinned/door_01_rtdoor_reference.usd", 0.0, 90.0),
        ],
        &[("Door_01_Frame_wide_reference_005", "frame"), ("SM_Fixture_RoundDoor_B", "door_left"), ("SM_Fixture_RoundDoor_A", "door_right")],
    ),
];

/// Portal's maps to bring along: the test chambers, in order, and the escape.
const MAPS: &[&str] = &[
    "testchmb_a_00",
    "testchmb_a_01",
    "testchmb_a_02",
    "testchmb_a_03",
    "testchmb_a_04",
    "testchmb_a_05",
    "testchmb_a_06",
    "testchmb_a_07",
    "testchmb_a_08",
    "testchmb_a_09",
    "testchmb_a_10",
    "testchmb_a_11",
    "testchmb_a_13",
    "testchmb_a_14",
    "testchmb_a_15",
    "escape_00",
    "escape_01",
    "escape_02",
];

/// Animations to bring along: (our model name, [(the MDL's animation, our name for it)]). A
/// model with any is drawn as its `@idle` pose, or `@idle_closed` when it has no `@idle`
/// (the pellet launcher). With neither, it keeps its bind pose.
const ANIMATIONS: &[(&str, &[(&str, &str)])] = &[
    // The paddle snapping up to throw, holding, then settling back into its pit.
    ("faith_plate", &[("@angled", "angled"), ("@straightup", "straightup")]),
    // (Its bind pose has it hanging upside down: its idle stands it up.)
    ("turret", &[("@idle", "idle")]),
    // Rest pose is `@idle_closed` (see `convert_model`): the bind pose isn't the closed mouth.
    ("pellet_launcher", &[("@idle_closed", "idle_closed"), ("@open", "open"), ("@close", "close"), ("@idle_open", "idle_open")]),
];

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let out = arg(&args, "--out").unwrap_or_else(|| Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/extracted"));
    let find = |flag: &str, folder: &str| -> Option<PathBuf> {
        arg(&args, flag).or_else(|| steam_dirs().into_iter().map(|d| d.join(folder)).find(|p| p.is_dir()))
    };
    let mut games = Vec::new();
    let rtx_dir = find("--portal-rtx", "PortalRTX");
    // Where Portal's maps are (loose BSPs).
    let mut maps_dir = None;
    if let Some(p1) = find("--portal", "Portal") {
        maps_dir = Some(p1.join("portal/maps"));
        println!("Portal:   {}", p1.display());
        if let Some(g) = Game::open("portal", &p1, &["portal/portal_pak_dir.vpk", "hl2/hl2_sound_misc_dir.vpk", "hl2/hl2_misc_dir.vpk", "hl2/hl2_textures_dir.vpk"], &["portal"]) {
            games.push(g);
        }
    } else if let Some(g) = rtx_dir.as_deref().and_then(|d| {
        // Portal with RTX carries Portal's own files too.
        Game::open("portal", d, &["portal_rtx/portal_pak_dir.vpk", "hl2/hl2_sound_misc_dir.vpk", "hl2/hl2_misc_dir.vpk", "hl2/hl2_textures_dir.vpk"], &["portal_rtx"])
    }) {
        println!("Portal:   (Portal with RTX's copy)");
        maps_dir = rtx_dir.as_ref().map(|d| d.join("portal_rtx/maps"));
        games.push(g);
    } else {
        println!("Portal not found (pass --portal <dir>)");
    }
    let remix = rtx_dir.as_deref().and_then(remix::Remix::open);
    match (&rtx_dir, &remix) {
        (Some(d), Some(_)) => println!("Portal with RTX: {}", d.display()),
        (Some(d), None) => println!("Portal with RTX: {} (no rtx-remix/mods/gameReadyAssets there)", d.display()),
        (None, _) => println!("Portal with RTX not found (pass --portal-rtx <dir>): textures without physically based maps"),
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
    // `--attachment <mdl path> <name>`: where a model's attachment is (Source's space).
    if let Some(i) = args.iter().position(|a| a == "--attachment") {
        let (Some(path), Some(name)) = (args.get(i + 1), args.get(i + 2)) else { return };
        println!("{path} {name}: {:?}", read_any(path).and_then(|m| attachment_point(&m, name)));
        return;
    }
    // `--inspect <mdl path>...`: describe models (parts, bones, size) instead of extracting.
    if let Some(i) = args.iter().position(|a| a == "--inspect") {
        for path in &args[i + 1..] {
            inspect(&games, path);
        }
        return;
    }
    // One model, without rewriting the other extracted files. The pellet launcher's bind
    // pose isn't its closed mouth; `--only-launchers` writes it resting on `@idle_closed`.
    if args.iter().any(|a| a == "--only-launchers") {
        let Some(g) = games.iter().find(|g| g.name == "portal") else {
            eprintln!("Portal's files weren't found");
            std::process::exit(1);
        };
        let mut want: Vec<(String, String)> = ANIMATIONS
            .iter()
            .find(|(n, _)| *n == "pellet_launcher")
            .map(|(_, a)| a.iter().map(|(t, o)| (t.to_string(), o.to_string())).collect())
            .unwrap_or_default();
        let bytes = loop {
            let pairs: Vec<(&str, &str)> = want.iter().map(|(t, o)| (t.as_str(), o.as_str())).collect();
            match convert_model(g, &games, "models/props/combine_ball_launcher.mdl", 0, 0.0, &pairs) {
                Err(e) if e.starts_with("no animation ") && !want.is_empty() => {
                    let missing = e.trim_start_matches("no animation ").to_string();
                    let k = want.iter().position(|(t, _)| *t == missing).unwrap();
                    if missing.starts_with('@') {
                        want.remove(k);
                    } else {
                        want[k].0 = format!("@{missing}");
                    }
                }
                r => break r,
            }
        };
        let bytes = bytes.unwrap_or_else(|e| {
            eprintln!("launcher: {e}");
            std::process::exit(1);
        });
        for rel in ["models/pellet_launcher.glb", "models/map/props/combine_ball_launcher.glb"] {
            let file = out.join(rel);
            std::fs::create_dir_all(file.parent().unwrap()).expect("create model dir");
            std::fs::write(&file, &bytes).expect("write launcher");
            println!("  {}", file.display());
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

    let (mut n, mut pbr) = (0, 0);
    for (name, candidates) in TEXTURES {
        let mut done = false;
        let dir = out.join("textures");
        for suffix in PBR_SUFFIXES {
            let _ = std::fs::remove_file(dir.join(format!("{name}{suffix}.png")));
        }
        for c in *candidates {
            let Some(bytes) = read_any(c) else { continue };
            match decode_vtf(&bytes) {
                Ok(img) => {
                    img.save(dir.join(format!("{name}.png"))).expect("write texture");
                    done = true;
                    n += 1;
                    // (Not glass: Portal with RTX's is a translucent shader, without the panes' alpha.)
                    let material = remix.as_ref().filter(|_| !name.starts_with("glass")).zip(remix::vtf_hash(&bytes)).and_then(|(r, h)| Some((r, r.material(h)?)));
                    if let Some((r, m)) = material {
                        match write_pbr(r, &m, &dir, name) {
                            Ok(()) => pbr += 1,
                            Err(e) => eprintln!("  {name}: Portal with RTX's maps: {e}"),
                        }
                    }
                    break;
                }
                Err(e) => eprintln!("  {c}: {e}"),
            }
        }
        if !done {
            missing.push(*name);
        }
    }
    println!("  {n} textures ({pbr} with Portal with RTX's maps)");

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
    if let Some(r) = &remix {
        let mut n = 0;
        let to_ours = glam::Quat::from_mat3(&glam::Mat3::from_cols(glam::Vec3::NEG_Z, glam::Vec3::NEG_X, glam::Vec3::Y));
        for (name, files, parts) in RTX_MODELS {
            let convert = |p: glam::Vec3| to_ours * (p * 0.0254);
            let rename = |joint: &str| parts.iter().find(|(j, _)| *j == joint).map(|(_, ours)| ours.to_string());
            match usd::convert(r, files, &convert, to_ours, &rename) {
                Ok((parts, materials)) if !parts.is_empty() => {
                    std::fs::write(out.join("models").join(format!("{name}.glb")), glb::write(&parts, &materials, &[])).expect("write model");
                    n += 1;
                }
                Ok(_) => eprintln!("  {name}: no parts"),
                Err(e) => eprintln!("  {name}: {e}"),
            }
        }
        println!("  {n} of Portal with RTX's models");
    } else {
        for (name, ..) in RTX_MODELS {
            let _ = std::fs::remove_file(out.join("models").join(format!("{name}.glb")));
        }
    }
    // Portal's own test chambers (`--no-maps` skips them).
    if let Some(dir) = maps_dir.filter(|d| d.is_dir() && !args.iter().any(|a| a == "--no-maps")) {
        let (mut n, mut mats) = (0, 0);
        let mut models: Vec<maps::UsedModel> = Vec::new();
        for name in MAPS {
            let Ok(bytes) = std::fs::read(dir.join(format!("{name}.bsp"))) else { continue };
            match maps::extract(name, &bytes, &read_any, remix.as_ref(), &out) {
                Ok((_, _, m, used)) => {
                    n += 1;
                    mats += m;
                    for (model, skin, anims) in used {
                        match models.iter_mut().find(|(m, s, _)| *m == model && *s == skin) {
                            Some((_, _, list)) => list.extend(anims.into_iter().filter(|a| !list.contains(a)).collect::<Vec<_>>()),
                            None => models.push((model, skin, anims)),
                        }
                    }
                }
                Err(e) => eprintln!("  {name}: {e}"),
            }
        }
        println!("  {n} of Portal's maps ({mats} materials)");
        // Their props' models, once each (kept from an earlier run unless they animate, or with
        // `--props`), with
        // the animations the maps play on them (under the sequence's name, or `@` and it).
        let (mut made, mut failed, mut animated) = (0, 0, 0);
        if let Some(g) = games.iter().find(|g| g.name == "portal") {
            for (model, skin, anims) in &models {
                let file = out.join("models").join(format!("{}.glb", maps::prop_glb(model, *skin)));
                if file.exists() && anims.is_empty() && !args.iter().any(|a| a == "--props") {
                    continue;
                }
                // Posed by its idle where it has one (some models' bind pose isn't how they
                // stand: Portal's turret hangs upside down in its).
                let mut want: Vec<(String, String)> = anims.iter().map(|a| (a.clone(), a.clone())).collect();
                if !want.iter().any(|(t, _)| t == "idle") {
                    want.push(("@idle".into(), "idle".into()));
                }
                // The launcher's mouth: the maps only play `close`, and it rests closed.
                if model.contains("combine_ball_launcher") {
                    for a in ["open", "close", "idle_closed", "idle_open"] {
                        if !want.iter().any(|(_, o)| o == a) {
                            want.push((format!("@{a}"), a.to_string()));
                        }
                    }
                }
                let bytes = loop {
                    let pairs: Vec<(&str, &str)> = want.iter().map(|(t, o)| (t.as_str(), o.as_str())).collect();
                    match convert_model(g, &games, model, *skin as usize, 0.0, &pairs) {
                        Err(e) if e.starts_with("no animation ") => {
                            let missing = e.trim_start_matches("no animation ").to_string();
                            let k = want.iter().position(|(t, _)| *t == missing).unwrap();
                            if missing.starts_with('@') {
                                want.remove(k);
                            } else {
                                want[k].0 = format!("@{missing}");
                            }
                        }
                        // Animations it can't read: still, then.
                        Err(_) if !want.is_empty() => want.clear(),
                        r => break r,
                    }
                };
                match bytes {
                    Ok(bytes) => {
                        let _ = std::fs::create_dir_all(file.parent().unwrap());
                        std::fs::write(&file, bytes).expect("write model");
                        made += 1;
                        animated += usize::from(!want.is_empty());
                    }
                    Err(e) => {
                        eprintln!("    {model}: {e}");
                        failed += 1;
                    }
                }
            }
        }
        println!("  {} prop models in them ({made} written, {animated} with animations, {failed} not converted)", models.len());
        // GLaDOS's lines: the scenes the maps play, and the sounds they speak.
        let image = read_any("scenes/scenes.image");
        let mut waves = HashMap::new();
        for script in ["scripts/npc_sounds_aperture_ai.txt", "scripts/game_sounds_portal.txt", "scripts/npc_sounds_glados_cores.txt", "scripts/level_sounds_escape.txt", "scripts/npc_sounds_turret_portal.txt"] {
            if let Some(t) = read_any(script) {
                for (k, v) in scenes::soundscripts(&String::from_utf8_lossy(&t)) {
                    waves.entry(k).or_insert(v);
                }
            }
        }
        match image.as_deref().and_then(scenes::Image::parse) {
            Some(img) => {
                let mut files: Vec<String> = Vec::new();
                for name in MAPS {
                    let Ok(text) = std::fs::read_to_string(out.join("maps").join(format!("{name}.ents"))) else { continue };
                    for e in bsp::parse_entities(&text) {
                        if let Some(f) = bsp::key(&e, "SceneFile") {
                            let f = f.replace('\\', "/").to_ascii_lowercase();
                            if !files.contains(&f) {
                                files.push(f);
                            }
                        }
                    }
                }
                let (mut table, mut lines) = (String::new(), 0);
                for f in &files {
                    let Some(scene) = img.scene(f) else { continue };
                    let mut said = Vec::new();
                    for (start, script) in &scene.lines {
                        let Some(wave) = waves.get(&script.to_ascii_lowercase()) else { continue };
                        let stem = Path::new(wave).file_stem().and_then(|s| s.to_str()).unwrap_or("line");
                        let ours = format!("vo_{stem}");
                        let file = out.join("sounds").join(format!("{ours}.wav"));
                        // (Written again if an earlier run left it empty.)
                        if std::fs::metadata(&file).map_or(true, |m| m.len() == 0) {
                            match read_any(&format!("sound/{wave}")) {
                                Some(bytes) => std::fs::write(&file, bytes).expect("write sound"),
                                None => {
                                    let _ = std::fs::remove_file(&file);
                                    continue;
                                }
                            }
                        }
                        said.push(format!("{start}:{ours}"));
                        lines += 1;
                    }
                    let fired: Vec<String> = scene.triggers.iter().map(|(at, n)| format!("{at}:{n}")).collect();
                    table.push_str(&format!("{f}|{}|{}|{}\n", scene.length, said.join(";"), fired.join(";")));
                }
                std::fs::write(out.join("maps").join("scenes.txt"), table).expect("write scenes");
                println!("  {} of GLaDOS's scenes ({lines} lines)", files.len());
            }
            None => println!("  no scenes.image: GLaDOS stays quiet"),
        }
    }
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
    let mut bone_bounds = vec![(glam::Vec3::splat(f32::INFINITY), glam::Vec3::splat(f32::NEG_INFINITY)); uses.len()];
    for v in model.vertices() {
        if let Some(b) = v.bone_weights.weights().max_by(|a, b| a.weight.total_cmp(&b.weight)).and_then(|b| bone_bounds.get_mut(b.bone_id as usize)) {
            let p = glam::Vec3::from(convert_point(root_turn(&model, v.position.into())));
            *b = (b.0.min(p), b.1.max(p));
        }
    }
    for (i, b) in model.bones().enumerate() {
        let p = convert_point(root_turn(&model, world[i].to_array()));
        let (lo, hi) = bone_bounds[i];
        let span = if uses[i] > 0 { format!(", {lo:.3} .. {hi:.3}") } else { String::new() };
        println!("  bone {i} {:?} parent {} at {:.3?} ({} vertices{span})", b.name, b.parent, p, uses[i]);
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

/// The maps written beside a texture when Portal with RTX remastered it.
const PBR_SUFFIXES: [&str; 3] = ["_normal", "_mr", "_emissive"];

/// The largest a remastered map is kept (they come at up to 4096 square).
const PBR_MAX: u32 = 2048;

/// Write a Portal with RTX material's maps for texture `name`: its base colour over
/// `<name>.png`, then `<name>_normal.png`, `<name>_mr.png` and `<name>_emissive.png`.
fn write_pbr(r: &remix::Remix, m: &remix::Material, dir: &Path, name: &str) -> Result<(), String> {
    let baked = r.bake(m, PBR_MAX)?;
    for (img, suffix) in [(&baked.albedo, ""), (&baked.normal, "_normal"), (&baked.metallic_roughness, "_mr"), (&baked.emissive, "_emissive")] {
        if let Some(img) = img {
            img.save(dir.join(format!("{name}{suffix}.png"))).map_err(|e| e.to_string())?;
        }
    }
    Ok(())
}

fn decode_vtf(bytes: &[u8]) -> Result<image::RgbaImage, String> {
    decode_vtf_frame(bytes, 0)
}

fn decode_vtf_frame(bytes: &[u8], frame: u32) -> Result<image::RgbaImage, String> {
    let mut data = bytes.to_vec();
    let vtf = vtf::from_bytes(&mut data).map_err(|e| e.to_string())?;
    let img = vtf.highres_image.decode(frame).map_err(|e| e.to_string())?;
    Ok(img.to_rgba8())
}

/// How many frames a VTF has (an animated or switched texture has more than one).
fn vtf_frames(bytes: &[u8]) -> u32 {
    match bytes.get(24..26) {
        Some(b) if bytes.starts_with(b"VTF\0") => u16::from_le_bytes([b[0], b[1]]).max(1) as u32,
        _ => 1,
    }
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
    glb::Material { texture_png, blend: flag("$translucent"), mask: flag("$alphatest"), ..Default::default() }
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

/// Where a model's attachment (`ball_attachment` on Portal's pellet catcher) is in its bind
/// pose, in its own space (Source's axes and units).
pub fn attachment_point(mdl: &[u8], name: &str) -> Option<glam::Vec3> {
    let mdl = vmdl::Mdl::read(mdl).ok()?;
    let a = mdl.attachments.iter().find(|a| a.name.eq_ignore_ascii_case(name))?;
    let bone = mdl.bones.get(usize::try_from(a.local_bone).ok()?)?;
    let m: [[f32; 4]; 3] = bytemuck::cast(bone.pose_to_bone);
    let to_bone = glam::Affine3A::from_cols_array(&[m[0][0], m[1][0], m[2][0], m[0][1], m[1][1], m[2][1], m[0][2], m[1][2], m[2][2], m[0][3], m[1][3], m[2][3]]);
    let l: [[f32; 4]; 3] = bytemuck::cast(a.local);
    Some(to_bone.inverse().transform_point3(glam::Vec3::new(l[0][3], l[1][3], l[2][3])))
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
        // `@idle` where the model has one (a turret's bind pose hangs upside down). A pellet
        // launcher has no `@idle`; `@idle_closed` is how it rests. Anything else keeps the
        // bind pose, so this doesn't move models that already posed from `@idle`.
        let rest = if let Some(idle) = decoded.iter().find(|a| a.name.eq_ignore_ascii_case("@idle")) {
            idle.world(0, &parents)
        } else if let Some(closed) = decoded.iter().find(|a| a.name.eq_ignore_ascii_case("@idle_closed")) {
            closed.world(0, &parents)
        } else {
            to_bone.iter().map(|m| m.inverse()).collect()
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
                    None => glb::Material::default(),
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
