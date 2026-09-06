# Bounded fine volumes: rectangular runs and exact physical ray chords

The experimental fine-volume core advances DEST-01/02, PHYS-01 and VIS-01 without changing the
playable world's one-metre cells or its current authority/physics/network contracts. It now
provides exact material occupancy, box edits, boundary rectangles and physical segment
intersections from the **same** immutable geometry. It now has a typed specialization of the shared
World chunk storage and a static-geometry transaction/checkpoint component; see
[`world-geometry.md`](world-geometry.md). It is not yet enabled in the playable world's rifle
transaction, character/rigid-body collision, structural analysis or asynchronous render workers.
The native demo still does not meet the photoreal reference.

## Why the octree was replaced

The initial Morton cube implementation at `8fd7231` refused the small off-grid box
`[1,2,3)..[27,38,49)` under its 8,192-leaf bound. The replacement retains the same 256-unit page
and exact materials/integrity but uses rectangular runs. It needs seven leaves for that complete
cut, without coarsening geometry, hiding a refusal or increasing the leaf/work limits.

Each immutable flat leaf stores six u16 bounds and a two-byte Voxel: fourteen bytes. Leaves form
a complete nested partition: Z slabs, Y bands within a slab, X runs within a band. Canonicalization
runs **inside-out**: merge equal X materials/integrity, then identical X profiles across adjacent
Y bands, then identical XY profiles across adjacent Z slabs. Fixed axis order makes the result
unique for a given field; it does not guarantee orientation-independent cost or minimal box count.
Air integrity is always zero. Other materials remain occupied even at integrity zero, matching
the existing Voxel contract.

