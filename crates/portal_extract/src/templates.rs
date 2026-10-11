//! Entity templates a map spawns as it starts: an `env_entity_maker` makes a copy of a
//! `point_template`'s entities where the maker is, turned as it's turned (Portal's pellet
//! catchers are built this way, out of sight, and put in place as the map loads). Those
//! entities are moved there here, so the map comes out as it plays. Templates of cubes are
//! left alone (the game's cube makers make those as they're asked to), as are ones made
//! later on, or by more than one maker.

use std::collections::HashMap;

use glam::{Mat3, Vec3};

use crate::bsp::{key, vec3_key, Entity};

/// Where a template's entities go: from about the template's origin to about the maker's,
/// turned (all in Source space).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Place {
    pub from: Vec3,
    pub to: Vec3,
    pub turn: Mat3,
}

impl Place {
    pub fn point(&self, p: Vec3) -> Vec3 {
        self.to + self.turn * (p - self.from)
    }
}

/// Source's angles (pitch, yaw, roll; degrees) as a rotation: its columns are forward, left
/// and up.
pub fn angles_to_mat(a: Vec3) -> Mat3 {
    let (p, y, r) = (a.x.to_radians(), a.y.to_radians(), a.z.to_radians());
    Mat3::from_rotation_z(y) * Mat3::from_rotation_y(p) * Mat3::from_rotation_x(r)
}

/// And back.
pub fn mat_to_angles(m: Mat3) -> Vec3 {
    let (f, l, u) = (m.x_axis, m.y_axis, m.z_axis);
    let pitch = (-f.z).atan2((f.x * f.x + f.y * f.y).sqrt());
    let yaw = f.y.atan2(f.x);
    let roll = l.z.atan2(u.z);
    Vec3::new(pitch.to_degrees(), yaw.to_degrees(), roll.to_degrees())
}

fn fmt3(v: Vec3) -> String {
    let f = |x: f32| {
        let x = if x.abs() < 0.0005 { 0.0 } else { x };
        let s = format!("{x:.3}");
        s.trim_end_matches('0').trim_end_matches('.').to_string()
    };
    format!("{} {} {}", f(v.x), f(v.y), f(v.z))
}

/// Move the entities of each template made as the map starts to where it's made. Returns
/// the brush models moved (by model number), for their faces and boxes to follow.
pub fn place(ents: &mut [Entity]) -> HashMap<usize, Place> {
    let lower = |s: &str| s.to_ascii_lowercase();
    // Makers fired as the map starts (by a logic_auto).
    let mut at_start = std::collections::HashSet::new();
    for e in ents.iter().filter(|e| key(e, "classname") == Some("logic_auto")) {
        for (k, v) in e {
            if k.starts_with("On") {
                let f: Vec<&str> = v.split([',', '\x1b']).collect();
                if f.len() >= 2 && f[1].eq_ignore_ascii_case("ForceSpawn") {
                    at_start.insert(lower(f[0]));
                }
            }
        }
    }
    let makers: Vec<(String, Vec3, Vec3)> = ents
        .iter()
        .filter(|e| key(e, "classname") == Some("env_entity_maker") && key(e, "targetname").is_some_and(|n| at_start.contains(&lower(n))))
        .filter_map(|e| Some((lower(key(e, "EntityTemplate")?), vec3_key(e, "origin")?, vec3_key(e, "angles").unwrap_or(Vec3::ZERO))))
        .collect();
    let mut moved = HashMap::new();
    for (template, to, maker_angles) in &makers {
        if makers.iter().filter(|m| m.0 == *template).count() > 1 {
            continue;
        }
        let Some(t) = ents.iter().find(|e| key(e, "classname") == Some("point_template") && key(e, "targetname").is_some_and(|n| lower(n) == *template)) else { continue };
        let members: Vec<String> = t.iter().filter(|(k, _)| lower(k).starts_with("template")).map(|(_, v)| lower(v)).collect();
        let from = vec3_key(t, "origin").unwrap_or(Vec3::ZERO);
        let place = Place { from, to: *to, turn: angles_to_mat(*maker_angles) * angles_to_mat(vec3_key(t, "angles").unwrap_or(Vec3::ZERO)).transpose() };
        let is_member = |e: &Entity| key(e, "targetname").is_some_and(|n| members.contains(&lower(n)));
        if ents.iter().filter(|e| is_member(e)).any(|e| key(e, "classname").is_some_and(|c| c.starts_with("prop_physics"))) {
            continue;
        }
        for e in ents.iter_mut().filter(|e| is_member(e)) {
            if let Some(m) = key(e, "model").and_then(|v| v.strip_prefix('*')).and_then(|v| v.parse::<usize>().ok()) {
                moved.insert(m, place);
            }
            let origin = vec3_key(e, "origin");
            let angles = vec3_key(e, "angles").unwrap_or(Vec3::ZERO);
            for (k, v) in e.iter_mut() {
                if k.eq_ignore_ascii_case("origin") {
                    if let Some(o) = origin {
                        *v = fmt3(place.point(o));
                    }
                } else if k.eq_ignore_ascii_case("angles") {
                    *v = fmt3(mat_to_angles(place.turn * angles_to_mat(angles)));
                }
            }
            // A point entity without angles is turned too (a brush's turn is in its faces).
            if !e.iter().any(|(k, _)| k.eq_ignore_ascii_case("angles")) && key(e, "model").is_none_or(|m| !m.starts_with('*')) && place.turn != Mat3::IDENTITY {
                e.push(("angles".into(), fmt3(mat_to_angles(place.turn))));
            }
        }
    }
    moved
}

