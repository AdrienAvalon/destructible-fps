# Industrial visual target and native evidence

## Reference, not a delivered image

The user's ruined-factory reference remains the visual target, with the ambition to exceed it in
actual play. It is not a screenshot of this engine. A darker texture pass, higher average FPS or a
generated illustration cannot satisfy this target. The current native scene remains visibly far
below it.

The next map work must address the reference's actual composition and construction:

- a layered concrete frame and recessed masonry, believable section sizes, exposed floor depths,
  asymmetric structural damage and irregular silhouettes instead of a regular circular bore;
- grounded rubble with a convincing range of fragment sizes and actual contacts, not floating
  facades or a decorative shell over different collision geometry;
- a varied quarry courtyard, broken terrain transitions, sparse grass and background landforms;
- coherent scanned material scale, weathering, fine surface response, overcast light, plausible
  interior darkness and eventually local reflected light, without crushing shadows to hide defects.

The intact map and damaged states must both hold up at player height, in wide/approach/close views
and while moving. Native captures and CPU/GPU tails accompany each slice. This remains the immediate
priority ahead of broad AI/editor infrastructure or another large fine-weapon integration.

The subsequent [roof-ruin increment](industrial-roof.md) records thinner actual roof sections,
an open damaged bay, its support/topology checks and native evidence. It does not satisfy the
full reference or establish calibrated roof collapse.

The [worn hardstand increment](industrial-hardstand.md) adds a real layered concrete forecourt,
open joints and chipped edges while preserving the existing rubble and its soil supports.

The [open ruined-bay increment](industrial-bay.md) removes the upper left infill and its entire
steel frame, exposes the interior and adds grounded shards with exact regional connectivity checks.

[Explicit cut finishes](fine-surface-finishes.md) separate rubble-top appearance from physical
integrity, removing exterior brick tiling from those authored cut surfaces without geometry changes.

The [soil tiling increment](soil-tiling.md) reduces the scan's repeating ground pattern using
bounded translation-only blending, retaining the physical texture scale and collision source.

The [asymmetric bay revision](industrial-bay-profile.md) replaces the straight V profile with
chipped shoulders and removes the thin bridge over the passage in actual collision geometry.

The [fragment-face finish revision](fragment-face-finishes.md) distinguishes the retained facade
skin from exposed cut sides on the large authored debris, without changing its physical geometry.

The [polygonal rubble revision](polygon-rubble.md) changes those large pieces into grounded,
asymmetric physical footprints and keeps original skin on an explicit rotated side plane.

The [full-resolution haze experiment](occluded-haze.md) was rejected after native comparison and
GPU measurement: its scene-wide visual benefit did not justify the measured cost. The runtime
was restored; the next milestone prioritizes a cohesive reference scene over isolated corrections.

The [reference-scene foundation](reference-scene.md) adds supported broken storeys, continuous
grounded collapse masses and an irregular physical soil edge. Its native inspection also exposed
and reproduced a floating-marker raster defect missed by the earlier compute-only material probe.
The correction preserves a flat cut classification while retaining the continuous wall-depth
gradient. This does not remove the remaining stepped geometry or close the photorealistic gate.

## First material-condition increment

`weather_scanned` in the production world shader adds continuous multiscale surface condition after
the existing scanned triplanar sample and before the existing damage/core layers. Soil receives
dry mineral color variation and darker damp-looking patches. Brick and concrete receive elongated
discoloration in their original local frame. This is baked-looking past weathering, not simulated
rain, puddles, water flow, erosion or a change in physical material. It intentionally follows an
object when it rotates. No ground-height assumption paints a false contact band onto bodies.

The condition changes albedo and perceptual roughness only; damp soil stays at least 0.48 roughness.
It adds no texture samples, textures, vertices, collider, shadow caster, material ID or world state.
At that increment the retained scan grain still tiled: macro variation reduces uniformity but is not stochastic
texture de-tiling or geometric detail. Darkening intentionally changes reflectance; the altered
surface is not a newly calibrated scan. Light/exposure and the source asset packs are unchanged.

