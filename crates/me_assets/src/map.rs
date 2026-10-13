//! Mirror's Edge's maps: what a level package places (meshes, volumes, lights, starts) and the
//! shapes its meshes collide with, read from the package as the game cooked it.
//!
//! Everything here is in Unreal's units and axes (centimetres; X forward, Y right, Z up); the
//! game turns them into its own.

use crate::package::{Cursor, Export, Package};
use crate::props::Props;
use crate::Result;

/// A placed static mesh (a `StaticMeshActor`, an `InterpActor`'s mesh, ...).
#[derive(Clone, Debug)]
pub struct Placed {
    pub class: String,
    pub name: String,
    /// The mesh's export in this package (0-based), and its name.
    pub mesh: usize,
    pub mesh_name: String,
    pub location: [f32; 3],
    /// Pitch, yaw, roll, in 65536ths of a turn.
    pub rotation: [i32; 3],
    /// The actor's and its component's scales together.
    pub scale: [f32; 3],
    /// Whether Faith collides with it.
    pub blocks: bool,
    /// Hidden in the game (an editor-only mesh).
    pub hidden: bool,
    /// The component's materials, replacing the mesh's own (0: the mesh's).
    pub materials: Vec<i32>,
    /// The actor's export (0-based).
    pub export: usize,
    /// A mesh from another package (an import): that package's name (then `mesh` is
    /// `usize::MAX`, and `mesh_name` its name there).
    pub import: Option<String>,
}

/// A volume (`BlockingVolume`, `TdLadderVolume`, ...): its brush's polygons, placed.
#[derive(Clone, Debug)]
pub struct Volume {
    pub class: String,
    pub name: String,
    pub location: [f32; 3],
    pub rotation: [i32; 3],
    pub scale: [f32; 3],
    pub pre_pivot: [f32; 3],
    /// The brush's polygons, in its own space (before `pre_pivot`, scale, turn, location).
    pub polys: Vec<Vec<[f32; 3]>>,
    /// Its properties, for whatever the class needs (a ladder's type, ...).
    pub export: usize,
}

impl Volume {
    /// A point of the brush where the level has it.
    pub fn world(&self, v: [f32; 3]) -> [f32; 3] {
        let p = [
            (v[0] - self.pre_pivot[0]) * self.scale[0],
            (v[1] - self.pre_pivot[1]) * self.scale[1],
            (v[2] - self.pre_pivot[2]) * self.scale[2],
        ];
        let r = rotate(self.rotation, p);
        [r[0] + self.location[0], r[1] + self.location[1], r[2] + self.location[2]]
    }
}

/// Any other actor: where it stands, which way it faces, and its export for its properties.
#[derive(Clone, Debug)]
pub struct Other {
    pub class: String,
    pub name: String,
    pub location: [f32; 3],
    pub rotation: [i32; 3],
    pub export: usize,
}

#[derive(Clone, Debug, Default)]
pub struct Map {
    pub meshes: Vec<Placed>,
    pub volumes: Vec<Volume>,
    pub others: Vec<Other>,
}

/// Unreal's rotation matrix's rows: where a rotator takes X, Y and Z.
pub fn axes(r: [i32; 3]) -> [[f32; 3]; 3] {
    let t = |u: i32| u as f64 * std::f64::consts::TAU / 65536.0;
    let (sp, cp) = t(r[0]).sin_cos();
    let (sy, cy) = t(r[1]).sin_cos();
    let (sr, cr) = t(r[2]).sin_cos();
    let f = |v: [f64; 3]| [v[0] as f32, v[1] as f32, v[2] as f32];
    [
        f([cp * cy, cp * sy, sp]),
        f([sr * sp * cy - cr * sy, sr * sp * sy + cr * cy, -sr * cp]),
        f([-(cr * sp * cy + sr * sy), cy * sr - cr * sp * sy, cr * cp]),
    ]
}

/// A vector turned by a rotator.
pub fn rotate(r: [i32; 3], v: [f32; 3]) -> [f32; 3] {
    let m = axes(r);
    [
        v[0] * m[0][0] + v[1] * m[1][0] + v[2] * m[2][0],
        v[0] * m[0][1] + v[1] * m[1][1] + v[2] * m[2][1],
        v[0] * m[0][2] + v[1] * m[1][2] + v[2] * m[2][2],
    ]
}

