# Exact fine surfaces in the native renderer

`fine-geometry-demo` displays the real typed `RefinedWorld` through the existing Vulkan/PBR/HDR
renderer and bounded `MeshScheduler`. It does not render an AI image, use a coarse collision proxy,
or feed partial cells to the coarse game authority. It is an inspection scene with four **authored**
states: intact layered masonry, a shallow chip, a through-bore, and a large breach. These are not
calibrated bullet/blast outcomes. The optional industrial inspection uses the same smooth terrain
as the existing map alongside a fine layered masonry patch. Existing playable weapon/multiplayer
paths are unchanged. This is a visible integration gate, **not photorealism or final gameplay**.

```bash
cargo run --release --bin fine-geometry-demo
cargo run --release --bin fine-geometry-demo -- --smoke-seconds 8
cargo run --release --bin fine-geometry-demo -- --world industrial --smoke-seconds 12
cargo run --release --bin fine-mesh-benchmark -- --iterations 100
cargo run --release --bin fine-mesh-benchmark -- --world industrial --iterations 100
cargo build --release --bin fine-geometry-demo
python tools/tooling_smoke.py renderdoc --world fine-inspection
python tools/tooling_smoke.py renderdoc --world fine-industrial
```

Left/right arrows choose a state; up/down orbit; W/S zoom; Escape closes the viewer. There is no
first-person collision controller in this inspection camera. The mesh and exact static collision
queries consume the same fine volume, but player movement through this graphical scene is not
validated by this viewer. The smoke requires all four states to be actually presented at least 35
times, finished meshing and real GPU timing samples. Missing display/GPU, errors, or early closure
cannot pass. Only one job is in flight; a newer user selection discards stale results. No world
mutation, server, network listener, credential, package or privilege is required.

## Fine architecture inside the smooth industrial environment

`mesh_hybrid_chunks` consumes a complete immutable `RefinedWorld`. Uniform sources preserve the
existing exact architecture and Surface Nets terrain byte-for-byte in regression fixtures. Around
each actual fine cell, a one-cell Chebyshev collar keeps neighboring uniform cells exact. The
derived dual-cell stencil samples only one cell around its owner; the collar therefore prevents
partial occupancy from being substituted into a uniform stencil. A partial cell returns no uniform
voxel, never fake air or a representative solid. Existing mixed exact/derived junction pinning and
exact-side render-cap ownership close the transition; these caps are render closure, not new
physical solids. Fine/fine and fine/uniform boundaries retain the canonical perimeter subdivisions.

Collar membership reads the bounded sparse fine-page iterator, not a dense scan or truncated list.
The shared work meter also charges collar construction and every derived source lookup. Query
exhaustion is sticky and rejects the whole candidate. The collar filter derives its halo from the
same seven-cell dependency constant used by coarse masonry shading. `hybrid_dirty_chunks` unions
all affected chunks at edges/corners, validates coordinates first and bounds input edit count. Its
output may exceed one job for general edits; callers must slice it without raising the job caps.

The industrial fixture replaces only twelve cells with a 312.5 mm layered wall, aligned to the
existing facade. The four cut stages remain authored circular test geometry, not simulated weapon
energy or structural failure. Tests compare full independent remeshing across every industrial
stage with the eight-chunk dirty region; nonempty geometry outside it is unchanged. Additional
six-direction exposed-junction rays detect view-through cracks without a solid block hiding the
seam, while a ray through the actual bore must remain unobstructed.

The viewer bootstraps one fixed source stage with one chunk per worker job, displaying its partial
stream until all chunks are loaded. Stage controls are disabled during bootstrap. Subsequent
changes submit the bounded dirty set together (up to the existing sixteen-chunk job limit), retain
the prior GPU state while staging its CPU meshes, then upload the replacement
set, including empty chunks, before the next render. Stale selections discard unpublished results.
This is a presentation-boundary replacement, not a crash-safe GPU allocation transaction. A current
CPU error, count/identity mismatch or five-second worker deadline exits visibly; there is no
unbounded automatic retry. An extraction failure for an abandoned stage is reported and discarded
after verifying its source identity. GPU/device allocation failure does not promise recoverable rollback.

