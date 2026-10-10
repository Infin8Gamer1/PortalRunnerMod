//! Source MDL animations (the ones stored in the .mdl itself, as Portal 2's props have): each
//! bone's place at each frame. `vmdl` reads these too, but treats the stored values as
//! unsigned, scrambles the angle axes and leaves out each bone's rest pose, so they're decoded
//! here the way the engine does (`CalcBoneQuaternion` / `CalcBonePosition` in the Source SDK).
//!
//! Everything is in Source space (inches, +X forward, +Z up): convert after posing.

use glam::{Affine3A, EulerRot, Quat, Vec3};

/// One animation: each frame's pose, a place (relative to its parent) per bone.
pub struct Anim {
    pub name: String,
    pub fps: f32,
    pub frames: Vec<Vec<(Vec3, Quat)>>,
}

impl Anim {
    /// Every bone's transform in model space at `frame`.
    pub fn world(&self, frame: usize, parents: &[i32]) -> Vec<Affine3A> {
        let pose = &self.frames[frame.min(self.frames.len() - 1)];
        let mut out: Vec<Affine3A> = Vec::with_capacity(pose.len());
        for (i, (p, q)) in pose.iter().enumerate() {
            let local = Affine3A::from_rotation_translation(*q, *p);
            // Source orders bones so a parent always comes before its children.
            let w = match usize::try_from(parents[i]).ok().and_then(|p| out.get(p)) {
                Some(parent) => *parent * local,
                None => local,
            };
            out.push(w);
        }
        out
    }
}

const RAWPOS: u8 = 0x01;
const RAWROT: u8 = 0x02;
const ANIMPOS: u8 = 0x04;
const ANIMROT: u8 = 0x08;
const DELTA: u8 = 0x10;
const RAWROT2: u8 = 0x20;

fn bytes<const N: usize>(d: &[u8], at: usize) -> Result<[u8; N], String> {
    d.get(at..at + N).and_then(|b| b.try_into().ok()).ok_or_else(|| format!("animation data runs past the file (at {at})"))
}
fn i32_at(d: &[u8], at: usize) -> Result<i32, String> {
    bytes(d, at).map(i32::from_le_bytes)
}
fn i16_at(d: &[u8], at: usize) -> Result<i16, String> {
    bytes(d, at).map(i16::from_le_bytes)
}
fn u16_at(d: &[u8], at: usize) -> Result<u16, String> {
    bytes(d, at).map(u16::from_le_bytes)
}
fn f32_at(d: &[u8], at: usize) -> Result<f32, String> {
    bytes(d, at).map(f32::from_le_bytes)
}
fn u8_at(d: &[u8], at: usize) -> Result<u8, String> {
    d.get(at).copied().ok_or_else(|| format!("animation data runs past the file (at {at})"))
}
fn offset(base: usize, rel: i32) -> Result<usize, String> {
    base.checked_add_signed(rel as isize).ok_or_else(|| "animation offset out of range".to_string())
}

fn cstr(d: &[u8], at: usize) -> String {
    let s = d.get(at..).unwrap_or_default();
    String::from_utf8_lossy(&s[..s.iter().position(|&b| b == 0).unwrap_or(s.len())]).into_owned()
}

/// A run-length coded track (`mstudioanimvalue_t`): its raw value at `frame`.
fn track_value(d: &[u8], mut at: usize, frame: usize) -> Result<f32, String> {
    let mut k = frame;
    loop {
        let valid = u8_at(d, at)? as usize;
        let total = u8_at(d, at + 1)? as usize;
        if total == 0 {
            return Ok(0.0);
        }
        if k < total {
            // Frames past the run's stored values hold its last one.
            let i = if k < valid { k + 1 } else { valid };
            return Ok(i16_at(d, at + i * 2)? as f32);
        }
        k -= total;
        at += (valid + 1) * 2;
    }
}

/// Three tracks (`mstudioanim_valueptr_t`: offsets from itself, 0 for none) at `frame`.
fn tracks(d: &[u8], at: usize, frame: usize, scale: Vec3) -> Result<Vec3, String> {
    let mut v = [0.0; 3];
    for (a, out) in v.iter_mut().enumerate() {
        let off = i16_at(d, at + a * 2)?;
        if off > 0 {
            *out = track_value(d, at + off as usize, frame)? * scale[a];
        }
    }
    Ok(Vec3::from(v))
}

