# Exact world material traces and penetration inspection

Status: **shared static query and native read-only probe, not completed fine weapon gameplay**.
The existing coarse rifle retains its authoritative damage/cadence/ammunition/network behavior.
No partial cell is promoted to that authority, erased wholesale or approximated by a solid proxy.

`world::query::ray::trace_materials` reads the same sealed `StaticGeometry` as fine character
collision and mesh extraction. `FixedRay` supplies one canonical integer-micrometre segment with
an inward-rounded endpoint, at most 120m long. Both broad and narrow phases use exactly this same
origin and delta. The coarse cell iterator is only a conservative candidate set; zero-distance
contacts are retained, because a real positive chord can round to zero micrometres. Candidate
positions are sorted/deduplicated, then each page traces the complete original segment, not a
subsegment clipped to rounded coarse entry/exit distances.

Uniform cells use an allocation-free analytic page path; refined cells use the existing exact
rectangular material traversal. Output includes world cell, actual leaf, exact rational entry/exit,
source geometry fingerprint and work counters. Chords sort front-to-back without overlap and retain
real air gaps. Tangencies and endpoint-only contacts have no positive material thickness; parallel
minimum faces belong to a volume, maximum faces do not. This is a **point-segment** contract, not
finite-diameter projectile cover. A regression explicitly preserves the difference from the coarse
rifle's conservative seam protection; this probe must not be silently substituted into that path.

## Aggregate bounds

| Resource per complete trace | Maximum |
| --- | ---: |
| Broad-phase entries and unique queried metre cells | 512 |
| Aggregate leaf traversal visits | 262,144 |
| Retained output chords | 4,096 |
| Temporary per-page chords / visits | 768 / 32,768 |

Custom limits only lower these caps. Uniform reads, including air, cost one leaf visit; refined
pages share the remaining global allowance. Sort comparison work is separately bounded by cell
and chord counts. Output vectors use bounded fallible reservation; existing broad-phase allocation
retains the engine allocator's normal OOM policy. This is not an allocation-count or whole-process
memory guarantee. A complete trace can refuse because of distant geometry even if a later weapon
would stop earlier; near-to-far short-circuit policy remains future work.

Every refusal returns an error and no trace/probe prefix. The inspector exits visibly on a failed
current request. There is no invented clear path, fake infinite resistance, partial damage or
automatic coarse fallback. A future enclosing weapon transaction must reject or reschedule a
failed candidate without spending ammunition or publishing any geometry. A geometry fingerprint
binds only the sampled static field, not body pose, weapon state, simulation tick or authorization.

## Fictional penetration-work probe

`ballistics::fine::probe_static_rifle` evaluates the existing fictional 750-unit rifle energy and
material resistance against those static chords. It merges exactly adjacent intervals only when
both material and integrity match, including across pages. It rounds once per merged run, not per
arbitrary leaf, so off-ray leaf partition changes cannot alter resistance. Real air gaps are never
merged. Integral work uses 1e-6 game units and a documented floor of one integrity unit for still
occupied zero-integrity solids.

Entry/exit ordering is exact integer/rational geometry. For work, the Euclidean segment length is
bounded outward to a whole micrometre (less than 1um extra), each merged chord length rounds upward
to 1/256um, and work rounds upward to one micro-unit. Intermediate u128 bounds follow from the
120m ray limit; there is no floating-point authority arithmetic. This is a discrete gameplay
resistance model, not calibrated ballistics, kinetic-energy simulation or a real bullet calibre.
The reported stopping **interval** is not an exact stop point and does not authorize a whole-leaf cut.

The native inspector evaluates centre/rim probes on each fully installed stage and checks a tiny
physical overlap inside the same material. P evaluates the current camera aim only when streaming
is idle. Quantizing the visual camera uses floating point locally, then submits the bounded integer
query. Probe work is occasional inspection work, not a per-frame or network-receive task. Probes
do not supply dynamic-body cover, damage, muzzle flight, recoil, ammunition or server authority.

## Validation and next integration

