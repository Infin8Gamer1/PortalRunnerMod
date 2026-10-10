# Vendored crates

## bevy_solari 0.19.1

Bevy's ray tracer, as published, plus one fix in `src/scene/blas.rs`
(`prepare_raytracing_blas`, marked "Portal Runner patch").

Solari compacts each mesh's BLAS through a queue, and compacts at most 400k vertices of them a
frame. If a mesh is uploaded again before its queue entry has been reached, the queue holds two
"not started" entries that both point at the mesh's new BLAS. Both then prepare its compaction,
and wgpu reports "Compaction is already being prepared", after which the game crashes. That
happens when big meshes use up the per-frame budget (Portal with RTX's floor button) while
others are being re-uploaded (Faith's third-person body). The fix drops a mesh's older queue
entries when it's queued again.

Drop this, and the `[patch.crates-io]` entry in the root `Cargo.toml`, once Bevy's own release
carries a fix.