fn quat48(d: &[u8], at: usize) -> Result<Quat, String> {
    let (x, y, zw) = (u16_at(d, at)?, u16_at(d, at + 2)?, u16_at(d, at + 4)?);
    let x = (x as f32 - 32768.0) / 32768.0;
    let y = (y as f32 - 32768.0) / 32768.0;
    let z = ((zw & 0x7fff) as f32 - 16384.0) / 16384.0;
    let w = (1.0 - x * x - y * y - z * z).max(0.0).sqrt();
    Ok(Quat::from_xyzw(x, y, z, if zw & 0x8000 != 0 { -w } else { w }).normalize())
}

fn quat64(d: &[u8], at: usize) -> Result<Quat, String> {
    let b = u64::from_le_bytes(bytes(d, at)?);
    let f = |shift: u32| ((b >> shift) & 0x1f_ffff) as f32 - 1_048_576.0;
    let (x, y, z) = (f(0) / 1_048_576.5, f(21) / 1_048_576.5, f(42) / 1_048_576.5);
    let w = (1.0 - x * x - y * y - z * z).max(0.0).sqrt();
    Ok(Quat::from_xyzw(x, y, z, if b >> 63 != 0 { -w } else { w }).normalize())
}

fn vec48(d: &[u8], at: usize) -> Result<Vec3, String> {
    let h = |o: usize| u16_at(d, at + o).map(|b| half::f16::from_bits(b).to_f32());
    Ok(Vec3::new(h(0)?, h(2)?, h(4)?))
}

/// Source's angles (`RadianEuler`: roll about X, pitch about Y, yaw about Z) as a rotation.
fn euler(a: Vec3) -> Quat {
    Quat::from_euler(EulerRot::ZYX, a.z, a.y, a.x)
}

fn v(v: vmdl::Vector) -> Vec3 {
    Vec3::new(v.x, v.y, v.z)
}

