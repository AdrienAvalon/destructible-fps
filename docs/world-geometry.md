# Exact geometry in the shared World storage

This lot moves the fine-volume primitive into the real world's chunk/COW storage and adds a
bounded static-geometry transaction/checkpoint component. It advances DEST-01/02 and NET-01;
it does **not** activate fine geometry in the playable server, physical consumers, renderer or
live network protocol. The current demo still has metre-cell destruction and is not photorealistic.

## A compile-time migration boundary

`WorldStorage<G>` contains the existing chunk map, immutable chunk references, dense two-byte
Voxel payload, revision/occupancy bookkeeping, vacancy observation identity, tick and fingerprint.
`World` is an alias for its uniform specialization: a zero-sized `UniformGeometry` adds no
per-cell or per-chunk extension allocation. Existing uniform setters, enumerators, fingerprint
recomputation, physics, snapshots and server constructors keep that concrete specialization.

`RefinedWorld` uses the **same** storage with a private sparse map inside each occupied chunk.
Only mixed cells get a `RefinedVolume`; uniform cells keep their two-byte dense representation.
A refined slot's dense placeholder is AIR, but chunk/world occupancy counts its actual occupied
page. All generic helpers are metadata-only: observation, snapshot cloning, tick, fingerprint,
chunk coordinates and dense-payload statistics. No generic helper samples the dense voxel array.
Fine recomputation and enumeration explicitly account for the sparse extensions.

`GeometryCell` has private variants and canonical constructors. It exposes either the exact
uniform voxel or the exact volume, never a representative material. One-leaf volumes collapse
to uniform cells; all-air cells disappear. Non-air integrity zero remains occupied. Local air
inputs normalize to integrity zero; encoded air with nonzero integrity is rejected before that
normalization can hide malformed input. Full before-state equality compares canonical leaves,
not fingerprints alone.

Promotion from the uniform world checks resident bounds before cloning dense payloads. The
reverse conversion checks **all** chunks and refuses any refined cell. There is no `Deref`,
implicit downgrade or coarse `voxel` method on `RefinedWorld`. Compile-fail tests ensure a fine
world cannot enter the existing server constructor or its uniform voxel accessor.

This is a migration boundary, not a permanent second simulation. Before production adoption,
the existing authority must own this component and its next sequence alongside fine rigid bodies,
material damage, collisions, cover, structural support, asynchronous meshing and snapshot repair.
It must not expose `GeometryState::prepare` as a client command API.

## Transaction and memory invariants

A geometry transaction carries sequence, tick, whole-component pre/post fingerprints and sorted
unique exact before/after cells. Preparation and application validate before states and final
resource counts. Publication requires exclusive `&mut` ownership; it installs a complete COW map
candidate only after validation. All removals are staged before additions, so replacing cells at
the chunk/page limit cannot transiently allocate more live candidate geometry than the original
or final state. No-op cell changes retain chunk observations. Failed transactions preserve all
cells, observations, tick and sequence. Occupied and absent observations still reject ABA reuse.
Independent immutable readers retain the entire old state, never half of a multi-chunk operation.

| Accepted resource | Limit |
| --- | --- |
| Occupied dense chunks | 512 |
| Occupied metre cells | 262,144 |
| Refined cells / aggregate refined leaves | 4,096 / 131,072 |
| One page | existing 8,192 leaves |
| One transaction | 256 cells / 32,768 leaves counting both before and after |
| Encoded checkpoint / transaction section | 4 MiB / 512 KiB |

The bounds are simultaneous, not promises that every combination of maxima is accepted. A
checkpoint has `41 + 15 * occupied_cells + 11 * refined_pages + 8 * refined_leaves` bytes. This
inequality is checked during **every** transaction, not just when attempting a repair: an accepted
world remains serializable. For example a full 262,144-cell uniform world fits, but replacing four
of its cells by maximum-leaf pages exceeds four MiB by 85 bytes and must refuse atomically.

