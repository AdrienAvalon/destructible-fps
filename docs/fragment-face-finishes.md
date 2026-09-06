# Explicit original and broken fragment faces

This is the initial face-policy receipt. The subsequent [polygonal rubble revision](polygon-rubble.md)
adds retained-plane metadata and intentionally revises the twelve large pieces' physical shapes.

The native ruined-factory scene is still below the photorealistic reference. This slice addresses
one material error: large bay fragments previously wrapped the intact brick scan around every
vertical side. Now each of those twelve authored pieces preserves its courtyard-facing (+Z) skin,
with exposed top and other sides using the existing cut-core material. The underside remains
unchanged. This is an authored convention, not measured fracture history, rotated-body surface
lineage, a new damage model, or a silhouette improvement. Sparse/blocky rubble remains further work.

## Immutable render metadata

`SurfaceFinishes::with_policies` accepts sorted unique `(position, FinishPolicy)` pairs. `CutTop`
retains the previous behavior. `CutTopAndSides(face)` accepts only the four horizontal-axis faces;
Y-face choices are rejected. The original eight small pieces and fifteen upper facade cut pages
keep `CutTop`; twelve large bay fragments receive the new policy. No global height/material guess
enrolls other cells. There are still 35 entries, below the unchanged 64-cell/8,192-leaf caps.

Exact immutable source validation, homogeneous Brick/Concrete-only pages and bounded allocations
are retained. The registry's v2 fingerprint includes each policy byte as well as coordinates and
source-volume fingerprint. Geometry equality intentionally does not include render appearance;
worker identities do, and the stream rejects a mismatched finish key before resident publication.
Conflicting policies for the same position are rejected as duplicates, not treated as two cells.

The exact quad face selects the vertical sides. Existing terrace-top recognition takes precedence:
an upward shading normal above 0.5 is cut even on a microscopic riser oriented toward the original
side. Whole footprint sides keep their geometric normals under the existing terrace recognizer.
This prevents intact-scan stripes across a sloped cut top without smoothing an intact face boundary.
Each quad already owns separate fan vertices, so marker discontinuities require no extra vertices
or indices. The new predicate is charged four work units per emitted enrolled fine quad, within the existing
mesh-job meter. This is bounded work accounting, not a measured CPU-cycle count.

The shader, scan pack, texture count, physical material/integrity, collider geometry, mesh positions,
normals and indices are unchanged. Only the explicit per-quad marker changes. The original four
physical world fingerprints from `industrial-bay-profile.md` remain fixed. The appearance key is
`5f7c1dbebb836d040e5410fb5ade12db`, identical across the four authored stages. Across four dirty
meshes, 126,536 vertices carry the cut marker, compared with 99,992 in the preceding top-only scene.

## Validation and native comparison

Independent integration fixtures exercise all six geometric faces, four preserved-side choices,
Brick and Concrete, an unenrolled neighbor and exact vertex bytes after removing only markers.
Every triangle has one marker, every face is observed, indices stay identical, and policy changes
produce distinct reproducible keys. Negative cases include invalid orientations, duplicate,
oversized and unsorted lists, absent source, unsupported material, stale geometry and the global
leaf cap through the new constructor. Existing scheduler and stream tests check source/appearance
identity and continued operation after rejected jobs. A dedicated unit test covers terrace-riser
precedence. These tests do not establish a general dynamic-fracture surface lineage system.

Graphify guided inspection of the existing source/mesher/worker path and was refreshed afterwards.
Claude's bounded analysis and final review were available. Its possible shared-vertex conflict
does not apply to the existing per-quad fan representation; exact byte/index and triangle-marker
tests establish that distinction. Source hashes deliberately exclude render appearance. Its final
confirmed concerns were addressed: predicate charges now apply only to enrolled quads, and the
industrial marker oracle checks all side-marked vertices against an independent list of the twelve
allowed pages, excluding all original small shards and facade caps. The six-face fixture also
checks exact work deltas (existing lookup/source validation plus four units per enrolled quad),
constructor equivalence and conflicting-policy rejection. No limits were raised, and no second
review or larger Claude budget was requested. Full-map construction was already exercised in
each of the four stage/finish integration cases, contrary to the review's missing-test suggestion.

On 2026-09-06 the final fail-fast `/tmp/fps-fragment-faces-SxSPj9/validate.sh` completed with
exit 0 after the review corrections and test-helper extraction: formatting, strict all-target
Clippy, 548 ordinary tests in each debug/release profile, two compile-fail doctests, all six
normally ignored real-GPU tests explicitly run in both profiles, targeted network/secure transport/
authority/server-process/OIDC checks, destruction (500 events), structure (100 iterations), physics
(1,024 bodies/300 ticks), snapshot (20 iterations), locked standalone builds, twenty industrial
mesh runs per stage, playable smoke (5 s), exact inspector (8 s) and three industrial views (12 s).
Earlier partial passes are not the final receipt. No dependency/tool installation, shader, source
asset pack, infrastructure, authority or permissions changed.