The smaller variation fades with the existing material-space fragment footprint. A bounded integer
lattice hash is used for this new condition noise. The first GPU continuity test exposed a
0.00239 roughness difference across a 0.0002 m seam using the previous float hash; the new noise
passes the same test. Existing procedural fracture/steel hashes are not changed by this increment.
GPU numerical validation executes production WGSL over 512 samples, including signed chunk seams
on all three axes, near/far footprints, vertical/horizontal surfaces, upper-bound albedo, unchanged
normal/metalness and unaffected material cases. This is one NVIDIA Vulkan device's evidence, not
cross-vendor bitwise equivalence. This render-only noise never enters replicated physics.

This increment does **not** complete the visual gate: geometric map detail, wide/approach/player
motion comparisons, local bounced lighting and convincing ruined-factory composition remain work.

## Validation receipt, 2026-09-06

Local evidence: `/tmp/fps-visual-weathering-HQ6TK6/`. Final shader SHA-256:
`9bb206960d47b6e705f7d3ded6b4da3974a5192b7c9dcf083044cbe96b675a53`;
final native inspection binary:
`a3b188d279d61bfd32f23e7053c7d20ef53bd21868300ddf967236938624bc4c`.
Parent revision is `ad0e2e0`; this receipt describes the material-condition diff, not the parent.

Formatting, strict Clippy, all 513 ordinary tests in each of debug/release, targeted network,
secure transport/authority/process and OIDC checks, two compile-fail doctests, and all six explicitly
invoked real-GPU checks in both profiles passed. Destruction (500 events), structure (100 iterations),
physics (1,024 bodies/300 ticks) and snapshot (20 iterations) benchmarks passed. Native range,
exact-inspection and industrial-inspection smokes completed, including all four displayed fine
stages and their material probes. No headless skip was counted as coverage.

The main validation script initially stopped before native launch because `/usr/bin/time` was
absent. The separate `native.sh` completed all three native checks using the existing Python
`resource.getrusage(RUSAGE_CHILDREN)` to observe peak child RSS. No package was installed. The first
script's exit 127 is not reported as success; the preceding test logs and successful recovery are
separate evidence.

Linux 7.2.2-1-cachyos, Rust 1.97.1, i7-13700H, RTX 4050 Laptop 6,141 MiB, NVIDIA 610.57.04;
native release Vulkan, 1440×900, 4× MSAA, exposure 0.75. These runs exclude RenderDoc and compilation,
but retain the engine's timestamp queries; clocks are not fixed. They are short inspection runs,
not a sustained combat, 1080p release-budget or cross-platform acceptance test.

| Final native inspection | CPU with present p50 / p95 / p99 (ms) | GPU p50 / p95 / p99 (ms) | Peak RSS (KiB) |
| --- | --- | --- | --- |
| Industrial, 12 s | 2.624 / 13.201 / 16.741 | 1.789 / 2.596 / 3.374 | 267,660 |
| Exact fixture, 8 s | 16.656 / 16.817 / 16.925 | 0.996 / 4.005 / 4.022 | 265,240 |

Industrial GPU telemetry contains 2,665 samples, with zero dropped queries; stage frame counts are
37/36/36/2,416. Bootstrap took 1,327.778 ms; subsequent mesh transitions took 16.750/13.404/13.195 ms.
The source fingerprints and generated geometry counts match the unchanged four-stage fixture.
These numbers do not establish a shader speedup or a controlled before/after cost delta. In
particular, the exact fixture's GPU tail must not be hidden behind the industrial median.

Claude's bounded analysis was available. Its continuity and albedo-bound concerns were checked
and addressed with production GPU tests. The original-frame orientation is intentional historical
weathering; keeping the albedo histogram unchanged would contradict the requested darker surface
condition. The final targeted two-file review timed out after 112 seconds (10,882-byte context):
no external final approval is claimed, and no retry/budget increase was performed. Graphify's
updated index/doctor passed; WGSL itself remains source-reviewed because the graph does not parse it.

