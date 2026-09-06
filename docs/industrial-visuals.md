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

## First material-condition increment

`weather_scanned` in the production world shader adds continuous multiscale surface condition after
the existing scanned triplanar sample and before the existing damage/core layers. Soil receives
dry mineral color variation and darker damp-looking patches. Brick and concrete receive elongated
discoloration in their original local frame. This is baked-looking past weathering, not simulated
rain, puddles, water flow, erosion or a change in physical material. It intentionally follows an
object when it rotates. No ground-height assumption paints a false contact band onto bodies.

The condition changes albedo and perceptual roughness only; damp soil stays at least 0.48 roughness.
It adds no texture samples, textures, vertices, collider, shadow caster, material ID or world state.
The retained scan grain still tiles: macro variation reduces uniformity but is not stochastic
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
Formatting, strict Clippy, 517 ordinary tests in **each** of debug/release, the targeted network,
secure transport/authority/server-process and OIDC tests, two compile-fail doctests and all six
explicit real-GPU checks in both profiles passed. The prescribed destruction/structural/physics/
snapshot benchmarks and the range/exact/industrial native smokes passed. Graphify update/doctor
passed. The first failed test expected 20 refined pages even after entire pages became AIR; the
corrected test explicitly requires 17 in the final state and verifies unchanged rubble identity.

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