Regression fixtures cover six directions across signed chunk seams, a 7.8125mm bore and its
fractional edge, coplanar/tangent/endpoint/inside cases, sub-micrometre grazing chords, analytic
uniform-versus-page equivalence, 500 fixed-seed world rays against an independent exhaustive slab
oracle, fine layers with known thickness/work, oblique material cost, partition invariance,
zero-integrity occupancy, far coordinates, checkpoint equality and matching budget refusal.
Sixteen 256-layer pages accept exactly 4096 chords; the seventeenth refuses, and lowering the
aggregate work by one refuses without changing source state. Existing page-level dense midpoint
oracles continue to exercise irregular fine partitions.

Claude's resumed analysis suggested an endpoint mismatch, but source inspection proves both phases
consume `ray.origin + ray.delta`; no independently reconstructed ideal endpoint exists. Its proposed
infinite-resistance fallback conflicts with atomic refusal and was not adopted. The useful
partition/fast-path/checkpoint/point-versus-projectile tests are explicit. This probe is not a second
game authority and must not grow into one.

Per the updated [roadmap](production-roadmap.md), **renderer and photoreal industrial map quality
take priority after this in-flight increment**. Future fine weapon work must consume these same
chords with bounded finite-footprint localized edits, aggregate regional staging, body cover,
atomic weapon/world promotion and repair/late-join tests, rather than merely erase intersected leaves.

## Validation receipt, 2026-09-06

Source baseline `fb77399` plus this increment. Linux 7.2.2-1-cachyos, Rust 1.97.1, i7-13700H,
RTX 4050 Laptop 6141 MiB/NVIDIA 610.57.04, unchanged power/frequency policy. Formatting and strict
all-target Clippy pass. **513 ordinary tests pass in each debug/release profile**, followed by all
six real-Vulkan tests explicitly in each profile, two compile-fail doctests and the targeted network,
secure transport/authority/process and OIDC suites. Required destruction-500, structural-100,
physics-1024/300 and snapshot-20 benchmarks pass. These unchanged coarse gameplay benchmarks do
not establish fine-weapon, body or multiplayer support.

Real release smoke passes for the playable range (5s), exact inspector (8s) and industrial inspector
(12s). Both inspectors verify all eight centre/rim material and overlap probes on their four
displayed stages. The industrial bore correctly reaches the opposite hall wall at z=-15 while its
rim still hits the front wall at z=15; the isolated open bore finds no further static material.
The four industrial paired-probe/overlap/logging observations take 0.03958–0.04522 ms CPU. Four
observations are not a percentile, sustained throughput, worst-case latency or allocation count.

At native 1440x900/4xMSAA, industrial GPU total p50/p95/p99 is 1.988/2.684/3.496 ms (2204 samples,
zero discarded); CPU frame time including presentation is 3.064/16.523/16.797 ms. Presented stages:
`[36,36,36,1951]`. Exact inspection GPU p99 is 0.981 ms and CPU-with-presentation p99 is 16.948 ms,
with stages `[37,36,36,418]`, zero discarded GPU samples. Scheduling, clocks and thermals were not
controlled: no speedup claim or full-game frame-budget acceptance follows from these short views.

The resumed external analysis produced an advisory report. The final full-diff review failed with
CLI code 1 after 101s and no valid report; it is **missing independent final review**, not approval.
The separate Claude-helper repair passed its own review, 29 tests and final live schema-validated
smoke; that does not substitute for this game's review. Codex completed source review and all local
checks above. Neither historical failure is retroactively assigned a cause without evidence.

Temporary logs and the exact validation harness: `/tmp/fps-fine-ballistics-3SXANY/`.
Final native executable SHA-256:
`e8e657c6840c3284497e42102bd9dcb3428c8f331889baa438c604a24904c1dd`.
No new capture was produced or deleted in this increment. The previously captured industrial
thumbnail was inspected again to set the next visual priorities; its geometry is unchanged and
plainly not the photoreal reference. The full game goal remains active.