Inspection bookkeeping is bounded to 256 scene chunks, 524,288 resident vertices and 1,572,864
resident indices. A dirty replacement additionally stays within 16 chunks and the existing fine
vertex/index output caps. Source preparation stays outside presentation. Stage latency includes
worker polling/frame scheduling; sum-of-job work/line counts are not a single-job budget or unique
global cache size. Extraction throughput and GPU frame time are measured separately.

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
state but exits on a failed current job; it does not claim general interactive recovery.

Emitted counts are not reserved capacity. Rust vectors/maps may reserve spare bounded capacity;
reserve calls report allocation failure where supported, while Arc creation follows the normal
process allocator OOM policy. One worker, one queued request and one queued result bound retained
jobs; the viewer further permits only one outstanding request. Fine scene admission, workload
slicing, buffer residency and liveness under worst-case damage still need integration before this
path is enabled in a match.

## Following gates

- Extend the proved fine/exact/smoothed inspection transition to live, changing gameplay snapshots
  and resident chunk streaming without weakening its seam and work-budget guarantees.
- Schedule geometry edits, seams and remeshing through actual weapon-authoritative transactions,
  invalidate changed chunks **and** their edge/corner neighbors, and keep collision/render state
  coherent across repairs. The industrial inspector now replaces only its complete dirty region.
- Add fine body topology/rendering, network state/repair, reliable dynamic contact and full-tensor
  angular response; the existing live authority still rejects a `RefinedWorld`.
- Develop non-axis-aligned fracture surfaces, material blending, rubble and art direction. Better
  geometry alone does not supply the reference scene's indirect lighting or environmental detail.

The benchmark repeats immutable authored snapshots, with authoring outside the timed region. The
industrial benchmark times the same complete eight-chunk replacement job used by the viewer;
its bootstrap measurement separately processes all 132 chunks one at a time. It
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

### Hybrid integration review

A bounded targeted Claude review covered the mesher, sparse iterator, scheduler and inspection
delivery state machine; Codex separately reviewed GUI options, fixtures, benchmarks and the capture
whitelist. It led to explicit stale-failure reporting/discard, resident-overflow and mid-derived-query
exhaustion tests, and a full 4,096-page sparse-source test. That far-page fixture consumes 12,289
charged work units for an empty requested chunk, including metadata traversal and all 4,096 cell
visits. The implementation still scans sparse page metadata per job; a spatial query optimization
remains useful for heavily refined worlds, and worst-case page density is not a frame-time proof.

Two suggested changes were not justified by the actual code. `nearby_masonry_damage` samples raw
uniform geometry and does not consult collar classification, so its radius six does not add two
more cells to the collar dependency. The pre-existing constant assertion `2 * dependency_radius <
CHUNK_EDGE` already protects eight-corner dirty-chunk enumeration. Neither radius nor resource
limits were increased. Final independent tests and native evidence remain required after review.

## Initial exact-surface validation receipt, 2026-09-06

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

The full game goal remains active. Fine architecture now has an inspection integration with the
smooth industrial environment; real weapon/body/network promotion remains next. This standalone
inspection must not become a substitute for those requirements.

## Hybrid industrial validation receipt, 2026-09-06

Source baseline `7d04717`, plus this increment. Linux 7.2.2-1-cachyos, Rust 1.97.1,
i7-13700H, RTX 4050 Laptop (6,141 MiB)/NVIDIA 610.57.04. Final strict Clippy/formatting pass;
**498 ordinary tests in each debug/release profile** pass. All six actual Vulkan tests were also
run explicitly in each profile, plus two compile-fail doctests, the targeted network, secure
transport/authority/process and OIDC suites, and 22 offline tooling tests. Required destruction-500,
structural-100, physics-1024/300 and snapshot-20 benchmarks pass; replicas converge and the unchanged
physics fixture finishes with 1,024 sleeping bodies. These remain coarse gameplay/physics checks,
not evidence of fine weapons or bodies.