Matched native close-fracture captures used the unchanged camera, frame 400, exposure and four-stage
industrial fixture; both were successfully replayed with RenderDoc 1.45/Vulkan and visually inspected:

| Capture under `target/tooling/` | State | Capture bytes | Draws / textures |
| --- | --- | --- | --- |
| `renderdoc-i9ribs33/breach-thumbnail.png` | Before, parent shader | 305,216,754 | 219 / 14 |
| `renderdoc-zj6fsmvc/breach-thumbnail.png` | Final material-condition shader | 305,557,586 | 219 / 14 |

The actual gain is modest: damp-looking ground patches and less uniform masonry; the regular bore,
oversized architecture and sparse courtyard remain conspicuous. An intermediate capture is retained
in `renderdoc-tgq_t1ay/`, but it is not substituted for the final-binary capture. All capture timings
are instrumented and excluded from the table above.

To keep the existing active evidence guard, four explicitly selected old runs (`renderdoc-_67zyjnn`,
`renderdoc-b9q2wtx8`, `renderdoc-e29v4fmy`, `renderdoc-eov12e3a`) were moved into the existing ignored
`target/tooling-archive-lHJXgO/`. Nothing was deleted; complete runs remain recoverable there. After
the final capture active evidence is about 1.8 GiB, the local archive about 2.0 GiB. These are not
off-machine backups; neither retention limits nor capture permissions were increased.

## Authored breach and grounded fragment increment

The next industrial inspection increment replaces only the final wall aperture with an asymmetric,
ground-reaching masonry outline. The side strips and the upper band remain connected to the
surrounding wall. The local grid stays at 1/256 m; the contour is sampled every 4 units. All four
small exact-inspection fingerprints and the first three industrial **wall** states are unchanged.

Eight fixed brick/concrete fragments now occupy the adjacent courtyard in **all four** industrial
snapshots. The stage names describe the inspected wall, not the entire courtyard. The fragments
are authored scenery in the actual material volume, not newly generated blast debris: their mass
is not presented as the missing wall mass, they are not simulated bodies, and fine gameplay remains
disabled. No new server, weapon or collider-proxy path is introduced.

Each fragment owns a distinct one-metre page, previously empty, immediately above a completely
solid ground page. Construction refuses missing support or occupied placement without modifying
the source. Its bevelled footprint and sloping top are filled using ordinary bounded volume edits,
with 8-unit horizontal samples and 4-unit height quantization. Each filled column extends directly
to the supporting ground; neither hanging columns nor a hollow visual shell is authored. The same
stored geometry supplies physical overlap/ray queries, surface extraction and shadow casting.

This visibly improves the silhouette and adds foreground objects, but the inspected capture still
shows corrugated fine-grid fragment tops, overly regular architecture and sparse scenery. Smooth
looking fragment surfaces consistent with physical geometry, convincing clustered rubble, proper
building-scale ruins, vegetation, composition and broader camera checks remain acceptance work.
The increment is not promoted as photorealistic, an explosion simulation or a finished map.

Measured scene residency and dirty-region output, under unchanged default limits:

| Wall state | Refined pages / leaves | Quads / vertices | Work units |
| --- | --- | --- | --- |
| Intact | 20 / 3,272 | 8,804 / 41,051 | 3,167,865 |
| Shallow chip | 20 / 3,472 | 9,198 / 43,315 | 3,285,817 |
| Through bore | 20 / 3,540 | 9,208 / 43,343 | 3,317,353 |
| Breach | 17 / 4,157 | 10,150 / 48,683 | 3,715,941 |

The final aperture empties three whole wall pages, canonically represented as AIR. The eight rubble
pages stay identical across stages. Work is below the 4,194,304-unit job cap but uses about 89% of
it in the final eight-chunk replacement: more content cannot be added without checking this headroom.
The full initial scene contains 132 chunks, 55,132 quads and 226,363 vertices; bootstrap is split into
bounded chunk jobs, not a single job that exceeds the output cap.

