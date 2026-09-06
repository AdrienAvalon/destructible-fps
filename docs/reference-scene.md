# Ruined-factory reference scene

Historical receipt for `cedb136`. The native industrial viewer now uses the
[oblique inspection scene](convex-inspection.md); `industrial_reference_world` remains the unchanged
regression fixture documented below. These measurements and images belong to that earlier build.

This is an authored native geometry foundation, **not photorealistic visual acceptance** and not
fine weapon, structural-collapse or multiplayer integration. The full game goal remains open.

At that historical revision, `fine-geometry-demo --world industrial --view approach|wide|fracture`
and the corresponding mesh benchmark used `industrial_reference_world`. The older `industrial_inspection_world` is kept
unchanged as a frozen material/geometry regression fixture. Neither a generated concept nor a
Blender-only asset demonstration counts as output of this scene.

## Actual material geometry

- Two 25 cm concrete floor remnants at y=5 and y=9 have different receding broken fronts. Four
  continuous 37.5 cm posts and supporting cross-beams connect every authored solid to four complete
  concrete footings. The complete working space, including air between floors, must be empty before
  authoring. The installation adds 189 pages/731 leaves under local limits of 192/1,000.
- Two grounded collapse lobes replace the legacy twenty isolated fragments. Their 670 continuous
  columns occupy 54 pages/1,820 leaves. Three selected slab caps use 12.5 cm columns; the aggregate
  uses 25 cm columns. A clear 3 m approach and the existing projecting piers remain untouched. Low
  aggregate below 50 cm becomes stone toward the toe; concrete caps and occupied volume are unchanged.
- An eight-point irregular soil edge replaces the upper 12.5 cm of the old paving. The lower
  87.5 cm soil bed remains intact. Boundary columns are 12.5 cm wide and the fully reclaimed interior
  canonicalizes to uniform soil. This is changed material occupancy, not a painted overlay.

All changes are prepared on immutable candidate worlds with sorted bounded transactions and explicit
source-conflict refusal. Visible fine geometry and material queries use the same exact boundary.
These authored supports and piles do not prove that an explosion dynamically produced them or that
the fine structural solver can detach them. The server promotion gate remains separate.

## Appearance provenance and regression boundaries

`BrokenMasonry` is an explicit appearance policy for mixed brick/concrete/stone refined pages with
some masonry. It marks all exposed brick/concrete faces as cut core and leaves stone alone. Existing
homogeneous cut-top/retained-side policies, material IDs, integrity, geometry and protocol stay
unchanged. The 47 masonry pages plus 15 bay tops use 62 of the unchanged 64 entries. Selection checks
all 54 exact apron pages, including stone-only pages, and refuses a stale source before returning.

The new key tag is distinct from every valid retained-side orientation and plane coordinate. Work
remains charged to the existing mesh-job meter; all selection, source-leaf, per-job output/work and
resident limits remain in force. Appearance is not physical damage or authenticated state.

Tests cover exact material rays through all columns, quarter-metre floor thickness and clear
storeys, positive-face support connectivity, three character sweeps, invalid foundations and occupied
source refusal, exact occupancy preservation during toe material recomposition, six-face masonry
appearance and unchanged stone/unregistered vertex bytes. Full-scene tests compare every stage's
complete and incremental mesh bytes and all source cells outside the twelve-cell changing patch.

### Raster interpolation defect found by native inspection

The first native captures still showed intact scanned masonry on parts of the authored rubble.
The CPU mesh and finish key were correct, but the fragment shader compared an interpolated floating
marker to exactly -2.0. A real Vulkan raster test reproduced 3,188 false negatives in the brick and
concrete control triangles when vertex W differed; equal-W controls had none. The existing compute
shader tests passed because they did not exercise vertex-to-fragment interpolation.

