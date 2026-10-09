//! StaticMesh (LOD0 geometry and its material sections) as stored by Mirror's Edge (UE3
//! file version 536, licensee 43). The layout follows UE Viewer's UStaticMesh3 for that
//! version:
//!
//! bounds (7 f32) · body setup (object) · kDOP nodes and triangles (bulk arrays) · internal
//! version · [names, if internal version >= 17] · LODs. In LOD0: bulk data (skipped) ·
//! sections · vertex stream (positions) · UV stream (packed tangent and normal, a colour,
//! UVs as floats or halves) · shadow-volume stream · vertex count · indices.

use crate::package::{Cursor, Package};
use crate::props::Props;
use crate::{Error, Result};

#[derive(Clone, Debug)]
pub struct StaticSection {
    /// Package object index of the material.
    pub material: i32,
    pub first_index: u32,
    pub triangles: u32,
}

#[derive(Clone, Debug)]
pub struct StaticMesh {
    pub name: String,
    /// Unreal units (cm), Unreal axes (Z up).
    pub positions: Vec<[f32; 3]>,
    pub normals: Vec<[f32; 3]>,
    pub tangents: Vec<[f32; 3]>,
    pub uvs: Vec<[f32; 2]>,
    pub indices: Vec<u32>,
    pub sections: Vec<StaticSection>,
    /// Bounds: centre and half size (cm).
    pub origin: [f32; 3],
    pub extent: [f32; 3],
}

/// A bulk-serialised array: element size, count, then the elements. Returns (size, count,
/// where the data starts) and moves past it.
fn bulk(c: &mut Cursor) -> Result<(usize, usize, usize)> {
    let size = c.i32()?;
    if !(0..=4096).contains(&size) {
        return Err(Error::Format(format!("bulk element size {size} at {}", c.pos - 4)));
    }
    let n = c.count(size.max(1) as usize)?;
    let at = c.pos;
    c.skip(n * size as usize)?;
    Ok((size as usize, n, at))
}

fn packed_normal(b: &[u8]) -> [f32; 3] {
    [b[0] as f32 / 127.5 - 1.0, b[1] as f32 / 127.5 - 1.0, b[2] as f32 / 127.5 - 1.0]
}

fn half(bits: u16) -> f32 {
    let sign = if bits & 0x8000 != 0 { -1.0 } else { 1.0 };
    let exp = ((bits >> 10) & 0x1f) as i32;
    let frac = (bits & 0x3ff) as f32;
    match exp {
        0 => sign * frac * 2f32.powi(-24),
        31 => sign * f32::INFINITY,
        _ => sign * (1.0 + frac / 1024.0) * 2f32.powi(exp - 15),
    }
}

