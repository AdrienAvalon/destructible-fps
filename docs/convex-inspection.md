# Oblique native inspection geometry

This is a geometry/rendering milestone, **not photorealistic acceptance**, automatic collapse,
fine weapon integration, or multiplayer promotion. The user-provided ruined-factory target stays
the acceptance reference. A canonical oblique slab removes a representation bottleneck; it cannot
by itself supply production-quality rubble, landscape, assets or illumination.

The [durable checkpoint](checkpoints/2026-09-06.md) preserves selected native images and a new CPU
validation receipt. The original `/tmp/fps-convex-scene-6d4fQA/` logs mentioned below are historical
and no longer available; do not rely on that temporary directory to resume the project.

## One physical source

`src/convex/shape.rs` validates a closed convex solid before publication. Local integer coordinates
are units of **1/256 metre**, bounded to 0..2048 (eight metres, not 2048 metres). Integer origins
stay within ±16384 metres, preserving those lattice positions exactly in float32 after translation.
There are at most 32 vertices, 16 planar convex polygon faces, 8 indices per face, 64 unique edges,
and 32 fragments per immutable inspection scene. Canonical sorting changes indices, not positions
or outward winding. Duplicate/unused vertices, nonplanar/concave/star faces, duplicate supporting
planes, inward faces, open edges, disconnected/pinched surfaces and zero volume are refused.
Every edge has two opposite incidences; Euler, surface connectivity and vertex links are checked.

The mesh uses only these source vertices and triangle fans with hard per-face lighting normals.
No smoothed shell or unrelated coarse collider is substituted. Existing `CpuBodyMesh` upload is
used as a fixed instance transport, **not** as an assertion of simulated rigid-body state. Its
maximum cell is inclusive. Each fragment adds at most 128 render vertices and 288 indices.

