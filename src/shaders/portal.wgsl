// A portal's surface: what its camera sees, in screen space, inside an oval with a glowing
// rim. Closed (no partner), a swirl of its colour.

#import bevy_pbr::forward_io::VertexOutput
#import bevy_pbr::mesh_view_bindings::view

struct PortalUniform {
    color: vec4<f32>,
    // x: linked (show the view), y: time, z: open (0..1, the opening animation),
    // w: mode (0 the oval surface, 1 the tunnel box: no oval, 2 the deepest view: no texture).
    params: vec4<f32>,
}

@group(#{MATERIAL_BIND_GROUP}) @binding(0) var<uniform> portal: PortalUniform;
@group(#{MATERIAL_BIND_GROUP}) @binding(1) var view_tex: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(2) var view_smp: sampler;

fn hash(p: vec2<f32>) -> f32 {
    return fract(sin(dot(p, vec2<f32>(127.1, 311.7))) * 43758.5453);
}

fn noise(p: vec2<f32>) -> f32 {
    let i = floor(p);
    let f = fract(p);
    let u = f * f * (3.0 - 2.0 * f);
    let a = mix(hash(i), hash(i + vec2<f32>(1.0, 0.0)), u.x);
    let b = mix(hash(i + vec2<f32>(0.0, 1.0)), hash(i + vec2<f32>(1.0, 1.0)), u.x);
    return mix(a, b, u.y);
}

@fragment
fn fragment(in: VertexOutput) -> @location(0) vec4<f32> {
    let col = portal.color.rgb;
    let linked = portal.params.x;
    let t = portal.params.y;
    let open = max(portal.params.z, 0.001);
    let mode = portal.params.w;

    let p = in.uv * 2.0 - 1.0;
    let ang = atan2(p.y, p.x);
    var r = 0.0;
    if mode != 1.0 {
        // The oval, with a slowly crawling edge.
        let edge = 1.0 - 0.035 * noise(vec2<f32>(ang * 3.0 + 10.0, t * 1.7));
        r = length(p) / open;
        if r > edge {
            discard;
        }
        r = r / edge;
    }

    var c: vec3<f32>;
    if linked > 0.5 && mode < 1.5 {
        // The main pass's own viewport: with DLSS it draws at a lower resolution than the
        // window's (which `viewport` is), and is upscaled after.
        let screen = (in.position.xy - view.main_pass_viewport.xy) / view.main_pass_viewport.zw;
        c = textureSample(view_tex, view_smp, screen).rgb;
    } else {
        // Closed: a slow swirl of its colour.
        let lp = length(p);
        let s = noise(vec2<f32>(ang * 2.0 + t * 1.3 + lp * 5.0, lp * 4.0 - t * 0.8));
        let s2 = noise(vec2<f32>(ang * 5.0 - t * 2.1, lp * 9.0 + t));
        c = col * (0.25 + 0.55 * s + 0.25 * s2) * (0.55 + 0.6 * lp);
    }
    if mode != 1.0 {
        // The rim: bright, flickering, fading inwards.
        let flick = 0.75 + 0.25 * noise(vec2<f32>(ang * 9.0, t * 7.0));
        let rim = smoothstep(0.80, 0.98, r);
        c = mix(c, col * 1.8 * flick, rim);
        c = c + col * 0.22 * smoothstep(0.55, 0.98, r) * flick;
        // Opening: a flash of colour across the whole surface.
        c = mix(c, col * 1.4, clamp(1.0 - portal.params.z, 0.0, 1.0) * 0.8);
    }
    return vec4<f32>(c, 1.0);
}
