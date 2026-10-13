//! What the editor's content browser offers, and how each lands where it's dropped.

use bevy::prelude::*;

use aperture::brush::{box_brush, Brush, Contents};
use aperture::doc::{Kind, LevelDoc, Object};
use aperture::{CubeKind, Inputs, Mat, Paint, PortalColor};
use faith_move::Aabb;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Category {
    Geometry,
    Puzzle,
    Light,
    Gel,
    Gameplay,
    Lights,
    Parkour,
    /// Brushes of Portal's materials.
    PortalBrushes,
    /// Portal's entities: logic, triggers, moving brushes.
    PortalLogic,
    /// Portal's sounds, effects and the screen's looks (and decals).
    PortalEffects,
    Props,
}

impl Category {
    pub const ALL: [Category; 11] = [Category::Geometry, Category::Puzzle, Category::Light, Category::Gel, Category::Gameplay, Category::Lights, Category::Parkour, Category::PortalBrushes, Category::PortalLogic, Category::PortalEffects, Category::Props];

    pub fn name(self) -> &'static str {
        match self {
            Category::Geometry => "Geometry",
            Category::Puzzle => "Puzzle",
            Category::Light => "Lasers & bridges",
            Category::Gel => "Gel",
            Category::Gameplay => "Gameplay",
            Category::Lights => "Lights",
            Category::Parkour => "Parkour",
            Category::PortalBrushes => "Portal brushes",
            Category::PortalLogic => "Portal logic",
            Category::PortalEffects => "Portal sound & effects",
            Category::Props => "Props",
        }
    }
}

/// How a piece sits on the surface it's dropped on (`n` is the surface's normal).
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Place {
    /// A box resting on the surface.
    Box,
    /// A box centred on the surface (a cut goes through the wall).
    Through,
    /// A box sunk into the surface, its top flush (goo in a pit).
    Sunk,
    /// A box standing on the floor across the way the camera looks (doors, grills).
    Upright,
    /// A thin box lying on the surface (gel).
    Flat,
    /// The point on the surface, facing away from the camera (buttons, the start).
    Floor,
    /// The point on the surface, facing out of it (emitters, catchers, portals).
    Wall,
    /// This far out from the surface (cubes: their centre).
    Out(f32),
    /// A whole room round the point, its floor there.
    Room,
}

pub struct Entry {
    pub name: &'static str,
    pub category: Category,
    pub place: Place,
    /// A box's size (when it's placed as one).
    pub size: Vec3,
    pub kind: fn() -> Kind,
    /// The material it shows, for the tile.
    pub mat: Option<Mat>,
    /// A colour and a glyph for the tile when it has no material.
    pub color: [u8; 3],
    pub glyph: &'static str,
}

const fn e(name: &'static str, category: Category, place: Place, size: Vec3, kind: fn() -> Kind, color: [u8; 3], glyph: &'static str) -> Entry {
    Entry { name, category, place, size, kind, mat: None, color, glyph }
}

const fn block(name: &'static str, mat: Mat, size: Vec3, kind: fn() -> Kind) -> Entry {
    Entry { name, category: Category::Geometry, place: Place::Box, size, kind, mat: Some(mat), color: [200, 200, 200], glyph: "" }
}

const P: Vec3 = Vec3::ZERO;

/// How many texture widths a metre Portal's textures show at its usual scale (a 512 texel
/// texture across 128 units).
pub const PORTAL_TEXTURE_SCALE: f32 = 1.0 / (128.0 * 0.0254);

/// A box brush of one of Portal's materials (sized where it's placed).
fn brush(mat: &str, contents: Contents, noportal: bool) -> Kind {
    Kind::Brush(box_brush(&Aabb::from_center_size(Vec3::ZERO, Vec3::ONE), mat, contents, PORTAL_TEXTURE_SCALE, noportal))
}

/// One of Portal's point entities, with these keys.
fn point(class: &str, keys: &[(&str, &str)]) -> Kind {
    Kind::Entity { class: class.into(), keys: keys.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect(), brushes: Vec::new() }
}

/// A decal (an overlay) a metre across, facing out of what it's on.
fn decal(mat: &str) -> Kind {
    let h = 0.5;
    Kind::Overlay { mat: mat.into(), corners: [Vec3::new(-h, -h, 0.0), Vec3::new(h, -h, 0.0), Vec3::new(h, h, 0.0), Vec3::new(-h, h, 0.0)], uv: [[1.0, 1.0], [0.0, 1.0], [0.0, 0.0], [1.0, 0.0]], normal: Vec3::NEG_Z }
}

