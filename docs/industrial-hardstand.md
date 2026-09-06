# Worn industrial hardstand

## Scope and geometry

This is an authored fine-inspection map increment, not runtime paving, navigation validation or a
mass-conserved destruction event. The photorealistic ruined-factory reference is still the target,
not an achieved result. Lighting, scanned textures, shaders, network authority and budgets are
unchanged here.

`src/mesh/fine/fixture/hardstand.rs` replaces the upper 12.5 cm of 566 existing one-metre soil cells
with concrete or air. Local coordinates are 256 units per metre: the original soil occupies
`y=[0,224)`, concrete/air `y=[224,256)`, and surviving concrete reaches world y=1. This is an actual
material volume shared by fine meshing and physical queries, not a render-only surface or decal.
It does not change the older uniform-world playable demo or activate the fine character controller.

The envelope is x=-24..12, z=22..34 plus x=-8..8, z=17..21 (inclusive), entirely at y=0:
37×13 + 17×5 = 566 cells. Four-metre bays have 8/256 m (3.125 cm) open joints, with chipped corners
varying deterministically from 3/16 to 10/16 m and a ragged front edge sampled every 1/8 m. Every
remaining concrete column rests directly on retained soil. The eight existing rubble pieces and
their supporting soil cells are outside this envelope and remain byte-identical. No material is
added above the original surface; removed cap areas expose soil at y=0.875.

The input must contain full Soil and AIR immediately above every edited cell. Existing masonry,
partially damaged ground and placed objects fail closed. All edits are prepared privately and
published as a new world only after successful sorted batches of at most 256 changes. Reinstalling
over an already-authored apron is rejected. The input is immutable and its tick is retained.
The largest measured batch contains 1,816 fine leaves, below the unchanged 32,768 transaction cap.

## Checks and limitations

Three focused tests cover source/envelope preservation, all-column support by exact soil volume and
leaf bounds, deterministic replay in three bounded transactions, negative source/occupied-above
cases, vertical material chords and a horizontal ray through an open joint into its concrete side.
The cap chord is exactly 125,000 micrometres. Full-scene tests retain the same source apron in all
four wall stages and compare complete versus dirty-remeshed vertex bytes and indices. The native
camera grounding test now queries the actual refined map, not only the older coarse preset.

The first 1/16 m leading-edge sampling produced too many leaves for the existing 16,384-leaf
content guard in the final breach state. Sampling that edge at 1/8 m reduced the final scene to
13,672 leaves without changing the guard, engine caps or slab joints. Initial resident content is
283,645 vertices and 521,244 indices, below the unchanged 400,000/1,200,000 content guard and
524,288/1,572,864 renderer limits. Fine pages are 2,367 initially, 2,364 after the low wall breach.
Maximum dirty-job work is 1,974,187 of 4,194,304; final eight-job replacement totals 6,301,264 work,
64,248 vertices and 145,272 indices. These are content counts, not a speedup claim.

The joints are shallow real cavities. Point-ray and camera tests do not prove capsule traversal,
step-up behaviour, AI navigation or accessibility across chipped corners. Those gates are still
required before using this authored ground with the fine playable controller. The regular bay
pattern, sparse dressing, flat soil outside the apron, lighting and background remain visual work;
this increment alone cannot establish photorealism.

## Validation and native evidence

Parent `e15fb17`, native inspector SHA-256
`897752606468838cb7502002e82dc03e18d5d9112936f12e93a2a6d860510cef`.
`/tmp/fps-industrial-hardstand-EOd4lg/validate.sh` ran fail-fast and completed with exit 0: format,
strict all-target Clippy, 534 ordinary tests each in debug/release, required targeted network,
secure transport/authority/process/OIDC suites, two compile-fail doctests, all six explicit GPU
checks in both profiles, destruction/structure/physics/snapshot benchmarks, twenty fine mesh runs
per stage and five native smokes. The extra corner/front-edge ray assertions added during the run
were subsequently rerun successfully in both profiles. The eight bounded-tool Python tests passed.

Native Vulkan release at 1440×900, 4× MSAA, exposure 0.75; Linux 7.2.2-1-cachyos, Rust 1.97.1,
i7-13700H, RTX 4050 Laptop 6,141 MiB, NVIDIA 610.57.04. These measurements followed the standalone
build, with no compilation, RenderDoc or screen recording running. Clocks and compositor pacing
were not fixed. No controlled speedup, sustained combat, server load or cross-OS acceptance is
claimed from these short runs.