/// Every animation stored in the model (`mdl`: the .mdl file's bytes). Ones kept in a separate
/// .ani file, and delta (layered) ones, are skipped.
pub fn read(mdl: &[u8], bones: &[&vmdl::mdl::Bone]) -> Result<Vec<Anim>, String> {
    // studiohdr_t: numlocalanim, localanimindex.
    let count = i32_at(mdl, 180)?.max(0) as usize;
    let index = i32_at(mdl, 184)? as usize;
    let rest: Vec<(Vec3, Quat)> = bones.iter().map(|b| (v(b.pos), Quat::from_xyzw(b.quaternion.x, b.quaternion.y, b.quaternion.z, b.quaternion.w))).collect();
    let mut out = Vec::new();
    for a in 0..count {
        // mstudioanimdesc_t (100 bytes).
        let desc = index + a * 100;
        let name = cstr(mdl, offset(desc, i32_at(mdl, desc + 4)?)?);
        let fps = f32_at(mdl, desc + 8)?;
        let flags = i32_at(mdl, desc + 12)?;
        let frame_count = i32_at(mdl, desc + 16)?.max(1) as usize;
        let (block, anim_index) = (i32_at(mdl, desc + 52)?, i32_at(mdl, desc + 56)?);
        let (section_index, section_frames) = (i32_at(mdl, desc + 80)?, i32_at(mdl, desc + 84)?.max(0) as usize);
        if flags & 0x0004 != 0 {
            // STUDIO_DELTA: a layer added on top of another pose.
            continue;
        }
        let mut frames = Vec::with_capacity(frame_count);
        let mut external = false;
        for frame in 0..frame_count {
            // Long animations are cut into sections, each with its own data.
            let (block, chain, f) = if section_frames != 0 {
                let (section, f) = if frame_count > section_frames && frame == frame_count - 1 {
                    ((frame_count - 1) / section_frames + 1, 0)
                } else {
                    (frame / section_frames, frame % section_frames)
                };
                let s = offset(desc, section_index)? + section * 8;
                (i32_at(mdl, s)?, i32_at(mdl, s + 4)?, f)
            } else {
                (block, anim_index, frame)
            };
            if block != 0 {
                external = true;
                break;
            }
            let mut pose = rest.clone();
            let mut at = offset(desc, chain)?;
            loop {
                // mstudioanim_t: bone, flags, offset to the next bone's.
                let bone = u8_at(mdl, at)? as usize;
                let fl = u8_at(mdl, at + 1)?;
                let next = i16_at(mdl, at + 2)?;
                let data = at + 4;
                if let (Some(b), Some(slot)) = (bones.get(bone), pose.get_mut(bone)) {
                    let rot = if fl & RAWROT != 0 {
                        quat48(mdl, data)?
                    } else if fl & RAWROT2 != 0 {
                        quat64(mdl, data)?
                    } else if fl & ANIMROT != 0 {
                        let mut a = tracks(mdl, data, f, v(b.rot_scale))?;
                        if fl & DELTA == 0 {
                            a += Vec3::new(b.rot.x, b.rot.y, b.rot.z);
                        }
                        euler(a)
                    } else if fl & DELTA != 0 {
                        Quat::IDENTITY
                    } else {
                        slot.1
                    };
                    let pos_at = data + if fl & (RAWROT | ANIMROT) != 0 { 6 } else if fl & RAWROT2 != 0 { 8 } else { 0 };
                    let pos = if fl & RAWPOS != 0 {
                        vec48(mdl, pos_at)?
                    } else if fl & ANIMPOS != 0 {
                        let mut p = tracks(mdl, pos_at, f, v(b.pos_scale))?;
                        if fl & DELTA == 0 {
                            p += v(b.pos);
                        }
                        p
                    } else if fl & DELTA != 0 {
                        Vec3::ZERO
                    } else {
                        slot.0
                    };
                    *slot = (pos, rot);
                }
                if next == 0 {
                    break;
                }
                at = offset(at, next as i32)?;
            }
            frames.push(pose);
        }
        if !external {
            out.push(Anim { name, fps, frames });
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn shorts(v: &[i16]) -> Vec<u8> {
        v.iter().flat_map(|s| s.to_le_bytes()).collect()
    }

    #[test]
    fn tracks_hold_their_last_value_and_go_negative() {
        // A run of 3 frames with 2 stored values, then a run of 2 with 1, then the end.
        let header = |valid: u8, total: u8| i16::from_le_bytes([valid, total]);
        let d = shorts(&[header(2, 3), 10, -20, header(1, 2), 7, header(0, 0)]);
        let got: Vec<f32> = (0..6).map(|f| track_value(&d, 0, f).unwrap()).collect();
        assert_eq!(got, [10.0, -20.0, -20.0, 7.0, 7.0, 0.0]);
    }

    #[test]
    fn angles_turn_as_source_turns_them() {
        // Source's AngleQuaternion, written out.
        let source = |a: Vec3| {
            let (sy, cy) = (a.z * 0.5).sin_cos();
            let (sp, cp) = (a.y * 0.5).sin_cos();
            let (sr, cr) = (a.x * 0.5).sin_cos();
            Quat::from_xyzw(sr * cp * cy - cr * sp * sy, cr * sp * cy + sr * cp * sy, cr * cp * sy - sr * sp * cy, cr * cp * cy + sr * sp * sy)
        };
        for a in [Vec3::new(0.3, -1.1, 2.0), Vec3::new(-2.5, 0.4, -0.7), Vec3::new(1.57, 0.0, 0.0)] {
            assert!(euler(a).abs_diff_eq(source(a), 1e-5), "{a}");
        }
    }

    #[test]
    fn packed_rotations_unpack() {
        let q48: Vec<u8> = [32768u16, 32768, 16384].iter().flat_map(|s| s.to_le_bytes()).collect();
        assert!(quat48(&q48, 0).unwrap().abs_diff_eq(Quat::IDENTITY, 1e-4));
        let q64 = (1u64 << 20) | (1 << 41) | (1 << 62);
        assert!(quat64(&q64.to_le_bytes(), 0).unwrap().abs_diff_eq(Quat::IDENTITY, 1e-4));
        // (Nearly) half a turn about Z: z as high as 15 bits go.
        let q48: Vec<u8> = [32768u16, 32768, 32767].iter().flat_map(|s| s.to_le_bytes()).collect();
        assert!(quat48(&q48, 0).unwrap().angle_between(Quat::from_rotation_z(std::f32::consts::PI)) < 0.03);
    }
}