The regression tests verify the full-versus-incremental scene meshes, persistent fragment identity,
the original four exact-fixture fingerprints, empty corners/tops, exact filled-unit counts, ground
contact, multiple top heights and rejection of unsupported/occupied placements. The native stage
probes still check central passage, retained rim and matching physical overlap in every state.

Claude's analysis and targeted runtime review were available. The review intentionally received
runtime code plus a test summary, not the complete test diff. Codex independently checked the actual
tests and confirmed that `IVec3` is the project's integer type with derived `Ord`, not glam's type.
The local `GeometryState` starts an inspection-only sequence at 1 and prepares at tick 1; it is not
a replacement server authority. No blocking runtime defect was identified by the provided review.

### Ruin-increment validation receipt, 2026-09-06

Parent `cf4fc58`, final native inspection binary SHA-256
`99c565b8d9ed5eb9177a3f8c9c11ace76b1bc44193c2f807491816475b1981c8`.
Logs are in `/tmp/fps-industrial-ruins-x1Lz83/`; `validate.sh` completed with exit 0.
Formatting, 517 ordinary tests in **each** of debug/release, the targeted network,
secure transport/authority/server-process and OIDC tests, two compile-fail doctests and all six
explicit real-GPU checks in both profiles passed. The prescribed destruction/structural/physics/
snapshot benchmarks and the range/exact/industrial native smokes passed. Graphify update/doctor
passed. The first failed test expected 20 refined pages even after entire pages became AIR; the
corrected test explicitly requires 17 in the final state and verifies unchanged rubble identity.

Correction discovered during the following normal-reconstruction increment: the earlier claim of
strict Clippy success was incorrect. The separate shell command continued into successful tests
after Clippy rejected a wildcard import; a later pass also exposed a redundant `into_iter()`.
The prior test results above are valid, but do not imply lint success. Both warnings are corrected
in the subsequent increment, whose single validation script includes Clippy under
`set -euo pipefail` and refuses to continue after any failed check.

Same observed Linux/Rust/NVIDIA/RTX 4050 Laptop/i7-13700H hardware as the preceding receipt,
1440×900, 4× MSAA, exposure 0.75. Final measurements were taken after compilation, without RenderDoc,
with existing timestamp telemetry and no fixed clocks. Initial exploratory timings overlapped
other checks and are not used below. Twenty repeated extractions per stage also passed under the
unchanged mesh bounds; they repeatedly process nonempty fixed geometry rather than a vanishing scene.

| Final native inspection | CPU with present p50 / p95 / p99 (ms) | GPU p50 / p95 / p99 (ms) | Peak RSS (KiB) |
| --- | --- | --- | --- |
| Industrial, 12 s | 3.202 / 13.040 / 16.752 | 2.219 / 2.851 / 3.857 | 268,556 |
| Exact fixture, 8 s | 16.658 / 16.826 / 16.932 | 1.009 / 3.966 / 4.007 | 264,132 |

Industrial GPU samples: 2,381, zero dropped queries; stage frame counts: 38/37/38/2,123. Bootstrap
took 1,462.520 ms; replacements took 29.824/16.694/20.293 ms. This is not a sustained multiplayer
combat test or proof of the 1080p shipping frame-time target.

The native before image remains `target/tooling/renderdoc-zj6fsmvc/breach-thumbnail.png`.
The inspected after image is `target/tooling/renderdoc-0by5c3zd/breach-thumbnail.png`, captured and
replayed successfully with RenderDoc 1.45/Vulkan: 304,649,528 bytes, 219 draws, 14 textures. Camera,
frame 400, exposure and source material packs are unchanged. Its binary hash matches the final
validated binary above. This frame proves the visible aperture and fragment increment, not a wide
or moving-camera photorealistic acceptance. Instrumented capture timings remain excluded.

The previous intermediate `renderdoc-tgq_t1ay` run was moved from the active directory to the
existing ignored `target/tooling-archive-lHJXgO/` before this capture. It remains fully recoverable;
nothing was deleted, and no retention or permission limit was increased.

