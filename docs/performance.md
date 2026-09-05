# Performance evidence

Performance observations are point-in-time results tied to a command, scene, build, resolution, and
machine. They are not portable guarantees or substitutes for the later platform matrix.

## 2026-09-05 — Stage 1 telemetry baseline

Command:

```bash
cargo run --release --bin playable-demo -- --showcase --smoke-seconds 8
```

Environment and scene:

- NVIDIA GeForce RTX 4050 Laptop GPU, proprietary NVIDIA driver, Vulkan backend;
- 1,440×900 client viewport;
- 71,304 solid voxels, 128 allocated/rendered chunks, 43,912 exposed faces;
- 2,048² directional comparison shadow map;
- release profile with fat LTO and one code-generation unit;
- bounded window containing the most recent 4,096 completed samples.

| Measurement | p50 | p95 | p99 | max |
|---|---:|---:|---:|---:|
| Redraw interval | 1.049 ms | 6.638 ms | 8.292 ms | 10.652 ms |
| CPU frame work | 1.045 ms | 6.631 ms | 8.285 ms | 10.646 ms |
| GPU shadow pass | 0.061 ms | 0.078 ms | 0.079 ms | 0.079 ms |
| GPU world and HUD pass | 0.065 ms | 0.085 ms | 0.086 ms | 0.086 ms |
| GPU total from shadow start to world/HUD end | 0.136 ms | 0.174 ms | 0.176 ms | 0.191 ms |

The four-slot asynchronous GPU readback ring dropped zero samples. CPU frame work deliberately
includes surface acquisition, command encoding, queue submission, and presentation, but not GPU
completion. GPU values come from native pass timestamp queries multiplied by the adapter timestamp
period. The scene is still small and uses simple materials; these results establish instrumentation,
not the final photorealistic content budget.

## 2026-09-05 — asynchronous initial meshing

The release showcase smoke initialized the Vulkan device and pipelines without performing CPU
meshing on the presentation thread. A dedicated worker then meshed the 128 chunks in
distance-prioritized batches of 16 from one shared immutable world snapshot. The renderer reached
all 43,912 exposed faces in 32.4 ms after the game runtime started and still completed the five-second
GPU smoke with zero dropped timestamp samples. The initial pending set is explicitly capped at 512
chunks; larger future worlds require the Stage 1 residency streamer rather than an unbounded queue.

## 2026-09-05 — camera-frustum culling

The five-second release showcase kept 90 of 128 resident chunks in the camera frustum and submitted
90 world draw calls. All 128 chunks remained in the directional shadow pass deliberately: geometry
outside the camera can still cast a visible shadow. GPU frame p99 was 0.201 ms in this run, but the
scene is too small and run-to-run variance too large to attribute a speedup. This result proves the
culling decision and counters; the later representative-scene gate will establish performance impact.

## 2026-09-05 — authoritative destruction baseline

Command:

```bash
cargo run --release --bin destruction-benchmark -- --events 500
```

The representative multi-material scene completed 500 server-authoritative destruction, structural
analysis, body promotion, protocol-v3 fragmentation, reordering, decode, reassembly,
client-application, and final-verification cycles at 21,637 events/s. Event latency was 0.012 ms
p50, 0.217 ms p95, and 0.343 ms p99, or 2.0% of one 60 Hz frame budget. The run produced 847
application datagrams (0.527 MiB), fractured 13,009 voxels, detached 1,361 voxels into 32 active
bodies, and ended with identical static-world and body-set state on server and client.

## 2026-09-05 — structural island extraction

Command:

```bash
cargo run --release --bin structural-benchmark -- --iterations 100
```

The fixture severs a single connector below a concrete slab of 8,192 voxels. Each analysis validates
the canonical after-state, searches only components adjacent to the edit, proves the slab has no
foundation path, computes mass/bounds, and reproduces the same 128-bit island fingerprint.

| Measurement | Result |
|---|---:|
| Combined throughput | 243 analyses and promotions/s |
| Topology analysis p50 | 2.616 ms |
| Topology analysis p95 | 2.731 ms |
| Topology analysis p99 | 2.771 ms |
| Body promotion p50 | 1.458 ms |
| Body promotion p95 | 1.529 ms |
| Body promotion p99 | 1.549 ms |
| Combined p50 | 4.076 ms |
| Combined p95 | 4.262 ms |
| Combined p99 | 4.314 ms |
| Combined max | 4.330 ms |

The promotion step revalidates the read-only island proof, canonical material voxels, six-neighbour
connectivity and identity before computing fixed-unit centre of mass and diagonal inertia. The
combined result is below the 12 ms server-work target on this fixture. Static-world detachment and
replication are integrated separately in the end-to-end benchmark; fixed-step motion, collision
solving, sleeping, and progressive stress remain later promotion gates.

## 2026-09-05 — replicated body rendering

Command:

```bash
cargo run --release --bin playable-demo -- --showcase --smoke-seconds 5
```

The deterministic showcase first severed a fragile support to guarantee one replicated body, then
breached the main facade. Body geometry was built by the same single-queue bounded background worker
as chunk geometry, in local coordinates, and uploaded into a fixed 1,024-instance transform arena.
The Vulkan run reported 1/1 body visible, 90/128 chunks visible, 91 world draws, and 129 shadow draws.
With fixed-step motion enabled, GPU total was 0.137 ms p50, 0.158 ms p95, 0.195 ms p99, and 0.198 ms
maximum across 2,426 completed samples, with zero dropped timestamp samples. The body reached the
static ground and the smoke gate reported 1/1 body sleeping. The scene is intentionally small: this
validates the body shader, upload, culling, state replication, and draw paths, not the final
active-body rendering budget.

## 2026-09-05 — fixed-step body simulation

Command:

```bash
cargo run --release --bin physics-benchmark -- --bodies 1024 --ticks 300
```

The fixture starts the full active-body limit in 256 vertical groups above a static voxel floor. It
exercises integer gravity, swept floor queries, deterministic sleep, and sweep-and-prune candidate
generation; settled bodies intentionally overlap because body-body response is the next solver gate.

| Measurement | Result |
|---|---:|
| Tick p50 | 0.029 ms |
| Tick p95 | 0.136 ms |
| Tick p99 | 0.146 ms |
| Tick max | 0.172 ms |
| Maximum updated bodies | 1,024 |
| Maximum broad-phase pairs | 1,536 |
| Final sleeping bodies | 1,024/1,024 |

This core-solver result is comfortably below the 12 ms server-work target but excludes body-body
impulses, rotation, interest filtering, serialization, socket I/O, and other gameplay systems. Those
costs require separate promotion evidence before the Stage 2 exit gate can pass.
