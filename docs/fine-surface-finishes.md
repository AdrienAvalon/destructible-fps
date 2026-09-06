# Explicit render-only cut finishes

This receipt describes the original twenty-piece rollout. The subsequent
[bay profile revision](industrial-bay-profile.md) extends the same bounded registry to authored
facade caps and records its intentional source-layout changes and new scene fingerprints.
The later [fragment-face policies](fragment-face-finishes.md) add explicit retained sides and
version the appearance key without changing those physical scene fingerprints.

**Later correction:** the [reference-scene raster regression](reference-scene.md) proved that
identical -2 markers at all three vertices do not ensure bit-identical perspective-interpolated
fragment values. The original exact floating comparison could therefore restore the exterior scan
on some cut pixels. Classification now happens at the vertex and uses a separate flat integer
varying; the continuous coarse depth is preserved. The earlier compute helper and still-image
checks below did not prove this interpolation boundary correct.

## Separate appearance from material integrity

The twenty authored industrial rubble pieces no longer need a fictitious integrity reduction to
show a broken top. `SurfaceFinishes` is a bounded, immutable render-only sidecar, built explicitly
from the existing shard registry. It is not inferred from world height, damage or a conveniently
shaped object. Physical cells, integrity, ray/collision queries and all four world fingerprints
remain identical to parent `176f97c`.

The constructor accepts at most 64 sorted unique fine-cell positions and 8,192 aggregate expected
leaves. Cells must contain homogeneous Brick or Concrete plus optional AIR, never uniform cells,
mixed masonry or other materials. It retains immutable expected cell values; before extracting
any candidate, the worker verifies exact source equality for all entries and charges that work.
Missing or changed source pages fail the whole candidate. There is no truncated fallback.
The per-cell binary search is bounded and charged; default meshing remains unstyled.

A version-salted deterministic key covers integer cell coordinates and expected volume
fingerprints. It is a local stale-result key, not authentication or the authoritative world hash.
The scheduler carries the immutable sidecar and echoes its key. The inspector verifies both the
world and finish keys before publication, including when a result is stale. The fixed industrial
scene reuses the same sidecar across four states because those twenty cells are unchanged.
Dynamic fracture provenance, runtime finish editing/invalidation and networked material history
are **not** implemented by this sidecar. Such changes require a new explicitly authored finish set
and a correctly invalidated/rebuilt renderer, not blindly retaining stale entries.

## Mesh and shader contract

The existing render-only `fracture_depth` attribute now has three disjoint meanings: -1 ordinary,
-2 explicitly exposed core, and 0..1 the older coarse mesher's through-wall depth. Vertex layout
and GPU buffer sizes are unchanged. On a tagged cell, upward cap faces and the bounded reconstructed
top terrace risers receive -2 (shading normal y > 0.5); side and bottom faces remain unchanged.
This is a top-finish policy in the authored material frame, not a universal fracture-face detector.

Each exact surface quad already owns its fan vertices, so the same marker is assigned to its
centre and boundary vertices; no extra vertex splitting is needed and no triangle interpolates
between finish classes. The fan emission was extracted into a helper without changing index order,
allocation checks or work charges. The unchanged coarse 0..1 path retains its previous behaviour.

The new pure production WGSL helper bypasses the exterior scan on tagged Brick/Concrete cores.
Brick receives rough ceramic colour variation and concrete a nonmetallic aggregate response.
It does not copy mortar grooves from an exterior scan or paint fictitious reinforcing steel.
Two bounded integer-lattice noise scales fade with the already-computed material-space fragment
footprint; no derivative is evaluated inside the conditional branch. The surface remains rough,
metalness is zero, and its existing geometric/reconstructed normal is retained. There are no new
textures, geometry, colliders, opacity or light sources. This is procedural cut appearance, not a
calibrated fracture scan, displaced microgeometry or physical reinforcement.

## Targeted evidence

Four integration tests cover explicit styling, per-triangle marker consistency, byte-identical
attributes after clearing only the new marker, unchanged index/vertex counts, the four frozen
physical world fingerprints, invalid lists/materials, leaf-budget overflow, stale sources and
worker recovery after a refused candidate. Empty finishes preserve default mesh bytes. The
publication test rejects a wrong finish key without changing resident meshes or the displayed
stage. Full versus dirty meshing is now compared using the actual styled industrial path, and the
benchmark prepares the sidecar outside its extraction timer.

Across all four dirty-stage meshes, 98,992 vertex markers change and no other vertex field does.
The common finish fingerprint is `01b85dd07820b8b0e3daf896d1484bdc`. GPU tests execute the actual
production helper over 128 signed-position samples, checking finite bounded reflectance, roughness,
zero metalness, unchanged normal, coordinate-seam continuity, far-field filtering and strict
selection of -2 only for masonry. Existing projection, weathering and environment tests remain.
The test checks branch selection separately; it does not claim a full scanned-texture render from
the compute harness. Native captures validate the actual material branch in the game.

