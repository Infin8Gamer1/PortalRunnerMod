enable wgpu_ray_query;

#define_import_path bevy_solari::scene_bindings

#import bevy_pbr::lighting::perceptualRoughnessToRoughness
#import bevy_pbr::pbr_functions::calculate_tbn_mikktspace

struct InstanceGeometryIds {
    vertex_buffer_id: u32,
    vertex_buffer_offset: u32,
    index_buffer_id: u32,
    index_buffer_offset: u32,
    triangle_count: u32,
}

struct VertexBuffer { vertices: array<PackedVertex> }

struct IndexBuffer { indices: array<u32> }

struct PackedVertex {
    a: vec4<f32>,
    b: vec4<f32>,
    tangent: vec4<f32>,
}

struct Vertex {
    position: vec3<f32>,
    normal: vec3<f32>,
    uv: vec2<f32>,
    tangent: vec4<f32>,
}

fn unpack_vertex(packed: PackedVertex) -> Vertex {
    var vertex: Vertex;
    vertex.position = packed.a.xyz;
    vertex.normal = vec3(packed.a.w, packed.b.xy);
    vertex.uv = packed.b.zw;
    vertex.tangent = packed.tangent;
    return vertex;
}

struct Material {
    normal_map_texture_id: u32,
    base_color_texture_id: u32,
    emissive_texture_id: u32,
    metallic_roughness_texture_id: u32,

    base_color: vec3<f32>,
    perceptual_roughness: f32,
    emissive: vec3<f32>,
    metallic: f32,
    _padding: vec3<f32>,
    reflectance: f32,
}

const TEXTURE_MAP_NONE = 0xFFFFFFFFu;

const MIRROR_ROUGHNESS_THRESHOLD = 0.001f;

struct LightSource {
    kind: u32, // 1 bit for kind, 31 bits for extra data
    id: u32,
}

const LIGHT_SOURCE_KIND_EMISSIVE_MESH = 0u;
const LIGHT_SOURCE_KIND_DIRECTIONAL = 1u;

struct DirectionalLight {
    direction_to_light: vec3<f32>,
    cos_theta_max: f32,
    luminance: vec3<f32>,
    inverse_pdf: f32,
}

const LIGHT_NOT_PRESENT_THIS_FRAME = 0xFFFFFFFFu;

// A portal's opening (an ellipse `half_x` by `half_y` across `right` and `up`), facing `normal`;
// `other` is its partner, `to_other` takes what goes into it to where it comes out of that.
struct Portal {
    center: vec3<f32>,
    half_x: f32,
    normal: vec3<f32>,
    half_y: f32,
    right: vec3<f32>,
    other: u32,
    up: vec3<f32>,
    _padding: f32,
    to_other: mat4x4<f32>,
    from_other: mat4x4<f32>,
}

// The linked portals: none, or a pair.
struct Portals {
    count: u32,
    portals: array<Portal, 2>,
}

@group(0) @binding(0) var<storage> vertex_buffers: binding_array<VertexBuffer>;
@group(0) @binding(1) var<storage> index_buffers: binding_array<IndexBuffer>;
@group(0) @binding(2) var textures: binding_array<texture_2d<f32>>;
@group(0) @binding(3) var samplers: binding_array<sampler>;
@group(0) @binding(4) var<storage> materials: array<Material>;
@group(0) @binding(5) var tlas: acceleration_structure;
@group(0) @binding(6) var<storage> transforms: array<mat4x4<f32>>; // TODO: Use mat3x4<f32>?
@group(0) @binding(7) var<storage> previous_frame_transforms: array<mat4x4<f32>>; // TODO: Use mat3x4<f32>?
@group(0) @binding(8) var<storage> geometry_ids: array<InstanceGeometryIds>;
@group(0) @binding(9) var<storage> material_ids: array<u32>; // TODO: Store material_id in instance_custom_index instead?
@group(0) @binding(10) var<storage> light_sources: array<LightSource>;
@group(0) @binding(11) var<storage> directional_lights: array<DirectionalLight>;
@group(0) @binding(12) var<storage> previous_frame_light_id_translations: array<u32>;
@group(0) @binding(13) var brdf_dfg_lut: texture_2d<f32>;
@group(0) @binding(14) var brdf_dfg_lut_sampler: sampler;
@group(0) @binding(15) var<storage> portals: Portals;

const RAY_T_MIN = 0.001f;
const RAY_T_MAX = 100000.0f;

const RAY_NO_CULL = 0xFFu;

const NO_PORTAL = 0xFFFFFFFFu;
// How far short of a portal's surface a ray stops before going through it (the portal lies on
// its wall's surface), and how far past the other it starts again.
const PORTAL_EPSILON = 0.01f;