/// One of Portal's brush entities (one box brush of `mat`, sized where it's placed).
fn brushed(class: &str, keys: &[(&str, &str)], mat: &str, contents: Contents) -> Kind {
    let Kind::Brush(b) = brush(mat, contents, false) else { unreachable!() };
    Kind::Entity { class: class.into(), keys: keys.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect(), brushes: vec![b] }
}

pub static CATALOG: &[Entry] = &[
    block("White panels", Mat::White, Vec3::new(2.0, 2.0, 2.0), || Kind::Block { mat: Mat::White }),
    block("Black metal", Mat::Black, Vec3::new(2.0, 2.0, 2.0), || Kind::Block { mat: Mat::Black }),
    block("Glass", Mat::Glass, Vec3::new(3.0, 3.0, 0.1), || Kind::Block { mat: Mat::Glass }),
    block("Frosted glass", Mat::Frosted, Vec3::new(3.0, 3.0, 0.1), || Kind::Block { mat: Mat::Frosted }),
    block("Grating", Mat::Grate, Vec3::new(2.0, 0.1, 2.0), || Kind::Block { mat: Mat::Grate }),
    block("Runner red", Mat::Runner, Vec3::new(2.0, 1.0, 2.0), || Kind::Block { mat: Mat::Runner }),
    block("Crate", Mat::Prop, Vec3::new(1.0, 1.0, 1.0), || Kind::Block { mat: Mat::Prop }),
    block("Rooftop", Mat::Roof, Vec3::new(4.0, 1.0, 4.0), || Kind::Block { mat: Mat::Roof }),
    block("ME wall", Mat::Wall, Vec3::new(4.0, 4.0, 0.5), || Kind::Block { mat: Mat::Wall }),
    block("Concrete", Mat::Concrete, Vec3::new(4.0, 1.0, 4.0), || Kind::Block { mat: Mat::Concrete }),
    e("Player clip", Category::Geometry, Place::Box, Vec3::new(2.0, 3.0, 0.2), || Kind::Block { mat: Mat::Clip }, [90, 160, 220], "CLIP"),
    e("Black stripe", Category::Geometry, Place::Flat, Vec3::new(4.0, 0.02, 0.3), || Kind::Decor { mat: Mat::Black }, [40, 40, 44], "⬛"),
    e("Cut (doorway)", Category::Geometry, Place::Through, Vec3::new(2.0, 3.0, 2.0), || Kind::Cut, [220, 70, 70], "✂"),
    e("Room", Category::Geometry, Place::Room, P, || Kind::Block { mat: Mat::White }, [235, 235, 235], "🔲"),
    e("Floor button", Category::Puzzle, Place::Floor, P, || Kind::FloorButton { sig: 0, cube_only: false }, [200, 60, 50], "🔴"),
    e("Cube button", Category::Puzzle, Place::Floor, P, || Kind::FloorButton { sig: 0, cube_only: true }, [200, 120, 50], "⭕"),
    e("Pedestal button", Category::Puzzle, Place::Floor, P, || Kind::Pedestal { sig: 0, time: 4.0 }, [210, 80, 60], "🔘"),
    e("Storage cube", Category::Puzzle, Place::Out(0.45), P, || Kind::Cube { cube: CubeKind::Storage }, [150, 150, 160], "📦"),
    e("Companion cube", Category::Puzzle, Place::Out(0.45), P, || Kind::Cube { cube: CubeKind::Companion }, [230, 120, 170], "♥"),
    e("Redirection cube", Category::Puzzle, Place::Out(0.45), P, || Kind::Cube { cube: CubeKind::Reflector }, [200, 60, 60], "💎"),
    e("Cube dropper", Category::Puzzle, Place::Out(0.5), P, || Kind::Dropper { cube: CubeKind::Storage, inputs: Inputs::always() }, [120, 120, 130], "⬇"),
    e("Door", Category::Puzzle, Place::Upright, Vec3::new(3.0, 3.0, 0.2), || Kind::Door { inputs: Inputs::always() }, [170, 170, 175], "🚪"),
    e("Emancipation grill", Category::Puzzle, Place::Upright, Vec3::new(3.0, 3.0, 0.05), || Kind::Fizzler { inputs: Inputs::always() }, [90, 170, 230], "〰"),
    e("Faith plate", Category::Puzzle, Place::Floor, P, || Kind::FaithPlate { target: Vec3::ZERO, apex: 2.0, inputs: Inputs::always() }, [230, 200, 60], "⤴"),
    e("Turret", Category::Puzzle, Place::Floor, P, || Kind::Turret, [235, 235, 235], "👁"),
    e("Laser emitter", Category::Light, Place::Wall, P, || Kind::Laser { inputs: Inputs::always() }, [240, 60, 50], "➡"),
    e("Laser catcher", Category::Light, Place::Wall, P, || Kind::Catcher { sig: 0 }, [240, 120, 50], "🎯"),
    e("Laser relay", Category::Light, Place::Floor, P, || Kind::Relay { sig: 0 }, [240, 150, 60], "🔆"),
    e("Light bridge", Category::Light, Place::Wall, P, || Kind::Bridge { inputs: Inputs::always() }, [120, 200, 255], "🌉"),
    e("Pellet launcher", Category::Light, Place::Wall, P, || Kind::PelletLauncher { inputs: Inputs::always(), life: 12.0 }, [255, 170, 60], "💥"),
    e("Pellet catcher", Category::Light, Place::Wall, P, || Kind::PelletCatcher { sig: 0 }, [255, 200, 90], "⏺"),
    e("Bounce gel pipe", Category::Gel, Place::Wall, P, || Kind::GelPipe { paint: Paint::Bounce, inputs: Inputs::always(), rate: 30.0, speed: 7.0 }, [40, 120, 255], "💧"),
    e("Speed gel pipe", Category::Gel, Place::Wall, P, || Kind::GelPipe { paint: Paint::Speed, inputs: Inputs::always(), rate: 30.0, speed: 7.0 }, [255, 140, 30], "💧"),
    e("Portal gel pipe", Category::Gel, Place::Wall, P, || Kind::GelPipe { paint: Paint::Portal, inputs: Inputs::always(), rate: 30.0, speed: 7.0 }, [240, 240, 240], "💧"),
    e("Water pipe", Category::Gel, Place::Wall, P, || Kind::GelPipe { paint: Paint::Water, inputs: Inputs::always(), rate: 30.0, speed: 7.0 }, [120, 200, 230], "💧"),
    e("Bounce gel", Category::Gel, Place::Flat, Vec3::new(2.0, 0.02, 2.0), || Kind::Painted { paint: Paint::Bounce, normal: Vec3::Y }, [40, 120, 255], "⬛"),
    e("Speed gel", Category::Gel, Place::Flat, Vec3::new(2.0, 0.02, 6.0), || Kind::Painted { paint: Paint::Speed, normal: Vec3::Y }, [255, 140, 30], "⬛"),
    e("Portal gel", Category::Gel, Place::Flat, Vec3::new(2.0, 0.02, 3.0), || Kind::Painted { paint: Paint::Portal, normal: Vec3::Y }, [240, 240, 240], "⬛"),
    e("Spawn", Category::Gameplay, Place::Floor, P, || Kind::Spawn { gravity: None }, [80, 220, 120], "🏃"),
    e("Checkpoint", Category::Gameplay, Place::Box, Vec3::new(4.0, 3.0, 4.0), || Kind::Checkpoint { name: "Checkpoint".into(), spawn: Vec3::new(0.0, -1.5, 0.0) }, [80, 200, 220], "⚑"),
    e("Exit", Category::Gameplay, Place::Box, Vec3::new(3.0, 3.0, 3.0), || Kind::Exit, [80, 230, 90], "🏁"),
    e("Hint", Category::Gameplay, Place::Box, Vec3::new(4.0, 3.0, 4.0), || Kind::Hint { text: "A hint".into() }, [180, 120, 230], "?"),
    e("Goo", Category::Gameplay, Place::Sunk, Vec3::new(4.0, 1.0, 4.0), || Kind::Goo, [110, 130, 40], "☠"),
    e("Moving platform", Category::Gameplay, Place::Box, Vec3::new(3.0, 0.5, 3.0), || Kind::Platform { travel: Vec3::new(0.0, 0.0, -6.0), speed: 2.0, inputs: Inputs::always() }, [60, 60, 66], "🔀"),
    e("Point light", Category::Lights, Place::Out(0.5), P, || Kind::PointLight { color: Vec3::new(1.0, 0.95, 0.85), intensity: 300_000.0, range: 15.0, radius: 0.1, shadows: false }, [255, 235, 180], "💡"),
    e("Spot light", Category::Lights, Place::Out(0.15), P, || Kind::SpotLight { color: Vec3::new(1.0, 0.97, 0.92), intensity: 800_000.0, range: 25.0, radius: 0.1, angle: 30.0, soft: 0.3, shadows: false }, [255, 245, 220], "🔦"),
    e("Sun", Category::Lights, Place::Out(3.0), P, || Kind::Sun { color: Vec3::new(1.0, 0.96, 0.9), illuminance: 10_000.0 }, [255, 210, 90], "☀"),
    e("Ceiling lamp", Category::Lights, Place::Out(0.05), P, || Kind::Lamp, [255, 245, 200], "⬜"),
    e("Blue portal", Category::Gameplay, Place::Wall, P, || Kind::Portal { color: PortalColor::Blue }, [40, 140, 255], "⚪"),
    e("Orange portal", Category::Gameplay, Place::Wall, P, || Kind::Portal { color: PortalColor::Orange }, [255, 140, 30], "⚪"),
    e("Indicator wire", Category::Gameplay, Place::Out(0.01), P, || Kind::Wire { sig: 0, points: vec![Vec3::new(0.0, 0.0, -3.0)] }, [90, 160, 255], "⚡"),
    e("Zipline", Category::Parkour, Place::Out(3.0), P, || Kind::ZipLine { to: Vec3::ZERO, sag: 0.0 }, [230, 170, 60], "↘"),
    e("Swing pole", Category::Parkour, Place::Out(2.4), P, || Kind::SwingPole { to: Vec3::ZERO }, [200, 200, 210], "➖"),
    e("Balance beam", Category::Parkour, Place::Out(0.3), P, || Kind::Beam { to: Vec3::ZERO, solid: true }, [60, 60, 66], "➖"),
    e("Ladder", Category::Parkour, Place::Wall, P, || Kind::Ladder { height: 4.0, pipe: false, exit: true }, [200, 60, 50], "⬆"),
    e("Drainpipe", Category::Parkour, Place::Wall, P, || Kind::Ladder { height: 4.0, pipe: true, exit: true }, [150, 150, 160], "⬆"),
    e("Barge door", Category::Parkour, Place::Upright, Vec3::new(1.4, 2.2, 0.1), || Kind::BargeDoor, [170, 120, 80], "🚪"),
    e("Barbed wire", Category::Parkour, Place::Box, Vec3::new(2.0, 0.7, 0.4), || Kind::BarbedWire, [150, 150, 150], "〰"),
    e("Soft landing", Category::Parkour, Place::Box, Vec3::new(3.0, 0.5, 3.0), || Kind::SoftPad { solid: true }, [80, 120, 200], "⬛"),
    e("Portal white wall", Category::PortalBrushes, Place::Box, Vec3::new(3.25, 3.25, 0.4), || brush("plastic/plasticwall001a", Contents::Solid, false), [235, 235, 235], "⬜"),
    e("Portal concrete floor", Category::PortalBrushes, Place::Box, Vec3::new(3.25, 0.4, 3.25), || brush("concrete/concrete_modular_floor001a", Contents::Solid, false), [190, 190, 185], "▭"),
    e("Portal black metal", Category::PortalBrushes, Place::Box, Vec3::new(3.25, 3.25, 0.4), || brush("metal/metalwall048b", Contents::Solid, true), [60, 62, 66], "⬛"),
    e("Portal metal floor", Category::PortalBrushes, Place::Box, Vec3::new(3.25, 0.4, 3.25), || brush("metal/metalwall_bts_005a", Contents::Solid, true), [90, 90, 95], "▭"),
    e("Portal glass", Category::PortalBrushes, Place::Box, Vec3::new(3.25, 3.25, 0.1), || brush("glass/glasswindow_refract01", Contents::Window, true), [150, 210, 230], "▯"),
    e("Portal light panel", Category::PortalBrushes, Place::Box, Vec3::new(1.6, 0.1, 1.6), || brush("lights/white009", Contents::Solid, true), [255, 250, 225], "💡"),
    e("Nodraw", Category::PortalBrushes, Place::Box, Vec3::new(2.0, 2.0, 2.0), || brush("tools/toolsnodraw", Contents::Solid, false), [200, 180, 60], "ND"),
    e("Player clip", Category::PortalBrushes, Place::Box, Vec3::new(2.0, 3.0, 0.2), || brush("tools/toolsplayerclip", Contents::Clip, true), [150, 90, 200], "CLIP"),
    e("Relay", Category::PortalLogic, Place::Out(0.5), P, || point("logic_relay", &[]), [255, 215, 60], "⇢"),
    e("Timer", Category::PortalLogic, Place::Out(0.5), P, || point("logic_timer", &[("RefireTime", "1")]), [255, 215, 60], "⏱"),
    e("Map start (auto)", Category::PortalLogic, Place::Out(0.5), P, || point("logic_auto", &[]), [255, 215, 60], "▶"),
    e("Counter", Category::PortalLogic, Place::Out(0.5), P, || point("math_counter", &[("min", "0"), ("max", "2")]), [255, 215, 60], "#"),
    e("Case", Category::PortalLogic, Place::Out(0.5), P, || point("logic_case", &[]), [255, 215, 60], "⑂"),
    e("Trigger (once)", Category::PortalLogic, Place::Box, Vec3::new(3.0, 3.0, 3.0), || brushed("trigger_once", &[("spawnflags", "1")], "tools/toolstrigger", Contents::Empty), [255, 140, 40], "⬚"),
    e("Trigger", Category::PortalLogic, Place::Box, Vec3::new(3.0, 3.0, 3.0), || brushed("trigger_multiple", &[("spawnflags", "1"), ("wait", "1")], "tools/toolstrigger", Contents::Empty), [255, 140, 40], "⬚"),
    e("Sliding door (brush)", Category::PortalLogic, Place::Upright, Vec3::new(3.25, 3.25, 0.2), || brushed("func_door", &[("movedir", "-90 0 0"), ("speed", "100"), ("wait", "-1")], "metal/metalwall048b", Contents::Solid), [150, 150, 160], "🚪"),
    e("Toggled wall (brush)", Category::PortalLogic, Place::Box, Vec3::new(3.25, 3.25, 0.4), || brushed("func_brush", &[("Solidity", "0")], "metal/metalwall048b", Contents::Solid), [120, 120, 130], "◫"),
    e("GLaDOS scene", Category::PortalLogic, Place::Out(0.5), P, || point("logic_choreographed_scene", &[("SceneFile", "")]), [255, 120, 120], "💬"),
    e("Map light", Category::PortalLogic, Place::Out(0.5), P, || point("light", &[("_light", "255 255 255 200")]), [255, 245, 200], "💡"),
    e("Map spot light", Category::PortalLogic, Place::Out(0.15), P, || point("light_spot", &[("_light", "255 255 255 400"), ("_cone", "45"), ("pitch", "-90")]), [255, 245, 220], "🔦"),
    e("Landmark", Category::PortalLogic, Place::Out(0.5), P, || point("info_landmark", &[]), [80, 220, 120], "⚓"),
    e("Level change", Category::PortalLogic, Place::Box, Vec3::new(3.0, 3.0, 3.0), || brushed("trigger_changelevel", &[("map", ""), ("landmark", "")], "tools/toolstrigger", Contents::Empty), [80, 220, 120], "⇥"),
    e("Portal gun pickup", Category::PortalLogic, Place::Out(0.6), P, || point("weapon_portalgun", &[("CanFirePortal1", "1"), ("CanFirePortal2", "0")]), [240, 240, 240], "🔫"),
    e("Target (point)", Category::PortalLogic, Place::Out(0.5), P, || point("info_target", &[]), [255, 215, 60], "✛"),
    e("Fan (brush)", Category::PortalLogic, Place::Box, Vec3::new(3.0, 0.2, 3.0), || brushed("func_rotating", &[("maxspeed", "180"), ("fanfriction", "20"), ("spawnflags", "17"), ("message", "")], "metal/metalwall048b", Contents::Solid), [150, 150, 160], "🌀"),
    e("Push volume", Category::PortalLogic, Place::Box, Vec3::new(3.0, 3.0, 3.0), || brushed("trigger_push", &[("pushdir", "-90 0 0"), ("speed", "300"), ("spawnflags", "9")], "tools/toolstrigger", Contents::Empty), [120, 200, 255], "⇧"),
    e("Hurt volume", Category::PortalLogic, Place::Box, Vec3::new(3.0, 3.0, 3.0), || brushed("trigger_hurt", &[("damage", "20"), ("spawnflags", "1")], "tools/toolstrigger", Contents::Empty), [220, 60, 50], "✚"),
    e("Portal detector", Category::PortalLogic, Place::Through, Vec3::new(3.0, 3.0, 0.4), || brushed("func_portal_detector", &[], "tools/toolstrigger", Contents::Empty), [90, 170, 255], "◎"),
    e("Portal bumper", Category::PortalLogic, Place::Through, Vec3::new(2.0, 2.0, 0.4), || brushed("func_portal_bumper", &[], "tools/toolsinvisible", Contents::Empty), [200, 140, 255], "⊟"),
    e("No-portal volume", Category::PortalLogic, Place::Through, Vec3::new(3.0, 3.0, 0.4), || brushed("func_noportal_volume", &[], "tools/toolsinvisible", Contents::Empty), [90, 90, 100], "⊘"),
    e("Security camera", Category::PortalLogic, Place::Wall, P, || point("npc_security_camera", &[]), [235, 235, 235], "📷"),
    e("Sound", Category::PortalEffects, Place::Out(0.5), P, || point("ambient_generic", &[("message", ""), ("health", "10"), ("radius", "1250"), ("pitch", "100"), ("spawnflags", "0")]), [120, 220, 160], "🔊"),
    e("Soundscape", Category::PortalEffects, Place::Out(1.5), P, || point("env_soundscape", &[("soundscape", "portal_testchmb.explore_space"), ("radius", "512")]), [120, 200, 220], "🎶"),
    e("Particles", Category::PortalEffects, Place::Out(1.5), P, || point("info_particle_system", &[("effect_name", "elevator_beam"), ("start_active", "1")]), [200, 220, 255], "✨"),
    e("Steam", Category::PortalEffects, Place::Wall, P, || point("env_steam", &[("InitialState", "1"), ("Speed", "120"), ("JetLength", "80"), ("StartSize", "10"), ("EndSize", "25"), ("Rate", "26"), ("rendercolor", "210 210 210"), ("renderamt", "255")]), [220, 220, 225], "♨"),
    e("Sparks", Category::PortalEffects, Place::Wall, P, || point("env_spark", &[("spawnflags", "64"), ("MaxDelay", "2"), ("Magnitude", "2"), ("TrailLength", "1")]), [255, 210, 80], "✴"),
    e("Spotlight beam", Category::PortalEffects, Place::Wall, P, || point("point_spotlight", &[("spawnflags", "3"), ("SpotlightLength", "300"), ("SpotlightWidth", "40"), ("rendercolor", "255 255 255")]), [255, 245, 220], "🔦"),
    e("Beam", Category::PortalEffects, Place::Out(0.5), P, || point("env_beam", &[("LightningStart", ""), ("LightningEnd", ""), ("BoltWidth", "2"), ("rendercolor", "255 255 255"), ("texture", "sprites/laserbeam.spr"), ("spawnflags", "1")]), [255, 120, 90], "〰"),
    e("Screen shake", Category::PortalEffects, Place::Out(0.5), P, || point("env_shake", &[("amplitude", "4"), ("frequency", "2.5"), ("duration", "1"), ("radius", "500")]), [230, 180, 120], "📳"),
    e("Screen fade", Category::PortalEffects, Place::Out(0.5), P, || point("env_fade", &[("duration", "2"), ("holdtime", "0"), ("renderamt", "255"), ("rendercolor", "0 0 0")]), [60, 60, 70], "◐"),
    e("Colour correction", Category::PortalEffects, Place::Out(0.5), P, || point("color_correction", &[("filename", "materials/correction/cc_dim_lights.raw"), ("minfalloff", "-1"), ("maxfalloff", "-1"), ("maxweight", "1"), ("StartDisabled", "1"), ("fadeInDuration", "1"), ("fadeOutDuration", "1")]), [220, 120, 200], "🎨"),
    e("Tonemap controller", Category::PortalEffects, Place::Out(0.5), P, || point("env_tonemap_controller", &[]), [255, 220, 120], "☼"),
    e("Decal", Category::PortalEffects, Place::Wall, P, || decal("decals/decalsigncaution001c"), [230, 200, 60], "⚠"),
];

