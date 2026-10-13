# Remastered materials

Mirror's Edge's materials remastered for ray tracing (#101). The game keeps its own art (its
diffuse, normal and specular maps, read from your install); this folder adds what it's made of:

- `me.ron`: the table. Each surface (plaster, concrete, roof, painted or bare metal, wood,
  fabric, ...) has a detail set, its real size, roughness and metalness, and how strongly its
  detail shows. Rules give each material (by its path) its surface.
- `surfaces/<set>/`: the detail sets, 1024 square: `albedo.jpg` (only its brightness is used,
  to vary the game's colour), `normal.jpg` (OpenGL convention) and `rough.jpg`.

The game bakes each material once (`src/me_remaster.rs`): the game's colour varied by the
detail's brightness and kept under real white paint's, its normal map with the detail's over it,
and a roughness and metalness map; with mipmaps.

## Sources

All CC0 (public domain): free to use, change and share.

| Set | From | By | One tile |
|---|---|---|---|
| `plaster` | [white_plaster_02](https://polyhaven.com/a/white_plaster_02) (Poly Haven) | Rob Tuytel | 1.00 m |
| `concrete` | [concrete_wall_008](https://polyhaven.com/a/concrete_wall_008) (Poly Haven) | Dario Barresi, Charlotte Baglioni | 2.71 m |
| `concrete_floor` | [concrete_floor_01](https://polyhaven.com/a/concrete_floor_01) (Poly Haven) | Rob Tuytel | 2.00 m |
| `plywood` | [plywood](https://polyhaven.com/a/plywood) (Poly Haven) | Rob Tuytel | 0.50 m |
| `hessian` | [hessian_230](https://polyhaven.com/a/hessian_230) (Poly Haven) | colormass, Rico Cilliers | 0.27 m |
| `painted_metal` | [PaintedMetal004](https://ambientcg.com/a/PaintedMetal004) (ambientCG) | ambientCG (Lennart Demes) | 1.00 m |
| `brushed_metal` | [Metal054A](https://ambientcg.com/a/Metal054A) (ambientCG) | ambientCG (Lennart Demes) | 1.00 m |