This is a custom discrete material field, not a signed-distance solver or implementation of
[Houston, Wiebe and Batty's RLE sparse level sets](https://benhouston3d.com/siggraph/2004-1.html).
Their use of runs and efficient indexed access informs the direction; their complexity/performance
claims are not transferred to this implementation.

## Atomic candidates and resource contracts

Box edits stream inner-normalized profiles through separate page/slab/band candidate buffers.
When a completed slab matches the last candidate slab, only that candidate's upper Z bounds
extend. The original page and all retained readers stay immutable. A failed edit returns an
error with no partially installed geometry.

| Resource | Hard bound |
| --- | --- |
| Finest coordinate precision | 256 units/page axis, nominal 3.90625 mm in a metre |
| Accepted page leaves | 8,192 |
| Edit work steps | 65,536 |
| Candidate buffers | page 8,192 + slab 8,192 + band min(256, requested leaf cap) |
| Standalone encoded page | 65,545 bytes |
| Boundary rectangles / traversal steps | 32,768 / 262,144 |
| Material ray chords / traversal steps | 768 / 32,768 |

Each source step, profile-comparison leaf and merge/copy leaf is charged to the edit budget.
Already canonical slabs entirely outside the cut are copied directly, charging their leaf count
in one checked batch rather than rebuilding their bands/runs. Frontier profiles still coalesce;
this reduces work without pretending the flat candidate copy is independent of page size.
Binary group/skip searches have separately bounded logarithmic cost. Already-normalized band/slab
prefixes cannot shrink through an unrelated later profile: equality is checked before appending,
so there is no unlimited deferred prefix or uncounted speculative tree expansion.

Maximum accepted leaf payload rises from 65,536 to 114,688 bytes because each leaf has explicit
rectangular bounds. Combined edit vector payload capacity is at most 232,960 bytes. Old source
readers, final Arc copy, encoded buffers and allocator metadata are additional: these are per-page
contracts, **not** a total process-memory guarantee. Vector reservation is fallible; standard Arc
allocation still follows the process allocator's OOM policy. Profile scratch starts small and
grows within bounds. The typed World component now bounds accepted aggregate geometry and
transactions as documented in `world-geometry.md`; retained readers, workers and whole-process
residency still need a shared runtime budget before gameplay integration.

Material volumes sum exactly to 256³. Density-weighted mass stays as an exact kg / 256³ numerator
for a metre page. A finest wood cell is about 38.7 mg, not zero mass. The current whole-kilogram
body/inertia formats must migrate before they can represent these fragments faithfully.

## Queries, visible boundaries and real air gaps

Point lookup selects canonical Z/Y groups and then the containing X interval. Rectangular queries
skip nonintersecting groups by binary search; surface extraction does not rescan every page leaf
for every face. It emits only intersections between this page's solid face and a neighboring air
run, on the exact shared integer plane. Different occupied materials never emit internal sheets.
All six known neighbor pages are mandatory; air means known empty space, not an unloaded chunk.

The result is a set of rectangles, not necessarily squares. Exact coverage does not eliminate
T-junctions or constitute a conforming, smooth photoreal triangle manifold. Collision uses the
volumetric meaning, not an unvalidated cosmetic mesh.

`overlaps_solid_bounded` is the interval-skipping, early-exit box query with a caller-selected
work cap (maximum three steps per possible page leaf). Exhaustion is an error, never a fake clear
path or solid fallback. The original `overlaps_solid` convenience scan stays explicitly marked
as unsuitable for hot physics loops; it is not the budgeted consumer API.

`trace_segment` adds a physical geometry query: endpoints are integer micrometres local to the
metre page, bounded to +/-128 m. The planes remain exact rational micrometres
(`local_unit * 1_000_000 / 256`), rather than rounding both geometry and endpoints to a coarser
grid. Clipping and parameter ordering use integer/i128 arithmetic. It returns ordered,
positive-length material chords, including exact entry/exit parameters and true air gaps.
A tangent or endpoint-only touch has no thickness. A parallel ray on a minimum face belongs to
the box; one on its maximum face does not. Sorting has no allocation and an independent
768-hit cap; a page-crossing line can cross at most 766 finest-grid cell intervals.

The query is deliberately not a weapon policy, energy solver, lag compensation or damage
transaction. It must eventually be consumed by the same authoritative fine state as character
collision, cover and structural support, not substituted for the current coarse ray in isolation.

## Strict standalone DFVL v2

A nine-byte header carries magic/version/leaf count. Each eight-byte record contains three
little-endian upper bounds followed by material/integrity. Lower bounds follow from the complete
nested partition. Decode checks page completion, positive extents, consistent band/slab ends,
canonical X/Y/Z profiles, materials, original air-integrity byte, exact length and trailing data
before constructing accepted state. It does not normalize malformed input. The material-accounting
match is exhaustive so new enum variants cannot silently index past the table.

DFVL v1 is explicitly rejected. It was the preceding isolated experiment, not the world's
snapshot schema. The typed World component now encloses DFVL v2 in static-geometry sections;
the existing live gameplay snapshots do not consume those sections yet.
No user backups or private saves were scanned. The larger v2 per-record/maximum byte contract is
intentional and documented; existing world delta v6, snapshot v2, four-MiB snapshot guard and
1,200-byte gameplay MTU are unchanged. Neither DFVL format contains authority, world coordinates,
sequencing, before/after fingerprints or framing. Do not send this stream directly as a datagram.
The new deterministic fingerprint domain identifies the new canonical representation; it remains
a divergence witness, not authentication.

## Comparison and adverse cases

Both versions were tested using identical fixed geometry sequences and their original unchanged
8,192-leaf/65,536-edit-work caps. The old release library was retained before rebuilding. The stress
driver was compiled against it and against the new library using Rust 1.97.1, optimization level 3,
fat LTO and panic abort. Baseline identities:

- old simple benchmark SHA-256: `0368430fe2ec37a8dd37e0d87565fc86dd719eaefdf081c3b3d099086754f457`;
- old release rlib SHA-256: `60b4ef45f2ecbc024d0870fe3447550718116bfc511ce8aebddd3168a30aa57f`;
- common stress source SHA-256: `f4d88eeef7f2c6bec99524934b323c5db91e9683b19ac5bdc22ad3ee0bb254cd`.

| Identical case | Cube octree | Rectangular runs |
| --- | --- | --- |
| One finest empty cell | 57 leaves / 180 bytes | 7 leaves / 65 bytes |
| Thin 2×2×32-unit bore | 127 leaves / 390 bytes | 6 leaves / 57 bytes |
| Aligned 128³-unit breach | 36 leaves / 117 bytes | 6 leaves / 57 bytes |
| Off-grid box above | LeafBudget refusal | complete, 7 leaves / 65 bytes |
| Sphere r16, all six axis orders | complete, 6,462 leaves | complete, 1,369 leaves |
| Sphere r32, all six axis orders | incomplete | complete, 5,757 leaves |
| Sphere r64 | incomplete | incomplete, 1,345–4,590 of 12,853 row edits accepted |
| Diagonal slab | incomplete | incomplete, 4,648–5,363 of 16,384 row edits accepted |
| Seeded 1,000 small cubes, continuing after refusals | 36 accepted, 964 refused | 402–418 accepted, remainder refused |

Sphere radius is in finest local units: r32 is 12.5 cm and r64 is 25 cm. Complete sphere results
are symmetric; the order of intermediate carving still changes transient work/fragmentation.
For incomplete cases, the accepted prefix is **not** the intended final shape. Failed attempts
leave their source unchanged; the stress harness reports incomplete output rather than hiding it.
The six permutations exercise both orientation and traversal-order effects, not a claim of an
axis-agnostic encoding.

Measured sphere-r32 construction used 3,209 sequential row edits and roughly 0.225–0.364 seconds
in the initial local comparison. That is not a 60-Hz explosion operation. Large curved cuts,
diagonals, high accumulated damage and bounded atomic region staging remain promotion blockers.
The representation is substantially more useful for fine thin/offset damage, not a universal
solution to arbitrary geometry. A hybrid brick/analytic boundary representation or an explicitly
validated physical precision policy may still be needed; a hidden invulnerable fallback is not
acceptable.

## Reproduce and continue toward the playable world

```bash
cargo test --lib volume::
cargo run --release --bin volume-benchmark -- --iterations 500
cargo run --release --bin volume-stress-benchmark
```

The simple benchmark resets its source each iteration and reports first/p50/p95/p99/max plus
work/geometry/capacity counters. The old off-grid refusal is now a required successful case.
The stress benchmark has fixed bounded inputs, no arguments, reports every incomplete case,
checks exact carved volume for disjoint rows, checks codec round trips and verifies source
fingerprints on refusals. Its timings are one construction per shape/orientation, not statistical
whole-game latency. Neither program establishes global allocation counts, world retention,
bandwidth, GPU budgets, cross-OS determinism or photorealism.

Next gates remain explicit:

1. Choose/validate large-cut geometry and atomic regional staging under global memory/work limits;
   do not implement explosions as thousands of unbudgeted synchronous micro-edits.
2. World COW observations, fingerprints and static-geometry transactions/checkpoints now have a
   typed implementation (`world-geometry.md`). Finish the enclosing world/body transaction,
   actual snapshot repair and late-join migration, retaining the uniform fast path.
3. Use a shared physical transform/occupancy for bullets, character and body collision, with
   precise integer penetration work and server-only damage decisions. The new segment primitive
   supplies one piece, not the completed integration.
4. Implement partial-face support connectivity and multiple components per page; migrate body
   mass/inertia, meaningful fragment geometry and progressive failure.
5. Integrate budgeted worker meshes and conforming/smooth boundaries; inspect native moving-camera
   holes, collision correspondence and layered fractures in the industrial scene.
6. Prove two-client damage/build/repair and sustained physics/network/GPU behavior before enabling
   the field in ordinary gameplay. The full game acceptance contract is unchanged.

## Independent review and additional guards

Claude's bounded no-tool analysis led to the common old/new stress driver and six-axis curved,
diagonal and accumulated-damage evidence. Its initial estimate that radius 32 would already fail
was not borne out; radius 64 and heavy accumulation do fail, so the underlying concern remains.
Normalization order and candidate buffering were confirmed in code, not changed to follow an
ambiguous description of outer traversal order.

The final external review covered complete current runtime sources, full segment tests and selected
codec tests, not the entire game or every test/benchmark source. Codex subsequently added max-size
ray and six-neighbor surface tests, explicit Voxel-contract tests, the bounded overlap query and
direct copies of unaffected slabs. There was no repeated reviewer loop. Those final edits receive
the same complete local validation; the external review is not represented as covering a later diff.

The initial recorded stress refusals were leaf-cap exhaustion, not evidence of the hypothesized
work-budget refusal. Unaffected-slab copying addresses measured full-page processing cost without
raising limits. A worst-case 8,192-leaf all-material page now exercises diagonal and 128-run axial
segment queries; an 8,192-leaf solid/air checkerboard with six equally detailed neighbors exercises
exact acceptance of 24,576 surface rectangles and refusal one below that output cap.

## Core-volume milestone validation, 2026-09-06 (before typed World integration)

The implementing working tree is based on `8fd7231`. Hardware/toolchain: Intel i7-13700H,
RTX 4050 Laptop, NVIDIA 610.57.04, Linux 7.2.2-1-cachyos, Rust 1.97.1, release fat LTO. Desktop
power/frequency policy was unchanged and not pinned. The benchmarks ran sequentially after
compilation, without another intentionally concurrent benchmark. First iteration is not a
cold-boot/cache-flushed measurement.

Final 500-reset-sample microbenchmarks (edit + surface + codec, milliseconds):

| Case | First | p50 | p95 | p99 | Maximum |
| --- | --- | --- | --- | --- | --- |
| Finest cell | 0.00488 | 0.00131 | 0.00135 | 0.00137 | 0.00488 |
| Thin bore | 0.00203 | 0.00110 | 0.00114 | 0.00120 | 0.00283 |
| Aligned breach | 0.00138 | 0.00111 | 0.00114 | 0.00115 | 0.00138 |
| Formerly refused off-grid box | 0.00199 | 0.00130 | 0.00136 | 0.00140 | 0.05900 |

The maxima are retained, including scheduling/allocation jitter; these are not game FPS or tick
measurements. Unaffected-slab copying reduced the complete r32 construction range to
90.722–326.908 ms across the six orders, with identical 5,757-leaf geometry. The seeded cube
sequence now takes 23.390–24.767 ms total, preserving the same 402–418 accepted attempts and
explicit remaining refusals. Its maximum single edit is 0.086–0.110 ms across the six runs.

A separate 500-iteration child-process resource check reports 12,624 KiB peak RSS, including
launch/process overhead, not just page payload. User-space `perf stat` on 10,000 reset iterations
reports 49.77 ms task-clock, 240,410,413 cpu_core cycles and 888,149,495 cpu_core instructions.
cpu_atom counters were not counted in that run; this is not coverage of both CPU core types,
kernel work or the rest of the host. No profiler rights, sysctls or hardware settings changed.

Final source passed `cargo fmt --check`, strict all-target Clippy, 428 ordinary tests in each of
debug/release, then all six normally ignored real-Vulkan tests explicitly in each profile. The
24 volume/segment tests include independent non-dyadic occupancy/surface/ray oracles and max-size
budgets. Targeted network/secure/OIDC checks, 22 offline tooling tests, all four required legacy
benchmarks and both volume benchmarks completed; stress refusals remain reported as above, not
counted as successful shapes. Updated Graphify reported no stale files or dangling endpoints.

Both five-second native range and industrial-breach smokes exited successfully. Industrial:
1440×900, 4× MSAA, 96,113 coarse solids, 132 chunks, 50,723 faces, CPU frame-window
p50/p95/p99 16.648/16.879/33.279 ms, GPU total 2.787/4.542/4.602 ms, zero discarded timestamp
samples. CPU includes presentation pacing. There is still no fine-volume rendering or sustained
multiplayer combat in this smoke; it is a non-regression check, not the final visual/performance gate.