## Shading reconstruction for fine terraces

`mesh/fine/normals.rs` recognizes nearly planar, homogeneous brick/concrete/stone fragments from
their **actual** volume and exposed surface rectangles. It reconstructs shading normals only.
Positions, triangle indices, material occupancy, collision queries and shadow-caster geometry
remain exact and unchanged. No smoothing displacement, new texture/asset, exposure change,
shadow-bias change or GPU feature is involved.

Recognition is deliberately limited: solid material must stay inside the page's X/Z boundary,
below its top, and reach local Y=0; multiple materials/integrities and visible undersides above
Y=0 reject the candidate. This local shape check is not a new world-support/structural test.
Area-integrated second moments fit the top rectangles by least squares, including their width²/12
and depth²/12 terms so coplanar subdivision does not change the mathematical fit. Ill-conditioned,
flat or steeper-than-45-degree fits fall back. Every top corner must lie within 6/256 m vertically
of the plane; that is a **recognition threshold**, not permission to move geometry.

The normal applies to accepted tops and short internal risers, never indiscriminately across
the whole fragment. Risers are at most 8/256 m tall, remain near the fitted plane, and require a
positive geometric-normal dot product above 0.001. The entire adjacent base strip must be solid:
testing only the face's Y origin would incorrectly soften short upper pieces of an external side.
Outer footprint sides and undersides retain their original normals. Since accepted fragments do
not touch their page sides or top, no cross-page smoothing seam is introduced by this treatment.
Larger, curved, mixed-material and multi-page pieces retain the old treatment pending broader work.

All work is charged to the existing mesh-job meter. Moment accumulation and footprint checks use
constant scratch space, with bounded interval traversal for base-strip occupancy. A strip is one
lattice unit wide and high; its length L admits at most 3L visited slabs/bands/runs. The query cap
uses that geometric bound, not the page's maximum leaf count. Exhaustion is an
error propagated through the whole mesh candidate, not permission to publish a partially smoothed
scene. The industrial final dirty-region work increases from 3,715,941 to 3,769,734 units; geometry
counts remain 10,150 quads and 48,683 vertices. No budget was increased.

Tests recognize all eight authored fragments and reject the layered wall pages; exercise signed
slopes, unit/outward normals, split upper outer sides, flat/mixed/overhanging/boundary/nonplanar
fallback, fit/subdivision invariance and exhaustion both during fitting and a base-strip query.
An additional dense fixture contains 2,658 leaves and 6,361 surface quads; 3,056 receive reconstructed
normals, using 45,368 recognition/query work units. It tests the length-bounded strip cap beyond
the eight default fragments; it is not a universal CPU-latency guarantee for every valid page.
Ordered position/index regression checksums were measured from `c62c96a` **before** adding this
code and still match in all four industrial states. Those FNV checksums are fixture regression
oracles, not a security or authentication mechanism. Full-versus-incremental scene identity passes.

The actual same-camera capture shows the corrugated highlight pattern removed from the foreground
concrete fragment while its silhouette remains unchanged. It does not prove stair silhouettes or
self-shadowing are invisible from every distance and light angle; lower player views, motion and
raking-light inspection remain part of the broader visual gate. The overall map still lacks the
reference's composition, architectural detail, natural terrain, vegetation and lighting richness.

### Terrace-normal validation receipt, 2026-09-06

Parent `c62c96a`, final standalone native inspection SHA-256
`23d90aa7e841437f300c1bfccc260ce8d9d600d6256f42b83b4392e3b24369e5`.
Evidence: `/tmp/fps-rubble-normals-2oXgN1/`. After tightening the base-strip bound and adding its
dense test, the entire `validate.sh` was rerun on the final code with `set -euo pipefail`, including
Clippy in the same fail-fast sequence. Final result: exit 0; formatting, strict all-target Clippy,
523 ordinary tests in each of debug/release, targeted network/secure transport/authority/process/
OIDC checks, two compile-fail doctests and all six explicit real-GPU checks in both profiles passed.
The prescribed destruction/structural/physics/snapshot benchmarks, 20 repeated fine extractions per
industrial stage and the range/exact/industrial native smokes also passed. Graphify update/doctor
passed. The earlier incorrect lint claim is corrected in the preceding receipt, not hidden by these
new results.