Maximum dense payload is four MiB; maximum accepted leaf payload is 1.75 MiB (131,072 × 14 bytes).
Sparse maps, chunk/world metadata, allocator overhead, source readers, COW candidates, encoded
buffers and decoded temporary pages are additional. A checkpoint sorts at most three MiB of
12-byte coordinates, not a vector of wide geometry values. The diagnostic `occupied_cells`
enumerator does materialize those values and should not become a hot-path API.
Vector reservation for encoded/decoded streams is fallible; standard HashMap/BTreeMap/Box/Arc
allocation keeps the engine's process OOM policy. This is not a global process-memory guarantee.
Future workers and retained snapshots still require a shared residency/backpressure budget.

Uniform fingerprints remain byte-for-byte compatible with the old World token. Refined tokens
bind the page fingerprint to world coordinates in a separate domain. These noncryptographic
divergence witnesses are **not** authentication, collision resistance or anti-cheat proof.

## Component encoding, not another gameplay protocol

`DFGC` v1 checkpoints encode header (magic/version, tick, next sequence, fingerprint, cell count)
and canonical coordinate/cell records. `DFGT` v1 transactions encode sequence, tick, pre/post
fingerprints, count and before/after records. Cells carry an explicit uniform/refined tag; refined
payloads contain a length-delimited, strict DFVL v2 page. Uniform pages may not be encoded using
the refined tag. Encoders always produce a unique stream, and decoders reject rather than repair
noncanonical data, ordering, unknown tags/versions, overflow, truncated/trailing bytes or budgets.

Checkpoint decode builds an unpublished bounded candidate; it checks sparse chunk count **before**
allocating another dense chunk. Transaction decode only validates bounded syntax: application
must still verify full before states, ordering boundary and final fingerprint. Zero sequencing,
replays, gaps, exhausted sequence increments and backwards ticks refuse without mutation.
The final valid sequence is `u64::MAX - 1`; its resulting high-water mark `u64::MAX` is an explicit
terminal, checkpointable/readable state. It cannot issue another transaction or wrap to zero.

These sections contain **only static geometry**, not bodies, players, ammo, inventories or an
authentication envelope. Checkpoint decode alone provides neither freshness nor permission to
install it; the future enclosing authenticated repair workflow must establish both. Existing
delta v6, snapshot v2, repair state machine, queues and 1,200-byte datagram limits are unchanged.
There is no live handler, network fragmentation or persistent save writer for these sections yet.
Do not send a section as a datagram or claim the staging tests are real multiplayer repair.

## Validation and next integration gate

Targeted regressions cover fine-only chunks, canonical promotion/demotion, exact volume holes,
negative/extreme coordinates, multi-chunk atomic failures, COW readers, no-op and ABA observations,
two component replicas with a deliberate missing transaction and checkpoint restoration, exact
resident/transaction/page caps and hostile oversized streams. Seeded byte mutations must either
refuse or re-encode identically. Compile-fail tests guard against implicit coarse consumers.

`cargo run --release --bin world-geometry-benchmark` measures the industrial world's static
component. It alternates prebuilt uniform/fine cells in fixed batches of 1, 64 and 256 for 100
iterations, so it neither fires into already empty cells nor shrinks the scene. It separates
prepare, authority apply, codec and two replica applications, with ten full checkpoint round trips
per batch. It deliberately excludes volume carving, actual transport, weapon rules, structural
work, physics, rendering and durable storage. This is not a 60-Hz combat acceptance test.
Cells are the first occupied coordinates in canonical order, including ground; these clustered,
simple bores do not represent dispersed worst-case edits or maximum-leaf pages.

The next gate is a shared world-space exact query/collision path, then fine topology/body mass
and inertia, and finally authoritative damage plus existing snapshot/delta transport migration
and asynchronous rendering. Until those agree, keep the compile-time boundary and do not pass
partial geometry through coarse material/occupancy proxies.

## Contradictory review disposition

The initial Claude analysis correctly emphasized refined-only occupancy, snapshotability and
multi-chunk publication; the implementation and tests explicitly cover them. Its calculation
equating dense resident payload with encoded checkpoint size is not the wire format (records
encode occupied cells only), but the independent requirement that every accepted state fit its
checkpoint is retained. Its hypothesized generic dense reads are absent in the resulting source;
uniform samplers stay on `World`, and metadata helpers share no dense occupancy scan. Canonical
leaf coverage/merging is already enforced by DFVL v2. Coordinate-bound fingerprints do not simply
cancel a material swap, but still have no cryptographic security claim. Review is advisory and
does not replace source verification or tests.

