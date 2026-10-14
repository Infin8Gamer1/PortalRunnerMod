//! Portal with RTX's remastered models (USD, mostly binary crate files) to our glTF parts.
//!
//! A remaster sits where the original Source model did: Source units (inches), Z up. Its
//! meshes become parts the way a Source model's bones do: a skinned mesh splits by the joint
//! that moves each vertex most (named after the joint, so `Button` is the pedestal's button
//! as in the MDL), a still mesh is one part named after its prim.

use std::collections::HashMap;

use glam::{DMat4, DQuat, DVec3, EulerRot, Vec3};
use openusd::sdf::{self, Value};
use openusd::usd::{Prim, Stage};

use crate::glb;
use crate::remix::{Input, Material, Remix};

/// The largest a remastered model's maps are kept.
const MAP_MAX: u32 = 2048;

fn get(p: &Prim, name: &str) -> Option<Value> {
    p.attribute(name).get::<Value>().ok().flatten()
}

fn vec3(v: &Value) -> Option<DVec3> {
    match v {
        Value::Vec3f(v) => Some(DVec3::new(v.x as f64, v.y as f64, v.z as f64)),
        Value::Vec3d(v) => Some(DVec3::new(v.x, v.y, v.z)),
        Value::Float(f) => Some(DVec3::splat(*f as f64)),
        Value::Double(f) => Some(DVec3::splat(*f)),
        _ => None,
    }
}

fn matrix(m: &[f64; 16]) -> DMat4 {
    // USD writes row vectors, translation in the last row: the same numbers, read as columns.
    DMat4::from_cols_array(m)
}

/// A prim's own transform, from its `xformOpOrder`.
fn local_transform(p: &Prim) -> DMat4 {
    let Some(Value::TokenVec(order)) = get(p, "xformOpOrder") else { return DMat4::IDENTITY };
    let mut m = DMat4::IDENTITY;
    for op in order {
        let op = op.as_str();
        let (invert, name) = match op.strip_prefix("!invert!") {
            Some(n) => (true, n),
            None => (false, op),
        };
        let Some(v) = get(p, name) else { continue };
        let kind = name.trim_start_matches("xformOp:").split(':').next().unwrap_or_default();
        let deg = |v: &Value| vec3(v).map(|d| d * std::f64::consts::PI / 180.0);
        let one = |v: &Value| match v {
            Value::Float(f) => Some((*f as f64).to_radians()),
            Value::Double(f) => Some(f.to_radians()),
            _ => None,
        };
        let t = match kind {
            "translate" => vec3(&v).map(DMat4::from_translation),
            "scale" => vec3(&v).map(DMat4::from_scale),
            "rotateXYZ" => deg(&v).map(|a| DMat4::from_euler(EulerRot::ZYX, a.z, a.y, a.x)),
            "rotateXZY" => deg(&v).map(|a| DMat4::from_euler(EulerRot::YZX, a.y, a.z, a.x)),
            "rotateYXZ" => deg(&v).map(|a| DMat4::from_euler(EulerRot::ZXY, a.z, a.x, a.y)),
            "rotateYZX" => deg(&v).map(|a| DMat4::from_euler(EulerRot::XZY, a.x, a.z, a.y)),
            "rotateZXY" => deg(&v).map(|a| DMat4::from_euler(EulerRot::YXZ, a.y, a.x, a.z)),
            "rotateZYX" => deg(&v).map(|a| DMat4::from_euler(EulerRot::XYZ, a.x, a.y, a.z)),
            "rotateX" => one(&v).map(DMat4::from_rotation_x),
            "rotateY" => one(&v).map(DMat4::from_rotation_y),
            "rotateZ" => one(&v).map(DMat4::from_rotation_z),
            "orient" => match &v {
                Value::Quatf(q) => Some(DMat4::from_quat(DQuat::from_xyzw(q.x as f64, q.y as f64, q.z as f64, q.w as f64))),
                Value::Quatd(q) => Some(DMat4::from_quat(DQuat::from_xyzw(q.x, q.y, q.z, q.w))),
                _ => None,
            },
            "transform" => match &v {
                Value::Matrix4d(x) => Some(matrix(&x.0)),
                _ => None,
            },
            _ => None,
        };
        if let Some(t) = t {
            m *= if invert { t.inverse() } else { t };
        }
    }
    m
}