/// The entry with this name.
pub fn find(name: &str) -> usize {
    CATALOG.iter().position(|e| e.name == name).unwrap_or_else(|| panic!("no catalog entry {name}"))
}

fn snap(v: f32, g: f32) -> f32 {
    if g > 0.0 { (v / g).round() * g } else { v }
}

/// `p` snapped to the grid along the surface (not off it, so it stays on the surface).
pub fn snap_along(p: Vec3, n: Vec3, g: f32) -> Vec3 {
    let mut q = p;
    for i in 0..3 {
        if n[i].abs() < 0.5 {
            q[i] = snap(p[i], g);
        }
    }
    q
}

/// The next signal no source drives yet.
fn free_signal(doc: &LevelDoc) -> usize {
    let mut n = 0;
    for o in &doc.objects {
        if let Some(s) = o.kind.drives() {
            n = n.max(s + 1);
        }
        if let Some(i) = o.kind.inputs() {
            n = n.max(i.all.iter().map(|s| s + 1).max().unwrap_or(0));
        }
    }
    n
}

/// The signal the newest source drives (what a new door or laser most likely wants).
fn newest_source(doc: &LevelDoc) -> Option<usize> {
    doc.objects.iter().rev().find_map(|o| o.kind.drives())
}

/// The objects an entry makes, dropped at `p` on a surface facing `n`, seen from a camera
/// looking along `look`.
pub fn place(entry: &Entry, doc: &LevelDoc, p: Vec3, n: Vec3, look: Vec3, grid: f32) -> Vec<Object> {
    let n = if n.length_squared() > 0.0 { n.normalize() } else { Vec3::Y };
    let p = snap_along(p, n, grid);
    let mut kind = (entry.kind)();
    // Sources get a signal of their own; doors, bridges and platforms listen to the newest
    // source (lasers, grills and droppers are usually on from the start).
    match &mut kind {
        Kind::FloorButton { sig, .. } | Kind::Pedestal { sig, .. } | Kind::Catcher { sig } | Kind::Relay { sig } | Kind::PelletCatcher { sig } => *sig = free_signal(doc),
        Kind::Wire { sig, .. } => *sig = newest_source(doc).unwrap_or(0),
        Kind::Door { inputs } | Kind::Bridge { inputs } | Kind::Platform { inputs, .. } => {
            if let Some(s) = newest_source(doc) {
                *inputs = Inputs::on(s);
            }
        }
        Kind::FaithPlate { target, .. } => {
            let flat = Vec3::new(look.x, 0.0, look.z).normalize_or(Vec3::NEG_Z);
            *target = p + flat * 6.0;
        }
        Kind::Painted { normal, .. } => *normal = axis(n),
        _ => {}
    }
    let mut size = entry.size;
    let half_along = |s: Vec3| (s * n.abs()).length() * 0.5;
    let mut o = match entry.place {
        Place::Box => Object::sized(kind, p + n * half_along(size), size),
        Place::Through => Object::sized(kind, p, size),
        Place::Sunk => Object::sized(kind, p - n * half_along(size), size),
        Place::Upright => {
            // Thin across the way the camera looks.
            if look.x.abs() > look.z.abs() {
                size = Vec3::new(size.z, size.y, size.x);
            }
            Object::sized(kind, p + Vec3::Y * size.y * 0.5, size)
        }
        Place::Flat => {
            // Lying on the surface: thin along its normal.
            let a = axis(n).abs();
            let thin = size.y;
            let flat = Vec3::new(size.x, size.z, size.z);
            let s = if a.y > 0.5 { size } else if a.x > 0.5 { Vec3::new(thin, flat.x, flat.y) } else { Vec3::new(flat.x, flat.y, thin) };
            Object::sized(kind, p + n * (thin * 0.5 + 0.005), s)
        }
        Place::Floor => {
            let mut o = Object::new(kind, p);
            o.face(Vec3::new(look.x, 0.0, look.z));
            o.yaw = snap(o.yaw, 15.0);
            o.pitch = 0.0;
            o
        }
        Place::Wall => {
            let mut o = Object::new(kind, p + n * 0.01);
            o.face(axis(n));
            // A portal on a floor or ceiling: its top away from the camera.
            if n.y.abs() > 0.5 {
                let yaw = if n.y > 0.0 { look.x.atan2(look.z) } else { (-look.x).atan2(-look.z) };
                o.yaw = snap(yaw.to_degrees(), 90.0);
            }
            o
        }
        Place::Out(d) => Object::new(kind, p + n * d),
        Place::Room => return room(p, grid),
    };
    // Lines run the way the camera looks (a swing bar across it), from where it's dropped.
    let flat = Vec3::new(look.x, 0.0, look.z).normalize_or(Vec3::NEG_Z);
    let across = Vec3::Y.cross(flat);
    match &mut o.kind {
        Kind::ZipLine { to, .. } => *to = flat * 10.0 - Vec3::Y * 2.5,
        Kind::SwingPole { to } => {
            *to = across * 3.0;
            o.at -= across * 1.5;
        }
        Kind::Beam { to, .. } => *to = flat * 4.0,
        Kind::BargeDoor => {
            // Faith comes at it from the camera's side.
            o.face(flat);
            o.yaw = snap(o.yaw, 90.0);
        }
        Kind::Ladder { .. } => {
            o.face(Vec3::new(n.x, 0.0, n.z));
            o.pitch = 0.0;
        }
        // A spot light shines out of what it's on; the sun down at a slant, the way the camera
        // looks.
        Kind::SpotLight { .. } => o.face(axis(n)),
        Kind::Sun { .. } => o.face(flat * 0.6 - Vec3::Y),
        _ => {}
    }
    if o.kind.is_box() || o.size != Vec3::ZERO {
        // Its corner on the grid along the surface.
        let lo = o.at - o.size * 0.5;
        let snapped = snap_along(lo, n, grid);
        o.at += snapped - lo;
    }
    // A brush (a brush entity's) as big as the box it was placed as.
    let size = std::mem::take(&mut o.size);
    match &mut o.kind {
        Kind::Brush(b) => *b = sized(b, size),
        Kind::Entity { brushes, .. } if brushes.len() == 1 => brushes[0] = sized(&brushes[0], size),
        _ => o.size = size,
    }
    vec![o]
}