// A ray through the scene's geometry alone.
fn trace_ray_plain(ray_origin: vec3<f32>, ray_direction: vec3<f32>, ray_t_min: f32, ray_t_max: f32, ray_flag: u32) -> RayIntersection {
    let ray = RayDesc(ray_flag, RAY_NO_CULL, ray_t_min, ray_t_max, ray_origin, ray_direction);
    var rq: ray_query;
    rayQueryInitialize(&rq, tlas, ray);
    rayQueryProceed(&rq);
    return rayQueryGetCommittedIntersection(&rq);
}

struct PortalCrossing {
    index: u32,
    t: f32,
}

// The linked portal a ray goes into (from in front, through its opening) first between
// `ray_t_min` and `ray_t_max`, and where along it; NO_PORTAL if none.
fn portal_crossing(ray_origin: vec3<f32>, ray_direction: vec3<f32>, ray_t_min: f32, ray_t_max: f32) -> PortalCrossing {
    var best = PortalCrossing(NO_PORTAL, ray_t_max);
    for (var i = 0u; i < portals.count; i++) {
        let p = portals.portals[i];
        let into = dot(ray_direction, p.normal);
        if into > -1e-5 { continue; }
        let t = dot(p.center - ray_origin, p.normal) / into;
        if t < ray_t_min || t > best.t { continue; }
        let q = ray_origin + ray_direction * t - p.center;
        let x = dot(q, p.right) / p.half_x;
        let y = dot(q, p.up) / p.half_y;
        if x * x + y * y > 1.0 { continue; }
        best = PortalCrossing(i, t);
    }
    return best;
}

// After `trace_ray`: how many portals the ray went through, and the transform that takes
// where it ended up back to where that appears from where it started (behind the portal it
// went into: "unfolded").
var<private> portals_crossed: u32;
var<private> portal_unfold: mat4x4<f32>;

// A ray through the scene, through the linked portals (at most two of them): what it hits, as
// it really is. `portals_crossed` and `portal_unfold` say how it got there.
fn trace_ray(ray_origin: vec3<f32>, ray_direction: vec3<f32>, ray_t_min: f32, ray_t_max: f32, ray_flag: u32) -> RayIntersection {
    var origin = ray_origin;
    var direction = ray_direction;
    var t_min = ray_t_min;
    var t_max = ray_t_max;
    portals_crossed = 0u;
    portal_unfold = mat4x4<f32>(vec4(1.0, 0.0, 0.0, 0.0), vec4(0.0, 1.0, 0.0, 0.0), vec4(0.0, 0.0, 1.0, 0.0), vec4(0.0, 0.0, 0.0, 1.0));
    for (var k = 0u; k < 2u; k++) {
        let crossing = portal_crossing(origin, direction, t_min, t_max);
        if crossing.index == NO_PORTAL { break; }
        // (Up to the portal: what's in front of it.)
        let hit = trace_ray_plain(origin, direction, t_min, max(t_min, crossing.t - PORTAL_EPSILON), ray_flag);
        if hit.kind != RAY_QUERY_INTERSECTION_NONE { return hit; }
        let p = portals.portals[crossing.index];
        origin = (p.to_other * vec4(origin + direction * crossing.t, 1.0)).xyz;
        direction = normalize((p.to_other * vec4(direction, 0.0)).xyz);
        t_max -= crossing.t;
        t_min = PORTAL_EPSILON;
        portals_crossed += 1u;
        portal_unfold = portal_unfold * p.from_other;
    }
    return trace_ray_plain(origin, direction, t_min, t_max, ray_flag);
}

fn sample_texture(id: u32, uv: vec2<f32>) -> vec3<f32> {
    return textureSampleLevel(textures[id], samplers[id], uv, 0.0).rgb; // TODO: Mipmap
}

struct ResolvedMaterial {
    base_color: vec3<f32>,
    emissive: vec3<f32>,
    reflectance: f32,
    perceptual_roughness: f32,
    roughness: f32,
    metallic: f32,
}

struct ResolvedRayHitFull {
    world_position: vec3<f32>,
    previous_frame_world_position: vec3<f32>,
    world_normal: vec3<f32>,
    geometric_world_normal: vec3<f32>,
    world_tangent: vec4<f32>,
    uv: vec2<f32>,
    triangle_area: f32,
    triangle_count: u32,
    material: ResolvedMaterial,
}

fn resolve_material(material: Material, uv: vec2<f32>) -> ResolvedMaterial {
    var m: ResolvedMaterial;

    m.base_color = material.base_color.rgb;
    if material.base_color_texture_id != TEXTURE_MAP_NONE {
        m.base_color *= sample_texture(material.base_color_texture_id, uv);
    }

    m.emissive = material.emissive.rgb;
    if material.emissive_texture_id != TEXTURE_MAP_NONE {
        m.emissive *= sample_texture(material.emissive_texture_id, uv);
    }

    m.reflectance = material.reflectance;

    m.perceptual_roughness = material.perceptual_roughness;
    m.metallic = material.metallic;
    if material.metallic_roughness_texture_id != TEXTURE_MAP_NONE {
        let metallic_roughness = sample_texture(material.metallic_roughness_texture_id, uv);
        m.perceptual_roughness *= metallic_roughness.g;
        m.metallic *= metallic_roughness.b;
    }

    m.roughness = m.perceptual_roughness * m.perceptual_roughness;

    return m;
}