Parent `14da254`; final standalone inspector SHA-256:
`c73ddfb640498161966f29a6d61902a4098e77c5f8fe4c73a2cbc417b5bc0d9b`.
Initial residency remains 303,368 vertices/569,826 indices; final dirty replacement remains
80,947/186,384. Maximum dirty-job work is 2,842,697 of 4,194,304. Final eight-job total work is
7,528,838 (31,920 additional charged units, not a single job). Cold full-map extraction was
108.263 ms; final warm eight-job p50/p95/p99 was 28.485/29.262/29.433 ms. Physical source geometry
and its unchanged 16,384-leaf content guard retain 450 leaves of headroom in the final stage.

Short uninstrumented native observations on Intel i7-13700H, NVIDIA RTX 4050 Laptop (6,141 MiB,
driver 610.57.04), Linux Vulkan release, 1,440 x 900, 4x MSAA, exposure 0.75. Milliseconds:

| View | CPU with present p50/p95/p99 | GPU p50/p95/p99 | Peak child RSS KiB |
|---|---|---|---|
| Fracture | 3.541 / 3.784 / 4.267 | 2.933 / 2.981 / 3.618 | 269,316 |
| Approach | 3.199 / 3.363 / 3.977 | 2.593 / 2.620 / 3.376 | 269,688 |
| Wide | 3.016 / 3.191 / 3.479 | 2.409 / 2.450 / 2.749 | 269,652 |

All four stages were presented with zero dropped GPU queries. Cold stage publication reached
414.120 ms, separate from frame time. The timings are short, sequential and mostly final-state;
they are not a controlled speedup, sustained combat, multi-OS/hardware or release-gate proof.
No shader sample count was raised. Allocation counts, fine authoritative server tick tails and
network loss/correction rates remain unmeasured by this appearance slice. Separate unchanged
coarse benchmarks retained synchronized destruction replicas; physics tick p95/p99 1.149/1.173 ms,
structural combined 4.080/4.152 ms, snapshot total 12.626/13.833 ms. These do not activate fine combat.

The three final captures completed their owned 12-second native smoke and RenderDoc 1.45 Vulkan
replay (frame 400, fourteen textures each). All use the standalone hash above. The early fracture
and approach captures match the final standalone rebuild; an early wide capture made with Cargo's
test-built executable (`bd16208798bfa27c96069b43ed83809426e2d5748ca6b36e7dceb6a5222ea53f`)
was set aside and repeated after the final build. Captures are instrumented visual evidence, not
the uninstrumented performance runs above. Paths are local under `target/tooling/`, not tracked:

| View | Parent comparison | Final capture | Final bytes | Draw calls |
|---|---|---|---|---|
| Fracture | `renderdoc-msjyprzo` | `renderdoc-omuve6fg` | 305,976,186 | 219 |
| Approach | `renderdoc-7xlu5bsg` | `renderdoc-omwnmcmw` | 300,470,644 | 221 |
| Wide | `renderdoc-hp627oog` | `renderdoc-g6p1g_le` | 294,812,126 | 245 |

The unmodified final `breach-thumbnail.png` images were inspected. The close view shows cut
material replacing the brick pattern on lateral broken faces while the original front skin stays
textured. Opening, ground contacts, silhouettes, nearby small shards and facade remain unchanged.
The effect is subtle at approach distance and negligible in the wide composition. This passes
the narrow material-distinction comparison, not overall photoreal acceptance: visible stair steps,
sparse regular placements, simplistic interiors and inadequate environment/light richness remain.
The next visual effort needs to address shape and coherent ruin composition, not present these
surface changes as a substitute for believable geometry.

`target/fragment-faces-motion-BAfLpY/motion.mp4` records only the verified owned game window:
six seconds, 1,440 x 900, 15 FPS, ninety frames with bounded orbit/zoom input. Its isolated
20-second native smoke and recorder completed successfully. Unmodified extracted frames 5, 25
and 75 (`motion-01.png` through `motion-03.png`) were inspected; the different finishes stay on
their respective surfaces in those views. This limited check does not prove full temporal
stability, new physical traversal or performance. The standalone SHA-256 was checked again after
recording and was unchanged.

Older `renderdoc-zsfxn2v0`, `renderdoc-ufhcowwu`, `renderdoc-cc3glnp2` and the test-built wide
capture `renderdoc-wes943uq` were moved intact into `target/tooling-archive-lHJXgO/` to retain the
direct parent/final pairs within the existing active evidence budget. No files were deleted; moves
are recoverable local archives, not backups. The 2 GiB/2,000-entry active and 512 MiB per-capture
guards were unchanged. The full photorealistic, destructible, multi-OS multiplayer FPS goal remains
active and unfulfilled.
