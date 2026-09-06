# Open ruined facade bay

## Authored geometry, not structural simulation

The left upper facade is now genuinely missing in the fine industrial inspection. Its remaining
masonry, open interior and additional rubble are material volumes shared by rendering and physical
queries. This is not a cosmetic facade over a different collider. It remains authored content:
there is no calibrated collapse, mass-conserved explosion or activation of fine multiplayer combat.
The user's photorealistic ruined-factory reference is still unfulfilled.

The pipeline builds the original low wall stages, eight original shards, windows, roof ruin,
hardstand, then the missing upper bay and twelve additional shards. Each intermediate candidate
is private; an error returns no scene. `bay::install` changes only x=-19..-11, y=4..13, z=15 and
the two existing projecting beam stubs in the same x range at y=13,z=16. It leaves the low
12-cell wall patch, whole roof, primary columns, hardstand and all thirteen other windows unchanged.
The full original six-by-five-metre steel frame is validated with the same window builder, then
removed entirely. Clipping individual bars could leave steel suspended above a missing sill.

The masonry cut samples a versioned asymmetric integer height profile every 32/256 m horizontally,
with 16/256 m vertical quantization. All cuts are AIR replacements in the actual source; surviving
material and integrity are unchanged. The initial input guard incorrectly assumed that sill/lintel
Concrete extended beyond the window width; a failing test identified Brick at (-19,6,15), and the
guard was corrected against `industrial.rs` without weakening it to accept arbitrary material.
Reinstallation on an already-cut frame, a missing frame cell or unexpected lower masonry is rejected.

The extra shards use the existing bounded wedge builder, with 16-unit sampling only for the new
pieces; the original eight retain 8-unit sampling and their frozen mesh-position/index oracle.
The new twelve cells are (-19,16), (-18,16), (-16,16), (-12,16), (-11,17), (-18,17), (-17,18),
(-16,18), (-14,18), (-12,19), (-14,20), (-18,21) in x/z, all at y=1. Each needs an AIR cell and a
full supporting y=0 cell; no existing piece or apron cell is overwritten. Bases are flat on the
soil, with sloped tops, rather than rotated boxes falsely claimed to have exact ground contact.
The dressing is not the complete removed mass, and the absent steel is not conserved in these pieces.

## Source and topology evidence

Three new tests exercise all four original low-wall stages, immutable source/repeatability, exact
scope and material preservation, full-frame removal, negative source guards, and actual material
rays through the missing mullion, sill and lintel. The cut changes 73 cells and removes 498,442,240
fine solid units in every stage. The volume of a full metre cell is 256 cubed; no new material is
introduced by the bay subtraction. Its single transaction keeps the unchanged 256-change and
32,768 before-plus-after-leaf caps.

The topology test uses the exact rectangular solid leaves in x=-20..-10,y=0..14,z=15..16, not
point samples on a raster. Positive-area face contacts form a test-only coordinate-compressed 3D
graph rooted at the unchanged y=0 foundation. Every leaf is reachable in all four stages:
587/689/723/1,023 boxes respectively, under the explicit 2,048-box quadratic-test bound. This
includes the projecting column layer; a two-dimensional silhouette alone would not establish
these contacts. Edge/corner-only contacts do not count. This proves regional connectivity, not
structural strength, safe cantilevers or a complete-building load path. Roof source remains
byte-identical to the separately tested previous roof increment.

The ground-contact test now covers all twenty shard profiles and their exact solid-volume sums;
occupied/unsupported placement and replay are rejected without mutation. All twenty are accepted
by bounded shading-normal reconstruction, adding 68,143 work (test ceiling 150,000). Whole-map
incremental publication tests still compare complete vertex bytes and indices in every stage.

Initial scene: 2,351 fine pages, 15,404 leaves, 303,735 resident vertices and 570,708 indices.
Final low breach: 2,348 pages, 16,289 leaves. The existing 16,384-leaf **content** guard now has
only 95 leaves spare: do not append more detailed content blindly or silently raise this guard.
The engine-wide cap remains 131,072 leaves and 4,096 pages. Maximum dirty job work is 2,807,101
of 4,194,304; final eight-job replacement totals 7,619,908 work, 84,338 vertices and 194,736
indices. Total work is not a single-job count. No renderer or transaction budget was increased.

## Validation and review

Parent `780f71c`, final standalone inspector SHA-256:
`8e707ccdb86d8fefb42c15abf6d6106142dff55aa7940165ce477b97a2940a50`.
`/tmp/fps-industrial-bay-OsZUfl/validate.sh` finished with exit 0: formatting, strict all-target
Clippy, 537 ordinary tests in each debug/release profile, required targeted network/secure
transport/authority/process/OIDC suites, two compile-fail doctests, all six explicit real-GPU
checks in both profiles, destruction/structure/physics/snapshot benchmarks, twenty fine mesh runs
per stage and five native smoke runs. The eight bounded-tool Python tests also passed. Graphify
update/doctor completed; its index is not a substitute for these tests.