Final release Vulkan measurements after compilation, without RenderDoc, with engine timestamp
telemetry: Linux 7.2.2-1-cachyos, Rust 1.97.1, i7-13700H, RTX 4050 Laptop 6,141 MiB, NVIDIA 610.57.04,
1440×900, 4× MSAA, exposure 0.75. Clocks and compositor pacing are not fixed: especially the lower
CPU-with-present tail must not be attributed to this normal treatment as an established speedup.

| Final native inspection | CPU with present p50 / p95 / p99 (ms) | GPU p50 / p95 / p99 (ms) | Peak RSS (KiB) |
| --- | --- | --- | --- |
| Industrial, 12 s | 2.705 / 2.855 / 3.257 | 2.093 / 2.120 / 2.631 | 267,924 |
| Exact fixture, 8 s | 1.345 / 1.516 / 1.635 | 0.746 / 0.760 / 0.767 | 263,148 |

Industrial GPU samples: 4,456, zero dropped queries; stage frames 42/42/42/4,174. Bootstrap took
342.919 ms; replacements 18.509/18.582/18.391 ms. Source fingerprints, quad/vertex counts and ordered
geometry signatures remain unchanged. These short inspection runs are not multiplayer combat,
cross-platform acceptance or a controlled before/after timing experiment.

Before: `target/tooling/renderdoc-0by5c3zd/breach-thumbnail.png`.
Final after: `target/tooling/renderdoc-cg9_31h0/breach-thumbnail.png`, successfully captured/replayed
with RenderDoc 1.45/Vulkan, 304,891,228 bytes, 219 draws, 14 textures. Both were inspected at the same
camera/frame 400, material packs, exposure and shadow settings. The after-capture executable hash
matches the final standalone binary above. The screenshot is native output, not a generated target.

Claude supplied an analysis and a targeted runtime review, with tests summarized rather than a
complete test-diff review. Codex independently inspected the vertex appender (it does not deduplicate
vertices by normals), ran the actual tests, inspected the captures and corrected the validation
receipt. The review's dense-work concern led to the explicit 3L strip bound and dense regression.
All shape/budget findings were checked in the code; no final photorealistic or all-angle shadow
acceptance is claimed.

Two completed older/intermediate runs, `renderdoc-i9ribs33` and `renderdoc-xvdidype`, were moved into
the existing ignored `target/tooling-archive-lHJXgO/`, with their complete artifacts preserved.
No deletion, retention-limit increase or permission expansion occurred. Active evidence is about
1.8 GiB and the recoverable local archive about 2.9 GiB; neither is an off-machine backup.

## Industrial fenestration and player-height inspection

The fine industrial fixture now installs fourteen open steel window frames into the actual source
volumes. They have 6.25 cm wide, 12.5 cm deep sections, three vertical mullions and a middle transom,
with no opaque pretend glass. Twelve openings are 6×5 m; the last opening on each side is 5×5 m,
respecting the concrete column which the coarse builder installs after its original cutout. Shared
coarse layout constants keep those placements related to their source. This adds architectural
detail, not finished photorealism, calibrated glazing or a simulated broken-window system.

Every complete opening must initially be empty and its full perimeter solid. The operation refuses
the whole candidate if any placement fails. All pre-existing solid cells remain unchanged. Eight
rectangular bars are unioned before each page enters one sorted transaction per window, at most
30 cells. All fourteen transactions belong to one authored snapshot time; their sequence advances,
not the simulation tick. The caller's immutable world remains untouched. The 410 new steel pages
persist across the four wall states. They use the same exact material source for queries and mesh,
but fine authoritative weapon damage and support-driven detachment remain explicitly pending.