The vertex shader now classifies the authored cut marker before rasterization and carries a separate
flat integer flag. The original fracture-depth varying remains continuous for the coarse wall's
layered shading. No vertex-buffer stride, physical material, geometry, network field or collision
changes. This follows [WGSL interpolation semantics](https://www.w3.org/TR/WGSL/#interpolation):
floating varyings default to perspective interpolation, while flat integer values are not
interpolated. Exact counts are hardware-specific; the regression must require correct final
classification, not require every driver to reproduce the old error.

## Visual assessment and next gate

The initial native comparison shows useful architectural depth and a less regular paving edge.
It also exposes a dominant failure: the two collapse lobes resemble stepped ramps. Increasing normal
fit tolerances cannot remove their actual axis-aligned silhouette. The thick original frame, flat
empty foreground and simplistic interior illumination remain far from the supplied reference.

The next geometry gate is a convincing large **oblique broken slab**, with the same visible boundary
used by physical rays and contacts, then an asymmetric accumulation of different-sized pieces and
finer aggregate. Do not disguise coarse steps with a detailed unrelated mesh, make new solid cover
indestructible, or call another texture-only change photorealism. Vegetation, weathering, background
layers and bounced light remain necessary parts of the complete scene, not substitutes for its forms.

Source inspection identifies a closed, bounded convex fragment with quantized vertices as the
appropriate next proof: derive its planes and render triangles from the same vertices, then test
rays and continuous contacts against them. Existing `LocalBox`/`SurfaceQuad` represent axial surfaces;
`RotatedVoxelShape` supplies conservative rotated-voxel AABBs, not exact convex contacts. A shared
triangulated heightfield could serve the aggregate/terrain later, but does not by itself represent
an overturned slab or independent oblique underside. Codec, damage and server promotion remain
explicit subsequent work, not capabilities inferred from a static convex demonstration.

## Independent review

One bounded Claude analysis and one targeted final review covered the supplied finish-policy diff,
apron installation/selection excerpts and a factual summary of the remaining geometry/tests, not a
full repository or image review. Codex independently read all changed source and tests. The review
led to a clearer exhaustive policy match and a test of every valid orientation/plane key. Its MSRV
question is resolved by `Cargo.toml` requiring Rust 1.97; authoring order comes from `BTreeMap`, and
the exact 670-ray/support checks are present and executed. Existing finish-policy mesh tests and
legacy fixture oracles remain; no separate cross-binary parent mesh-byte comparison is claimed.

The interpolated-marker shader defect was found independently during subsequent native inspection,
then reproduced and corrected with a new actual-raster test. It was not discovered or reviewed by
that earlier Claude call. The separate visual review and Codex agree that the new geometry improves
composition but still fails the photorealistic goal. Normal engine tests and native evidence, not
agreement between reviewers, determine what is demonstrated.

## Validation receipt, 2026-09-06

Parent `e6cf620`, plus this increment. `/tmp/fps-reference-scene-N1oMS6/validate.sh` completed with
exit 0: formatting, strict all-target Clippy, **574 ordinary tests in each of debug and release**,
seven actual Vulkan tests explicitly in each profile, two compile-fail doctests, targeted network,
secure transport/authority/process and OIDC suites, the four prescribed simulation/storage
benchmarks, twenty industrial extractions per stage, ordinary playable smoke, lighting stress,
small exact inspection and all three industrial native views. An initial Clippy stop concerned
three arithmetic expressions in the new raster oracle; they were corrected and the complete
pipeline rerun. The raster test records zero false negatives on 8,768 masonry pixels and zero
false positives; the continuous-depth oracle's maximum error is 0.000006145 in both profiles.

The unchanged 500-event destruction fixture finishes with synchronized replicas; the unchanged
1,024-body/300-tick physics fixture finishes with 1,024 sleeping bodies. These are not fine weapon
or fine rigid-body coverage. Graphify update/doctor report a fresh local AST index with no missing
endpoints; WGSL is outside that index and was independently inspected and actually GPU-tested.

The reference scene has 132 chunks and at most 12,568 refined leaves, 281,462 resident vertices and
517,518 indices. Maximum one-chunk work is 1,383,824 under the unchanged 4,194,304-unit job ceiling.
Initial extraction: 100.903 ms, 276,868 vertices/506,172 indices. Final dirty replacement retains
54,433 vertices/122,688 indices across eight jobs. Warm-process extraction timings (20 samples;
authoring and destruction of completed output excluded; p99 is the maximum at this sample size):

| Stage | Total charged work | p50 / p95 / p99 (ms) |
| --- | ---: | --- |
| Intact | 4,384,193 | 19.678 / 19.830 / 19.854 |
| Shallow chip | 4,502,874 | 19.793 / 19.916 / 20.248 |
| Through bore | 4,534,887 | 20.008 / 20.303 / 20.825 |
| Breach | 4,735,790 | 20.573 / 21.214 / 22.682 |

Native release measurements, after all compilation and without RenderDoc/recording: Linux
7.2.2-1-cachyos, Rust 1.97.1, i7-13700H, RTX 4050 Laptop 6,141 MiB, NVIDIA 610.57.04, Vulkan,
1440×900, 4× MSAA, exposure 0.75. No clock locking, cold-boot trial or compositor control.

| View / duration | CPU with present p50 / p95 / p99 (ms) | GPU p50 / p95 / p99 (ms) | Peak RSS (KiB) |
| --- | --- | --- | ---: |
| Fracture / 12 s | 3.518 / 3.749 / 4.339 | 2.902 / 2.963 / 3.788 | 295,396 |
| Approach / 12 s | 3.220 / 3.445 / 4.060 | 2.612 / 2.653 / 3.466 | 293,160 |
| Wide / 12 s | 3.038 / 3.157 / 3.460 | 2.431 / 2.465 / 2.702 | 293,664 |
| Small exact / 8 s | 1.418 / 1.588 / 1.685 | 0.822 / 0.834 / 0.840 | 279,636 |

All four authored stages are displayed and queried; all these runs have zero dropped GPU timing
samples. Industrial bootstrap delivery takes 345–416 ms; that is asynchronous scheduling latency,
not the extraction timer above. Short fixed inspections are not sustained destructive combat,
server-tail or cross-platform acceptance. RSS is not an allocation count. This changed scene is
not a controlled throughput comparison with the sparse-rubble parent; no global optimization claim.

Final standalone builds were republished after all release tests. SHA-256:

- `fine-geometry-demo`: `f8f2539de682878d1f55bac4c50e284c51f939be277877eab853dd2d9be53d6b`.
- `fine-mesh-benchmark`: `4331fe89f8d84d76d6febe7a11ed777995f836214e307078e6b8ce22a070b9fe`.

### Native visual evidence

Final actual RenderDoc 1.45/Vulkan frame-400 captures replay successfully and their owned twelve-
second native runs complete. All use the final inspector hash, unchanged cameras, five scan layers,
HDR pack, exposure and shadow settings; fourteen textures each. Paths are under ignored `target/tooling/`:

| View | Final run / thumbnail | Capture bytes / draws |
| --- | --- | --- |
| Fracture | `renderdoc-rsw0zcnl/breach-thumbnail.png` | 308,653,142 / 219 |
| Approach | `renderdoc-ppixdap9/breach-thumbnail.png` | 305,158,268 / 221 |
| Wide | `renderdoc-18dbdixe/breach-thumbnail.png` | 297,322,950 / 245 |

Same-scene pre-raster-fix captures are retained in `renderdoc-w_8qapyy` (fracture),
`renderdoc-ej27oax0` (approach) and `renderdoc-c335uea3` (wide), with inspector hash
`691e9185c1b2edddf5d56a4734377d89c9b73d12dacfe158469e0d4dd3602c69`. The final close capture
removes the intermittent intact scan on the aggregate. It also makes the inadequate stepped shape
and procedural core response unmistakable. The visual gate is still **not passed**.

A final bounded motion check used only the freshly launched, PID-verified game window inside the
existing network/PID sandbox: six seconds at 15 FPS, 90 frames at 1440×900, finite orbit/zoom input,
then clean completion of the owned twenty-second smoke. `target/reference-scene-final-motion-Ikf2oE/`
retains the harness, video, logs and extracted frames 5/25/75, all three visually inspected. The
matching earlier unfixed run remains in `target/reference-scene-motion-LEh4up/`. Final binary hashes
were checked again after capture/motion. Recording timings are excluded from the performance table;
this spot-check does not establish exhaustive temporal stability or every-angle fracture quality.

The three parent captures (`renderdoc-ysgt2yk7`, `renderdoc-1813af1w`, `renderdoc-zr_7b41o`) and
two earlier rejected-haze captures (`renderdoc-_p52o3qx`, `renderdoc-oxi9b1wt`) were moved intact
to the existing `target/tooling-archive-lHJXgO/`. No evidence was deleted or retention limit raised.
Active RenderDoc evidence is about 1.8 GiB; the separate archive is recoverable local evidence,
not an off-machine backup. No package, host privilege, listener, infrastructure or remote Git
state was changed for this increment.