Point rays clip the original quantized `FixedRay` against the same integer planes. SAT contacts
test face normals, box axes and edge×box axes; fragment overlap tests both face sets and edge pairs.
Continuous sweeps assume a fixed AABB orientation and constant translation, not angular CCD.
The algorithm follows [Eberly, separating axes, sections 4 and 6](https://www.geometrictools.com/Documentation/MethodOfSeparatingAxes.pdf).
Physical queries use integer scaled micrometres and rational segment parameters, with checked
conversions and i128 cross comparisons. Raw normal components are bounded by 2×2048² = 2^23.
Local query coordinates are bounded to ±256 m, box widths to 16 m and displacement to 128 m.
Far-away misses are rejected in the broad phase before local projection.

Positive-volume initial overlap refuses movement. Tangent sliding and moving away do not block;
directed contact rounds toward the start in whole micrometres. A query error is never converted
into permission to move. Rays through convex faces use strict interiors, unlike the historical
minimum-closed box-ray convention; boundary travel is not claimed to have material thickness.
This is not a complete slope controller or contact-manifold solver.

## Composition and native publication

`InspectionGeometry` owns an immutable `RefinedWorld` snapshot and canonical fragments. Its
constructor refuses positive-volume overlap with actual world leaf boxes or other fragments;
boundary contact is allowed. Queries explicitly visit both sources and return `World`/`Fragment`
attribution. Errors invalidate the complete result, with no partial trace or permissive fallback.
Scene fingerprints include both sources and are deterministic identity checks, not authentication.

There is deliberately no `StaticGeometry` implementation or `GeometryCell` variant for these
fragments. Existing codecs and authority paths remain unchanged. The compile-fail boundary prevents
accidentally passing this composite to consumers that would omit its oblique solids. Authoritative
damage, structural separation, rigid-body transforms and network replication of this representation
remain future work with their own invariants and validation gates.

The native industrial inspector now uses `industrial_oblique_base`; the old stepped collapse apron
is actually absent. `industrial_reference_world` and older fixtures remain frozen regressions.
Four 187.5 mm concrete slabs share exact interfaces with grounded wedges; 16 smaller angular fragments
complete this first authored arrangement. Complete ground cells are checked throughout supporting
footprints, not only at corners. This does not model structural equilibrium or an explosion history.

`PreparedGeometry` requires four stages, exact matching world Arcs and identical canonical fragments
at all stages before deriving their one-time meshes. The stream reserves their geometry inside the
existing resident caps. They upload when the first full source stage is ready; any error exits before
the next render. Each stage probes fragment rays, overlaps and sweeps. The P camera probe uses the
composite trace and does not fabricate penetration energy for unimplemented material interactions.

## Review and validation boundaries

Independent constructor, query, render and composition tests cover malformed shapes, exact
quantization at both render-domain extremes, analytical slabs, six-direction legacy-box oracles,
edge-cross-only separating cases, bounded work, repeated contact steps, missing ground, source
overlaps, competing closest contacts and mesh/source consistency. Full-map resident tests include
every chunk plus all fragments, retaining previous job caps and content headroom.

Claude's bounded plan review identified useful checks for renderer precision, caller error handling
and source binding. Its claims that the coordinate limit was 2048 m and normals needed 2^39 were
independently rejected: the input bound is 2048 **lattice units**, explicitly 8 m. No review verdict
replaced source inspection or testing. A first native preview was visually rejected because the
identical chamfered shapes resembled pedestals and exposed cores had oversized color patches;
its capture is retained separately from the final measurements.

The final bounded Claude review covered selected runtime excerpts, not every added line. It found
no blocking defect. Its chord-limit concern was checked in `world/query/ray.rs`: the base trace
rejects zero/oversized limits and every append is capped, so it cannot supply an oversized prefix.
Constructor constants are the explicit larger shared limits, not the default 64-operation query
budget. Signed fractions normalize negative denominators, and the sweep rejects entry after t=1;
existing six-direction/endpoint tests cover those paths. Remaining visual flattening risk is judged
from native captures, not dismissed by the shader tests. Two additional tests after review cover
the complete 32-fragment constructor budget (and refusal of 33), invalid zero-chord limits, and
coplanar rays at both upper-open and lower-closed voxel interfaces touching strict convex faces.
Neither interface invents a duplicate material chord or a false overlap.

## Cut surfaces

The first preview exposed large orange/white patches on the procedural cut cores. The production
shader now uses approximately 1 cm aggregate and 3 mm grain, with lower mineral contrast and
roughness centred on 0.94. Detail fades to its mean before it is unresolved; the normal and the
flat cut-provenance marker are untouched. This still uses two noise calls and no additional texture
fetches. It is not a scanned fracture surface or a claim that unchanged operation count proves
unchanged GPU cost. GPU tests compare changes over 100 µm, 1 cm and 1 m, enforce the filtered mean
at 2 cm/pixel, and retain the original soil/environment/normal/provenance checks.

## Executable evidence — 2026-09-06

`/tmp/fps-convex-scene-6d4fQA/validate.sh` completed with exit 0: formatting, strict all-target
Clippy, 605 ordinary tests in both debug and release, network/security/authority/standalone-process
and OIDC regressions, three compile-fail documentation tests, and all seven explicitly requested
GPU tests in each profile. The two additional review tests subsequently passed with all ten
`convex_scene` tests in both profiles: **607 distinct ordinary tests per profile** in total.
Clippy/formatting were rerun after those test-only additions. No production test was skipped as
headless coverage. The four required destruction/structural/physics/snapshot benchmarks, five-second
playable smoke, fifteen-second lighting stress, eight-second fine inspection, and twelve-second
industrial smokes in all three views completed. This is regression evidence, not fine multiplayer
promotion or a worst-case loaded-game performance claim.

After every release test, all three standalone binaries were rebuilt. The last test-only rebuild
confirmed the measured binaries were byte-identical:

- `fine-geometry-demo`: `a8ce87f8b9eee42e7609874cf1fc608cc9d3b522acf069d8123ca989cf010ff3`
- `fine-mesh-benchmark`: `8ce09561e440d2e9fa69326445ca1426f24e63cd855bc20f3a263466c945ddc6`

Hardware: i7-13700H, RTX 4050 Laptop 6141 MiB, NVIDIA 610.57.04, Linux 7.2.2-1-cachyos,
Rust 1.97.1. Native Vulkan 1440×900, existing 4×MSAA/exposure 0.75. CPU includes presentation;
GPU timestamps are measured independently. No compilation, RenderDoc or other GPU tests ran
concurrently with these performance smokes. Clocks were not locked and this is one local run,
not a controlled speedup against the different previous scene.

| View | CPU p50 / p95 / p99 ms | GPU p50 / p95 / p99 ms | Peak RSS KiB |
|---|---:|---:|---:|
| Fracture | 3.555 / 3.756 / 4.115 | 2.943 / 3.001 / 3.504 | 292428 |
| Approach | 3.168 / 3.386 / 3.729 | 2.556 / 2.601 / 3.096 | 289400 |
| Wide | 3.012 / 3.167 / 3.458 | 2.406 / 2.446 / 2.722 | 291976 |
| Fine inspection, unchanged fixture | 1.416 / 1.564 / 1.681 | 0.816 / 0.827 / 0.830 | 277248 |

All four authored stages were presented and probed in every industrial smoke, with zero dropped
GPU samples. The final composite fingerprint is `539fb6c946e1740ba68478924d4b36f0`; its voxel
source is `96469965d0b41705ea6f6cdea0c350d5`, finish key `0b1d4107de83db376972b6af965df962`.
The full 132-chunk final resident set has 268694 vertices/485082 indices including the fixed
24 fragments (552 vertices/816 indices), below both previous resident caps and content headroom.
The first full CPU extraction took 98.074 ms outside presentation. Twenty warm final dirty-set
extractions measured p50/p95/p99 16.058/16.247/16.316 ms, eight one-chunk jobs, maximum per-job
work 1362621/4194304. Fixed convex mesh extraction separately measured 0.0032/0.0048/0.0113 ms;
it is precomputed once, not done on a network or frame receive path.

## Native visual inspection

Actual RenderDoc 1.45 Vulkan frame-400 captures, each followed by clean native completion:

- Fracture: `target/tooling/renderdoc-3k2fwsm7` — 306526484 bytes, 261 draws, 14 textures.
- Approach: `target/tooling/renderdoc-vtfscgga` — 304955239 bytes, 269 draws, 14 textures.
- Wide: `target/tooling/renderdoc-udkd93qu` — 297524586 bytes, 293 draws, 14 textures.

Each directory retains its `.rdc`, `renderdoc-result.json`, `process.log` and actual runtime
`breach-thumbnail.png`. These are separate from uninstrumented performance results. Previous
final reference-scene captures remain beside them. Older pre-raster-fix captures
`renderdoc-w_8qapyy`, `renderdoc-ej27oax0`, `renderdoc-c335uea3` and the rejected pedestal preview
`renderdoc-wweou3rw` were moved without deletion into `target/tooling-archive-lHJXgO/`, retaining
the active 2 GiB tooling cap. This local archive is recoverable evidence, not an off-machine backup.

The native images show real oblique boundaries without the old staircase caps, and no large
camouflage patches on the cores. **They still fail the photorealistic target**: the supporting
wedges and slabs are too congruent, small pieces read as sparse placed props, cores look too flat
at ordinary viewing distance, and the broad terrain/architecture lack the reference's density,
natural irregularity and lighting depth. Keep this as a tested engine capability, not visual
acceptance. The next visual work needs a cohesive asset/geometry production pipeline, irregular
supported rubble at several scales, credible fracture detail and richer environment composition;
more isolated shader noise is not a substitute.
