# Soil scan repetition reduction

## Scope and source evidence

The previous material shader sampled the soil scan at `projection_uv * tile_scale` with a repeat
sampler. `assets/materials/sources.json` declares the physical tile as 1.3 x 1.3 m; the bounded
offline pack decoder stores its reciprocal. Weather modulation was applied afterwards, so the
stone/grain pattern still repeated even when broad colour varied. Changing that physical scale
would resize the stones, not eliminate repetition. The package and its authored scale stay intact.

This increment changes only soil sampling in `src/shaders/world.wgsl`. No world, mesh, collision,
integrity, damage, networking, camera, lighting, exposure, texture binding or asset bytes change.
The other four scanned material layers retain the direct sampling path; explicit cut cores and
procedural materials are not randomized. Geometry residency and worker budgets are unchanged.

The method is a limited translation-only application of triangular-grid blending, informed by
Morten S. Mikkelsen's [Practical Real-Time Hex-Tiling](https://jcgt.org/published/0011/03/05/).
The implementation uses normalized fourth-power barycentric weights; it does not implement that
paper's full colour-dependent blending or claim histogram preservation. No third-party code or
dependency is imported.

## Rendering contract

An equilateral grid chooses three integer sites using floor (including negative coordinates).
The existing unsigned integer lattice hash chooses one repeat-texture offset per site. Each site
keeps its offset across triangle/grid boundaries. An outgoing site's weight goes to zero on its
edge, leaving the same two shared sites on both sides. Nonnegative fourth-power weights sum to
one, so channel blending is convex with no overshoot or zero denominator.

Offsets are translations only: there is no change of scan scale, rotation, material tangent frame
or local object anchoring. Albedo/roughness and normal/metalness are packed into two texture arrays
and use the same sites and weights. Each normal is decoded to tangent slope before blending, then
projected through the existing signed axis frame and combined by the existing triplanar weights.
The smooth geometric normal remains the base for the existing surface-gradient reconstruction.
Weathering runs once afterwards in unchanged object-local coordinates, not at randomized offsets.

Explicit texture gradients originate at the uniform fragment entry point. A constant translation
has identity derivative, so the original projected gradients choose each sample's mip and anisotropy;
no implicit derivative of a hashed coordinate is introduced. At the final 1 x 1 mip all sites read
the same values. This is not exact integration of the blend mask over a large pixel footprint and
does not prove every minification angle free of aliasing. There is no discontinuous distance switch.

The bounded cost is three sample pairs per active soil projection (six fetches, at most 18 over
three axes), versus one pair per active axis before. Other materials remain one pair per axis.
No texture/allocation/residency cost is added; GPU frame cost still requires measured evidence.
The algorithm is camera/time independent. Cross-backend floating-point identity is not claimed,
and cosmetic texture coordinates never enter authoritative world fingerprints.

## Regression evidence

The existing explicit Vulkan integration test now binds the actual embedded five-layer scan pack,
with production formats, mips and filtering, and calls `scanned_plane`/`scan_plane` in production WGSL
using explicit gradients. The compute bindings are test-only; runtime bindings are unchanged.
There are 1,024 signed-grid edge probes and a separate deterministic spatial sample set for contrast.
Keeping those populations separate avoids biasing the contrast measurement toward repeated seam
coordinates. Checks cover finite bounded channels and offsets, normalized weights, nonmetallic
soil, shared diagonal/x/y boundaries, non-soil direct-path equality, old-period repetition and
the common final mip for both colour and slope. Existing six-axis projection, transformed-normal,
weathering, environment and cut-core tests remain active.

Targeted NVIDIA Vulkan observation: red-channel RMS difference after an old one-tile translation
is 0.076766 (the original direct sampler remains equal within 0.0001). New/original red-channel
spatial variance ratio is 0.995177 over this deterministic set, and the maximum colour/roughness
edge-probe difference is 0.004374 for a +/-0.00001 UV displacement. These are bounded regression
observations, not a universal histogram, photometric calibration or subjective quality score.

Following external review, 40 additional samples execute the full `sample_scanned` path across
all five layers, six signed axes and two oblique normals with the real scale uniform. They compare
against a frozen copy of the parent `2224b89` projection (with an explicit soil-only plane hook),
including the old divide-after-frame operation on unaffected materials. Colours, roughness,
metalness and normals agree within 0.00001; normals are finite and unit length. This is not a
CPU reimplementation. The test also checks the `Material::Soil` ID and source-manifest first layer.
The harness calls these checks explicitly, without string-based entry-point injection. Both debug
and release explicit GPU tests passed again after these test-only additions.

## Native receipt and remaining work

On 2026-09-06 `/tmp/fps-soil-tiling-mBCKJI/validate.sh` completed with exit 0: formatting, strict
Clippy, 542 ordinary tests in each debug/release profile, six explicitly executed real-GPU tests
per profile, two compile-fail doctests, targeted network/secure transport/authority/server-process/
OIDC checks, destruction (500 events), structure (100 iterations), physics (1,024 bodies/300 ticks),
snapshot (20 iterations), locked standalone release builds, industrial meshing (20 iterations per
stage), playable smoke (5 s), exact inspector (8 s), and three industrial views (12 s each). The
post-review changes were confined to the ignored GPU harness; strict Clippy was rerun, and that
test passed again in both debug and release. There was no runtime change after the full receipt.