| Native view | CPU with present p50 / p95 / p99 (ms) | GPU p50 / p95 / p99 (ms) | Peak RSS (KiB) |
| --- | --- | --- | --- |
| Fracture, 12 s | 3.012 / 3.193 / 3.549 | 2.399 / 2.442 / 2.820 | 269,844 |
| Approach, 12 s | 2.714 / 2.908 / 3.453 | 2.103 / 2.136 / 2.806 | 269,580 |
| Wide, 12 s | 2.413 / 2.616 / 2.870 | 1.800 / 1.847 / 2.204 | 270,024 |
| Small exact fixture, 8 s | 1.395 / 1.539 / 1.661 | 0.795 / 0.806 / 0.813 | 263,572 |

Industrial GPU sample counts were 4,012 / 4,435 / 4,993 with no dropped queries; every view showed
and probed all four stages. Bootstrap took 313–364 ms and stage replacements about 33–40 ms.
The twenty-repeat CPU mesh p50 values were 22.812 / 23.204 / 24.156 / 24.809 ms. The added geometry
increases extraction work; the resulting content still stays within the existing budgets.

Initial source fingerprint: `d08485ca6c1b16be27b11abac6f52b9e`; final low-wall breach:
`4b2d949f10762162c43ab41a662b8f27`. The other two stages are captured in the native logs. Scene
preparation happens outside extraction and frame timings.

Graphify's local AST index was updated and checked; sources and tests remain the evidence. Claude
provided one analysis and one review of the complete new runtime/tests and all modified Rust diffs.
Its useful request for targeted corner/front-edge rays was implemented and tested. The concern
about capsule traversal is retained as a limitation, not dismissed by point-ray success. Its
suggested area calculation omitted inclusive endpoints; the exact 566-cell envelope is independently
counted by tests. The existing global scene guard is 16,384 leaves, not a per-cell guard; no limit
was enlarged. The equal-tick transaction contract is independently exercised by successful replay.
The changed viewpoint test belongs to the fine inspector, whose actual map it now checks; other
coarse-map tests are unchanged. Documentation and native evidence are Codex's own checks, not a
claim that Claude inspected unseen files or ran the engine.

## Native visual comparison

Matching before views are the previous roof increment's `target/tooling/renderdoc-_r7ggpz1/`
(wide), `renderdoc-4xg63vm2/` (approach), and `renderdoc-ua8lc2px/` (fracture), each with
`breach-thumbnail.png`. Lighting, camera and material settings are unchanged. Final after captures
use the standalone binary above, RenderDoc 1.45, Vulkan frame 400 and 14 textures:

| View | Thumbnail under `target/tooling/` | Capture bytes / draws |
| --- | --- | --- |
| Wide | `renderdoc-ac0rxduu/breach-thumbnail.png` | 296,233,637 / 245 |
| Approach | `renderdoc-2b8g2syb/breach-thumbnail.png` | 302,539,875 / 221 |
| Fracture | `renderdoc-wnn_t8qo/breach-thumbnail.png` | 305,403,398 / 219 |

All three completed capture/replay and their owned native game smokes; all thumbnails were
inspected against the before views. The apron breaks up the previous uniform soil and supplies
actual section depth at its joints. The close rubble remains grounded; no added facade shell or
hidden collider was introduced. The result remains a sparse and overly regular industrial scene,
well below the user's photorealistic reference. A fixed shot does not prove absence of shimmer.

The earlier approach preview `renderdoc-6aape9me/` used a pre-final executable hash
`fd1495fb0820ae81b6b9c9695b74d0cc6667510acaa49431d0a0a01b3dc560ea` and was not reused as final
binary evidence. It and the three older pre-roof window captures (`renderdoc-zmj7dvlf`,
`renderdoc-x3m4m1h7`, `renderdoc-h8iebesu`) were moved intact into
`target/tooling-archive-lHJXgO/`. No evidence was deleted or retention cap raised. These are local
recoverable archives, not off-machine backups.

The existing finite motion harness was reused in `target/hardstand-motion-HZUXux/`: six seconds of
orbit/zoom, 90 frames at 1440×900/15 FPS, only the PID-verified owned X11 game window. Network/PID
isolation and file/time limits were retained, with no global input hook or new installation. Both
recording and the 20-second native smoke completed normally (43/43/43/939 frames across the four
stages, zero dropped GPU queries). Extracted video frames 5/25/75 were inspected and show real
camera movement with consistent slab/corner placement. This is a motion spot-check, not exhaustive
high-refresh shimmer or capsule traversal validation; recording timings are excluded above.
Final executable SHA remained unchanged after capture and motion. Active capture evidence is about
1.8 GiB and the local recoverable archive 6.0 GiB.