/// An object's properties and its archetypes' (in this package), nearest first: what an
/// object doesn't set itself it has from its archetype.
pub struct Chain<'a> {
    pkg: &'a Package,
    lists: Vec<Props>,
}

impl<'a> Chain<'a> {
    pub fn new(pkg: &'a Package, export: usize) -> Self {
        let mut lists = vec![];
        let mut at = export as i32 + 1;
        for _ in 0..8 {
            let Some(e) = pkg.exports.get(at as usize - 1) else { break };
            if let Ok(p) = Props::read_object(pkg, e) {
                lists.push(p);
            }
            if e.archetype <= 0 {
                break;
            }
            at = e.archetype;
        }
        Chain { pkg, lists }
    }

    fn find(&self, name: &str) -> Option<(&Props, &crate::props::Tag)> {
        self.lists.iter().find_map(|p| p.get(name).map(|t| (p, t)))
    }

    pub fn has(&self, name: &str) -> bool {
        self.find(name).is_some()
    }

    pub fn object(&self, name: &str) -> Option<i32> {
        let (p, _) = self.find(name)?;
        p.object(self.pkg, name)
    }

    pub fn vec3(&self, name: &str) -> Option<[f32; 3]> {
        let (p, _) = self.find(name)?;
        p.vec3(self.pkg, name)
    }

    pub fn rotator(&self, name: &str) -> Option<[i32; 3]> {
        let (p, _) = self.find(name)?;
        p.rotator(self.pkg, name)
    }

    pub fn f32(&self, name: &str) -> Option<f32> {
        let (p, _) = self.find(name)?;
        p.f32(self.pkg, name)
    }

    pub fn i32(&self, name: &str) -> Option<i32> {
        let (p, _) = self.find(name)?;
        p.i32(self.pkg, name)
    }

    pub fn bool(&self, name: &str) -> Option<bool> {
        self.find(name).map(|(_, t)| t.bool_value)
    }

    pub fn name(&self, name: &str) -> Option<String> {
        let (p, _) = self.find(name)?;
        p.name(self.pkg, name)
    }

    pub fn objects(&self, name: &str) -> Vec<i32> {
        self.find(name).map(|(p, _)| p.object_array(self.pkg, name)).unwrap_or_default()
    }

    /// The properties a struct property holds (`LightColor`, ...), as tagged values.
    pub fn bytes(&self, name: &str) -> Option<&'a [u8]> {
        let (_, t) = self.find(name)?;
        self.pkg.data.get(t.at..t.at + t.size)
    }
}

fn placed_in_level(pkg: &Package, e: &Export) -> bool {
    e.outer > 0 && pkg.object_name(e.outer) == "PersistentLevel"
}

/// Classes whose actors are meshes standing in the level.
const MESH_ACTORS: [&str; 4] = ["StaticMeshActor", "InterpActor", "KActor", "TdMovableStaticMeshActor"];