Final standalone SHA-256: `c68e2f32c2b34c13c84c0ecd4e70a213cfe6dfb332a2168fc3f6527ba6dd1e93`.
The standalone locked build was restored after `cargo test --release` republished a differently
built executable at the same path; final captures/measurements are identified by the standalone
hash, not by the filename alone. No tool, dependency, permission or infrastructure setting changed.

Short uninstrumented observations on Intel i7-13700H / NVIDIA RTX 4050 Laptop, Linux Vulkan,
release, 1,440 x 900, MSAA 4x, exposure 0.75, this change on parent `2224b89` (milliseconds):

| View | CPU with present p50/p95/p99 | GPU p50/p95/p99 | Peak child RSS KiB |
|---|---|---|---|
| Fracture | 3.849 / 4.011 / 4.282 | 3.238 / 3.285 / 3.610 | 283,256 |
| Approach | 3.427 / 3.675 / 3.890 | 2.814 / 2.860 / 3.224 | 282,780 |
| Wide | 2.987 / 3.150 / 3.430 | 2.380 / 2.419 / 2.728 | 289,052 |

Against the previous same-scene receipt, GPU p95 increases by 0.254/0.452/0.504 ms respectively
(about 8/19/26 percent). This is a measurable quality cost, **not a performance optimization**.
It is accepted for this visual increment with unchanged texture/geometry memory, not as a promise
for slower GPUs. These sequential short runs are not a thermally controlled paired benchmark,
long combat endurance test or cross-platform performance proof. All GPU timestamp samples were
retained. Cold full-map meshing was 106.705 ms; final eight-job meshing p50/p95/p99 was
28.790/31.045/31.523 ms. The fracture view's cold publication reached 412.852 ms; asynchronous
publication latency is not CPU frame time. Initial residency remains 303,735 vertices/570,708
indices. Per-allocation telemetry and representative fine-world server/bandwidth/loss/correction
measurements are not supplied by this rendering slice. The separate existing coarse benchmark
retained synchronized replicas after 500 events (latency p95/p99 0.246/0.413 ms); its physics tick
p95/p99 was 1.127/1.146 ms. These do not establish fine-world multiplayer destruction.

Graphify identified the material-loader path, then the actual Rust and WGSL sources were read;
the AST tool does not index WGSL. Its update/doctor completed without stale code. Claude's bounded
analysis and review returned `proceed_with_changes`, with no identified blocker. Confirmed gaps
in full projection coverage and harness injection were addressed above; layer ordering was checked
in the test. Timing and visual evidence remain Codex's responsibility. The proposed hash/weather
correlation was not demonstrated: weather uses different coordinates, scale and offsets; no
speculative salt change or claim of mathematical independence was added. No second review loop ran.

RenderDoc 1.45 captured and replayed Vulkan frame 400 for all three final views; each owned native
process completed its smoke check. All final captures use 14 textures, with these local ignored
directories below `target/tooling/` (each includes `breach-thumbnail.png`):

| View | Before directory | Final after directory | After bytes / draws |
|---|---|---|---|
| Wide | `renderdoc-9s6ux3oh` | `renderdoc-zsfxn2v0` | 294,659,504 / 245 |
| Approach | `renderdoc-d6wimdt3` | `renderdoc-ufhcowwu` | 301,315,015 / 221 |
| Fracture | `renderdoc-2y5qdlq_` | `renderdoc-cc3glnp2` | 305,605,559 / 219 |

Direct native inspection shows the conspicuous tiled stone patches on the soil replaced by a less
regular distribution, retaining scale and the same broad weathering. Structural geometry and
composition are unchanged. The improvement is modest, not photorealism. The stepped bay, regular
rubble silhouettes, uniform concrete and absent vegetation/background landforms remain dominant
gaps against the requested ruined-factory reference. No generated concept image is used as evidence.

Earlier bay captures `renderdoc-kfp853yw`, `renderdoc-744rm3lc`, `renderdoc-lof17ev1` and intermediate
soil captures `renderdoc-0aoe1yyf`, `renderdoc-2ez9rzwf` were moved intact into
`target/tooling-archive-lHJXgO/`. The intermediates are not claimed as final executable evidence.
Nothing was deleted; the active 2 GiB cap remains unchanged and the local archive is not a backup.

The existing bounded owned-window orbit/zoom harness completed in
`target/soil-motion-dM7FxG/`: six seconds, 90 frames at 15 FPS, 1,440 x 900, network/PID isolated,
only its verified game window recorded. Unmodified frames 5, 25 and 75 were inspected; the texture
remains attached to the ground without a visible whole-pattern jump in those samples. This is a
spot check, not a claim of globally shimmer-free high-rate motion. The game and recorder both
exited successfully; the standalone hash was verified again afterwards. Recording timings are
excluded from the performance table.
