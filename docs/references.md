# Reference and provenance index

This index preserves sources already used by the project, not a fresh literature review or a
claim that every technique on these pages is implemented. Follow the owning design document for
the actual implemented subset, experiments, limits and tests. Dependencies stay in `Cargo.lock`.

## Visual objective versus shipped content

- [Generated industrial target and original prompt](visual-direction.md): reference only, introduced
  in commit `a25baf8`; stored at `concepts/photoreal-industrial-target-v1.png`. It is not a runtime
  image, photograph of an existing playable location, or texture loaded by the game.
- [Current native checkpoint gallery](checkpoints/2026-09-06.md): actual RenderDoc Vulkan images,
  explicitly below target. Historical runtime views remain in `screenshots/` and Git history.
- [Material source manifest](../assets/materials/sources.json): five Poly Haven scans, exact URLs,
  authors, scale, dimensions and hashes; [cooking contract](../assets/materials/README.md).
- [HDR source manifest](../assets/environment/source.json): Overcast Soil (Pure Sky), authors,
  input hash and offline toolchain; [environment contract](../assets/environment/README.md).
- [Poly Haven asset license](https://polyhaven.com/license), rechecked 2026-09-06: the asset files
  are CC0 and may be redistributed. Website text, logos and example renders are not covered by that
  asset grant. Only reviewed cooked assets and our own runtime captures are included here; no page
  scrape or upstream gallery images. This does not set a license for the project's code.

## Geometry, simulation and rendering

| Source | Why it is referenced here |
| --- | --- |
| [Eberly — Method of Separating Axes](https://www.geometrictools.com/Documentation/MethodOfSeparatingAxes.pdf) | Convex SAT and translating contact; exact subset and bounds in [convex-inspection](convex-inspection.md) |
| [Houston, Wiebe, Batty — RLE sparse level sets](https://benhouston3d.com/siggraph/2004-1.html) | Representation context for [refined-volumes](refined-volumes.md), not a claim of a full level-set solver |
| [Hiller and Lipson — dynamic simulation](https://www.creativemachineslab.com/uploads/6/9/3/4/69340277/dynamicsimulation.pdf) | Structural-model background and limitations in [structural-elasticity](structural-elasticity.md) |
| [numgeo beam formulation](https://j-machacek.github.io/numgeo/theory/elements/beam.html) | Beam reference for analytical structural cases |
| [AutoFEM square beam torsion](https://autofem.com/examples/torsion_of_a_beam_with_the_squ.html) | Independent analytical validation example, not imported code |
| [Filament lighting reference](https://google.github.io/filament/Filament.md.html) | Diffuse convolution and prefiltered specular/BRDF separation, subset in environment documentation |
| [WGSL interpolation](https://www.w3.org/TR/WGSL/#interpolation) | Flat cut-provenance marker after the reproduced perspective-interpolation defect; [historical receipt](reference-scene.md) |

## Production tools

- [wgpu upstream](https://github.com/gfx-rs/wgpu) and [winit upstream](https://github.com/rust-windowing/winit):
  versions and enabled backends are defined by `Cargo.toml`, not the latest upstream release.
- [RenderDoc](https://github.com/baldurk/renderdoc), [Blender](https://www.blender.org/),
  [Tracy v0.14.1](https://github.com/wolfpld/tracy/releases/tag/v0.14.1) and its
  [manual](https://github.com/wolfpld/tracy/blob/v0.14.1/manual/tracy.md).
- [Local tool recipes and isolation](tooling.md),
  [observed versions/source identities](../tools/toolchain-observed.json).
- [Future agent interface](agent-tooling.md) records its external feasibility reference separately
  from the unimplemented game adapter. [Adaptive director](adaptive-director.md) is a design contract,
  not an enabled local model or API connection.

No external tutorial or research PDF is copied into this repository. Record new references next to
the implementation they inform; verify current licenses and APIs before importing new content.
