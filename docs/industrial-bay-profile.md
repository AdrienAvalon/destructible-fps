# Asymmetric facade breakup and open central notch

## Authored map revision

The ruined-factory reference still has not been achieved. This revision replaces the previous
two straight slopes of the upper bay with staggered shoulders and depth-varying chipped caps.
An early native view still showed a thin masonry bridge above the lower passage; the final
revision removes it in the actual source, joining the openings in the final breach stage.
This is authored architecture, not a calibrated explosive/structural event or mass-conserving rubble.

The upper scope stays x=-19..-11,y=4..13,z=15, plus the previously removed projecting beam stubs
at y=13,z=16. The explicit new scope includes only two lower cells: (-16,3,15),(-15,3,15), both
fully replaced by AIR in every stage. All other source cells, including the remaining ten low-patch
cells, roof, primary columns, thirteen other windows, courtyard and rubble are preserved.
The small exact inspection fixture and its geometry oracles are not changed.

`bay::install` now accepts the intended low-wall stage. It regenerates the exact expected patch
using the existing `install_wall` builder on an empty coarse world, then checks the two complete
input cells before authoring them. Unknown stage, incompatible stage, missing/altered lower cell,
invalid upper masonry, incomplete steel frame and reinstallation are rejected without mutating
the input. This setup work is bounded and outside mesh-worker/frame timing. It is not a network
stage selection API. Stages 0..2 happen to share the same two input cells; equivalence there is
legitimate, while stage 3 differs and cannot be substituted. “Intact” denotes the baseline without
the new centre impact, not a globally pristine facade: the authored ruin/notch exists in all stages.

## Source geometry and rendering

Fifteen integer control points define the profile across the nine-metre bay. Four horizontal
samples per cell (64/256 m) and 16/256 m vertical quantization bound detail. The rear 75 cm retains
the profile; the front 25 cm is chipped by up to 25 cm. A two-metre-wide central floor at y=3
removes the old bridge completely. Every operation replaces a real box with AIR; surviving
materials and integrity are not painted or weakened. The one transaction remains under the same
99-authoring-change, 256-transaction-change and 32,768 before-plus-after-leaf limits.

The existing render-only cut-top registry now includes the 15 surviving refined upper-bay pages
(132 expected leaves) alongside the unchanged twenty rubble pieces. These are selected only from
the named, bounded authored upper-bay region, not arbitrary bodies or world-height inference.
The lower two mixed-material cells are AIR, not reclassified as homogeneous masonry. The combined
35-cell registry passes its existing global 64-cell/8,192-leaf constructor limits and exact-source
worker validation; no cap or shader is changed. Its key is `89ba4577b3989359a75cd32f00c31e79` across
all four stages. The policy marks upward caps, not every vertical fracture face or dynamic history.

The four physical world fingerprints intentionally change because the map volumes change:

- baseline: `1a629069c0f3ed580cc133f983ac8b69`;
- shallow chip: `659cd20b263411984f033a46a2328fba`;
- through bore: `afe23513fdf7515c6339a144e64c23af`;
- breach: `018e80489b4591b3cf8f2c89dd90aec9`.

The finish-invariance test's scene hashes are advanced to this explicit layout revision, with
the old values retained in the previous receipts/Git history. No frozen mesh-position/index
oracle for the small fixture or original rubble is regenerated. Clearing only finish markers
still gives byte-identical full vertex arrays relative to unstyled meshing of the same source.
Across four dirty meshes, 99,992 vertices carry explicit cap markers.

## Targeted evidence

Six bay tests cover exact scope/pure subtraction, immutable/repeatable input, full-frame removal,
negative guards, the profile landmarks/bounds, depth chips, cap registry stability, bridge removal
and foundation connectivity. The exact positive-area 3D leaf-face graph reaches every surviving
box in the facade plus projecting column layer: 577/679/713/837 boxes for the four stages, below
the existing 2,048-box test bound. This is not a load-bearing strength or complete-building collapse
proof; corner/edge contact alone does not count as support.

Four material-ray probes between front and rear cap heights verify exactly 25 cm of missing front
material and 75 cm of retained back material using rational chord parameters, with clear rays
above the caps. A finite 0.60 x 1.80 x 0.60 m box sweeps through the old bridge location for two
metres in every stage; the original scene stops it and the candidate allows the complete move.
This proves the explicit AABB query at that location, not capsule movement or full player traversal.
The pure-subtraction test compares every out-of-scope occupied cell, not just aggregate hashes.
All four full-map-versus-dirty meshing comparisons and all four finish/worker regression tests pass.

The cut changes 81 source cells. Removed solid units are 615,686,144 for stages 0..2 and
608,927,744 for stage 3, whose initial lower patch already contains missing material. Initial
scene residency is 303,368 vertices/569,826 indices (previously 303,735/570,708); final dirty
replacement is 80,947/186,384 (previously 84,338/194,736). Removing the bridge and upper masonry
explains the reduction; rendering is still compared against complete extraction, not a missing-face
fallback. Initial source: 2,351 fine pages/15,403 leaves. Final source: 2,348 pages/15,934 leaves,
450 below the unchanged 16,384-leaf content guard. Maximum dirty job work is 2,824,733 of 4,194,304;
the final eight-job total is 7,496,918, not one job. No global/transaction/renderer cap is raised.

## Full validation, review and native evidence