One intermediate debug run under simultaneous release compilation failed in the existing impaired
network test: incoming sequence 20, expected 1, sixteen future packets/4,231 bytes retained. Source
inspection confirms this was another future packet, not the deliverable missing packet covered by
the earlier inbox fix in `performance.md`. No networking code, queue cap or impairment was changed.
After compilation ended, the complete debug suite, the targeted network suite and five additional
exact repetitions passed; the final batched debug/release suites also passed sequentially. This
preserves evidence of the fixture's sensitivity to accumulated future traffic before repair, not
a claim of robustness to arbitrary process starvation. Keep network repair stress on the backlog.

Final industrial extraction, 100 warm-process samples of one complete eight-chunk replacement job
(milliseconds; authoring and final mesh destruction outside the timed interval):

| Authored state | Vertices / triangles | Charged work | p50 | p95 | p99 |
| --- | ---: | ---: | ---: | ---: | ---: |
| Intact | 17,910 / 9,102 | 467,325 | 8.526 | 8.682 | 8.790 |
| Shallow chip | 20,174 / 10,972 | 583,901 | 8.989 | 9.237 | 9.352 |
| Through bore | 20,202 / 10,990 | 616,813 | 9.153 | 9.325 | 9.375 |
| Breach | 26,634 / 16,286 | 1,011,785 | 10.615 | 10.819 | 10.915 |

The process peaks at 11,880 KiB RSS, not an allocation count. The separately measured bootstrap
extracts 132 chunks, 203,222 vertices/101,758 triangles in 70.979 ms total, with at most 191,090
charged work per one-chunk job. Native initial delivery takes 1,274 ms due to progressive frame
scheduling; that is not CPU extraction time. All work/output caps are unchanged.

The first implementation scheduled each of the eight replacement chunks on a separate frame.
An uncontended native run measured about 133 ms per replacement despite about 10.6 ms extraction
p99. The final viewer batches those eight chunks under the existing sixteen-chunk job limit and
publishes only after the whole result. Full-scene tests prove byte-identical grouped/individual
meshes. Final native replacement latencies are 12.931/13.031/13.318 ms for the three changed states;
these three observations are not a statistically established latency percentile or a combat SLA.

Final native industrial smoke: 1,440×900, 4×MSAA, twelve seconds, presented counts
`[36,36,36,2467]`, zero dropped GPU timing samples. GPU total p50/p95/p99 is
1.801/2.440/3.126 ms; CPU wall time including presentation is 2.725/13.140/16.718 ms.
The earlier uncontended one-chunk delivery run recorded GPU total p99 7.956 ms; clocks/power were
not locked or sampled per frame, so no GPU speedup is attributed to batching. The final small exact
inspection also passes (eight seconds, `[38,36,37,2458]`, GPU p99 0.799 ms), as do the ordinary
five-second playable range and industrial smokes. These short authored scenes do not establish
1080p sustained-combat, server-tick or cross-OS release budgets.

Actual isolated RenderDoc capture/replay: `fine-industrial`, frame 400, **304,534,168 bytes,
219 draws, 14 textures, Vulkan**. The retained thumbnail shows the fine open circular aperture
inside the industrial facade with smooth ground and no detached wall ribbon in this view. Its
regular contour and repeated materials remain visibly non-photorealistic. This capture predates
the final delivery batching/error-handling changes; the full-scene grouped/individual regression
proves mesh equivalence, and the final native smokes exercise the final delivery code. Instrumented
capture timings and the initial concurrent build/benchmark smoke are excluded from the table.

Evidence: `/tmp/fps-hybrid-native-51pFu9/` and
`target/tooling/renderdoc-eov12e3a/`. Active tool evidence totals 2,103,685,816 bytes after capture;
the existing 2 GiB pre-run guard and separate earlier archive are unchanged. No evidence was
deleted, no cap was raised and no package, host policy or network service was changed.
Final executable SHA-256:

- `fine-geometry-demo`: `26bc96acb3ac3728b34cac113807d39e8c0bf60edc4639d931db782b8880b87d`
- `fine-mesh-benchmark`: `7438ca7777f3c99458507993aa2a5fe22dceaab9827ebb5b9be2ca41e559d495`

Following work: promote fine weapon edits into coherent render/collision/authority state, then
fine structural/body/network behavior, non-axis-aligned fracture detail, rubble and the authored
photoreal visual scene. Keep the game goal active; a regular prepared hole is not realistic blast.
