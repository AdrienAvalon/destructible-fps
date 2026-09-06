# Exact fine surfaces in the native renderer

`fine-geometry-demo` displays the real typed `RefinedWorld` through the existing Vulkan/PBR/HDR
renderer and bounded `MeshScheduler`. It does not render an AI image, use a coarse collision proxy,
or feed partial cells to the coarse game authority. It is an inspection scene with four **authored**
states: intact layered masonry, a shallow chip, a through-bore, and a large breach. These are not
calibrated bullet/blast outcomes. Existing playable maps, smoothed terrain, weapons and multiplayer
are unchanged. This increment is a visible integration gate, **not photorealism or final gameplay**.

```bash
cargo run --release --bin fine-geometry-demo
cargo run --release --bin fine-geometry-demo -- --smoke-seconds 8
cargo run --release --bin fine-mesh-benchmark -- --iterations 100
cargo build --release --bin fine-geometry-demo
python tools/tooling_smoke.py renderdoc --world fine-inspection
```

Left/right arrows choose a state; up/down orbit; W/S zoom; Escape closes the viewer. There is no
first-person collision controller in this inspection camera. The mesh and exact static collision
queries consume the same fine volume, but player movement through this graphical scene is not
validated by this viewer. The smoke requires all four states to be actually presented at least 35
times, finished meshing and real GPU timing samples. Missing display/GPU, errors, or early closure
cannot pass. Only one job is in flight; a newer user selection discards stale results. No world
mutation, server, network listener, credential, package or privilege is required.

## Geometry and seam contract

`mesh::fine::mesh_fine_chunks` accepts the sealed, shared `StaticGeometry` interface. Uniform cells
and refined cells use the same exact solid-owned boundary extraction against six explicit neighbors.
No representative voxel substitutes for partial occupancy. Internal solid/solid material interfaces
produce no sheet. The fixed 60-byte GPU vertex preserves surface material, flat outward normal and
damage; this first exact pass deliberately leaves the old synthetic fracture-depth overlay off.
The renderer's legacy `exposed_faces` statistic counts pairs of triangles, not these source
rectangles; the fine viewer and benchmark instead report explicit rectangle, vertex and triangle counts.

Naively splitting every boundary rectangle into two triangles leaves T-junctions where fine leaves
meet. The new path instead builds a centre fan with a canonical subdivided perimeter. For each edge,
the cache key identifies its variable-axis metre interval and its two fixed global lattice
coordinates. On the fixed axes a coordinate belongs to one cell, or both cells when it lies on a
metre boundary: at most four candidate pages. All intersecting closed leaf bounds contribute their
variable-axis endpoints, **including air and corner contacts**. The 257-entry cut set is clipped to
the emitted edge. Every source rectangle is contained in one `[0,256]^3` cell, so it cannot cross a
variable-axis metre interval. The canonical query uses the complete snapshot, not neighboring mesh
availability, chunk order or the requested chunk set. Its cache exists only for one job.

Binary-exact edge coordinates are multiples of 1/256 m; rectangle centres can use 1/512 m. Current
GPU vertices are absolute f32 metres, so this API rejects chunks outside `[-16384,16384)` metres
on **every** axis before multiplication or allocation. It does not silently collapse small faces
at distant positions. Large-world local coordinates/floating origin are a later renderer gate.

Closed test solids weld every triangle edge by half-lattice coordinates and require exactly two
opposite incidences, positive area/outward winding, and analytical signed volume. Six directional
cross-chunk tests include negative coordinates and fine/fine plus fine/uniform interfaces, meshed
together and independently. This proves those fixtures, not that arbitrary solids touching only
at an edge/point are topological two-manifolds. Exact fine surfaces remain axis-aligned; the authored
aperture samples every four lattice units (15.625 mm). Smooth fine contour extraction remains work.

## Aggregate limits and refusal

Each complete job shares these hard ceilings; custom limits can only lower them:

| Resource | Maximum |
| --- | ---: |
| Strictly ordered unique chunks | 16 |
| Emitted vertices | 262,144 |
| Emitted triangle indices | 786,432 |
| Source boundary rectangles | 32,768 |
| Charged work | 4,194,304 |
| Cached geometric lines | 32,768 |

Work includes every requested cell (even air), neighbor lookup, surface visit, rectangle, cache
lookup, candidate-page/leaf scan, perimeter step and emitted triangle. The page surface extractor
also retains its smaller existing local cap. One dense page can refuse before the aggregate cap;
legal storage does **not** imply affordable render geometry. No automatic coarse fallback or partial
chunk publication hides that refusal. The completed result carries its source fingerprint and
either the entire candidate or an explicit error. The inspection viewer retains its preceding GPU
state but exits on a failed job; it does not claim recovery or interactive continued operation.

Emitted counts are not reserved capacity. Rust vectors/maps may reserve spare bounded capacity;
reserve calls report allocation failure where supported, while Arc creation follows the normal
process allocator OOM policy. One worker, one queued request and one queued result bound retained
jobs; the viewer further permits only one outstanding request. Fine scene admission, workload
slicing, buffer residency and liveness under worst-case damage still need integration before this
path is enabled in a match.

## Following gates

- Preserve the existing smooth terrain when introducing exact fine architectural cells; prove the
  fine/exact/smoothed transition instead of substituting a coarse proxy or flattening all terrain.