/// What a level package places.
pub fn read(pkg: &Package) -> Map {
    let mut map = Map::default();
    for (i, e) in pkg.exports.iter().enumerate() {
        if !placed_in_level(pkg, e) {
            continue;
        }
        let class = pkg.class_name(e).to_string();
        let a = Chain::new(pkg, i);
        let location = a.vec3("Location").unwrap_or_default();
        let rotation = a.rotator("Rotation").unwrap_or_default();
        let draw = a.f32("DrawScale").unwrap_or(1.0);
        let d3 = a.vec3("DrawScale3D").unwrap_or([1.0; 3]);
        let scale = [draw * d3[0], draw * d3[1], draw * d3[2]];
        if MESH_ACTORS.contains(&class.as_str()) || a.has("StaticMeshComponent") {
            let Some(comp) = a.object("StaticMeshComponent").filter(|&c| c > 0) else { continue };
            let c = Chain::new(pkg, comp as usize - 1);
            let Some(mesh) = c.object("StaticMesh").filter(|&m| m != 0) else { continue };
            // (One from another package: which, and its name there.)
            let (mesh, import, mesh_name) = if mesh < 0 {
                let Some(imp) = pkg.imports.get((-mesh) as usize - 1) else { continue };
                if imp.class != "StaticMesh" {
                    continue;
                }
                let mut top = imp.outer;
                let mut top_name = String::new();
                while top < 0 {
                    let Some(o) = pkg.imports.get((-top) as usize - 1) else { break };
                    top_name = o.name.clone();
                    top = o.outer;
                }
                (usize::MAX, Some(top_name), imp.name.clone())
            } else {
                let mesh = mesh as usize - 1;
                if pkg.class_name(&pkg.exports[mesh]) != "StaticMesh" {
                    continue;
                }
                (mesh, None, pkg.exports[mesh].name.clone())
            };
            let cs = c.f32("Scale").unwrap_or(1.0);
            let c3 = c.vec3("Scale3D").unwrap_or([1.0; 3]);
            // Unreal's defaults: a static mesh actor blocks, its component collides with and
            // blocks what moves, boxes (people) included.
            let blocks = a.bool("bCollideActors").unwrap_or(true)
                && a.bool("bBlockActors").unwrap_or(true)
                && c.bool("CollideActors").unwrap_or(true)
                && c.bool("BlockActors").unwrap_or(true)
                && c.bool("BlockNonZeroExtent").unwrap_or(true);
            let hidden = a.bool("bHidden").unwrap_or(false) || c.bool("HiddenGame").unwrap_or(false);
            map.meshes.push(Placed {
                class,
                name: e.name.clone(),
                mesh,
                mesh_name,
                location,
                rotation,
                scale: [scale[0] * cs * c3[0], scale[1] * cs * c3[1], scale[2] * cs * c3[2]],
                blocks,
                hidden,
                materials: c.objects("Materials"),
                export: i,
                import,
            });
        } else if class.ends_with("Volume") || class == "Brush" {
            // (Its brush is a model inside it, the polygons inside that.)
            let inside = |outer: usize, class: &str| pkg.exports.iter().position(|x| x.outer == outer as i32 + 1 && pkg.class_name(x) == class);
            let polys = inside(i, "Model")
                .and_then(|m| inside(m, "Polys"))
                .map(|p| polys(pkg, &pkg.exports[p]).unwrap_or_default())
                .unwrap_or_default();
            map.volumes.push(Volume {
                class,
                name: e.name.clone(),
                location,
                rotation,
                scale,
                pre_pivot: a.vec3("PrePivot").unwrap_or_default(),
                polys,
                export: i,
            });
        } else {
            map.others.push(Other { class, name: e.name.clone(), location, rotation, export: i });
        }
    }
    map
}

/// A `Polys` object's polygons (a brush's faces), each its points in order.
pub fn polys(pkg: &Package, e: &Export) -> Result<Vec<Vec<[f32; 3]>>> {
    let props = Props::read_object(pkg, e)?;
    let mut c = Cursor::new(&pkg.data, props.end);
    let n = c.count(16)?;
    let _max = c.i32()?;
    let _owner = c.i32()?;
    let mut out = Vec::with_capacity(n);
    for _ in 0..n {
        c.skip(48)?; // base, normal, texture U and V
        let nv = c.count(12)?;
        let pts = (0..nv).map(|_| c.vec3()).collect::<Result<Vec<_>>>()?;
        // Flags, actor, item name, material, link, brush poly, shadow map scale, lighting
        // channels.
        c.skip(4 + 4 + 8 + 4 + 4 + 4 + 4 + 4)?;
        out.push(pts);
    }
    Ok(out)
}

/// One of a mesh's simple collision shapes, in the mesh's space.
#[derive(Clone, Debug)]
pub enum Shape {
    /// A convex hull: its corners, and its faces' planes (`n·p = d`, facing out).
    Hull { verts: Vec<[f32; 3]>, planes: Vec<[f32; 4]> },
    /// A box: its matrix's rows (X, Y, Z axes, then its centre) and its sizes along them.
    Box { tm: [[f32; 3]; 4], size: [f32; 3] },
    /// A capsule along its matrix's Z: radius, and the length of its straight part.
    Capsule { tm: [[f32; 3]; 4], radius: f32, length: f32 },
    Sphere { center: [f32; 3], radius: f32 },
}

