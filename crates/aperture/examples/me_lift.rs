//! A Mirror's Edge lift tried in the sim (#109): `me_lift <level file> x,y,z button_x,button_y,button_z
//! [seconds]`. She stands at x,y,z (our axes), presses use looking at the button, and waits; prints
//! her height and the level's moving bodies' every half second.
use aperture::doc::LevelDoc;
use aperture::sim::{Aim, Sim};
use faith_move::{Controller, Input, State};
use glam::Vec3;

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let v = |s: &str| -> Vec3 {
        let f: Vec<f32> = s.split(',').map(|x| x.parse().unwrap()).collect();
        Vec3::new(f[0], f[1], f[2])
    };
    let doc = LevelDoc::from_ron(&std::fs::read_to_string(&a[1]).expect("level")).expect("doc");
    let l = doc.build();
    let (at, button) = (v(&a[2]), v(&a[3]));
    let secs: f32 = a.get(4).and_then(|s| s.parse().ok()).unwrap_or(12.0);
    let mut sim = Sim::new(l);
    let mut c = Controller::new(aperture::tuning(5.3, 15.0), at, 0.0);
    c.state = State::Ground;
    let dt = 1.0 / 60.0;
    for f in 0..(secs / dt) as usize {
        if f == 60 {
            // (Looking at the button.)
            let eye = c.feet + Vec3::Y * 1.6;
            sim.use_key(Aim { eye, dir: (button - eye).normalize(), yaw: c.yaw });
            println!("use at {:.2}s: {:.2} m away", f as f32 * dt, (button - eye).length());
        }
        sim.step_player(&mut c, &Input::default(), dt);
        sim.step(dt, &mut c);
        if f % 30 == 0 {
            let me = sim.me.as_ref().unwrap();
            let moving: Vec<String> = me.bodies.iter().filter(|(_, b)| b.1.length() > 0.01).map(|(s, b)| format!("{s}:{:.1}", b.1.y)).collect();
            println!("{:5.2}s feet {:.2},{:.2},{:.2} {:?} bodies moved {moving:?}", f as f32 * dt, c.feet.x, c.feet.y, c.feet.z, c.state);
        }
    }
}
