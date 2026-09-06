# Grounded polygonal rubble and retained skin planes

Parent `2d0c61a`. The photorealistic ruined-factory reference remains unfulfilled. This authored
map revision replaces twelve large beveled rectangular rubble footprints with asymmetric convex
polygon footprints, exact quarter-turn orientations and varied thickness. The eight original
small pieces, their frozen position/index oracle, the facade, ground and the rest of the map stay
unchanged. This is static scene authoring, not mass-conserved explosive debris or a new collapse
simulation. It improves individual pieces, not yet a dense, plausibly settled rubble pile.

## Actual physical shape

`fixture/ruins/polygon.rs` owns three fixed five/six-edge convex outlines with a flat retained
edge at local z=208. Four exact integer quarter turns about (128,128) produce twelve variants.
The same inverse rotation maps the sample into both its footprint and affine top-height frame.
The authoring loop is bounded to 196 candidate columns per piece, with x/z spacing 16/256 m
(6.25 cm) and y-height quantization 4/256 m (1.5625 cm). Accepted column counts are 126, 94 and
113 by template. The 16-unit halo stays inside the original one-metre source cell.

The polygon is an authoring template, not a hidden ideal collider: center-sampled, quantized
columns are the exact stored physical solid and the input to meshing, ray and collision queries.
Their vertical sides still have visible steps. Every column starts at local y=0 and is at least
16 units high. Connectivity is checked by positive-area grid faces, not diagonal contact; each
piece has a full supporting ground cell. Placement refuses occupied targets or missing support
before applying its one bounded transaction. Material and integrity are preserved within each
new fragment; volume is intentionally different from the previous authored layout.

The existing shared `Shard` record still supplies placement, material, thickness and slopes;
polygon templates, rather than the legacy rectangular extent, determine these twelve footprints.
`place` accepts a bounded volume-builder function so the original small-piece path remains exact.
There is no renderer-only vertex displacement, rigid-body transform or physics proxy.

## Original skin must identify a plane

Oblique sampled boundaries contain multiple steps with the same outward X/Z orientation. Merely
preserving every +Z face would stripe intact brick skin across broken edges. The new immutable
`CutTopAndSidesAtPlane(face, coordinate)` policy preserves only the original geometric face on
one explicit local plane; other vertical steps are cut core. The four rotated retained planes
are +Z/208, -X/48, -Z/48 and +X/208. Actual generated surfaces contain positive original face area
and additional same-facing broken steps. Terrace-top shading takes precedence as before, avoiding
skin stripes across a sloped cut top. The underside remains unchanged.

The constructor rejects Y-face choices and coordinates above 256. In-range planes with no matching
surface are permitted and simply preserve no side; they do not create geometry. Existing CutTop
and CutTopAndSides behavior is unchanged. The registry v3 key includes a fixed three-byte policy
record (variant/face and little-endian coordinate), as well as exact source coordinates/volume.
Thus appearance keys intentionally change, independently of physical world fingerprints. The
existing worker/stream identity checks still reject stale appearance before publication.

The 35-entry registry stays below the same 64-cell/8,192-leaf caps. Scene cells, internal leaves
and authoring sample columns are different units. Old policies retain four charged work units per
enrolled quad; explicit-plane policies charge eight. Unenrolled quads incur no new predicate charge.
No shader, scan pack, renderer cap, physics limit, runtime authority or tool installation changes.

## Evidence and bounds

New targeted tests cover convex winding, asymmetric edges, exact bounded inverse rotations,
all vertical lattice samples at every grid center, volume sums, deterministic repetition,
positive-area footprint connectivity, source/other-cell preservation, reinstallation refusal,
rotated sample-height equality and positive original/broken face areas. 588 downward scene rays
verify actual stored top entry parameters, continuous material intervals to the base, exact voxel
material/integrity and clear unoccupied corners. Canonical Y bands may return several contiguous
intervals; the test proves their union, not an unjustified single-leaf assumption.

An independent notched-piece integration test checks four original-plane orientations, including
two same-facing surfaces on different planes. It proves per-triangle marker consistency and exact
indices/all other vertex bytes after clearing only markers. Negative cases reject invalid axes
and coordinates; distinct coordinates/policies produce reproducible distinct keys. The industrial
marker oracle independently restricts cut sides to the twelve authored cells and excludes their
rotated retained planes. All twenty pieces still pass existing terrace-normal recognition; no
normal-fitting thresholds or frozen original geometry checks were relaxed.

The four explicit physical source revisions are:

- baseline: `a31fc6bedc9498046ab5129d6b7c7b26`;
- shallow chip: `dce184dc3a5364c429771b224ae27ff5`;
- through bore: `169f63c4e1902400054d80200e9cd3e0`;
- breach: `b8f3d69f8722e4efa9fb0ded35405e86`.

The finish key is `2dad496e195bbc552b0aed3d82b86896` across all four stages. Initial source is
2,351 pages/14,545 leaves; final source 2,348 pages/15,076 leaves, 1,308 below the unchanged 16,384
content guard. Initial resident geometry is 297,519 vertices/555,624 indices; final dirty geometry
75,098/172,182, down from 303,368/569,826 and 80,947/186,384 respectively. The reduction comes from
smaller/thinner authored solids, not hiding faces. The four full-map versus dirty extraction tests
still compare complete meshes. Maximum dirty-job work is 2,473,384 of 4,194,304; final eight-job
total is 6,951,597, not a single-job allowance. No cap was raised.