The extra detail raises final dirty-region work beyond a single job's 4,194,304-unit ceiling. The
industrial inspector therefore partitions replacements into eight one-chunk jobs and publishes
the whole replacement atomically after validation. Existing per-job work/output, aggregate candidate,
resident scene and worker deadline limits are unchanged. Small exact inspection still uses its
previous grouped jobs. Full-scene tests cover all 132 chunks in every state and prove that changes
remain confined to the complete dirty region, with identical ordered mesh bytes after replacement.

The industrial benchmark now follows this partition and reports `jobs=8`, total work and maximum
per-job work separately. Its old single-batch timing baseline is **not directly comparable**. It
retains the completed meshes until after the extraction timer, including during bootstrap; the
old bootstrap helper dropped each mesh inside that timer. Neither change establishes a speedup.
The pre-fenestration position/index oracle remains an isolated normal-reconstruction test: a test-only
copy restores the previously empty steel-window cells. Full new-scene tests never remove frames;
new source fingerprints are expected, not evidence of a regression.

`fine-geometry-demo --world industrial --view wide|approach|fracture` provides reproducible native
views; `fracture` preserves the earlier close framing and remains the default. `wide` and `approach`
place the eye at y=2.65 m, 1.65 m above the actual ground verified by static queries. Orbit/zoom are
inspection controls, not a collision-constrained character. The capture launcher accepts the same
finite views only with `renderdoc --world fine-industrial`; its child maps them to literal arguments,
without widening file/network/retention limits. Rust and Python reject malformed view options.

### Fenestration validation receipt, 2026-09-06

Parent `496cef7`; final standalone native inspector SHA-256:
`3be60d8e6fbc316e339a779ea4f340dc18fd362c3efa4d756e3d1090e2f68c60`.
Evidence: `/tmp/fps-industrial-windows-AREXVy/`. Its fail-fast `validate.sh` finished with exit 0:
formatting, strict all-target Clippy, eight offline tool tests, 527 ordinary tests in each of debug
and release, targeted network/secure transport/authority/process/OIDC tests, two compile-fail
doctests, all six explicit actual-GPU tests in both profiles, the four prescribed simulation/storage
benchmarks, twenty repeated fine extractions per stage and five native smoke runs. After the review's
benchmark-label clarification, formatting/strict Clippy and the benchmark test target were rerun;
the latter has no unit tests and is not counted as additional behavioral coverage. The final native
build and real twenty-iteration benchmark include the explicit `jobs=8` labels. Graphify update and
doctor passed; source and diff checks were performed independently of its derived index.

New tests trace air/mullion/transom samples on every facade using the actual fixed-point material
query, validate exact authored steel volume and source immutability, preserve occupied masonry,
refuse occupied/unsupported openings and verify deterministic repeated authoring. Existing all-stage
full-versus-incremental checks now include all frames. The original exact fixture and the isolated
normal-only geometry oracles remain unchanged. Player-height camera tests query the original ground
and exercise finite unit directions under orbit/zoom; CLI tests reject unknown, duplicate or
inapplicable views.

Final scene bootstrap: 132 chunks, 58,240 quads, 244,167 vertices, 408,978 indices. Final damaged
snapshot: 427 refined pages and 6,683 leaves; the first three states have 430 pages. Final dirty
replacement: 10,921 quads, 53,106 vertices, 114,384 indices, total work 4,381,725 and maximum single
job work 1,417,128. Across all four states the maximum job is 1,417,205. World and job limits were
not increased. Preparing the fine source remains outside extraction and frame timing.

Uninstrumented-by-RenderDoc release Vulkan runs, after compilation, with normal engine GPU
timestamp queries: Linux 7.2.2-1-cachyos, Rust 1.97.1, i7-13700H, RTX 4050 Laptop 6,141 MiB,
NVIDIA 610.57.04, 1440×900, 4× MSAA, exposure 0.75. No clock locking or compositor control.

