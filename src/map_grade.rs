//! Portal's maps' colour correction (color_correction): each names a table (32x32x32, every
//! colour's corrected one, `correction/<name>.raw` in the extracted folder), weighed in near it
//! (or everywhere), switched and faded by the logic. And as she dies, Portal's death table.
//!
//! Source looks colours up in the table after tonemapping; Bevy's colour grading comes before
//! it (with only its saturation after). So each table is fitted to what Bevy's grading does:
//! how much it brightens or darkens greys (an exposure, in stops), how it bends them (a gamma)
//! and how much colour it keeps (a saturation). Portal's chamber tables are all but unchanged
//! colours; the dim lights' one darkens, the death one washes colour out.

use std::collections::HashMap;

use bevy::prelude::*;
use bevy::render::view::{ColorGrading, ColorGradingGlobal, ColorGradingSection};

use aperture::io::Kind;

use crate::{Game, PlayerCamera};

/// What a table does, as Bevy's grading does it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Grade {
    /// Stops.
    pub exposure: f32,
    /// The greys' curve, as a power (1: as they were; above: darker midtones).
    pub power: f32,
    pub saturation: f32,
}

impl Grade {
    pub const NONE: Grade = Grade { exposure: 0.0, power: 1.0, saturation: 1.0 };
}

const SIDE: usize = 32;

/// A table's grade: the greys' brightness fitted (least squares, in logs) to a gain and a
/// power, from the middle of the range up; colour from how much of the primaries' and
/// secondaries' it keeps.
pub fn fit(table: &[u8]) -> Grade {
    if table.len() != SIDE * SIDE * SIDE * 3 {
        return Grade::NONE;
    }
    let at = |r: usize, g: usize, b: usize| {
        let i = 3 * (r + SIDE * g + SIDE * SIDE * b);
        Vec3::new(table[i] as f32, table[i + 1] as f32, table[i + 2] as f32) / 255.0
    };
    let top = (SIDE - 1) as f32;
    // (All but black: Portal's ending fades to it.)
    if at(SIDE - 1, SIDE - 1, SIDE - 1).max_element() < 0.02 {
        return Grade { exposure: -10.0, power: 1.0, saturation: 1.0 };
    }
    let (mut n, mut sx, mut sy, mut sxx, mut sxy) = (0.0f32, 0.0f32, 0.0f32, 0.0f32, 0.0f32);
    for i in SIDE / 4..SIDE {
        let out = at(i, i, i).element_sum() / 3.0;
        if out <= 0.0 {
            continue;
        }
        let (x, y) = ((i as f32 / top).ln(), out.ln());
        n += 1.0;
        sx += x;
        sy += y;
        sxx += x * x;
        sxy += x * y;
    }
    let power = if n > 1.0 && (n * sxx - sx * sx).abs() > 1e-6 { (n * sxy - sx * sy) / (n * sxx - sx * sx) } else { 1.0 };
    let gain = ((sy - power * sx) / n.max(1.0)).exp();
    // (The table's in the screen's gamma: a gain there is 2.2 times the stops in light.)
    let exposure = 2.2 * gain.max(1e-3).log2();
    let mut kept = 0.0;
    let mut count = 0.0;
    for level in [SIDE - 1, SIDE * 3 / 4] {
        for c in [(1, 0, 0), (0, 1, 0), (0, 0, 1), (1, 1, 0), (0, 1, 1), (1, 0, 1)] {
            let out = at(c.0 * level, c.1 * level, c.2 * level);
            if out.max_element() > 0.0 {
                kept += (out.max_element() - out.min_element()) / out.max_element();
                count += 1.0;
            }
        }
    }
    let saturation = if count > 0.0 { kept / count } else { 1.0 };
    Grade { exposure: exposure.clamp(-10.0, 4.0), power: power.clamp(0.5, 2.0), saturation: saturation.clamp(0.0, 2.0) }
}

/// Grades weighed together (each weight 0 to 1; together, at most 1 in all, as Source has
/// them).
pub fn blend(grades: &[(Grade, f32)]) -> Grade {
    let total: f32 = grades.iter().map(|g| g.1.max(0.0)).sum();
    let scale = if total > 1.0 { 1.0 / total } else { 1.0 };
    let mut out = Grade::NONE;
    for (g, w) in grades {
        let w = w.max(0.0) * scale;
        out.exposure += w * g.exposure;
        out.power += w * (g.power - 1.0);
        out.saturation += w * (g.saturation - 1.0);
    }
    out
}

/// How much of a correction is weighed in at `d` metres: all of it within `near`, none past
/// `far`.
pub fn falloff(d: f32, near: f32, far: f32) -> f32 {
    if near.is_infinite() || d <= near {
        1.0
    } else if d >= far {
        0.0
    } else {
        1.0 - (d - near) / (far - near).max(1e-3)
    }
}

/// The tables, as fitted (or none, where there's no file), by name.
#[derive(Resource, Default)]
pub struct Grades(HashMap<String, Option<Grade>>);

impl Grades {
    fn get(&mut self, file: &str) -> Option<Grade> {
        if let Some(g) = self.0.get(file) {
            return *g;
        }
        let g = crate::extracted::find_root().and_then(|r| std::fs::read(r.join("correction").join(file)).ok()).map(|b| fit(&b));
        self.0.insert(file.to_string(), g);
        g
    }
}

#[derive(Default)]
pub struct GradeState {
    loads: Option<u64>,
    /// Each correction's (entity's) weight as it fades in and out.
    weights: HashMap<usize, f32>,
}

