# Experimental bounded fine-volume core

`volume.rs`, `volume/codec.rs` and `volume/surface.rs` are an isolated geometry foundation for
DEST-01/02, PHYS-01 and VIS-01. They do **not** change the playable world's one-metre cells,
collision, ray hits, structural graph, fragments, mesh workers or network protocols. The current
native demo remains visibly non-photorealistic. This module is not a claim of delivered localized
bullet holes, smooth fractures, realistic collapse or fine multiplayer destruction.

## Representation and invariants

A page has exact half-open integer coordinates `[0, 256)` on each axis. Interpreting a page as one
metre gives a finest nominal cell of 3.90625 mm. Uniform regions use a single eight-byte leaf, not
a dense 256³ allocation. A flat, Morton-ordered leaf stream partitions the complete page into
aligned dyadic cubes. Eight equal siblings recursively merge; material **and integrity** participate
in equality. Air always has zero integrity. Different edit orders producing the same field yield
the same canonical partition, bytes and deterministic 128-bit fingerprint. That fingerprint is a
corruption/divergence witness, not cryptographic authentication.

Leaves stay immutable behind `Arc` so candidate edits cannot change retained readers. Box edits
construct a separate stream and publish it only on success. Limits are explicit:

| Resource | Hard per-page bound |
| --- | --- |
| Finest subdivision depth | 8 |
| Canonical leaves | 8,192 |
| Edit node visits | 65,536 |
| Temporary canonical sibling carry | 56 additional leaves |
| Encoded page | 24,585 bytes |
| Extracted boundary squares | 32,768 |
| Surface visits | 262,144 |

Leaf-vector storage is at most 65,536 bytes per accepted page, excluding Arc/allocator metadata.
The edit scratch may retain at most 8,248 leaves (65,984 payload bytes); geometric growth starts
small instead of allocating the maximum for a uniform edit. The final immutable copy, old page,
retained readers and encoded buffers must be counted separately. Fallible vector reservation is
handled; the standard-library Arc allocation still follows the process allocator's OOM policy.
These are local data/work bounds, not a global process-memory guarantee or an allocation profiler.

Material volumes sum exactly to 256³ units. The density-weighted mass numerator retains the
fraction in kg / 256³ for a metre page. It must be summed across a fragment before rounding: a
finest wood cell is nonzero mass, about 38.7 mg. The legacy body format stores whole kilograms and
cannot consume this core without an explicit mass/inertia unit migration.

## Boundary surfaces

The extractor emits a square only from an occupied leaf toward known empty space. It recursively
subdivides a face when the opposing region has finer occupancy, including across all six page
boundaries. Materials sharing an occupied interface do not emit internal sheets. Exact dyadic
planes preserve coverage between coarse and fine pages; tests compare rasterized square coverage
against an independent dense occupancy oracle and check a one-finest-cell neighbor hole in every
signed direction. All six neighbor volumes are mandatory; an explicit air volume means known
empty space, never an absent/unloaded chunk.

These boundary squares can still contain T-junctions. They are not yet a conforming triangle
manifold, a smooth photoreal fracture surface, or a replacement collision mesh. Output and visit
budget failures discard the whole candidate; they do not return a partial/invisible wall.

A full depth-four solid/air checkerboard neighbor tests refusal *inside* recursive subdivision in
all six directions, followed by successful regeneration with unchanged source/neighbor bytes.

## Standalone codec, not a network upgrade

The `DFVL` v1 stream has a nine-byte magic/version/count header and three bytes per leaf:
depth, material and integrity. Morton offsets are implicit in the complete aligned partition.
Decode rejects oversized input before allocating, as well as bad counts/depth/material, nonzero
air integrity, misalignment, incomplete/overlong partitions, reducible siblings and trailing bytes.
Accepted bytes are already canonical; decode does not silently normalize malformed input.

There is no world location, transaction number, pre/post hash, authentication or fragmentation in
this stream. Do not put it directly into a gameplay datagram: the existing 1,200-byte MTU ceiling,
delta v6 and snapshot v2 are unchanged. World transactions must eventually validate/install all
affected pages and significant bodies atomically under global snapshot/retention/egress budgets.

## Known adverse case and promotion gates

An off-grid cut `[1, 2, 3)..[27, 38, 49)` into a uniform solid already exceeds the 8,192-leaf bound.
Its many finest-resolution surfaces are expensive even though its volume is modest. A regression
and benchmark intentionally preserve that refusal; the cap was not raised to make the case pass.
This is evidence that this representation alone is **not ready for arbitrary gameplay damage**.
Before promotion, compare bounded local bricks, coarser damage precision, or analytic cut geometry
with equivalent occupancy/cover guarantees. A server must not hide overload by making a wall
silently invulnerable or by accepting a visual-only hole.

Required integration work remains:

1. Choose a measured geometry representation that handles sustained arbitrary cuts, with explicit
   per-world/page-replacement memory and work budgets. Keep the current uniform world fast path.
2. Add refined state to World COW observations, fingerprints and versioned atomic delta/snapshot
   repair. Validate old/new pages before swapping; bound simultaneous reader/snapshot retention.
3. Share a single deterministic coordinate transform and refined occupancy with projectile rays,
   character/body collision and cover. Use separate macro/fine ray budgets and an independent
   grazing/boundary oracle. Integer energy needs sufficient precision for tiny material chords.
4. Build partial-face connectivity and multiple components within a page; migrate significant
   body mass, inertia and geometry. A coarse occupied neighbor is not evidence of a fine support.