/// A material prim's AperturePBR inputs.
fn material(stage: &Stage, path: &sdf::Path, usd_dir: &str) -> Material {
    let mut m = Material::default();
    let Ok(prim) = stage.prim(path.clone()) else { return m };
    let shaders = prim.children().unwrap_or_default().into_iter().filter(|c| c.type_name().ok().flatten().is_some_and(|t| t.as_str() == "Shader"));
    for shader in shaders {
        if let Some(Value::AssetPath(a)) = get(&shader, "info:mdl:sourceAsset") {
            m.translucent |= a.authored_path.contains("Translucent");
        }
        for a in shader.authored_attributes().unwrap_or_default() {
            let name = a.path().name().unwrap_or_default().to_string();
            let Some(input) = name.rsplit_once("inputs:").map(|(_, n)| n.to_string()) else { continue };
            let value = match a.get::<Value>().ok().flatten() {
                Some(Value::AssetPath(p)) => Input::Asset(p.authored_path),
                Some(Value::Float(f)) => Input::Number(f),
                Some(Value::Double(f)) => Input::Number(f as f32),
                Some(Value::Int(i)) => Input::Number(i as f32),
                Some(Value::Bool(b)) => Input::Flag(b),
                Some(Value::Token(t)) => match t.as_str() {
                    "octahedral" => Input::Number(0.0),
                    "tangent_space_ogl" => Input::Number(1.0),
                    "tangent_space_dx" => Input::Number(2.0),
                    _ => continue,
                },
                _ => continue,
            };
            m.set(&input, value);
        }
    }
    m.resolve(usd_dir);
    m
}

fn png(img: &image::RgbaImage) -> Result<Vec<u8>, String> {
    let mut out = Vec::new();
    img.write_to(&mut std::io::Cursor::new(&mut out), image::ImageFormat::Png).map_err(|e| e.to_string())?;
    Ok(out)
}

/// A material's maps, in a glTF material.
fn gltf_material(remix: &Remix, m: &Material) -> Result<glb::Material, String> {
    let baked = remix.bake(m, MAP_MAX)?;
    Ok(glb::Material {
        texture_png: baked.albedo.as_ref().map(png).transpose()?,
        color: None,
        blend: m.translucent,
        mask: false,
        normal_png: baked.normal.as_ref().map(png).transpose()?,
        metallic_roughness_png: baked.metallic_roughness.as_ref().map(png).transpose()?,
        emissive_png: baked.emissive.as_ref().map(png).transpose()?,
        emissive_strength: m.emissive_intensity.unwrap_or(1.0).max(1.0),
        roughness: None,
    })
}

/// A part being gathered: its pivot (in our space) and its vertices and triangles.
#[derive(Default)]
struct Gathered {
    pivot: Vec3,
    verts: Vec<([f32; 3], [f32; 3], [f32; 2])>,
    tris: HashMap<usize, Vec<u32>>,
}

/// What a still mesh's part should be called: the mesh, or (for a mesh named like its parent
/// with "Shape" on the end, as some exporters write them) the parent.
fn part_name(p: &Prim) -> String {
    let name = p.path().name().unwrap_or_default().to_string();
    match p.path().parent() {
        Some(parent) if name.ends_with("Shape") && parent.name().is_some_and(|n| name.starts_with(n)) => parent.name().unwrap_or_default().to_string(),
        _ => name,
    }
}

