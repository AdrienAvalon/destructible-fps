# Performance evidence

Performance observations are point-in-time results tied to a command, scene, build, resolution, and
machine. They are not portable guarantees or substitutes for the later platform matrix.

## 2026-09-05 — dedicated-process transport promotion

Source state: parent `ce27090` plus the dedicated transport change documented in this section.

`cargo test --all-targets` and `cargo test --release --all-targets` each passed 44 library tests,
three binary tests, and 18 integration tests. The new integration test starts the actual release or
debug dedicated-server child process, negotiates two independent loopback UDP clients, submits one
bounded command, drains fragmented deltas in sequence, and verifies identical static world, body
geometry, dynamic state, ID high-water mark, and fingerprints. Its release execution took 0.19 s;
that wall time is functional process-level evidence, not a latency or throughput benchmark.

The transport has explicit safety ceilings of 16 peers, 64 received datagrams, 256 queued commands,
32 simulated commands, and 4,096 attempted outbound datagrams per server tick. Complete out-of-order
client packets retain at most 16 packets and 8 MiB in addition to the existing bounded fragment
assembler. These are overload bounds, not the final 256-kbit/s per-player bandwidth policy; interest
management, acknowledgements, loss repair, and per-client budgets remain required.

The full promotion rerun produced the following point-in-time results:

| Fixture | p50 | p95 | p99 | max |
|---|---:|---:|---:|---:|
| Authoritative destruction event | 0.013 ms | 0.243 ms | 0.389 ms | not reported |
| Structural analysis plus body promotion | 4.024 ms | 4.195 ms | 4.285 ms | 4.295 ms |
| 1,024-body physics tick | 0.456 ms | 0.878 ms | 0.905 ms | 0.939 ms |
| Vulkan CPU frame work | 1.039 ms | 8.561 ms | 11.729 ms | 15.566 ms |
| Vulkan GPU total | 0.136 ms | 0.145 ms | 0.189 ms | 0.198 ms |

The five-second Vulkan showcase ran at 1,440×900 on the RTX 4050 Laptop GPU, completed 2,616 GPU
samples with zero drops, rendered 90/128 chunks and one replicated body, and exited cleanly.

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
analysis, body promotion, protocol-v4 fragmentation, reordering, decode, reassembly,
client-application, and final-verification cycles at 21,524 events/s. Event latency was 0.012 ms
p50, 0.229 ms p95, and 0.354 ms p99, or 2.1% of one 60 Hz frame budget. The run produced 838
application datagrams (0.515 MiB), fractured 13,009 voxels, detached 1,361 voxels into 32 active
bodies, and ended with identical static-world and body-set state on server and client.
Against the immediately preceding protocol-v3 run of the same deterministic fixture, compact IDs
reduced output from 847 to 838 datagrams and from 0.527 to 0.515 MiB (about 2.3%) without changing
the simulated result.

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
| Combined throughput | 250 analyses and promotions/s |
| Topology analysis p50 | 2.569 ms |
| Topology analysis p95 | 2.605 ms |
| Topology analysis p99 | 2.624 ms |
| Body promotion p50 | 1.425 ms |
| Body promotion p95 | 1.466 ms |
| Body promotion p99 | 1.491 ms |
| Combined p50 | 3.995 ms |
| Combined p95 | 4.058 ms |
| Combined p99 | 4.117 ms |
| Combined max | 4.225 ms |

The promotion step revalidates the read-only island proof, canonical material voxels, six-neighbour
connectivity and geometry fingerprint before computing fixed-unit centre of mass and diagonal
inertia. Runtime identity is a separate checked server-monotonic 64-bit value. The combined result
is below the 12 ms server-work target on this fixture. Static-world detachment and replication are
integrated separately in the end-to-end benchmark; fixed-step collision performance is recorded
below, while progressive stress remains a later promotion gate.

## 2026-09-05 — replicated body rendering

Command:

```bash
cargo run --release --bin playable-demo -- --showcase --smoke-seconds 5
```

The deterministic showcase first severed a fragile support to guarantee one replicated body, then
breached the main facade. Body geometry was built by the same single-queue bounded background worker
as chunk geometry, in local coordinates, and uploaded into a fixed 1,024-instance transform arena.
The Vulkan run reported 1/1 body visible, 90/128 chunks visible, 91 world draws, and 129 shadow draws.
With fixed-step motion enabled, GPU total was 0.150 ms p50, 0.176 ms p95, 0.177 ms p99, and 0.180 ms
maximum across 1,987 completed samples, with zero dropped timestamp samples. The body reached the
static ground and the smoke gate reported 1/1 body sleeping. The scene is intentionally small: this
validates the body shader, upload, culling, state replication, and draw paths, not the final
active-body rendering budget.

## 2026-09-05 — fixed-step body simulation

Command:

```bash
cargo run --release --bin physics-benchmark -- --bodies 1024 --ticks 300
```

The fixture starts the full active-body limit in 256 four-body columns above a static voxel floor. It
exercises integer gravity, swept static queries, exact voxel-column body contacts, bottom-up stacking,
wake-aware sleep, and bounded sweep-and-prune candidate generation. Every column must finish at the
four exact canonical heights or the benchmark fails.

| Measurement | Result |
|---|---:|
| Tick p50 | 0.410 ms |
| Tick p95 | 0.831 ms |
| Tick p99 | 0.848 ms |
| Tick max | 0.885 ms |
| Maximum updated bodies | 1,024 |
| Maximum broad-phase pairs | 768 |
| Static contact resolutions | 7,680 |
| Body contact resolutions | 60,416 |
| Final sleeping bodies | 1,024/1,024 |

The broad phase reports saturation only on the 8,193rd candidate; the complete tentative tick is
then discarded, so overload cannot commit a partial or tunnelling-prone result. This core-solver
result is comfortably below the 12 ms server-work target but still excludes horizontal impulses,
friction, restitution, rotation, interest filtering, serialization, socket I/O, and other gameplay
systems. Those costs require separate promotion evidence before the Stage 2 exit gate can pass.
