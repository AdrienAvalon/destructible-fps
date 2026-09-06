# Dynamic distant-sky visibility

The HDR sky must not illuminate every surface through walls. This increment masks its diffuse
and specular contributions using depth views of the **actual rendered geometry**. It does not
alter voxel state, collision, rigid-body simulation, materials, replication, commands or snapshots.
It is an intermediate visibility solution, not a global-illumination solver or photorealism proof.

## Runtime contract

Sixteen fixed directions use stratified upper-hemisphere samples and their antipodal partners.
Unlike eight equal octants, this includes low-elevation light entering through openings. Each view
owns one 1,024² layer of a `Depth32Float` array: **67,108,864 texel bytes (64 MiB)**, excluding driver
overhead. This is fixed allocation, not a constant-time guarantee: refresh work remains proportional
to admitted caster geometry. There are no extra dependencies, asset downloads or precomputed bakes.

Each orthographic view is 128 m wide and 256 m deep, anchored around the camera in 16 m cells and
snapped in light-space texels. The receiver's coverage follows the actual camera continuously,
not the cached anchor: full weighting to 32 m, smooth fade to unoccluded at 48 m. The maximum anchor
offset is sqrt(3)*8 m, so the receiver range plus drift and PCF footprint fits inside the 64 m view
radius. Casters beyond the view's finite depth remain missing coverage. This is not large-world
streaming, cascaded occlusion or an origin-rebasing solution.

The complete sixteen-view set is rebuilt in the same encoded frame after:

- accepted chunk upload/replacement, including an empty mesh that removes a chunk;
- body-mesh upload, body reset, or any actual instance-matrix change;
- remote-player instance additions, removals or pose changes;
- a camera-anchor change.

No-op poses and camera motion inside one cell reuse depth. Coverage uniforms still follow camera
motion during reuse. Skipped surface acquisition does not consume pending invalidation. A rebuild
is not spread over sixteen frames; no obsolete bake survives destruction after its current mesh
has reached the renderer. A mesh still pending in the background worker retains the previous
rendered geometry and therefore its previous lighting. Invalidation follows accepted mesh/pose
state, not unvalidated network input. Chunk and body casters are conservatively frustum-culled
per depth view; bounded remote-player instances are drawn together. Both sides cast depth so thin
derived cut surfaces are not silently omitted.

## Shading and limits

At a surface, sixteen linear comparison samples (hardware 2×2 PCF) provide a visibility estimate.
Diffuse weights are clamped normal/direction cosines, normalized by their sum. Specular weights
use a smooth lobe around the reflected view direction, with exponent interpolated from 32 to 1
by roughness. This is a **coarse specular approximation**, not a GGX visibility integral or a local
reflection. A geometric-normal receiver offset of 0.06 m, depth offset of 0.03/256 and raster slope
bias trade acne against leaks at the current metre-scale geometry; sub-voxel geometry will need
recalibration. A one-voxel closed room is covered by the real-GPU regression.

Only environment light is masked: the directional fill retains its own shadow map; sky background
and haze are not multiplied by this visibility. There is no arbitrary positive ambient floor.
The [Filament discussion of ambient occlusion](https://google.github.io/filament/Filament.md.html)
explains separating distant illumination from geometric visibility, and why diffuse and specular
occlusion are different approximations. Our directional maps and specular weights are a local
implementation choice, not Filament's algorithm or a claim of exact transport.

Haze now samples the environment's lowest-frequency prefilter, not the sharp HDR panorama.
Otherwise detailed clouds become visible on newly darkened walls. This fixes that image artifact;
it is not volumetric light transport. Haze is still global and can brighten an unlit interior.

Angular undersampling, the finite range, shadow-map resolution, silhouettes and grazing bias remain
limitations. Ground-blocked lower directions also suppress the HDR's approximate ground bounce;
real reflected light is **not** restored. Interiors are now darker, but they still lack local
bounce, color bleeding, emitters and physically based fog. Fine contact shadows, temporal filtering,
local reflection probes and better geometry remain separate work. Sixteen directions reduce the
visible bands from the initial eight-direction experiment; they do not eliminate aliasing.

## Tests and measurements

```bash
cargo test --lib render::sky
cargo test --lib render::sky -- --ignored --nocapture
cargo test --release --lib render::sky -- --ignored --nocapture
cargo test --test material_projection -- --ignored --nocapture
cargo run --release --bin playable-demo -- --showcase-closeup --smoke-seconds 12
cargo run --release --bin playable-demo -- --lighting-stress --smoke-seconds 15
cargo run --release --bin playable-demo -- --structural-lab --smoke-seconds 8
```

The hardware-only tests use the production depth pipelines, instance transforms and WGSL receiver:
one-voxel sealed walls/roof block light; removing the roof exposes only the geometrically visible
portion of the sky; a slab blocks it, translation removes that obstruction, and restoring then
deleting the slab releases it again. Independent CPU rays through the exact test geometry provide
the open-room reference. Bare floor stays lit without self-shadowing; matrix/cache reuse and
coverage across an anchor transition are tested. The renderer leaves the authority fingerprint
unchanged. The separate player instance draw path is tested with a blocking synthetic slab,
translated away, restored and removed. Sky GPU tests use the production comparison sampler.
The existing material GPU test additionally verifies low-frequency haze sampling against decoded
last-mip asset bytes. Renderer-level invalidation hooks are source-reviewed and exercised by the
live smokes, but not isolated pixel assertions: these hardware fixtures explicitly invalidate after
installing geometry/poses. There is no individual body-removal API; snapshot reset clears bodies.
Ordinary headless runs intentionally ignore hardware-only GPU tests and are not hardware evidence.

The explicitly synthetic `--lighting-stress` fixture moves up to 32 render instances, capped by the
current `MAX_SERVER_PEERS` (16 at this revision), on every frame while the camera orbits the breach.
Its smoke rejects cache hits or missing casters. It does not simulate those players' network,
movement authority or collisions; it measures sustained rendering refresh. The structural lab
separately exercises real automatic fracture, body motion and mesh updates.

The eight-query bounded profiler reports sun depth, sky visibility, HDR scene/resolve and
display/HUD separately. Each
readback slot carries the matching refresh flag: the refresh-only distribution cannot be diluted
by cached frames. Total GPU time includes all four stages. Reports also include cache hits,
rebuild count and last-frame sky draws. See `performance.md` for hardware, resolution, distributions,
limits and actual capture evidence. A favorable cache-hit average is not a performance gate.