Graphify guided dependency inspection and was refreshed/checked. Claude's bounded analysis was
available; its concerns about raster connectivity, rotation and retained-plane existence are
addressed by direct tests. Its 64-cell/196-column concern conflated distinct units. Frozen geometry
does not include render metadata, so changing appearance keys does not invalidate that oracle.
The complete changed Rust diff was submitted for final review, which ended with exit 75
(`call_budget`) and no usable result. No retry or budget increase was made. Codex owns the final
source review and validations; no external approval is claimed.

## Final validation and native measurements

On 2026-09-06 `/tmp/fps-polygon-rubble-IvrI94/validate.sh` completed with exit 0: formatting,
strict all-target Clippy, 555 ordinary tests in each debug/release profile, two compile-fail
doctests, all six normally ignored real-GPU tests explicitly executed in both profiles, targeted
network/secure transport/authority/server-process/OIDC checks, destruction (500 events), structure
(100 iterations), physics (1,024 bodies/300 ticks), snapshot (20 iterations), locked standalone
builds, twenty industrial mesh runs per stage, playable smoke (5 s), exact inspector (8 s) and
three industrial views (12 s each). Earlier targeted failures were corrected before this final
run; no headless skip is counted as success.

Standalone inspector SHA-256:
`27ff423cb65aec64f1e64b80607d5ad95eeff7ad079a818c088e356ff785285f`.
The early close capture used this same hash, verified after the final standalone rebuild.
Cold full-map extraction was 105.468 ms; final warm eight-job extraction p50/p95/p99 was
26.180/26.338/26.499 ms. Source occupancy remains 96,601 initially/96,598 in the final stage.
There are 105,404 cut-marked vertices across four dirty meshes; indices and all other vertex
attributes still match unstyled extraction of the same revised physical source.

Short uninstrumented native observations on Intel i7-13700H, RTX 4050 Laptop 6,141 MiB,
driver 610.57.04, Linux Vulkan release, 1,440 x 900, 4x MSAA, exposure 0.75. Milliseconds:

| View | CPU with present p50/p95/p99 | GPU p50/p95/p99 | Peak child RSS KiB |
|---|---|---|---|
| Fracture | 3.418 / 3.668 / 4.211 | 2.805 / 2.851 / 3.590 | 270,120 |
| Approach | 3.316 / 3.538 / 4.111 | 2.705 / 2.754 / 3.527 | 270,168 |
| Wide | 2.974 / 3.140 / 3.431 | 2.370 / 2.404 / 2.727 | 270,328 |

All four authored stages were presented, with zero dropped GPU timestamp queries. The wide run
had a 12.516 ms maximum CPU frame, distinct from its p99. Cold stage publication reached 409.159 ms,
not a synchronous frame time. Compared with the previous sequential receipt, close/wide GPU tails
are lower but approach tails are higher; this is not a controlled overall speedup claim. These
short, mostly-final-state observations do not qualify sustained combat, multi-OS/hardware or the
1080p shipping gate. Allocation counts, fine authoritative server ticks and network correction/loss
rates remain unmeasured by this slice. The unchanged coarse checks are separate: synchronized
500-event destruction replicas, physics tick p95/p99 1.134/1.163 ms, structural combined
3.804/3.832 ms and snapshot total 12.762/14.159 ms. Fine weapon/authority activation remains pending.

## Native visual comparisons

Three owned 12-second native captures completed with successful RenderDoc 1.45 Vulkan replay,
frame 400 and fourteen textures. They use the standalone hash above; instrumented capture timings
are not used as performance evidence. Local artifacts under `target/tooling/` are excluded from Git:

| View | Parent comparison | Final capture | Final bytes | Draw calls |
|---|---|---|---|---|
| Fracture | `renderdoc-omuve6fg` | `renderdoc-ysgt2yk7` | 306,383,269 | 219 |
| Approach | `renderdoc-omwnmcmw` | `renderdoc-1813af1w` | 302,170,280 | 221 |
| Wide | `renderdoc-g6p1g_le` | `renderdoc-zr_7b41o` | 295,551,138 | 245 |

The unmodified `breach-thumbnail.png` images were inspected. The close view shows thinner,
asymmetric and differently oriented fragments, with cut sides and preserved flat skin planes.
The original small fragments, facade opening and surrounding map remain stable. The shapes are
more varied, but boundary stairs remain visible, placements remain sparse/regular, and the broad
composition barely changes. Neither a photoreal scene nor a physically settled pile is claimed.

The older `renderdoc-msjyprzo`, `renderdoc-7xlu5bsg` and `renderdoc-hp627oog` were moved intact
into the existing `target/tooling-archive-lHJXgO/`. Direct parent/final pairs remain active. No files
were deleted; the moves are recoverable local archives, not backups. The 2 GiB/2,000-entry active
retention and 512 MiB per-capture limits were unchanged.

The first motion attempt under `target/polygon-rubble-motion-wZwYbX/` ended with process exit 143
before native completion was logged, although its video contains ninety frames. It is retained
but not counted as a successful smoke; the cause of that signal is unproven. The process handle
was terminal and its owned descendants were absent before a fresh bounded attempt was started.

`target/polygon-rubble-motion-recheck-ICzsHD/motion.mp4` completed successfully: only the verified
owned game window, 1,440 x 900, six seconds at 15 FPS, exactly ninety frames, with bounded orbit/zoom
inputs and a clean 20-second native smoke. No timeout or capability was expanded. Unmodified
frames 5, 25 and 75 (`motion-01.png` through `motion-03.png`) were extracted and inspected; the
fragment footprints, ground contacts and retained/cut planes remain coherent in those sampled
viewpoints. This limited sampling is not a full temporal-aliasing, traversal or performance proof.
The executable hash was verified again afterwards and remained unchanged. The overall FPS goal,
including photorealism, generalized destruction and multi-OS multiplayer shipping, remains open.