Native Vulkan release, 1440×900, 4× MSAA, exposure 0.75; Linux 7.2.2-1-cachyos, Rust 1.97.1,
i7-13700H, RTX 4050 Laptop 6,141 MiB, NVIDIA 610.57.04 (same local test setup as the parent).
Measurements ran after standalone compilation, without RenderDoc, recording or parallel game
processes. Clocks/compositor pacing are not fixed. No controlled speedup, multi-OS or sustained
combat acceptance is inferred from these short runs.

| Native view | CPU with present p50 / p95 / p99 (ms) | GPU p50 / p95 / p99 (ms) | Peak RSS (KiB) |
| --- | --- | --- | --- |
| Fracture, 12 s | 3.214 / 3.430 / 3.812 | 2.610 / 2.647 / 3.308 | 269,780 |
| Approach, 12 s | 2.802 / 2.935 / 3.496 | 2.191 / 2.224 / 2.803 | 270,244 |
| Wide, 12 s | 2.457 / 2.700 / 2.902 | 1.849 / 1.879 / 2.207 | 269,468 |
| Small exact fixture, 8 s | 1.373 / 1.542 / 1.648 | 0.776 / 0.790 / 0.796 | 263,672 |

All four stages were shown and probed in each industrial view, with 3,764/4,304/4,911 GPU samples
respectively and no dropped queries. Native bootstrap took 320–384 ms; replacements about 36–45 ms.
CPU twenty-repeat extraction p50 was 27.390/26.975/27.452/29.097 ms across the four stages. The
extra shards increase extraction work even though the missing wall removes geometry; source
preparation is outside these frame and mesh extraction timings.

Full-map initial extraction: 132 chunks, 68,475 quads and 17,515,529 aggregate work. Source
fingerprints: initial `13ea35e91e7c18f3b1e858034eadfd4f`, shallow `6c14778bf8bbe433f22a51bc6f33f99c`,
bore `a66a90932378a4f7de10cabe2b4d5589`, breach `884324bc62112f2f5263f6a3ee7359f6`.

Graphify's local AST index guided the call-path check; source and tests verified the conclusions.
Claude supplied one analysis. Its concern about support dimensionality prompted the exact 3D
leaf-face test; its ground-contact concern is covered by the twenty-shard test. Its request for
simulation/load proof is retained as an explicit unfulfilled goal, not inferred from connectivity.
The final review was submitted with the complete modified Rust diff and new source/tests, but
the bounded call returned exit 75 (`call_budget`), with no usable review. It was not retried,
no budget or permissions were raised, and no partial result is counted as approval. Codex owns
the integration, source review and validations.

## Native visual evidence

Before views from `780f71c` remain under `target/tooling/`: `renderdoc-ac0rxduu/` (wide),
`renderdoc-2b8g2syb/` (approach), `renderdoc-wnn_t8qo/` (fracture), each `breach-thumbnail.png`.
All after captures retain the same camera, exposure, lighting and material settings, use native
RenderDoc 1.45/Vulkan frame 400 with 14 textures, and completed replay plus their owned game smoke:

| View | Thumbnail under `target/tooling/` | Capture bytes / draws |
| --- | --- | --- |
| Wide | `renderdoc-kfp853yw/breach-thumbnail.png` | 294,469,530 / 245 |
| Approach | `renderdoc-744rm3lc/breach-thumbnail.png` | 302,098,614 / 221 |
| Fracture | `renderdoc-lof17ev1/breach-thumbnail.png` | 305,298,282 / 219 |

The approach capture preceded full validation; its executable hash matches the final standalone
binary after all checks. All three thumbnails were inspected. The formerly repeating upper bay
is open and exposes real interior beams and shading; the close shot shows the extra rubble and
retained lower patch. There is no suspended steel frame. The outline still reads as a stylized
V-shaped cut, the ground remains sparsely dressed, and the shards retain overly regular shapes
and triplanar brick pattern on their tops. Convincing fracture-interior material appearance,
asymmetric breakup, ground relief, lighting and background dressing remain work. These captures
do not meet the photorealism gate and are not generated reference illustrations.

The three older roof captures (`renderdoc-_r7ggpz1`, `renderdoc-4xg63vm2`, `renderdoc-ua8lc2px`)
were moved intact to `target/tooling-archive-lHJXgO/`, without deleting evidence or raising the
active retention cap. These local recoverable archives are not off-machine backups.

The finite existing motion harness ran in `target/bay-motion-NZBqry/`, recording six seconds of
orbit/zoom (90 frames, 1440×900 at 15 FPS) from only the PID-verified owned X11 game window.
Network/PID isolation, file/time bounds and cleanup remained unchanged. Both recording and the
20-second native smoke completed; stage frame counts were 43/43/43/940 with no dropped GPU queries.
Extracted frames 5/25/75 were inspected for actual camera movement and consistent attached ruin
and rubble placement. This remains a motion spot-check, not a full high-refresh shimmer test or
player traversal test. Recording timings are excluded from the performance table. Final binary
SHA remained unchanged. Active captures occupy about 1.8 GiB; the recoverable local archive 6.9 GiB.
