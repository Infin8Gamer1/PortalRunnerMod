//! A level's BSP (#100): the rooms, floors and walls its designers built from brushes rather
//! than placed meshes, as cooked into the level's `Model` (UE3 file version 536):
//!
//! bounds (7 f32) · vectors, points, nodes (bulk arrays) · surfaces (an owner, then a count
//! and 56-byte surfaces) · verts (bulk) · shared sides, zones (24 bytes each), polys (object) ·
//! leaf hulls, leaves (bulk) · root outside, linked · portal nodes, edges (bulk) · vertex count
//! and vertex buffer (bulk).
//!
//! A node is a convex polygon of one surface: its points are `verts[pool..pool + n]`. Its
//! texture coordinates are `(p - base) · U / 128` (and V), as the engine builds its vertex
//! buffer.

use crate::package::{Cursor, Package};
use crate::props::Props;
use crate::staticmesh::{StaticMesh, StaticSection};
use crate::{Error, Result};

/// What a level's BSP is called among its meshes (a placed mesh of this name is the level's
/// own BSP).
pub const BSP: &str = "BSP";

/// A surface isn't drawn (an invisible wall, or a face nothing sees).
const PF_INVISIBLE: u32 = 0x1;
/// A surface nothing collides with.
const PF_NOT_SOLID: u32 = 0x8;

#[derive(Clone, Debug)]
pub struct Surf {
    /// Package object index of its material.
    pub material: i32,
    pub flags: u32,
    pub base: [f32; 3],
    pub u: [f32; 3],
    pub v: [f32; 3],
    pub normal: [f32; 3],
}

#[derive(Clone, Debug)]
pub struct Face {
    pub surf: usize,
    /// Unreal units, its points in order round it.
    pub points: Vec<[f32; 3]>,
}

#[derive(Clone, Debug, Default)]
pub struct Bsp {
    pub surfs: Vec<Surf>,
    pub faces: Vec<Face>,
}

fn bulk(c: &mut Cursor, size: usize) -> Result<(usize, usize)> {
    let s = c.i32()?;
    if s as usize != size && !(s == 0 || size == 0) {
        return Err(Error::Format(format!("BSP: bulk element size {s}, not {size}, at {}", c.pos - 4)));
    }
    let n = c.count(s.max(1) as usize)?;
    let at = c.pos;
    c.skip(n * s as usize)?;
    Ok((n, at))
}