| Native view | CPU with present p50 / p95 / p99 (ms) | GPU p50 / p95 / p99 (ms) | Peak RSS (KiB) |
| --- | --- | --- | --- |
| Industrial fracture, 12 s | 2.974 / 3.122 / 3.508 | 2.357 / 2.421 / 2.815 | 267,792 |
| Industrial approach, 12 s | 2.823 / 2.979 / 3.374 | 2.206 / 2.259 / 2.814 | 282,928 |
| Industrial wide, 12 s | 2.134 / 2.308 / 2.510 | 1.526 / 1.559 / 1.653 | 281,368 |
| Small exact inspection, 8 s | 1.354 / 1.485 / 1.625 | 0.754 / 0.764 / 0.767 | 263,716 |

All three industrial runs displayed/probed all four complete stages; 4,063/4,271/5,633 GPU samples
respectively, zero dropped queries. Stage replacement latency is about 29–38 ms and bootstrap
281–356 ms. These are short fixed-scene developer observations, not a controlled speedup claim,
sustained 1080p combat budget, multiplayer or cross-platform acceptance.

Actual RenderDoc 1.45/Vulkan frame-400 captures were replayed successfully, each with 14 textures
and a fully completed native smoke. Source assets, light and exposure settings were not changed:

| Capture | Local thumbnail under `target/tooling/` | Capture bytes / draws |
| --- | --- | --- |
| Wide before, camera-only binary | `renderdoc-yqp_fdjh/breach-thumbnail.png` | 294,891,414 / 245 |
| Wide after | `renderdoc-zmj7dvlf/breach-thumbnail.png` | 295,016,147 / 245 |
| Approach after | `renderdoc-x3m4m1h7/breach-thumbnail.png` | 302,915,696 / 221 |
| Fracture after | `renderdoc-h8iebesu/breach-thumbnail.png` | 304,888,686 / 219 |

The camera-only before binary SHA-256 was
`44578798d8acaa7775c545a4c05599372b724c12cc066df68a318fd4e1fee329`; the after captures use the final
standalone binary above. The previous close reference remains
`renderdoc-cg9_31h0/breach-thumbnail.png`. There is no matched pre-change approach capture. The wide
comparison shows a modest but actual improvement in architectural detail; oversized regular concrete
sections, sparse ground, intact roof silhouette and simplistic interior lighting remain obvious.
This does not pass the broader ruined-factory visual gate.

Claude supplied one analysis and one targeted runtime review, with tests summarized rather than
the full test diff. Its benchmark-comparability concern produced the explicit job-count metadata
and explanation above. Alleged missing camera/parser/count tests were checked against the existing
new tests; an all-air noncanonical cell cannot be constructed through the sealed canonical type.
The pre-existing 4M cap is per job, not a scene-wide lifetime work cap; the retained aggregate
output and resident caps were verified in `Stream`, not inferred from the review.

Four completed older captures (`renderdoc-o4y70k5r`, `renderdoc-pvv6_ief`, `renderdoc-zj6fsmvc`,
`renderdoc-0by5c3zd`) were moved into the existing ignored `target/tooling-archive-lHJXgO/` without
deletion or retention-limit increases. Their original receipt paths now resolve through that
recoverable local archive; this is not an off-machine backup.

A separate finite moving-camera check used the already installed X11 input and FFmpeg tools inside
the existing network/PID sandbox. It addressed only the freshly launched game's PID-verified window,
sent bounded orbit/zoom keys, recorded six seconds at 15 FPS (90 frames, 1440×900) and required the
20-second native smoke to complete. The owned processes terminated normally. Evidence is retained
in `target/window-motion-MaXCij/`: `motion.mp4`, the finite harness, logs and three extracted native
frames (5/25/75) inspected for actual camera movement and attached window geometry. This is a small
motion spot-check, not an exhaustive shimmer, occlusion or high-refresh temporal acceptance test;
its screen recording/XWayland timings are excluded from the performance table. No additional tool
installation or global input hook was necessary. Active RenderDoc evidence is about 1.7 GiB, the
recoverable archive about 4.1 GiB and this separate bounded motion evidence about 8.6 MiB.
