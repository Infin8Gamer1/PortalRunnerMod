//! A minimal glTF binary (.glb) writer: a root node with a child node per part (each its own
//! mesh, placed at its pivot so it can be moved or turned about it), a primitive per material,
//! each material's texture embedded as PNG, and any animations as keyframes on the parts.

use serde_json::{json, Value};

#[derive(Default)]
pub struct Material {
    pub texture_png: Option<Vec<u8>>,
    /// Without a texture: its colour (linear RGBA), else a light grey.
    pub color: Option<[f32; 4]>,
    /// See-through (glass): alpha-blended.
    pub blend: bool,
    /// Cut out where the texture's alpha is low (grilles, decals).
    pub mask: bool,
    /// Physically based maps, when there are any (Portal with RTX's): a tangent-space normal
    /// map, glTF's metallic-roughness map, and what glows (with how brightly).
    pub normal_png: Option<Vec<u8>>,
    pub metallic_roughness_png: Option<Vec<u8>>,
    pub emissive_png: Option<Vec<u8>>,
    pub emissive_strength: f32,
}

/// One rigid piece of a model, named after the bone that moves it.
pub struct Part {
    pub name: String,
    /// Where it turns about (the bone), in the model's space; `verts` are relative to it.
    pub pivot: [f32; 3],
    pub verts: Vec<([f32; 3], [f32; 3], [f32; 2])>,
    /// (triangle indices into `verts`, material index).
    pub prims: Vec<(Vec<u32>, usize)>,
}

/// An animation: keyframe times, and each moving part's place (relative to the root) at each.
pub struct Clip {
    pub name: String,
    pub times: Vec<f32>,
    /// (part index, translation and rotation per keyframe).
    pub tracks: Vec<(usize, Vec<[f32; 3]>, Vec<[f32; 4]>)>,
}

/// The most triangles one primitive holds.
const MAX_TRIANGLES: usize = 65535;

fn pad4(buf: &mut Vec<u8>, byte: u8) {
    while buf.len() % 4 != 0 {
        buf.push(byte);
    }
}

