# Shared exact static collision and character movement

This increment connects the real `AuthoritativePlayer` and `ClientPrediction` to exact shared
world-space queries. Both the existing uniform `World` and typed `RefinedWorld` satisfy the sealed
`StaticGeometry` contract. Fine holes and remaining material affect the actual character solver,
not a separate demonstration controller or a coarse representative voxel.

The live multiplayer uniform-world path also uses this solver. Fine geometry remains deliberately
excluded from the full authority constructor, live damage/repair protocol and renderer. Fine body
mass, inertia, topology, static/dynamic contacts and structural support still need migration. The
standalone floating-point local-demo controller is not migrated by this lot. This is not fine-world
graphical multiplayer, a capsule controller, a completed crushing/death policy or photorealism.

The following [`mass-properties.md`](mass-properties.md) increment now shares exact rectangular
mass integrals with actual body promotion. Fine topology, body contacts, angular solver and live
body geometry remain unmigrated; character queries are not a substitute for those consumers.

## Exact coordinates and bounded traversal

`world/query.rs` represents nonempty half-open physical boxes in **1/256 micrometre** units.
Consequently every plane of the existing 256-unit/metre fine lattice is exact, including negative
page coordinates. Checked constructors reject world-domain overflow and empty/inverted bounds.
Positive-volume overlap excludes tangency. A continuous axis sweep visits the entire swept box,
not just its destination: a thin sheet cannot disappear between two sampled endpoints.

The leading face extends one scaled unit to include a solid exactly at the requested endpoint;
this query-only extension is clamped at the world boundary after validating the actual destination.
Transverse tangency permits sliding, inward motion from contact stops, and outward motion is free.
An existing positive overlap refuses rather than attempting an unproved depenetration. The nearest
signed gap is divided by 256, truncating the **offset** toward zero/start, not rounding an absolute
negative position. Whole-micrometre character state therefore remains less than one micrometre
outside a fractional contact plane, never inside it.

Candidate pages traverse a fixed coordinate order. Refined queries reuse the canonical slab/band/run
index and charge visited groups and leaves, including air. Callback early termination succeeds;
work exhaustion returns an error and discards the query's unpublished result. Traversal performs
no new collection allocation. Bounds and counters are independent of COW history and checkpoint
restoration; work counts are not elapsed-time guarantees.

| Query resource | Bound |
| --- | --- |
| Candidate cells in one query | 512, preflighted before traversal |
| Selectable cumulative query budget | 16,384 cells / 262,144 group-and-leaf visits |
| One axis displacement | 128 metres, with destination inside the world domain |
| Default complete character step | 128 cells / 65,536 visits shared by all its queries |
| Pending prediction history | 128 commands; each replayed step has the same default allowance |

Replay's aggregate upper bound is 16,384 cells and 8,388,608 visits, **not** one underfunded
262,144-visit allowance. A regression replays all 128 commands on an 8,192-leaf page, exceeds that
smaller aggregate allowance, and still exactly matches the server. This synchronous worst case
must be scheduled off render/receive paths before fine-world GUI activation. Bounded refusal is
not liveness: a permanently over-budget fine neighbourhood can repeatedly refuse a step. Fine
admission/residency must supply a tested scheduling/recovery policy before the live type boundary
is removed; an error must never be disguised as clear space or a permission to teleport.

## Atomic movement and recovery

Each step stages a complete player copy, including held input, input age, velocity and integration
remainders, then publishes only on success. Queries use the full 600 × 1,800 × 600 mm AABB, matching
construction exclusion bounds and removing the old one-millimetre head tolerance. A downward
one-micrometre sweep checks support every tick before jumping: removing a floor invalidates the
old grounded flag immediately. Fixed X/Y/Z axis order remains an approximation to a human body,
not arbitrary swept capsule/rotated-body dynamics. Contact at the exact destination now counts as
a collision and clears the axis velocity/remainder, even when the full requested distance is used.