Source geometry counts remain those of the bay increment: 2,348 fine pages and 16,289 leaves in
the final state, with only 95 leaves spare under the existing 16,384-leaf content guard. This lot
adds none. Initial resident geometry remains 303,735 vertices and 570,708 indices. Maximum dirty
job work increases from 2,807,101 to 2,821,337 (cap 4,194,304); final aggregate eight-job work rises
from 7,619,908 to 7,725,606, with the same 84,338 vertices/194,736 indices. No existing cap is raised.

## Full validation and native comparison

On 2026-09-06 the fail-fast receipt `/tmp/fps-cut-finish-CdLiyD/validate.sh` completed with exit 0:
formatting, strict all-target Clippy, 542 ordinary tests in each debug/release profile, the six
normally ignored real-GPU tests explicitly executed in each profile, two compile-fail doctests,
targeted network/secure transport/authority/server-process/OIDC checks, destruction (500 events),
structure (100 iterations), physics (1,024 bodies/300 ticks), snapshot (20 iterations), locked
standalone release builds, industrial meshing (20 iterations per stage), playable smoke (5 s),
exact inspector smoke (8 s) and all three industrial views (12 s each). Logs are local evidence,
not distribution artifacts. No cap, dependency, installed tool or infrastructure configuration changed.

The tested standalone executable SHA-256 is
`314b812a49d9dd9a86c4a9b908634bfc6e9495bdf544000af7cfc9b036b11a00`, verified again after
captures and motion. Measurements belong to this change on parent `176f97c`, release Linux Vulkan,
Intel i7-13700H / NVIDIA RTX 4050 Laptop, 1,440 x 900, MSAA 4x, exposure 0.75.
Uninstrumented short native smoke observations, in milliseconds:

| View | CPU with present p50/p95/p99 | GPU p50/p95/p99 | Peak child RSS KiB |
|---|---|---|---|
| Fracture | 3.551 / 3.776 / 4.129 | 2.929 / 3.031 / 3.589 | 286,744 |
| Approach | 2.978 / 3.194 / 3.771 | 2.362 / 2.408 / 3.134 | 289,440 |
| Wide | 2.498 / 2.716 / 2.943 | 1.889 / 1.915 / 2.303 | 285,640 |

No GPU timestamp samples were dropped. Cold full-map extraction was 107.248 ms; warm final-state
eight-job extraction p50/p95/p99 was 29.874/30.946/31.815 ms. Native cold stage publication reached
402.185 ms in the fracture view; worker latency is not a synchronous frame time. These short,
mostly final-state observations are not sustained combat/cross-platform performance guarantees or
a controlled claim of speedup. Allocation counts, network loss/correction rates and a representative
fine-world authoritative server tick are not measured by this visual slice. Separate existing
coarse benchmark observations include synchronized replicas after 500 events (latency p95/p99
0.252/0.430 ms), physics tick p95/p99 1.132/1.147 ms, structural combined p95/p99 3.809/3.846 ms
and snapshot total p95/p99 12.785/13.954 ms; they do not establish fine-world combat support.

RenderDoc 1.45 captured and replayed frame 400 on the same binary. Each owned native process also
completed its smoke check. All paths below are inside ignored `target/tooling/`:

| View | Before directory | After directory | After capture bytes / draws |
|---|---|---|---|
| Fracture | `renderdoc-lof17ev1` | `renderdoc-2y5qdlq_` | 304,969,701 / 219 |
| Approach | `renderdoc-744rm3lc` | `renderdoc-d6wimdt3` | 301,713,742 / 221 |
| Wide | `renderdoc-kfp853yw` | `renderdoc-9s6ux3oh` | 294,824,722 / 245 |

All contain `breach-thumbnail.png`; after captures use 14 textures. Direct native inspection shows
the brick top tiling replaced by a rough ceramic core while exterior sides, geometry and composition
remain unchanged. The farther views retain the same scene silhouette. The network/PID-isolated
owned-window motion harness in `target/cut-finish-motion-EllQM1/` completed with a six-second,
90-frame, 15 FPS video at 1,440 x 900; unmodified frames 5, 25 and 75 were inspected after orbit/zoom.
This is a bounded visual spot check, not proof of shimmer-free motion at every distance or gameplay FPS.
Older hardstand captures `renderdoc-ac0rxduu`, `renderdoc-2b8g2syb` and `renderdoc-wnn_t8qo` were
moved intact to the existing ignored `target/tooling-archive-lHJXgO/`; nothing was deleted and the
active artifact cap was not raised. This local archive is not a backup.

Graphify guided the source-path inspection and was refreshed/checked after code changes. Codex
reviewed the runtime diff and regression tests. The bounded Claude final review was invoked but its
result could not be recovered after output truncation; no external-review approval is claimed and
no retry or budget increase was used.

The reference-image target remains unmet: the rubble silhouettes are still too regular, the bay
opening is conspicuously stepped/V-shaped, ground repetition is visible and large structural faces
lack credible variation. Next rendering work should address those scene-scale defects, retaining
exact source/collision agreement and bounded geometry. This improvement only covers the twenty
authored rubble tops, not arbitrary dynamic fracture interiors, mass-conserving debris or collapse.