fn resolve_ray_hit_full(ray_hit: RayIntersection) -> ResolvedRayHitFull {
    let barycentrics = vec3(1.0 - ray_hit.barycentrics.x - ray_hit.barycentrics.y, ray_hit.barycentrics);
    return resolve_triangle_data_full(ray_hit.instance_index, ray_hit.primitive_index, barycentrics);
}

fn load_vertices(instance_geometry_ids: InstanceGeometryIds, triangle_id: u32) -> array<Vertex, 3> {
    let index_buffer = &index_buffers[instance_geometry_ids.index_buffer_id].indices;
    let vertex_buffer = &vertex_buffers[instance_geometry_ids.vertex_buffer_id].vertices;

    let indices_i = (triangle_id * 3u) + vec3(0u, 1u, 2u) + instance_geometry_ids.index_buffer_offset;
    let indices = vec3((*index_buffer)[indices_i.x], (*index_buffer)[indices_i.y], (*index_buffer)[indices_i.z]) + instance_geometry_ids.vertex_buffer_offset;

    return array<Vertex, 3>(
        unpack_vertex((*vertex_buffer)[indices.x]),
        unpack_vertex((*vertex_buffer)[indices.y]),
        unpack_vertex((*vertex_buffer)[indices.z])
    );
}

fn transform_positions(transform: mat4x4<f32>, vertices: array<Vertex, 3>) -> array<vec3<f32>, 3> {
    return array<vec3<f32>, 3>(
        (transform * vec4(vertices[0].position, 1.0)).xyz,
        (transform * vec4(vertices[1].position, 1.0)).xyz,
        (transform * vec4(vertices[2].position, 1.0)).xyz
    );
}

fn resolve_triangle_data_full(instance_id: u32, triangle_id: u32, barycentrics: vec3<f32>) -> ResolvedRayHitFull {
    let material_id = material_ids[instance_id];
    let material = materials[material_id];

    let transform = transforms[instance_id];
    let previous_frame_transform = previous_frame_transforms[instance_id];

    let instance_geometry_ids = geometry_ids[instance_id];
    let vertices = load_vertices(instance_geometry_ids, triangle_id);

    let world_vertices = transform_positions(transform, vertices);
    let world_position = mat3x3(world_vertices[0], world_vertices[1], world_vertices[2]) * barycentrics;

    let previous_frame_world_vertices = transform_positions(previous_frame_transform, vertices);
    let previous_frame_world_position = mat3x3(previous_frame_world_vertices[0], previous_frame_world_vertices[1], previous_frame_world_vertices[2]) * barycentrics;

    let uv = mat3x2(vertices[0].uv, vertices[1].uv, vertices[2].uv) * barycentrics;

    let local_tangent = mat3x3(vertices[0].tangent.xyz, vertices[1].tangent.xyz, vertices[2].tangent.xyz) * barycentrics;
    let world_tangent = vec4(
        normalize(mat3x3(transform[0].xyz, transform[1].xyz, transform[2].xyz) * local_tangent),
        vertices[0].tangent.w,
    );

    let local_normal = mat3x3(vertices[0].normal, vertices[1].normal, vertices[2].normal) * barycentrics; // TODO: Use barycentric lerp, ray_hit.object_to_world, cross product geo normal
    var world_normal = normalize(mat3x3(transform[0].xyz, transform[1].xyz, transform[2].xyz) * local_normal);
    let geometric_world_normal = world_normal;
    if material.normal_map_texture_id != TEXTURE_MAP_NONE {
        let TBN = calculate_tbn_mikktspace(world_normal, world_tangent);
        let T = TBN[0];
        let B = TBN[1];
        let N = TBN[2];
        var Nt = sample_texture(material.normal_map_texture_id, uv) * 2.0 - 1.0;
        Nt.z = sqrt(max(1.0 - dot(Nt.xy, Nt.xy), 0.0)); // Reconstruct Z to support two-channel normal maps
        world_normal = normalize(Nt.x * T + Nt.y * B + Nt.z * N);
    }

    let triangle_edge0 = world_vertices[0] - world_vertices[1];
    let triangle_edge1 = world_vertices[0] - world_vertices[2];
    let triangle_area = length(cross(triangle_edge0, triangle_edge1)) / 2.0;

    let resolved_material = resolve_material(material, uv);

    return ResolvedRayHitFull(
        world_position,
        previous_frame_world_position,
        world_normal,
        geometric_world_normal,
        world_tangent,
        uv,
        triangle_area,
        instance_geometry_ids.triangle_count,
        resolved_material,
    );
}