fn sub(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn dot(a: [f32; 3], b: [f32; 3]) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn cross(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}

fn unit(a: [f32; 3]) -> [f32; 3] {
    let l = dot(a, a).sqrt();
    if l > 0.0 { [a[0] / l, a[1] / l, a[2] / l] } else { a }
}

impl Bsp {
    /// The level's own model (the one whose outer is the level, not a brush's), if it has one.
    pub fn export(pkg: &Package) -> Option<usize> {
        pkg.exports.iter().position(|e| pkg.class_name(e) == "Model" && e.outer > 0 && pkg.class_name(&pkg.exports[e.outer as usize - 1]) == "Level")
    }

    pub fn read(pkg: &Package) -> Result<Bsp> {
        let e = &pkg.exports[Bsp::export(pkg).ok_or_else(|| Error::Missing("the level's Model".into()))?];
        let props = Props::read_object(pkg, e)?;
        let d = &pkg.data;
        let mut c = Cursor::new(d, props.end);
        c.skip(28)?;
        let (_, vectors) = bulk(&mut c, 12)?;
        let (_, points) = bulk(&mut c, 12)?;
        let (nodes, node_at) = bulk(&mut c, 64)?;
        let _owner = c.i32()?;
        let ns = c.count(56)?;
        let surf_at = c.pos;
        c.skip(ns * 56)?;
        let (nverts, vert_at) = bulk(&mut c, 24)?;
        let end = e.offset + e.size;
        let vec3 = |at: usize| -> Result<[f32; 3]> {
            if at + 12 > end {
                return Err(Error::Format(format!("BSP: index out of range at {at}")));
            }
            Cursor::new(d, at).vec3()
        };
        let mut surfs = Vec::with_capacity(ns);
        for k in 0..ns {
            let mut s = Cursor::new(d, surf_at + k * 56);
            let material = s.i32()?;
            let flags = s.u32()?;
            let (base, normal, u, v) = (s.i32()?, s.i32()?, s.i32()?, s.i32()?);
            surfs.push(Surf {
                material,
                flags,
                base: vec3(points + base.max(0) as usize * 12)?,
                normal: vec3(vectors + normal.max(0) as usize * 12)?,
                u: vec3(vectors + u.max(0) as usize * 12)?,
                v: vec3(vectors + v.max(0) as usize * 12)?,
            });
        }
        let mut faces = Vec::with_capacity(nodes);
        for k in 0..nodes {
            let n = node_at + k * 64;
            let mut q = Cursor::new(d, n + 16);
            let pool = q.i32()?;
            let surf = q.i32()?;
            let count = d[n + 54] as usize;
            if pool < 0 || surf < 0 || surf as usize >= ns || count < 3 || pool as usize + count > nverts {
                continue;
            }
            let mut pts = Vec::with_capacity(count);
            for j in 0..count {
                let p = Cursor::new(d, vert_at + (pool as usize + j) * 24).i32()?;
                pts.push(vec3(points + p.max(0) as usize * 12)?);
            }
            faces.push(Face { surf: surf as usize, points: pts });
        }
        Ok(Bsp { surfs, faces })
    }

    fn drawn(&self, s: &Surf, pkg: &Package) -> bool {
        s.flags & PF_INVISIBLE == 0 && s.material != 0 && pkg.object_name(s.material) != "DefaultMaterial"
    }

    /// What's drawn of it, a section per material, as a mesh (Unreal units, its points where
    /// they are in the level).
    pub fn mesh(&self, pkg: &Package) -> StaticMesh {
        let mut m = StaticMesh {
            name: BSP.into(),
            positions: vec![],
            normals: vec![],
            tangents: vec![],
            uvs: vec![],
            uv_sets: vec![],
            indices: vec![],
            sections: vec![],
            origin: [0.0; 3],
            extent: [0.0; 3],
        };
        let mut mats: Vec<i32> = self.surfs.iter().filter(|s| self.drawn(s, pkg)).map(|s| s.material).collect();
        mats.sort();
        mats.dedup();
        for mat in mats {
            let first = m.indices.len() as u32;
            for f in &self.faces {
                let s = &self.surfs[f.surf];
                if s.material != mat || !self.drawn(s, pkg) {
                    continue;
                }
                // (Its normal from its points: the surface's is shared by faces either way
                // round.)
                let mut n = [0.0; 3];
                for k in 1..f.points.len() - 1 {
                    let c = cross(sub(f.points[k], f.points[0]), sub(f.points[k + 1], f.points[0]));
                    n = [n[0] + c[0], n[1] + c[1], n[2] + c[2]];
                }
                let n = unit(n);
                let tx = unit(s.u);
                let w = if dot(cross(n, tx), s.v) < 0.0 { -1.0 } else { 1.0 };
                let at = m.positions.len() as u32;
                for p in &f.points {
                    let r = sub(*p, s.base);
                    m.positions.push(*p);
                    m.normals.push(n);
                    m.tangents.push([tx[0], tx[1], tx[2], w]);
                    m.uvs.push([dot(r, s.u) / 128.0, dot(r, s.v) / 128.0]);
                }
                // (Unreal's front faces wind clockwise, seen from in front.)
                for k in 1..f.points.len() as u32 - 1 {
                    m.indices.extend_from_slice(&[at, at + k + 1, at + k]);
                }
            }
            let triangles = (m.indices.len() as u32 - first) / 3;
            if triangles > 0 {
                m.sections.push(StaticSection { material: mat, first_index: first, triangles });
            }
        }
        let (mut lo, mut hi) = ([f32::MAX; 3], [f32::MIN; 3]);
        for p in &m.positions {
            for k in 0..3 {
                lo[k] = lo[k].min(p[k]);
                hi[k] = hi[k].max(p[k]);
            }
        }
        if !m.positions.is_empty() {
            m.origin = [(lo[0] + hi[0]) * 0.5, (lo[1] + hi[1]) * 0.5, (lo[2] + hi[2]) * 0.5];
            m.extent = [(hi[0] - lo[0]) * 0.5, (hi[1] - lo[1]) * 0.5, (hi[2] - lo[2]) * 0.5];
        }
        m
    }

    /// What collides of it: its solid faces' triangles (Unreal units), each wound so its
    /// front is the face's.
    pub fn collision_tris(&self) -> Vec<[[f32; 3]; 3]> {
        let mut out = vec![];
        for f in &self.faces {
            if self.surfs[f.surf].flags & PF_NOT_SOLID != 0 {
                continue;
            }
            for k in 1..f.points.len() - 1 {
                out.push([f.points[0], f.points[k + 1], f.points[k]]);
            }
        }
        out
    }
}