impl StaticMesh {
    pub fn read(pkg: &Package, name: &str) -> Result<Self> {
        let e = pkg.find("StaticMesh", name).ok_or_else(|| Error::Missing(format!("StaticMesh {name}")))?;
        let props = Props::read(pkg, e)?;
        let mut c = Cursor::new(&pkg.data, props.end);
        let origin = c.vec3()?;
        let extent = c.vec3()?;
        let _radius = c.f32()?;
        let _body_setup = c.i32()?;
        bulk(&mut c)?; // kDOP nodes
        bulk(&mut c)?; // kDOP triangles
        let internal_version = c.i32()?;
        if internal_version >= 17 {
            let n = c.count(8)?;
            c.skip(n * 8)?; // names
        }
        let nlod = c.count(1)?;
        if nlod == 0 {
            return Err(Error::Format("mesh has no LODs".into()));
        }

        // ---- LOD 0: its bulk data (raw triangles) first.
        let flags = c.u32()?;
        let _count = c.i32()?;
        let size_on_disk = c.i32()?;
        let _offset = c.i32()?;
        if flags & 1 == 0 && size_on_disk > 0 {
            c.skip(size_on_disk as usize)?;
        }
        let nsec = c.count(36)?;
        let mut sections = Vec::with_capacity(nsec);
        for _ in 0..nsec {
            let material = c.i32()?;
            let _f10 = c.i32()?;
            let _f14 = c.i32()?;
            let _shadow = c.i32()?;
            let first_index = c.u32()?;
            let triangles = c.u32()?;
            let _min_vertex = c.i32()?;
            let _max_vertex = c.i32()?;
            let _index = c.i32()?;
            let n = c.count(8)?;
            c.skip(n * 8)?;
            sections.push(StaticSection { material, first_index, triangles });
        }

        // Positions.
        let _vertex_size = c.i32()?;
        let _num_verts = c.i32()?;
        let (vsize, nverts, vat) = bulk(&mut c)?;
        if vsize != 12 {
            return Err(Error::Format(format!("vertex size {vsize}")));
        }
        let positions: Vec<[f32; 3]> = (0..nverts)
            .map(|i| {
                let mut v = Cursor::new(&pkg.data, vat + 12 * i);
                v.vec3()
            })
            .collect::<Result<_>>()?;

        // Tangent basis, colour and UVs.
        let num_uv = c.i32()?.clamp(1, 8) as usize;
        let _item_size = c.i32()?;
        let _uv_verts = c.i32()?;
        let full_precision = c.i32()? != 0;
        let (usize_, nuv, uat) = bulk(&mut c)?;
        if nuv != nverts {
            return Err(Error::Format(format!("{nuv} UV items for {nverts} vertices")));
        }
        let uv_bytes = if full_precision { 8 } else { 4 };
        if usize_ < 12 + uv_bytes * num_uv.min(1) {
            return Err(Error::Format(format!("UV item size {usize_}")));
        }
        let mut normals = Vec::with_capacity(nverts);
        let mut tangents = Vec::with_capacity(nverts);
        let mut uvs = Vec::with_capacity(nverts);
        for i in 0..nverts {
            let item = &pkg.data[uat + usize_ * i..uat + usize_ * (i + 1)];
            tangents.push(packed_normal(&item[0..4]));
            normals.push(packed_normal(&item[4..8]));
            // item[8..12]: vertex colour.
            let uv = &item[12..];
            uvs.push(if full_precision {
                [f32::from_le_bytes(uv[0..4].try_into().unwrap()), f32::from_le_bytes(uv[4..8].try_into().unwrap())]
            } else {
                [half(u16::from_le_bytes([uv[0], uv[1]])), half(u16::from_le_bytes([uv[2], uv[3]]))]
            });
        }

        // Shadow-volume stream, vertex count, then the indices.
        let _sv_item = c.i32()?;
        let _sv_verts = c.i32()?;
        bulk(&mut c)?;
        let _num_verts = c.i32()?;
        let (isize_, nidx, iat) = bulk(&mut c)?;
        let indices: Vec<u32> = (0..nidx)
            .map(|i| match isize_ {
                2 => Ok(u16::from_le_bytes([pkg.data[iat + 2 * i], pkg.data[iat + 2 * i + 1]]) as u32),
                4 => Ok(u32::from_le_bytes(pkg.data[iat + 4 * i..iat + 4 * i + 4].try_into().unwrap())),
                _ => Err(Error::Format(format!("index size {isize_}"))),
            })
            .collect::<Result<_>>()?;
        if indices.iter().any(|&i| i as usize >= nverts) {
            return Err(Error::Format("index past the vertices".into()));
        }
        for s in &sections {
            if (s.first_index + s.triangles * 3) as usize > indices.len() {
                return Err(Error::Format(format!("section past the indices ({} + {} x 3 > {})", s.first_index, s.triangles, indices.len())));
            }
        }
        Ok(StaticMesh { name: name.to_string(), positions, normals, tangents, uvs, indices, sections, origin, extent })
    }

    /// Every StaticMesh in a package, by name.
    pub fn names(pkg: &Package) -> Vec<String> {
        pkg.exports.iter().filter(|e| pkg.class_name(e) == "StaticMesh").map(|e| e.name.clone()).collect()
    }
}