5. Integrate budgeted asynchronous meshing, conforming/smooth boundaries and neighbor invalidation;
   inspect native moving-camera holes, cuts, silhouettes and collision correspondence.
6. Prove two-client damage/late-join/repair and sustained physics/network/memory/GPU behavior before
   enabling the representation in the industrial scene. Preserve the full game acceptance contract.

Sparse uniform tiles and local refinement are established principles; the
[OpenVDB overview](https://www.openvdb.org/documentation/doxygen/overview.html) informs this design
direction, not an assertion that this custom leaf stream implements OpenVDB or is the fastest choice.

## Reproducible checks

```bash
cargo test --lib volume::
cargo run --release --bin volume-benchmark -- --iterations 500
```

The benchmark resets the same solid before each sample. It reports first/p50/p95/p99/max timings
for edit, boundary extraction and codec round trip, plus exact leaves, bytes, geometry, visits and
vector capacities. The off-grid case measures an expected atomic refusal, not successful damage.
It does not measure a whole server tick, network, GPU, retained world memory, allocator counts or
combat latency; do not promote its microseconds as a game performance claim.

## Independent review disposition

Claude's bounded no-tool analysis and final review informed the local resource bounds and added
coverage. Codex checked the findings against `material.rs`, rather than accepting hypotheses about
code absent from the review context:

- `Voxel::is_solid()` depends on material, not integrity; non-air strength zero remains occupied.
  A new exhaustive test tries all 256 material identifiers with zero, one and maximum integrity,
  requiring identical accepted state from decode and local edits.
- `Voxel::from_wire()` already normalizes air. Comparing that returned voxel with its canonical
  version would incorrectly accept nonzero air integrity from the wire. The parser deliberately
  retains its check against the original byte.
- Material identifiers are currently exactly 0..7. An exhaustive material-to-accounting-slot
  match now additionally forces a compile-time decision when adding an enum variant, rather than
  letting a new discriminant silently index beyond the eight-slot table.
- Recursive surface-budget failures gained a complete, valid checkerboard fixture. A suggested
  stream of only 8,192 depth-eight leaves cannot cover a complete page and would test parser
  rejection instead of surface recursion, so it was not used.
- Arc OOM limitations, non-conforming surface topology and off-grid complexity remain explicit
  promotion blockers, not claimed fixes or hidden gameplay fallbacks.

The review was advisory, not proof of test execution; Codex runs the actual full validation matrix.
Its submitted code preceded these small hardening/tests; there was no repeated agent-review loop.

## Local validation, 2026-09-06

Measured in the implementing working tree based on `68ab538`, with Rust 1.97.1, release fat LTO,
Linux 7.2.2-1-cachyos, Intel i7-13700H and the existing desktop power/clock configuration (not pinned).
No other benchmark or compiler was intentionally run concurrently. Each case has 500 reset samples;
"first" is the first iteration, not a cold-boot/cache-flushed measurement.

| Isolated case | First ms | p50 ms | p95 ms | p99 ms | Max ms | Leaves / bytes / quads |
| --- | --- | --- | --- | --- | --- | --- |
| One finest empty cell | 0.01924 | 0.01405 | 0.01495 | 0.01572 | 0.01924 | 57 / 180 / 54 |
| 2×2×32-unit bore | 0.03300 | 0.03091 | 0.03276 | 0.03392 | 0.03931 | 127 / 390 / 112 |
| Aligned 128³-unit breach | 0.00661 | 0.00536 | 0.00557 | 0.00657 | 0.00978 | 36 / 117 / 76 |
| Off-grid cut, refused | 0.09747 | 0.08022 | 0.08228 | 0.08647 | 0.09747 | no accepted output |

Edit visits for the accepted cases are 65 / 145 / 41; surface visits are 472 / 954 / 184. Leaf
scratch capacities are 512 / 1,024 / 512 bytes; surface capacities are 768 / 1,536 / 1,536 bytes.
A separate completed run measured 12,544 KiB child-process peak RSS through Python's standard
`resource.getrusage(RUSAGE_CHILDREN)` after directly launching the benchmark. This includes process
and launch costs, not just leaf payload; it is not full-game memory, peak transactional allocator
tracking, or proof under near-cap world retention. The unavailable `/usr/bin/time` was not treated
as a successful measurement and did not require a package installation.

Final source passed strict all-target Clippy and formatting; all-target tests passed 418 ordinary
tests in each of debug/release. All six normally ignored real-Vulkan checks then passed explicitly
in each profile. Fifteen new volume tests and one benchmark argument test account for the new
coverage. The 22 offline tooling tests, targeted network/secure/OIDC checks and the four required
destruction (500 events), structure (100 iterations), physics (1,024 bodies/300 ticks), snapshot
(20 iterations) benchmarks also passed. Graphify's updated local code index reported no stale
files or dangling endpoints; it is not a substitute for these source/test checks.

Both five-second native range and industrial-breach smokes finished successfully on the RTX 4050
Laptop, NVIDIA 610.57.04, Vulkan, 1440×900, 4× MSAA. The industrial smoke retained 96,113 solids,
132 chunks and 50,723 mesh faces after the existing coarse breach. Its measured frame CPU
p50/p95/p99 was 16.655/16.843/17.061 ms; total GPU was 2.712/4.527/7.859 ms with no discarded
timestamp samples. Presentation pacing is included in the current CPU window, and there is no
sustained multiplayer combat or fine-volume rendering in this smoke. These are non-regression
checks, not photorealism, a 32-player tick budget or a cross-OS performance certification.
