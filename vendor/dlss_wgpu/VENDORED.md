# dlss_wgpu, vendored

`dlss_wgpu` 4.0.0 from crates.io, with these changes for PortalRunnerMod (#96):

- Builds against DLSS SDK 310.9.1 as well as 310.5.3: the bindings derive `Default`, so
  fields newer SDKs add are zeroed; the G-buffer attribute array is sized from the headers.
  `cfg(dlss_responsivity_mask)` is set where the headers have Ray Reconstruction's
  responsivity mask (310.9.1 on).
- `ray_reconstruction::RAY_RECONSTRUCTION_PRESET`: Ray Reconstruction's model preset, set as
  a hint for every quality mode before the feature's created (0: DLSS chooses; 4 D, 5 E,
  6 F). `PRESET_F_AVAILABLE` says whether the SDK built against has F: older SDKs'
  libraries crash creating the feature if asked for it.