/// What a mesh collides with: its simple shapes, if it has them and is set to use them for
/// people, else its triangles.
#[derive(Clone, Debug, Default)]
pub struct Collision {
    pub shapes: Vec<Shape>,
    pub tris: Vec<[[f32; 3]; 3]>,
}

fn matrix(data: &[u8], at: usize) -> Option<[[f32; 3]; 4]> {
    let mut c = Cursor::new(data, at);
    let mut m = [[0.0; 3]; 4];
    for row in &mut m {
        for v in row.iter_mut() {
            *v = c.f32().ok()?;
        }
        c.f32().ok()?;
    }
    Some(m)
}

/// A mesh's collision (`mesh` its export).
pub fn mesh_collision(pkg: &Package, mesh: usize) -> Collision {
    let mut out = Collision::default();
    let m = Chain::new(pkg, mesh);
    let simple = m.bool("UseSimpleBoxCollision").unwrap_or(true);
    if simple {
        if let Some(bs) = m.object("BodySetup").filter(|&b| b > 0) {
            out.shapes = body_shapes(pkg, bs as usize - 1);
        }
    }
    if out.shapes.is_empty() {
        out.tris = crate::staticmesh::StaticMesh::collision_tris(pkg, &pkg.exports[mesh]).unwrap_or_default();
    }
    out
}

/// An `RB_BodySetup`'s shapes.
pub fn body_shapes(pkg: &Package, body: usize) -> Vec<Shape> {
    let mut out = vec![];
    let Ok(props) = Props::read(pkg, &pkg.exports[body]) else { return out };
    let Some(t) = props.get("AggGeom") else { return out };
    let Ok(agg) = Props::read_list(pkg, t.at, t.at + t.size, "AggGeom") else { return out };
    let raw_array = |p: &Props, name: &str, size: usize| -> Vec<&[u8]> {
        let Some(t) = p.get(name) else { return vec![] };
        let mut c = Cursor::new(&pkg.data, t.at);
        let Ok(n) = c.count(size) else { return vec![] };
        (0..n).map(|i| &pkg.data[c.pos + i * size..c.pos + (i + 1) * size]).collect()
    };
    let f = |b: &[u8], i: usize| f32::from_le_bytes(b[i * 4..i * 4 + 4].try_into().unwrap());
    for el in agg.struct_array(pkg, "ConvexElems") {
        let verts = raw_array(&el, "VertexData", 12).iter().map(|b| [f(b, 0), f(b, 1), f(b, 2)]).collect();
        // (Stored distance first: d, then the normal.)
        let planes = raw_array(&el, "FacePlaneData", 16).iter().map(|b| [f(b, 1), f(b, 2), f(b, 3), f(b, 0)]).collect();
        out.push(Shape::Hull { verts, planes });
    }
    for el in agg.struct_array(pkg, "BoxElems") {
        let Some(tm) = el.get("TM").and_then(|t| matrix(&pkg.data, t.at)) else { continue };
        let size = [el.f32(pkg, "X").unwrap_or(0.0), el.f32(pkg, "Y").unwrap_or(0.0), el.f32(pkg, "Z").unwrap_or(0.0)];
        out.push(Shape::Box { tm, size });
    }
    for el in agg.struct_array(pkg, "SphylElems") {
        let Some(tm) = el.get("TM").and_then(|t| matrix(&pkg.data, t.at)) else { continue };
        out.push(Shape::Capsule { tm, radius: el.f32(pkg, "Radius").unwrap_or(0.0), length: el.f32(pkg, "Length").unwrap_or(0.0) });
    }
    for el in agg.struct_array(pkg, "SphereElems") {
        let Some(tm) = el.get("TM").and_then(|t| matrix(&pkg.data, t.at)) else { continue };
        out.push(Shape::Sphere { center: tm[3], radius: el.f32(pkg, "Radius").unwrap_or(0.0) });
    }
    out
}