/// The entity lump's text for these entities.
pub fn write(ents: &[Entity]) -> String {
    let mut s = String::new();
    for e in ents {
        s.push_str("{\n");
        for (k, v) in e {
            s.push_str(&format!("\"{k}\" \"{v}\"\n"));
        }
        s.push_str("}\n");
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn angles_round_trip() {
        for a in [Vec3::new(0.0, 90.0, 0.0), Vec3::new(-90.0, 0.0, 0.0), Vec3::new(30.0, -45.0, 10.0), Vec3::new(0.0, 270.0, 0.0)] {
            let m = angles_to_mat(a);
            let b = mat_to_angles(m);
            let n = angles_to_mat(b);
            assert!((m.x_axis - n.x_axis).length() < 1e-4 && (m.z_axis - n.z_axis).length() < 1e-4, "{a} -> {b}");
        }
        // Source's forward for a pitch: positive looks down.
        let f = angles_to_mat(Vec3::new(90.0, 0.0, 0.0)).x_axis;
        assert!((f - Vec3::NEG_Z).length() < 1e-5, "{f}");
    }

    #[test]
    fn a_template_made_at_the_start_moves_to_its_maker() {
        let e = |kv: &[(&str, &str)]| kv.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect::<Entity>();
        let mut ents = vec![
            e(&[("classname", "logic_auto"), ("OnMapSpawn", "maker,ForceSpawn,,0,-1")]),
            e(&[("classname", "env_entity_maker"), ("targetname", "maker"), ("origin", "100 0 0"), ("angles", "0 90 0"), ("EntityTemplate", "tmpl")]),
            e(&[("classname", "point_template"), ("targetname", "tmpl"), ("origin", "0 0 0"), ("Template01", "thing"), ("Template02", "trigger")]),
            e(&[("classname", "prop_dynamic"), ("targetname", "thing"), ("origin", "10 0 0"), ("angles", "0 0 0")]),
            e(&[("classname", "trigger_multiple"), ("targetname", "trigger"), ("model", "*3"), ("origin", "0 0 0")]),
        ];
        let moved = place(&mut ents);
        // Turned a quarter round: 10 units along X becomes 10 along Y.
        assert_eq!(key(&ents[3], "origin"), Some("100 10 0"));
        assert_eq!(key(&ents[3], "angles"), Some("0 90 0"));
        let p = moved[&3];
        assert!((p.point(Vec3::new(0.0, 5.0, 0.0)) - Vec3::new(95.0, 0.0, 0.0)).length() < 1e-4);
    }
}
