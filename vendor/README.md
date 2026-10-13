# Vendored crates

## bevy_solari 0.19.1

Bevy's ray tracer, as published, plus two fixes marked "Portal Runner patch": one in
`src/scene/blas.rs` (`prepare_raytracing_blas`), and one in `src/realtime/prepare.rs`
(`prepare_solari_lighting_resources`).

Solari compacts each mesh's BLAS through a queue, and compacts at most 400k vertices of them a
frame. If a mesh is uploaded again before its queue entry has been reached, the queue holds two
"not started" entries that both point at the mesh's new BLAS. Both then prepare its compaction,
and wgpu reports "Compaction is already being prepared", after which the game crashes. That
happens when big meshes use up the per-frame budget (Portal with RTX's floor button) while
others are being re-uploaded (Faith's third-person body). The fix drops a mesh's older queue
entries when it's queued again.

A camera's lighting resources are made once for its size, with DLSS ray reconstruction's
textures only if it has ray reconstruction then. Ray-traced lighting is switched on and off while
the game runs, and DLSS goes between ray reconstruction and plain upscaling with it: the second
fix makes them again when ray reconstruction comes or goes (else it denoises nothing, and the
traced light stays grainy).

Rays through cut-outs (alpha-masked materials: fences, grates, a facade's window cut-outs):
as published, Solari builds every BLAS opaque and has no alpha test, so the game left cut-out
meshes out of the traced scene (no shadow, nothing in reflections). Now the game lists the meshes
it draws with a cut-out material (`scene::RaytracingAlphaMasked`, a resource extracted to the
render world); `blas.rs` builds theirs non-opaque, the GPU material carries the clip value (in
what was padding: `binder.rs`, `raytracing_scene_bindings.wgsl`'s `Material.alpha_cutoff`), and
`trace_ray_plain` loops over candidate hits, confirming one only where the base colour's alpha
reaches it (`alpha_passes`). Opaque meshes stay on the fast path.

Drop these, and the `[patch.crates-io]` entry in the root `Cargo.toml`, once Bevy's own release
carries fixes.