/// Convert the meshes in these USD files (relative to `gameReadyAssets`, each with turns in
/// degrees about X then Z, in Source space, to line it up with the model it replaces: some are
/// built lying on their side, Remix's skinned captures are a quarter turn round) to parts, in
/// our space by `convert` (Source inches, Z up, to ours). `rename` gives each part (named after
/// its joint or prim) the name the game knows it by, or `None` to leave it out.
pub fn convert(remix: &Remix, files: &[(&str, f32, f32)], convert: &dyn Fn(Vec3) -> Vec3, to_ours: glam::Quat, rename: &dyn Fn(&str) -> Option<String>) -> Result<(Vec<glb::Part>, Vec<glb::Material>), String> {
    let mut parts: Vec<(String, Gathered)> = Vec::new();
    let mut materials: Vec<glb::Material> = Vec::new();
    let mut material_of: HashMap<(usize, String), usize> = HashMap::new();
    for (fi, (file, about_x, about_z)) in files.iter().enumerate() {
        let usd_dir = file.rsplit_once('/').map(|(d, _)| d).unwrap_or("");
        let stage = Stage::open(&remix.root().join(file).to_string_lossy()).map_err(|e| format!("{file}: {e}"))?;
        let root = stage.prim(sdf::Path::abs_root()).map_err(|e| e.to_string())?;
        // Skeletons first (bind transforms in our space, by joint name), then meshes.
        let mut skeletons: HashMap<String, Vec<(String, DMat4)>> = HashMap::new();
        let mut meshes: Vec<(Prim, DMat4)> = Vec::new();
        let line_up = DMat4::from_rotation_z((*about_z as f64).to_radians()) * DMat4::from_rotation_x((*about_x as f64).to_radians());
        let mut stack = vec![(root, line_up)];
        while let Some((prim, parent)) = stack.pop() {
            let world = parent * local_transform(&prim);
            let ty = prim.type_name().ok().flatten().map(|t| t.as_str().to_string()).unwrap_or_default();
            if prim.is_active().unwrap_or(true) {
                match ty.as_str() {
                    "Skeleton" => {
                        let joints = match get(&prim, "joints") {
                            Some(Value::TokenVec(j)) => j.iter().map(|t| t.as_str().rsplit('/').next().unwrap_or_default().to_string()).collect(),
                            _ => Vec::new(),
                        };
                        // In the skeleton's own space: Remix lines a skinned replacement up by
                        // its skeleton, whatever transforms sit above it (the door's has a
                        // quarter turn its points don't need).
                        let binds = match get(&prim, "bindTransforms") {
                            Some(Value::Matrix4dVec(m)) => m.iter().map(|m| line_up * matrix(&m.0)).collect(),
                            _ => Vec::new(),
                        };
                        skeletons.insert(prim.path().to_string(), joints.into_iter().zip(binds).collect());
                    }
                    "Mesh" => meshes.push((prim.clone(), world)),
                    _ => {}
                }
                for c in prim.children().unwrap_or_default() {
                    stack.push((c, world));
                }
            }
        }
        for (mesh, world) in meshes {
            let points: Vec<DVec3> = match get(&mesh, "points") {
                Some(Value::Vec3fVec(p)) => p.iter().map(|p| DVec3::new(p.x as f64, p.y as f64, p.z as f64)).collect(),
                _ => continue,
            };
            let (Some(Value::IntVec(counts)), Some(Value::IntVec(indices))) = (get(&mesh, "faceVertexCounts"), get(&mesh, "faceVertexIndices")) else { continue };
            let normals: Vec<DVec3> = match get(&mesh, "normals").or_else(|| get(&mesh, "primvars:normals")) {
                Some(Value::Vec3fVec(n)) => n.iter().map(|n| DVec3::new(n.x as f64, n.y as f64, n.z as f64)).collect(),
                _ => Vec::new(),
            };
            let uvs: Vec<[f32; 2]> = match get(&mesh, "primvars:st") {
                Some(Value::Vec2fVec(t)) => t.iter().map(|t| [t.x, 1.0 - t.y]).collect(),
                _ => Vec::new(),
            };
            let uv_index = match get(&mesh, "primvars:st:indices") {
                Some(Value::IntVec(i)) => Some(i),
                _ => None,
            };
            let left_handed = matches!(get(&mesh, "orientation"), Some(Value::Token(t)) if t.as_str() == "leftHanded");

            // Skinning: each point's strongest joint, and where the skeleton binds it.
            let skeleton = mesh.relationship("skel:skeleton").targets().ok().and_then(|t| t.first().map(|p| p.to_string())).and_then(|p| skeletons.get(&p));
            let mesh_joints: Option<Vec<String>> = match get(&mesh, "skel:joints") {
                Some(Value::TokenVec(j)) => Some(j.iter().map(|t| t.as_str().rsplit('/').next().unwrap_or_default().to_string()).collect()),
                _ => None,
            };
            let mut joint_of: Vec<Option<String>> = vec![None; points.len()];
            let mut geom_bind = DMat4::IDENTITY;
            if let (Some(skel), Some(Value::IntVec(ji)), Some(Value::FloatVec(jw))) = (skeleton, get(&mesh, "primvars:skel:jointIndices"), get(&mesh, "primvars:skel:jointWeights")) {
                if let Some(Value::Matrix4d(m)) = get(&mesh, "primvars:skel:geomBindTransform") {
                    geom_bind = matrix(&m.0);
                }
                let per = (ji.len() / points.len().max(1)).max(1);
                let names: Vec<String> = mesh_joints.clone().unwrap_or_else(|| skel.iter().map(|(n, _)| n.clone()).collect());
                for (v, slot) in joint_of.iter_mut().enumerate() {
                    let best = (0..per).filter_map(|k| Some((*ji.get(v * per + k)?, *jw.get(v * per + k)?))).max_by(|a, b| a.1.total_cmp(&b.1));
                    *slot = best.and_then(|(j, _)| names.get(j as usize).cloned());
                }
            }
            let bind_of = |joint: &str| skeleton.and_then(|s| s.iter().find(|(n, _)| n == joint).map(|(_, m)| *m));

            // Materials: per face from its subsets, else the mesh's own.
            let face_count = counts.len();
            let mut face_material: Vec<Option<String>> = vec![None; face_count];
            let own = mesh.relationship("material:binding").targets().ok().and_then(|t| t.first().map(|p| p.to_string()));
            for c in mesh.children().unwrap_or_default() {
                if c.type_name().ok().flatten().is_some_and(|t| t.as_str() == "GeomSubset") {
                    let target = c.relationship("material:binding").targets().ok().and_then(|t| t.first().map(|p| p.to_string()));
                    if let (Some(target), Some(Value::IntVec(faces))) = (target, get(&c, "indices")) {
                        for f in faces {
                            if let Some(slot) = face_material.get_mut(f as usize) {
                                *slot = Some(target.clone());
                            }
                        }
                    }
                }
            }
            let mut mat_index = |path: &Option<String>| -> Result<usize, String> {
                let key = (fi, path.clone().unwrap_or_default());
                if let Some(&i) = material_of.get(&key) {
                    return Ok(i);
                }
                let m = match path {
                    Some(p) => {
                        let m = material(&stage, &sdf::Path::new(p).map_err(|e| e.to_string())?, usd_dir);
                        gltf_material(remix, &m).map_err(|e| format!("{file} {p}: {e}"))?
                    }
                    None => glb::Material::default(),
                };
                materials.push(m);
                material_of.insert(key, materials.len() - 1);
                Ok(materials.len() - 1)
            };

            let normal_matrix = |m: &DMat4| glam::DMat3::from_mat4(*m).inverse().transpose();
            let still_name = part_name(&mesh);
            // A mesh named in the table is one part whatever its joints (the door's two leaves
            // share one).
            let whole = rename(&still_name).filter(|n| !n.is_empty()).is_some();
            // Each point's part (by its joint, or the mesh's own) and its transform, once.
            let mut part_of_name: HashMap<String, Option<usize>> = HashMap::new();
            let mut placements: Vec<(DMat4, glam::DMat3)> = Vec::new();
            let point_part: Vec<Option<(usize, usize)>> = joint_of
                .iter()
                .map(|joint| {
                    let skinned = skeleton.is_some() && joint.is_some();
                    let (name, place, pivot) = match joint.as_ref().and_then(|j| bind_of(j).map(|b| (j, b))).filter(|_| !whole) {
                        Some((j, bind)) => (j.clone(), line_up * geom_bind, convert(bind.w_axis.truncate().as_vec3())),
                        None if skinned => (still_name.clone(), line_up * geom_bind, convert(line_up.w_axis.truncate().as_vec3())),
                        None => (still_name.clone(), world, convert(world.w_axis.truncate().as_vec3())),
                    };
                    let part = (*part_of_name.entry(name.clone()).or_insert_with(|| {
                        let ours = rename(&name)?;
                        Some(match parts.iter().position(|(n, _)| *n == ours) {
                            Some(i) => i,
                            None => {
                                parts.push((ours, Gathered { pivot, ..Default::default() }));
                                parts.len() - 1
                            }
                        })
                    }))?;
                    let pi = placements.iter().position(|(m, _)| *m == place).unwrap_or_else(|| {
                        placements.push((place, normal_matrix(&place)));
                        placements.len() - 1
                    });
                    Some((part, pi))
                })
                .collect();
            let mut remap: HashMap<(usize, usize, usize), u32> = HashMap::new();
            let mut corner = 0usize;
            for (f, &n) in counts.iter().enumerate() {
                let n = n.max(0) as usize;
                let face: Vec<usize> = (corner..corner + n).collect();
                corner += n;
                if n < 3 {
                    continue;
                }
                let mat = mat_index(&face_material.get(f).cloned().flatten().or_else(|| own.clone()))?;
                for k in 1..n - 1 {
                    let tri = if left_handed { [face[0], face[k + 1], face[k]] } else { [face[0], face[k], face[k + 1]] };
                    for c in tri {
                        let Some(pi) = indices.get(c).map(|&i| i as usize) else { continue };
                        let (Some(&p), Some(Some((part, placement)))) = (points.get(pi), point_part.get(pi)) else { continue };
                        let ni = if normals.len() == indices.len() { c } else { pi };
                        let ti = match &uv_index {
                            Some(ix) => ix.get(c).map(|&i| i as usize).unwrap_or(0),
                            None if uvs.len() == indices.len() => c,
                            None => pi,
                        };
                        let g = &mut parts[*part].1;
                        let local = *remap.entry((pi, ni, ti)).or_insert_with(|| {
                            let (place, nm) = &placements[*placement];
                            let pos = convert(place.transform_point3(p).as_vec3()) - g.pivot;
                            let nrm = match normals.get(ni) {
                                Some(n) => {
                                    let world_n: DVec3 = *nm * *n;
                                    let ours: Vec3 = to_ours * world_n.as_vec3();
                                    ours.normalize_or_zero()
                                }
                                None => Vec3::Y,
                            };
                            g.verts.push((pos.to_array(), nrm.to_array(), uvs.get(ti).copied().unwrap_or([0.0, 0.0])));
                            (g.verts.len() - 1) as u32
                        });
                        g.tris.entry(mat).or_default().push(local);
                    }
                }
            }
        }
    }
    let out = parts
        .into_iter()
        .map(|(name, g)| {
            let mut prims: Vec<(Vec<u32>, usize)> = g.tris.into_iter().map(|(m, t)| (t, m)).collect();
            prims.sort_by_key(|p| p.1);
            glb::Part { name, pivot: g.pivot.to_array(), verts: g.verts, prims }
        })
        .collect();
    Ok((out, materials))
}
