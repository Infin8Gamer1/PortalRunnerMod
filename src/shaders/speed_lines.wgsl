// Speed lines: thin streaks rushing outwards round the edges of the screen, and a slight
// darkening at the corners, both as strong as params.x.

#import bevy_ui::ui_vertex_output::UiVertexOutput

struct SpeedLines {
    // x: strength (0..1), y: time, z: aspect ratio.
    params: vec4<f32>,
}

@group(1) @binding(0) var<uniform> lines: SpeedLines;

fn hash(n: f32) -> f32 {
    return fract(sin(n * 127.1) * 43758.5453);
}

@fragment
fn fragment(in: UiVertexOutput) -> @location(0) vec4<f32> {
    let strength = lines.params.x;
    let t = lines.params.y;
    let aspect = lines.params.z;
    let p = (in.uv - vec2<f32>(0.5)) * vec2<f32>(aspect, 1.0);
    let r = length(p);
    let a = atan2(p.y, p.x) / 6.2831853 + 0.5;

    // Spokes: N around the screen, each on or off, thin, sliding outwards in dashes.
    let n = 140.0;
    let cell = floor(a * n);
    let h = hash(cell);
    let h2 = hash(cell + 31.7);
    let across = abs(fract(a * n) - 0.5) * 2.0;
    let thin = 1.0 - smoothstep(0.05, 0.25 + 0.2 * h2, across);
    let along = fract(r * (1.2 + h) - t * (1.6 + 2.4 * h2) + h * 7.0);
    let dash = smoothstep(0.0, 0.08, along) * (1.0 - smoothstep(0.25, 0.55, along));
    let on = step(0.8 - strength * 0.6, h);
    let edge = smoothstep(0.32, 0.8, r);
    let streak = thin * dash * on * edge * strength * 0.3;

    let vignette = smoothstep(0.45, 1.05, r) * strength * 0.2;
    let alpha = clamp(streak + vignette, 0.0, 1.0);
    if alpha < 0.002 {
        discard;
    }
    let white = vec3<f32>(0.92, 0.96, 1.0);
    let col = white * (streak / max(alpha, 1e-4));
    return vec4<f32>(col, alpha);
}