Falling below the reset threshold checks the declared spawn before resetting. The authority
isolates a failed player so other players still simulate. Penetration, unsafe reset spawn and
out-of-world failures may recover only to a geometrically validated declared spawn slot not owned
by another session. Searches are bounded to 16 players and 16 slots. A recovered slot is reserved
before recovering the next player; if none is safe, a deferred counter is reported and later ticks
retry. Clearance covers static geometry, not dynamic-body or current-player overlap.

Recovery preserves the accepted input sequence but intentionally clears held movement/jump and
velocity. Acceptance is a high-water mark, not proof that every held command produced motion;
replaying the old jump after relocation would be unsafe. Ordinary fall reset uses the same policy.
There is no new death, damage, spectator or persistent recovery state machine. Budget failures do
not trigger relocation. The current live authority is uniform-only: its valid fixed-step speed/body
envelope fits the regular cell allowance, with regression coverage across page phases, velocity
extremes and signed integration remainders. That evidence does not establish fine-world liveness.

Prediction and reconciliation also publish atomically. A graphical geometry failure pauses local
movement instead of disconnecting. The GUI increments `next_input_sequence` only **after** successful
prediction and send; an early geometry return neither sends nor consumes it. Pending history is
retained for subsequent authoritative correction. Tests retry the same sequence after a failed
prediction and exercise multi-player deferred/simultaneous recovery without sequence reuse.

Control/player packet layouts are unchanged, but collision semantics changed: rebuild server and
clients together. Mixed old/new simulation compatibility is not established merely by equal wire
versions. Existing delta/snapshot framing, authentication and the 1,200-byte datagram cap are
unchanged. No fine geometry packet handler or newly exposed listener was added.

## Reproduction and evidence scope

```bash
cargo test --lib world::query::tests
cargo test --lib character::fine_tests
cargo test --lib network::player_recovery_tests
cargo run --release --bin character-benchmark -- --ticks 10000
cargo run --release --bin character-benchmark -- --ticks 10000 --fine
```

The character benchmark retains 16 players and a fixed 10,000-tick trajectory. Fine mode replaces
16 floor pages by full-occupancy alternating-material pages, each at 8,192 leaves, reaching the
existing 131,072-leaf component cap without raising it. The localized patch is not an all-players,
all-ticks worst-case scene. Identical occupied geometry must yield the same final movement checksum;
the driver also reports page/leaf work and maximum per-step visits. It excludes network, rendering,
structural work, dynamic bodies and full-history replay timing. It does not establish whole-server
tick performance or sustained photorealistic combat performance.

Regression coverage includes 2,000 seeded comparisons against an independent 8-cubed bitmap oracle,
negative fractional planes on all axes, tangency, world edges, intermediate thin obstacles,
cell/leaf refusals, an approximately 797 mm doorway that admits a 600 mm body and a 375 mm slit that
blocks it, terminal fall onto a 3.90625 mm sheet, a fractional ceiling, removed support, atomic
failures and replay after component checkpoint/transaction transfer. Those transfers are staging
codec tests, not actual fine-geometry network replication.

## External review disposition

The bounded Claude review received the full new static-query module and complete runtime consumer
diffs, but only summaries of tests; benchmark/docs and unchanged volume/storage internals were
excluded. Codex confirmed its budget-liveness warning as a **remaining fine-world integration gate**,
not grounds to teleport on exhaustion or to broaden the live authority type. GUI sequence ordering
was confirmed in the full function and its atomic retry test. Clearing held input and exact-endpoint
collision accounting are intentional semantics documented above. Callback stop returns `Ok` in
`visit_overlaps`; its private caller validates the allowance, so the remaining error is traversal
exhaustion. Additional post-review tests cover dense partial-scan refusal, the uniform query budget,
simultaneous distinct recovery slots and deferred recovery after reopening geometry. They are
locally validated, not represented as a second Claude review.

## Measured validation, 2026-09-06

Implementing tree based on `604d50b`, Rust 1.97.1 release/fat LTO, Intel i7-13700H, Linux
7.2.2-1-cachyos. Final character runs were sequential after builds, tests and graphical clients
finished; frequency policy was unchanged/unpinned. They are not cache-flushed cold starts, and
cold/warm frame budgets are not established. Character binary SHA-256:
`7a622b62b381ccc3d521941c04b2f45bd20e626cb09db1f2b0ab1c06a56f5cca`.

