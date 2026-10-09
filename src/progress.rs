//! What's been done: each chamber's best time and the last one played, saved to
//! `progress.txt` next to where you run the game (by chamber name, so reordering the list
//! doesn't scramble it).

use bevy::prelude::*;

const FILE: &str = "progress.txt";

#[derive(Resource, Debug, Default, Clone, PartialEq)]
pub struct Progress {
    /// (chamber name, best time in seconds).
    pub best: Vec<(String, f32)>,
    /// The chamber last played.
    pub last: Option<String>,
    /// Write changes to the file (not in screenshot capture runs).
    persist: bool,
}

impl Progress {
    pub fn best_for(&self, name: &str) -> Option<f32> {
        self.best.iter().find(|(n, _)| n == name).map(|(_, t)| *t)
    }

    /// Record a finish; true if it's a new best.
    pub fn finished(&mut self, name: &str, time: f32) -> bool {
        match self.best.iter_mut().find(|(n, _)| n == name) {
            Some((_, t)) if time < *t => {
                *t = time;
                true
            }
            Some(_) => false,
            None => {
                self.best.push((name.to_string(), time));
                true
            }
        }
    }

    pub fn parse(text: &str) -> Progress {
        let mut p = Progress::default();
        for line in text.lines() {
            let line = line.trim();
            if let Some(rest) = line.strip_prefix("best ") {
                // The time is the last word; the name is everything before it.
                if let Some((name, t)) = rest.rsplit_once(' ') {
                    if let Ok(t) = t.parse::<f32>() {
                        p.best.push((name.to_string(), t));
                    }
                }
            } else if let Some(name) = line.strip_prefix("last ") {
                p.last = Some(name.to_string());
            }
        }
        p
    }

    pub fn to_text(&self) -> String {
        let mut t = String::from("# Portal Runner progress\n");
        if let Some(l) = &self.last {
            t += &format!("last {l}\n");
        }
        for (n, b) in &self.best {
            t += &format!("best {n} {b}\n");
        }
        t
    }

    pub fn load() -> Progress {
        let mut p = std::fs::read_to_string(FILE).map(|t| Progress::parse(&t)).unwrap_or_default();
        p.persist = std::env::var_os("PORTAL_CAPTURE").is_none();
        p
    }

    pub fn save(&self) {
        if !self.persist {
            return;
        }
        if let Err(e) = std::fs::write(FILE, self.to_text()) {
            warn!("couldn't save {FILE}: {e}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_and_keeps_bests() {
        let mut p = Progress::default();
        assert!(p.finished("Chamber 01: Portals", 42.5));
        assert!(!p.finished("Chamber 01: Portals", 50.0));
        assert!(p.finished("Chamber 01: Portals", 40.25));
        p.finished("Rooftops", 61.0);
        p.last = Some("Rooftops".into());
        let q = Progress::parse(&p.to_text());
        assert_eq!(q, p);
        assert_eq!(q.best_for("Chamber 01: Portals"), Some(40.25));
        assert_eq!(q.best_for("Nope"), None);
    }
}