/// A box brush like `b` (its first face's material and texture scale), `size` across.
fn sized(b: &Brush, size: Vec3) -> Brush {
    let f = &b.faces[0];
    box_brush(&Aabb::from_center_size(Vec3::ZERO, size.max(Vec3::splat(0.01))), &f.mat, b.contents, f.u.truncate().length(), f.noportal)
}

/// A white room 10 x 5 x 10 m inside, floor at `p`.
fn room(p: Vec3, grid: f32) -> Vec<Object> {
    let p = Vec3::new(snap(p.x, grid.max(0.5)), p.y, snap(p.z, grid.max(0.5)));
    let (w, h, t) = (10.0, 5.0, 0.5);
    let lo = p - Vec3::new(w / 2.0, 0.0, w / 2.0);
    let hi = p + Vec3::new(w / 2.0, h, w / 2.0);
    let b = |mat: Mat, a: Vec3, c: Vec3| Object::sized(Kind::Block { mat }, (a + c) * 0.5, c - a);
    vec![
        b(Mat::White, Vec3::new(lo.x - t, lo.y - t, lo.z - t), Vec3::new(hi.x + t, lo.y, hi.z + t)),
        b(Mat::Black, Vec3::new(lo.x - t, hi.y, lo.z - t), Vec3::new(hi.x + t, hi.y + t, hi.z + t)),
        b(Mat::White, Vec3::new(lo.x - t, lo.y, lo.z - t), Vec3::new(lo.x, hi.y, hi.z + t)),
        b(Mat::White, Vec3::new(hi.x, lo.y, lo.z - t), Vec3::new(hi.x + t, hi.y, hi.z + t)),
        b(Mat::White, Vec3::new(lo.x, lo.y, lo.z - t), Vec3::new(hi.x, hi.y, lo.z)),
        b(Mat::White, Vec3::new(lo.x, lo.y, hi.z), Vec3::new(hi.x, hi.y, hi.z + t)),
        Object::new(Kind::Lamp, Vec3::new(p.x, hi.y - 0.05, p.z)),
    ]
}

/// The world axis nearest `n`.
pub fn axis(n: Vec3) -> Vec3 {
    let a = n.abs();
    if a.x >= a.y && a.x >= a.z {
        Vec3::X * n.x.signum()
    } else if a.y >= a.z {
        Vec3::Y * n.y.signum()
    } else {
        Vec3::Z * n.z.signum()
    }
}