/// Weighs in the corrections near her (each fading toward what it should be over its fade
/// time) and the death table as she dies, onto the view's grading.
pub fn update(
    mut commands: Commands,
    time: Res<Time>,
    game: Res<Game>,
    mut grades: ResMut<Grades>,
    mut st: Local<GradeState>,
    cams: Query<(Entity, Option<&ColorGrading>), With<PlayerCamera>>,
) {
    let dt = time.delta_secs();
    let fresh = st.loads != Some(game.loads);
    if fresh {
        st.loads = Some(game.loads);
        st.weights.clear();
    }
    let eye = game.ctrl.view().eye;
    let mut all: Vec<(Grade, f32)> = Vec::new();
    if let Some(io) = game.sim.io.as_ref() {
        for (e, ent) in io.ents.iter().enumerate() {
            let Kind::Correction { at, near, far, weight, file, enabled, fade_in, fade_out } = &ent.kind else { continue };
            let want = if *enabled { weight * falloff(at.distance(eye), *near, *far) } else { 0.0 };
            let w = st.weights.entry(e).or_insert(if fresh { want } else { 0.0 });
            let fade = if want > *w { *fade_in } else { *fade_out };
            let step = if fade <= 0.0 { f32::INFINITY } else { dt / fade };
            *w += (want - *w).clamp(-step, step);
            let w = *w;
            if w > 0.0 {
                if let Some(g) = grades.get(file) {
                    all.push((g, w));
                }
            }
        }
        // (Her death, over the moment the screen flashes for it.)
        if game.died < 0.6 {
            if let Some(g) = grades.get("cc_death.raw") {
                all.push((g, 1.0 - game.died / 0.6));
            }
        }
    }
    let g = blend(&all);
    for (cam, now) in &cams {
        if g == Grade::NONE {
            if now.is_some() {
                commands.entity(cam).remove::<ColorGrading>();
            }
            continue;
        }
        if now.is_some_and(|n| n.global.exposure == g.exposure && n.global.post_saturation == g.saturation && n.midtones.gamma == 1.0 / g.power) {
            continue;
        }
        let section = ColorGradingSection { gamma: 1.0 / g.power, ..default() };
        let grading = ColorGrading {
            global: ColorGradingGlobal { exposure: g.exposure, post_saturation: g.saturation, ..default() },
            shadows: section,
            midtones: section,
            highlights: section,
        };
        commands.entity(cam).insert(grading);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn table(f: impl Fn(Vec3) -> Vec3) -> Vec<u8> {
        let mut t = Vec::with_capacity(SIDE * SIDE * SIDE * 3);
        for b in 0..SIDE {
            for g in 0..SIDE {
                for r in 0..SIDE {
                    let c = f(Vec3::new(r as f32, g as f32, b as f32) / (SIDE - 1) as f32);
                    t.extend([c.x, c.y, c.z].map(|v| (v.clamp(0.0, 1.0) * 255.0).round() as u8));
                }
            }
        }
        t
    }

    #[test]
    fn tables_fit() {
        let same = fit(&table(|c| c));
        assert!(same.exposure.abs() < 0.05 && (same.power - 1.0).abs() < 0.02 && (same.saturation - 1.0).abs() < 0.01, "{same:?}");
        // Darker, more so in the middle (as the dim lights' table).
        let dim = fit(&table(|c| 0.5 * c.powf(1.5)));
        assert!((dim.exposure - 2.2 * 0.5f32.log2()).abs() < 0.1 && (dim.power - 1.5).abs() < 0.05, "{dim:?}");
        // Colour washed out (as the death table).
        let grey = fit(&table(|c| c.lerp(Vec3::splat(c.max_element()), 0.6)));
        assert!((grey.saturation - 0.4).abs() < 0.03, "{grey:?}");
        assert_eq!(fit(&table(|_| Vec3::ZERO)).exposure, -10.0);
        assert_eq!(fit(&[1, 2, 3]), Grade::NONE);
    }

    #[test]
    fn grades_blend_and_fall_off() {
        let dark = Grade { exposure: -2.0, power: 1.5, saturation: 0.5 };
        assert_eq!(blend(&[]), Grade::NONE);
        let half = blend(&[(dark, 0.5)]);
        assert!((half.exposure + 1.0).abs() < 1e-5 && (half.power - 1.25).abs() < 1e-5 && (half.saturation - 0.75).abs() < 1e-5);
        // (More than all of it is all of it.)
        assert!((blend(&[(dark, 1.0), (dark, 1.0)]).exposure + 2.0).abs() < 1e-5);
        assert_eq!(falloff(1.0, 2.0, 4.0), 1.0);
        assert_eq!(falloff(3.0, 2.0, 4.0), 0.5);
        assert_eq!(falloff(5.0, 2.0, 4.0), 0.0);
        assert_eq!(falloff(1e6, f32::INFINITY, f32::INFINITY), 1.0);
    }

    /// Portal's own tables (when they've been extracted): the chambers' all but unchanged,
    /// the dim lights' darker, the death one greyer.
    #[test]
    fn portals_tables_fit() {
        let Some(root) = crate::extracted::find_root() else { return };
        let get = |f: &str| std::fs::read(root.join("correction").join(f)).ok().map(|b| fit(&b));
        let Some(chamber) = get("cc_testchamber.raw") else { return };
        eprintln!("chamber {chamber:?}\ndim {:?}\ndeath {:?}\nlab {:?}", get("cc_dim_lights.raw"), get("cc_death.raw"), get("cc_lab.raw"));
        assert!(chamber.exposure.abs() < 0.3 && (chamber.power - 1.0).abs() < 0.15 && (chamber.saturation - 1.0).abs() < 0.05, "{chamber:?}");
        let dim = get("cc_dim_lights.raw").unwrap();
        assert!(dim.exposure < -1.5 && dim.power > 1.2, "{dim:?}");
        let death = get("cc_death.raw").unwrap();
        assert!(death.saturation < 0.6, "{death:?}");
    }
}