pub fn write(parts: &[Part], mats: &[Material], clips: &[Clip]) -> Vec<u8> {
    let mut bin: Vec<u8> = Vec::new();
    let mut views: Vec<Value> = Vec::new();
    let mut accessors: Vec<Value> = Vec::new();
    let mut images: Vec<Value> = Vec::new();
    let mut textures: Vec<Value> = Vec::new();
    let mut materials: Vec<Value> = Vec::new();
    let mut meshes: Vec<Value> = Vec::new();
    let mut nodes: Vec<Value> = vec![json!({"name": "root", "children": (1..=parts.len()).collect::<Vec<_>>()})];

    for m in mats {
        let mut pbr = json!({"metallicFactor": 0.0, "roughnessFactor": 0.6});
        let mut texture = |bin: &mut Vec<u8>, png: &[u8]| -> usize {
            pad4(bin, 0);
            let start = bin.len();
            bin.extend_from_slice(png);
            views.push(json!({"buffer": 0, "byteOffset": start, "byteLength": png.len()}));
            images.push(json!({"bufferView": views.len() - 1, "mimeType": "image/png"}));
            textures.push(json!({"source": images.len() - 1, "sampler": 0}));
            textures.len() - 1
        };
        if let Some(png) = &m.texture_png {
            pbr["baseColorTexture"] = json!({"index": texture(&mut bin, png)});
        } else {
            pbr["baseColorFactor"] = json!(m.color.unwrap_or([0.8, 0.8, 0.82, 1.0]));
        }
        if m.blend {
            pbr["roughnessFactor"] = json!(0.1);
        }
        if let Some(png) = &m.metallic_roughness_png {
            pbr["metallicRoughnessTexture"] = json!({"index": texture(&mut bin, png)});
            pbr["metallicFactor"] = json!(1.0);
            pbr["roughnessFactor"] = json!(1.0);
        }
        let mut mat = json!({"pbrMetallicRoughness": pbr, "doubleSided": true});
        if let Some(png) = &m.normal_png {
            mat["normalTexture"] = json!({"index": texture(&mut bin, png)});
        }
        if let Some(png) = &m.emissive_png {
            mat["emissiveTexture"] = json!({"index": texture(&mut bin, png)});
            mat["emissiveFactor"] = json!([1.0, 1.0, 1.0]);
            if m.emissive_strength > 1.0 {
                mat["extensions"] = json!({"KHR_materials_emissive_strength": {"emissiveStrength": m.emissive_strength}});
            }
        }
        if m.blend {
            mat["alphaMode"] = json!("BLEND");
        } else if m.mask {
            mat["alphaMode"] = json!("MASK");
            mat["alphaCutoff"] = json!(0.5);
        }
        materials.push(mat);
    }

    for part in parts {
        let mut primitives = Vec::new();
        // At most 65535 triangles a primitive: Bevy makes each its own mesh (with whatever
        // vertices it's given, so each gets just its own), and Solari takes no glowing mesh
        // larger than that as a light.
        let chunks = part.prims.iter().flat_map(|(indices, material)| indices.chunks(MAX_TRIANGLES * 3).map(move |c| (c, material)));
        for (indices, material) in chunks {
            let mut remap: std::collections::HashMap<u32, u32> = std::collections::HashMap::new();
            let mut verts = Vec::new();
            let local: Vec<u32> = indices
                .iter()
                .map(|&i| {
                    *remap.entry(i).or_insert_with(|| {
                        verts.push(part.verts[i as usize]);
                        (verts.len() - 1) as u32
                    })
                })
                .collect();
            let mut min = [f32::INFINITY; 3];
            let mut max = [f32::NEG_INFINITY; 3];
            for (p, _, _) in &verts {
                for a in 0..3 {
                    min[a] = min[a].min(p[a]);
                    max[a] = max[a].max(p[a]);
                }
            }
            // Interleaved vertices: 8 floats each.
            pad4(&mut bin, 0);
            let start = bin.len();
            for (p, n, uv) in &verts {
                for f in p.iter().chain(n.iter()).chain(uv.iter()) {
                    bin.extend_from_slice(&f.to_le_bytes());
                }
            }
            views.push(json!({"buffer": 0, "byteOffset": start, "byteLength": bin.len() - start, "byteStride": 32, "target": 34962}));
            let view = views.len() - 1;
            let count = verts.len();
            let a0 = accessors.len();
            accessors.push(json!({"bufferView": view, "byteOffset": 0, "componentType": 5126, "count": count, "type": "VEC3", "min": min, "max": max}));
            accessors.push(json!({"bufferView": view, "byteOffset": 12, "componentType": 5126, "count": count, "type": "VEC3"}));
            accessors.push(json!({"bufferView": view, "byteOffset": 24, "componentType": 5126, "count": count, "type": "VEC2"}));
            pad4(&mut bin, 0);
            let start = bin.len();
            for i in &local {
                bin.extend_from_slice(&i.to_le_bytes());
            }
            views.push(json!({"buffer": 0, "byteOffset": start, "byteLength": bin.len() - start, "target": 34963}));
            accessors.push(json!({"bufferView": views.len() - 1, "componentType": 5125, "count": local.len(), "type": "SCALAR"}));
            primitives.push(json!({
                "attributes": {"POSITION": a0, "NORMAL": a0 + 1, "TEXCOORD_0": a0 + 2},
                "indices": accessors.len() - 1,
                "material": material,
                "mode": 4
            }));
        }
        meshes.push(json!({"name": part.name, "primitives": primitives}));
        nodes.push(json!({"name": part.name, "mesh": meshes.len() - 1, "translation": part.pivot}));
    }
    let mut animations: Vec<Value> = Vec::new();
    for clip in clips {
        let mut floats = |bin: &mut Vec<u8>, values: &[f32], kind: &str, n: usize| -> usize {
            pad4(bin, 0);
            let start = bin.len();
            for f in values {
                bin.extend_from_slice(&f.to_le_bytes());
            }
            views.push(json!({"buffer": 0, "byteOffset": start, "byteLength": bin.len() - start}));
            let mut a = json!({"bufferView": views.len() - 1, "componentType": 5126, "count": values.len() / n, "type": kind});
            if kind == "SCALAR" {
                let (lo, hi) = values.iter().fold((f32::INFINITY, f32::NEG_INFINITY), |(lo, hi), v| (lo.min(*v), hi.max(*v)));
                a["min"] = json!([lo]);
                a["max"] = json!([hi]);
            }
            accessors.push(a);
            accessors.len() - 1
        };
        let input = floats(&mut bin, &clip.times, "SCALAR", 1);
        let mut samplers = Vec::new();
        let mut channels = Vec::new();
        for (part, translations, rotations) in &clip.tracks {
            let t = floats(&mut bin, &translations.concat(), "VEC3", 3);
            let r = floats(&mut bin, &rotations.concat(), "VEC4", 4);
            for (output, path) in [(t, "translation"), (r, "rotation")] {
                samplers.push(json!({"input": input, "output": output, "interpolation": "LINEAR"}));
                channels.push(json!({"sampler": samplers.len() - 1, "target": {"node": part + 1, "path": path}}));
            }
        }
        animations.push(json!({"name": clip.name, "samplers": samplers, "channels": channels}));
    }
    pad4(&mut bin, 0);

    let mut doc = json!({
        "asset": {"version": "2.0", "generator": "portal_extract"},
        "scene": 0,
        "scenes": [{"nodes": [0]}],
        "nodes": nodes,
        "meshes": meshes,
        "buffers": [{"byteLength": bin.len()}],
        "bufferViews": views,
        "accessors": accessors,
        "materials": materials,
    });
    if !animations.is_empty() {
        doc["animations"] = json!(animations);
    }
    if mats.iter().any(|m| m.emissive_png.is_some() && m.emissive_strength > 1.0) {
        doc["extensionsUsed"] = json!(["KHR_materials_emissive_strength"]);
    }
    if !images.is_empty() {
        doc["images"] = json!(images);
        doc["textures"] = json!(textures);
        doc["samplers"] = json!([{"magFilter": 9729, "minFilter": 9987, "wrapS": 10497, "wrapT": 10497}]);
    }
    let mut json_bytes = serde_json::to_vec(&doc).expect("json");
    pad4(&mut json_bytes, b' ');

    let total = 12 + 8 + json_bytes.len() + 8 + bin.len();
    let mut out = Vec::with_capacity(total);
    out.extend_from_slice(b"glTF");
    out.extend_from_slice(&2u32.to_le_bytes());
    out.extend_from_slice(&(total as u32).to_le_bytes());
    out.extend_from_slice(&(json_bytes.len() as u32).to_le_bytes());
    out.extend_from_slice(b"JSON");
    out.extend_from_slice(&json_bytes);
    out.extend_from_slice(&(bin.len() as u32).to_le_bytes());
    out.extend_from_slice(b"BIN\0");
    out.extend_from_slice(&bin);
    out
}