Microseconds per complete **16-character step**, not per whole multiplayer server tick:

| Static scene | p50 | p95 | p99 | Maximum | Total for 10,000 ticks |
| --- | --- | --- | --- | --- | --- |
| Uniform floor | 12.828 | 16.071 | 22.294 | 300.518 | 131.741 ms |
| Same occupancy, sixteen max-leaf pages | 13.269 | 21.407 | 180.657 | 272.477 | 187.242 ms |

Both runs finish at movement checksum `d4b3a3e56ec644039e584c3671603126` and visit 5,354,372
candidate cells over 160,000 player steps. Group/leaf visits are 5,354,372 versus 9,536,280;
maximum per-step visits are 60 versus 6,592, below unchanged allowances. Child-process peak RSS
is 12,056/11,972 KiB via `getrusage`, including launch/fixture overhead; this variation is not a
claim that fine storage consumes less memory. Allocation counts were not measured.

The retained prechange character executable measured p50/p95/p99 7.332/9.565/11.590 microseconds
on its uniform scene. The new solver is more expensive there; this is a correctness integration,
not a measured throughput optimization. That retained artifact predates the new query counters
and movement semantics and is not an independently attested commit-specific A/B build. Its
SHA-256 is `4673081ab86a86a48ac1686f7e0f4b4b601d5d8e54955b33c89dbcf1eb6b5235`.
A separate user-space `perf stat` run on fine mode measured 189.45 ms task-clock, 939,623,772
core cycles and 2,715,588,659 core instructions; the atom PMU counters were not counted, rather
than reported as zero. No system-wide profiling, capability grant or sysctl change was needed.

Final validation passed formatting, strict all-target Clippy and 462 ordinary tests in each of
debug/release, plus all six actual Vulkan tests in each profile and two compile-fail doctests.
Targeted network, secure transport/authority/process and OIDC suites passed. The 500-event
destruction check kept both replicas synchronized at `e309309ed7c650fc3ac194e285e983b2`, with
908 frames at MTU 1,200 and 0.567 MiB payload. Structural 100-iteration, physics 1,024-body/300-tick
and snapshot 20-iteration required benchmarks also passed. They retain their existing workloads
and are not evidence of fine body physics or controlled before/after performance improvements.

Real native range and industrial five-second smokes exited cleanly. The intact industrial scene
used RTX 4050 Laptop/NVIDIA 610.57.04, Vulkan, 1440×900, 4×MSAA and 50,767 faces. CPU frame-work
p50/p95/p99 was 3.476/16.745/16.867 ms (includes presentation pacing); GPU total was
1.808/2.231/2.521 ms, zero abandoned timestamp samples. This is a short unchanged coarse-scene
non-regression check, not fine graphics, sustained combat or photorealism evidence.

Two actual 1280×800 graphical clients also passed the industrial rifle smoke over loopback
QUIC/TLS. Both installed snapshots and four later deltas, presented their completed mesh work,
observed another player and acknowledged movement, reaching the same world fingerprint
`10943d403c41c0e67acdae3a0a76c4c7`. The shooter ended at ammo 29/87 and the observer at 30/90;
target timber was breached with its neighbour intact. The observer forced a second snapshot
and repaired a deliberately lost delta, RTT 16 ms/RTO 100 ms with one repair timing sample.
Both local transport queues dropped zero datagrams. These are loopback smoke observations,
not WAN latency, production loss-rate or cross-platform determinism measurements.

The server recorded no refused admission, handshake/admission failure, malformed datagram,
protocol rejection or gameplay queue drop. Its explicit 70-second harness deadline sent SIGINT;
it printed its clean STOP report and the timeout wrapper returned 124, not an unexplained server
crash. Both clients returned zero before that stop. Port 40001 was verified closed and all six
generated fixture files (including signing material and short-lived credentials) were removed.
No user/infrastructure secret was used or retained.

Raw artifacts are in `/tmp/fps-fine-collision-NgARon` (temporary evidence, not durable game saves).
The game goal remains active. This lot adds no package, public listener, host-policy change,
infrastructure deployment, external publication or claim of completed photorealism.