The final Claude review covered the complete new runtime/codec, complete then-current geometry
tests and the entire base World diff; it excluded the benchmark/docs and unchanged DFVL internals.
It found no source-proven blocking defect. Its all-air mixed-page concern was checked against the
DFVL parser: original air-integrity validation and canonical profile merging make that state
unconstructible. Additional tests now prove it, exercise 256/257 transaction records, backwards
prepare ticks and terminal sequence checkpoint round trips. The shared chunk revision is explicitly
local, wrapping and reset on reclamation; existing observation identity uses retained Arcs instead.
These post-review tests/documentation were validated locally, not sent through another review loop.

## Measured validation, 2026-09-06

Implementing tree based on `98fcd92`, Rust 1.97.1 release/fat LTO, Intel i7-13700H, Linux
7.2.2-1-cachyos. The final component benchmark ran after builds/tests finished, without another
intentional benchmark. Frequency policy stayed unchanged/unpinned. First iteration is not a
cache-flushed or cold-boot measurement. Binary SHA-256:
`0b0b2c46e3907de41a9799cd859fe00420623f16594aba92c6db2203a0f0772f`.

The fixture retains 96,321 occupied cells in 132 chunks. Milliseconds for the complete component
exchange (prepare + reference apply + encode/decode + two staging replica applies):

| Cells per change batch | First | p50 | p95 | p99 | Maximum | Section bytes |
| --- | --- | --- | --- | --- | --- | --- |
| 1 | 0.03487 | 0.01404 | 0.03220 | 0.03562 | 0.13611 | 126 |
| 64 | 0.06608 | 0.06279 | 0.09659 | 0.10646 | 0.10905 | 4,473 |
| 256 | 0.19694 | 0.19694 | 0.24989 | 0.26068 | 0.26393 | 17,721 |

The 256-cell case's reference apply alone is p50/p95/p99 0.05099/0.06174/0.06410 ms.
Its refined checkpoint is 1,457,912 bytes; encoding is p50/p95/p99
6.65444/6.83613/6.83613 ms, decoding 7.06510/7.17065/7.17065 ms (only ten checkpoint samples).
These multi-millisecond operations require scheduled work before network integration. The final
coarse-restored fingerprint is `b0907f09211a134f7584c7d7d8011c6d` in every batch and replica.
Child-process peak RSS is 11,968 KiB, including driver, coarse source, COW states and checkpoint
work, not a page-only allocation estimate. Allocation counts and whole-game retention were not
measured; these numbers are not complete multiplayer, physics or frame-time acceptance evidence.

Final local validation passed strict all-target Clippy, formatting, 444 ordinary tests in each
of debug/release (six GPU tests initially ignored in each), then those six actual Vulkan tests
explicitly in each profile and both compile-fail doctests. All 16 geometry regressions passed.
Targeted network, secure transport/authority/process and OIDC suites passed, as did 22 offline
tooling tests, Graphify freshness/doctor (zero dangling endpoints) and the four required legacy
benchmarks. The 500-event destruction run kept both replicas synchronized at
`e309309ed7c650fc3ac194e285e983b2`, 908 frames at MTU 1,200, 0.567 MiB payload. Legacy benchmark
timings are not used as a controlled before/after optimization claim.

Both real native Vulkan smokes exited cleanly. Industrial intact scene: RTX 4050 Laptop,
NVIDIA 610.57.04, 1440×900, 4×MSAA, 50,767 faces; CPU frame-window p50/p95/p99
16.659/16.879/33.345 ms (includes presentation pacing), GPU total 2.222/4.067/7.225 ms,
zero dropped timestamp samples. This five-second unchanged coarse scene is a non-regression
check, not fine rendering, sustained combat or photorealism evidence. No new package, profiler
privilege, system setting, public listener or infrastructure deployment was needed for this lot.

Raw local artifacts: `/tmp/fps-world-geometry-rfHeCE` (temporary, not durable saves or committed
assets). The engineering goal remains active; bodies/physical consumers and live transport are
not yet migrated.
