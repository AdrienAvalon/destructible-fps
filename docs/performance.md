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
analysis, body promotion, protocol-v2 fragmentation, reordering, decode, reassembly,
client-application, and final-verification cycles at 20,330 events/s. Event latency was 0.013 ms
p50, 0.226 ms p95, and 0.451 ms p99, or 2.7% of one 60 Hz frame budget. The run produced 847
application datagrams (0.525 MiB), fractured 13,009 voxels, detached 1,361 voxels into 32 active
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
| Combined throughput | 256 analyses and promotions/s |
| Topology analysis p50 | 2.592 ms |
| Topology analysis p95 | 2.624 ms |
| Topology analysis p99 | 2.691 ms |
| Body promotion p50 | 1.285 ms |
| Body promotion p95 | 1.327 ms |
| Body promotion p99 | 1.333 ms |
| Combined p50 | 3.880 ms |
| Combined p95 | 3.940 ms |
| Combined p99 | 3.995 ms |
| Combined max | 4.007 ms |

The promotion step revalidates the read-only island proof, canonical material voxels, six-neighbour
connectivity and identity before computing fixed-unit centre of mass and diagonal inertia. The
combined result is below the 12 ms server-work target on this fixture. Static-world detachment and
replication are integrated separately in the end-to-end benchmark; fixed-step motion, collision
solving, sleeping, and progressive stress remain later promotion gates.
