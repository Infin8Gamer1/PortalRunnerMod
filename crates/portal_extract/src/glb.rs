//! A minimal glTF binary (.glb) writer: one mesh, one shared vertex buffer (position,
//! normal, UV), a primitive per material, each with its texture embedded as PNG.

use serde_json::{json, Value};

pub struct Primitive {
    pub indices: Vec<u32>,
    pub texture_png: Option<Vec<u8>>,
}

fn pad4(buf: &mut Vec<u8>, byte: u8) {
    while buf.len() % 4 != 0 {
        buf.push(byte);
    }
}

pub fn write(verts: &[([f32; 3], [f32; 3], [f32; 2])], prims: &[Primitive]) -> Vec<u8> {
    let mut bin: Vec<u8> = Vec::new();
    let mut views: Vec<Value> = Vec::new();
    let mut accessors: Vec<Value> = Vec::new();
    let mut images: Vec<Value> = Vec::new();
    let mut textures: Vec<Value> = Vec::new();
    let mut materials: Vec<Value> = Vec::new();
    let mut primitives: Vec<Value> = Vec::new();

    let mut min = [f32::INFINITY; 3];
    let mut max = [f32::NEG_INFINITY; 3];
    for (p, _, _) in verts {
        for a in 0..3 {
            min[a] = min[a].min(p[a]);
            max[a] = max[a].max(p[a]);
        }
    }
    // Interleaved vertices: 8 floats each.
    let start = bin.len();
    for (p, n, uv) in verts {
        for f in p.iter().chain(n.iter()).chain(uv.iter()) {
            bin.extend_from_slice(&f.to_le_bytes());
        }
    }
    views.push(json!({"buffer": 0, "byteOffset": start, "byteLength": bin.len() - start, "byteStride": 32, "target": 34962}));
    let count = verts.len();
    accessors.push(json!({"bufferView": 0, "byteOffset": 0, "componentType": 5126, "count": count, "type": "VEC3", "min": min, "max": max}));
    accessors.push(json!({"bufferView": 0, "byteOffset": 12, "componentType": 5126, "count": count, "type": "VEC3"}));
    accessors.push(json!({"bufferView": 0, "byteOffset": 24, "componentType": 5126, "count": count, "type": "VEC2"}));

    for p in prims {
        pad4(&mut bin, 0);
        let start = bin.len();
        for i in &p.indices {
            bin.extend_from_slice(&i.to_le_bytes());
        }
        views.push(json!({"buffer": 0, "byteOffset": start, "byteLength": bin.len() - start, "target": 34963}));
        accessors.push(json!({"bufferView": views.len() - 1, "componentType": 5125, "count": p.indices.len(), "type": "SCALAR"}));
        let index_accessor = accessors.len() - 1;

        let mut pbr = json!({"metallicFactor": 0.0, "roughnessFactor": 0.6});
        if let Some(png) = &p.texture_png {
            pad4(&mut bin, 0);
            let start = bin.len();
            bin.extend_from_slice(png);
            views.push(json!({"buffer": 0, "byteOffset": start, "byteLength": png.len()}));
            images.push(json!({"bufferView": views.len() - 1, "mimeType": "image/png"}));
            textures.push(json!({"source": images.len() - 1}));
            pbr["baseColorTexture"] = json!({"index": textures.len() - 1});
        } else {
            pbr["baseColorFactor"] = json!([0.8, 0.8, 0.82, 1.0]);
        }
        materials.push(json!({"pbrMetallicRoughness": pbr, "doubleSided": true}));
        primitives.push(json!({
            "attributes": {"POSITION": 0, "NORMAL": 1, "TEXCOORD_0": 2},
            "indices": index_accessor,
            "material": materials.len() - 1,
            "mode": 4
        }));
    }
    pad4(&mut bin, 0);

    let mut doc = json!({
        "asset": {"version": "2.0", "generator": "portal_extract"},
        "scene": 0,
        "scenes": [{"nodes": [0]}],
        "nodes": [{"mesh": 0}],
        "meshes": [{"primitives": primitives}],
        "buffers": [{"byteLength": bin.len()}],
        "bufferViews": views,
        "accessors": accessors,
        "materials": materials,
    });
    if !images.is_empty() {
        doc["images"] = json!(images);
        doc["textures"] = json!(textures);
        doc["samplers"] = json!([{"magFilter": 9729, "minFilter": 9987, "wrapS": 10497, "wrapT": 10497}]);
        for t in doc["textures"].as_array_mut().unwrap() {
            t["sampler"] = json!(0);
        }
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
