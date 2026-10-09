//! A minimal glTF binary (.glb) writer: a root node with a child node per part (each its own
//! mesh, placed at its pivot so it can be moved or turned about it), a primitive per material,
//! each material's texture embedded as PNG.

use serde_json::{json, Value};

pub struct Material {
    pub texture_png: Option<Vec<u8>>,
    /// See-through (glass): alpha-blended.
    pub blend: bool,
    /// Cut out where the texture's alpha is low (grilles, decals).
    pub mask: bool,
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

fn pad4(buf: &mut Vec<u8>, byte: u8) {
    while buf.len() % 4 != 0 {
        buf.push(byte);
    }
}

pub fn write(parts: &[Part], mats: &[Material]) -> Vec<u8> {
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
        if let Some(png) = &m.texture_png {
            pad4(&mut bin, 0);
            let start = bin.len();
            bin.extend_from_slice(png);
            views.push(json!({"buffer": 0, "byteOffset": start, "byteLength": png.len()}));
            images.push(json!({"bufferView": views.len() - 1, "mimeType": "image/png"}));
            textures.push(json!({"source": images.len() - 1, "sampler": 0}));
            pbr["baseColorTexture"] = json!({"index": textures.len() - 1});
        } else {
            pbr["baseColorFactor"] = json!([0.8, 0.8, 0.82, 1.0]);
        }
        if m.blend {
            pbr["roughnessFactor"] = json!(0.1);
        }
        let mut mat = json!({"pbrMetallicRoughness": pbr, "doubleSided": true});
        if m.blend {
            mat["alphaMode"] = json!("BLEND");
        } else if m.mask {
            mat["alphaMode"] = json!("MASK");
            mat["alphaCutoff"] = json!(0.5);
        }
        materials.push(mat);
    }

    for part in parts {
        let mut min = [f32::INFINITY; 3];
        let mut max = [f32::NEG_INFINITY; 3];
        for (p, _, _) in &part.verts {
            for a in 0..3 {
                min[a] = min[a].min(p[a]);
                max[a] = max[a].max(p[a]);
            }
        }
        // Interleaved vertices: 8 floats each.
        pad4(&mut bin, 0);
        let start = bin.len();
        for (p, n, uv) in &part.verts {
            for f in p.iter().chain(n.iter()).chain(uv.iter()) {
                bin.extend_from_slice(&f.to_le_bytes());
            }
        }
        views.push(json!({"buffer": 0, "byteOffset": start, "byteLength": bin.len() - start, "byteStride": 32, "target": 34962}));
        let view = views.len() - 1;
        let count = part.verts.len();
        let a0 = accessors.len();
        accessors.push(json!({"bufferView": view, "byteOffset": 0, "componentType": 5126, "count": count, "type": "VEC3", "min": min, "max": max}));
        accessors.push(json!({"bufferView": view, "byteOffset": 12, "componentType": 5126, "count": count, "type": "VEC3"}));
        accessors.push(json!({"bufferView": view, "byteOffset": 24, "componentType": 5126, "count": count, "type": "VEC2"}));
        let mut primitives = Vec::new();
        for (indices, material) in &part.prims {
            pad4(&mut bin, 0);
            let start = bin.len();
            for i in indices {
                bin.extend_from_slice(&i.to_le_bytes());
            }
            views.push(json!({"buffer": 0, "byteOffset": start, "byteLength": bin.len() - start, "target": 34963}));
            accessors.push(json!({"bufferView": views.len() - 1, "componentType": 5125, "count": indices.len(), "type": "SCALAR"}));
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