On 2026-09-06 `/tmp/fps-bay-profile-J3dBRG/validate.sh` completed with exit 0: formatting, strict
all-target Clippy, 545 ordinary tests in each debug/release profile, the six normally ignored
real-GPU tests explicitly executed in both profiles, two compile-fail doctests, targeted network/
secure transport/authority/server-process/OIDC checks, destruction (500 events), structure (100
iterations), physics (1,024 bodies/300 ticks), snapshot (20 iterations), locked standalone builds,
twenty industrial mesh runs per stage, playable smoke (5 s), exact inspector (8 s), and three
industrial views (12 s each). No tool, dependency, shader, source asset pack or infrastructure
configuration changed. Local logs are evidence, not shipped artifacts.

Parent `a36550b`; final standalone inspector SHA-256:
`1e8c4f69b2e8aa1433e07350b391854bbcad1eca7c55db08dac151f57e621764`.
The early final-profile approach capture used this same executable hash, verified against the
standalone rebuild after all tests. The earlier bridge-retaining prototype was not accepted.

Short uninstrumented native observations: Intel i7-13700H / NVIDIA RTX 4050 Laptop, Linux Vulkan,
release, 1,440 x 900, MSAA 4x, exposure 0.75. Milliseconds unless otherwise indicated:

| View | CPU with present p50/p95/p99 | GPU p50/p95/p99 | Peak child RSS KiB |
|---|---|---|---|
| Fracture | 3.544 / 3.707 / 4.440 | 2.929 / 2.987 / 3.833 | 270,240 |
| Approach | 3.311 / 3.556 / 4.103 | 2.707 / 2.734 / 3.501 | 270,120 |
| Wide | 2.926 / 3.149 / 3.517 | 2.322 / 2.352 / 2.823 | 269,880 |

All four stages were presented, with no dropped GPU timestamp samples. Cold full-map extraction
was 110.211 ms; warm final eight-job extraction p50/p95/p99 was 28.180/28.412/28.907 ms. Cold native
publication reached 419.345 ms in the fracture view, distinct from a synchronous frame time.
Compared with the preceding sequential receipt, GPU p95 is lower in these views but p99 is higher;
there is no controlled global speedup claim. Sustained combat, other hardware/backends, allocation
counts, fine-world authoritative server ticks and network loss/correction rates remain unmeasured
by this slice. The unchanged coarse simulation checks are separate: physics tick p95/p99
1.171/1.199 ms, structural combined p95/p99 4.018/4.178 ms, snapshot total p95/p99 12.616/13.545 ms;
the 500-event destruction run retained synchronized replicas. They do not activate fine multiplayer.

Graphify guided the source-path inspection and was refreshed/checked after the changes. Claude's
bounded analysis highlighted registry-size and collision evidence; the actual registry contribution,
negative source guards, exact depth rays and finite-box sweep above address the confirmed gaps.
Its early concern used the whole 90-cell search region, not the actual 15 selected pages; the
existing constructor limits are global and were not relaxed. The final design also restored
450 leaves of headroom. A topology proof is not promoted to strength/capsule/collapse evidence.
The final review was submitted with the complete modified Rust diff, but returned exit 75
(`call_budget`) without a usable result. It was not retried and no budget was raised. Codex
performed the final source/test review and owns the integration; no external approval is claimed.

Final native captures completed their owned 12-second smoke and RenderDoc 1.45 Vulkan replay
(frame 400, fourteen textures each). Instrumented timings are not release performance evidence.
Paths below are local under `target/tooling/`, excluded from Git:

| View | Parent comparison | Final capture | Final bytes | Draw calls |
|---|---|---|---|---|
| Wide | `renderdoc-zsfxn2v0` | `renderdoc-hp627oog` | 295,016,500 | 245 |
| Approach | `renderdoc-ufhcowwu` | `renderdoc-7xlu5bsg` | 302,734,608 | 221 |
| Fracture | `renderdoc-cc3glnp2` | `renderdoc-msjyprzo` | 305,050,448 | 219 |

The three final unmodified `breach-thumbnail.png` views were inspected. The old central masonry
bridge is absent, the opening is continuous and the shoulders are staggered instead of a simple V.
Caps expose the existing cut-core finish. These are local improvements, not reference parity:
silhouettes remain stepped, the interior ceiling is flat, rubble is sparse/blocky, and courtyard,
lighting and environment detail remain visibly inadequate for the photoreal target. No generated
image substitutes for a native capture and no global structural-support claim follows from it.

`target/bay-profile-motion-oIzhoC/motion.mp4` records only the verified owned game window, with
bounded orbit/zoom inputs: 1,440 x 900, six seconds at 15 FPS, exactly ninety frames. The isolated
20-second native smoke and recorder exited successfully. Unmodified frames 5, 25 and 75 were
extracted as `motion-01.png` through `motion-03.png` and inspected: the opening remains visible
from the sampled changing viewpoints, without the deleted bridge reappearing. This limited
sampling is not a full temporal stability, player traversal or performance qualification. The
standalone executable hash was checked again afterwards and remained unchanged.

To keep capture retention bounded, the preceding pre-soil directories `renderdoc-9s6ux3oh`,
`renderdoc-d6wimdt3`, `renderdoc-2y5qdlq_`, and rejected bridge-retaining prototype
`renderdoc-2a_s2v8n` were moved intact into the existing local `target/tooling-archive-lHJXgO/`.
Nothing was deleted; these moves are recoverable, not backups. The direct parent/final comparison
sets remain active. The 2 GiB/2,000-entry retention and 512 MiB individual-capture guards were
not raised. The full FPS goal remains open; this receipt validates only the authored visual slice.