- Schedule geometry edits, seams and remeshing through actual weapon-authoritative transactions,
  invalidate changed chunks **and** their edge/corner neighbors, and keep collision/render state
  coherent across repairs. This viewer remeshes the complete union of its small scene's chunks.
- Add fine body topology/rendering, network state/repair, reliable dynamic contact and full-tensor
  angular response; the existing live authority still rejects a `RefinedWorld`.
- Develop non-axis-aligned fracture surfaces, material blending, rubble and art direction. Better
  geometry alone does not supply the reference scene's indirect lighting or environmental detail.

The benchmark repeats immutable authored snapshots, with authoring outside the timed region. It
reports first-run and p50/p95/p99/max extraction times, work and output counts. It is neither a
complete server tick nor a client frame and does not establish sustained combat performance.

## Independent review

One bounded `claude-pair` analysis and one review were used. The confirmed request for explicit
completion-state tests led to checks for stale, failed, mismatched, unsolicited and wrong-kind
worker results. Native smoke completion now has an explicit final flag, not just a count of earlier
frames, so early window closure remains failure. The review's demand for real runtime/capture
evidence is a promotion gate, not replaced by the analytical tests.

The suggested invalid-cell panic fixture is not constructible: `GeometryCell` owns a private enum,
its constructors canonicalize volumes, and the source trait is sealed. A cell necessarily has
either a uniform voxel or a refined volume. `page()`'s expectation asserts that internal invariant;
it is not a parser fallback. Fine Arc clones are constant-size handles, not cloned leaf arrays, and
the source plus six neighbors are included in charged cell/neighbor work. Creating uniform volume
Arcs still allocates; removing this allocation churn and measuring true allocation counts remain
optimization work. The benchmark's peak RSS must not be presented as an allocation count.

## Validation receipt, 2026-09-06

Source baseline `8424aad`, plus this increment. Linux 7.2.2-1-cachyos, Rust 1.97.1, i7-13700H,
RTX 4050 Laptop/NVIDIA 610.57.04. Final formatting/strict all-target Clippy pass; **485 ordinary
tests in each debug/release profile** pass, plus all six actual Vulkan tests explicitly in each
profile, two compile-fail doctests and 22 offline tooling tests. The targeted network, secure
transport/authority/process and OIDC suites pass. Seven new library tests exercise the mesh and
worker; two native-bin tests check options and result selection. Required destruction-500,
structural-100, physics-1024/300 and snapshot-20 benchmarks pass. The unchanged physics fixture
ends with 1024/1024 sleeping bodies; this is not fine-body coverage.

Fixed six-chunk extraction, 100 samples per stage (milliseconds):

| Authored state | Vertices / triangles | Charged work | p50 | p95 | p99 |
| --- | ---: | ---: | ---: | ---: | ---: |
| Intact | 1150 / 920 | 266874 | 0.989 | 1.018 | 1.107 |
| Shallow chip | 3414 / 2790 | 383450 | 1.364 | 1.407 | 1.430 |
| Through bore | 3442 / 2808 | 416362 | 1.452 | 1.496 | 1.504 |
| Breach | 9874 / 8104 | 811334 | 2.895 | 2.941 | 2.995 |

The benchmark process peak RSS is 12120 KiB. These are warm-process runs, not cold-boot trials or
fixed-clock thermal measurements. The separate native 1440×900, 4×MSAA, eight-second release smoke
presents `[37,37,36,2169]` frames for its four stages with no discarded GPU samples. GPU total
p50/p95/p99 is 0.690/0.951/0.953 ms; CPU frame time **including presentation** is
1.645/12.655/16.688 ms. Worker-to-upload latency for only four jobs spans 1.492–16.438 ms and is
not an extraction throughput distribution. The five-second native range and industrial-breach
checks also exit cleanly; industrial GPU total p99 is 2.669 ms, with zero bodies in that view.
Neither short scene establishes the full game's frame-time target or fine combat performance.

Actual isolated RenderDoc capture/replay passed for `fine-inspection`, `fine-geometry-demo`, frame
180: **247057841 bytes, 14 draw calls, 14 textures, Vulkan**. Its retained thumbnail was visually
inspected: an open circular aperture, visible cut thickness/layers and no detached facade ribbon
in this view. Its regular silhouette, repetitive coarse-looking materials and sparse pad are
plainly not realistic blast damage or photoreal art. Instrumented capture timings are excluded
from the release numbers above. No global capture hook, listener, host policy or package changed.

Local temporary evidence: `/tmp/fps-fine-render-0o3lrs/`; capture and actual PNG are retained together
under ignored `target/tooling/renderdoc-_67zyjnn/`. No evidence was deleted or retention cap raised.
Binary SHA-256 at validation:

- `fine-geometry-demo`: `a9d3ed042bba71e56aab47f510efa128d7ea2dd6f2db9221b3fa47855ff11aac`
- `fine-mesh-benchmark`: `f6eba0f83239ed8160503b8ea97f02b9796e498d8072a6a25846855fe251d8d6`

The full game goal remains active. The next integration is fine architecture with the existing
smooth environment, followed by real weapon/body/network promotion; this standalone inspection
must not become a substitute for those requirements.
