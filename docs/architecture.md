# Architecture and promotion gates

## Product intent

The target is a photorealistic first-person shooter with persistent, material-aware destruction.
The world is authoritative on dedicated servers and every gameplay-relevant fracture is reproduced
for all interested clients. Performance claims are accepted only with captured frame, simulation,
network, memory, and worst-case destruction measurements.

"Everything is destructible" is implemented at bounded physical resolutions:

1. terrain uses a sparse volumetric representation and can form craters or tunnels;
2. load-bearing structures use a material constraint graph;
3. detached pieces become rigid bodies at a resolution selected by their gameplay relevance;
4. settled or distant debris is merged into static clusters;
5. dust, chips, and non-gameplay fragments are deterministic cosmetic effects.

This preserves believable outcomes without attempting an impossible atom-level simulation.

## Runtime ownership

The dedicated server owns commands, damage, fracture, structural separation, rigid-body creation,
and persistent world state. Clients predict only reversible player and weapon motion. A client never
announces that a wall was destroyed; it requests an action and receives the resulting transaction.

Delta protocol v3 uses monotonically increasing sequences, independent 128-bit pre/post
fingerprints for the static world and active body set, bounded fragments, and before-state
validation. Detached body membership and integer dynamic state travel in separate canonical frames;
both are reconstructed and validated before any static-world write. Missing data stops application
and requests a snapshot. Corrupt or stale data cannot partially mutate a replica.

## Planned engine layers

### First playable slice — delivered

- Linux window and raw first-person input through `winit`;
- discrete-GPU preference with a safe `wgpu` Vulkan backend;
- deterministic multi-material test range rendered as face-culled chunk meshes;
- fixed-step movement, gravity, jump, voxel collision, ray targeting, and crosshair;
- rifle and explosive actions crossing the same server command, 1,200-byte fragmentation,
  out-of-order reassembly, fingerprint validation, and client-replica path covered by tests;
- distance-prioritized initial chunk streaming plus boundary-aware bounded background remeshing,
  both using shared immutable snapshots and 16-chunk initial batches;
- per-vertex voxel ambient occlusion, a 2,048² comparison shadow map, procedural lighting,
  roughness, surface variation, fog, and filmic tone mapping with exactly one display transfer;
- bounded CPU frame distributions and non-blocking GPU timestamp readback with a fixed four-slot
  ring and graceful capability fallback;
- conservative camera-frustum chunk culling with explicit resident, visible, world-draw, and
  shadow-draw counters;
- auto-terminating real-GPU smoke mode.

Gate evidence: the release smoke test created a Vulkan surface on the RTX 4050 Laptop GPU, validated
the WGSL pipelines, uploaded the complete representative world, presented continuously, and exited
cleanly. This is point-in-time developer-machine evidence, not a portable FPS guarantee.

### Milestone 1 — production-grade Vulkan visual slice

- Linux and Windows window/input abstraction;
- bindless material tables and physically based shading;
- large-world streaming beyond the bounded local bootstrap window;
- hierarchical-Z occlusion culling and indirect drawing beyond the delivered CPU chunk frustum;
- HDR output, temporal anti-aliasing, and measured dynamic resolution;
- a benchmark capture for the RTX 4050 Laptop at 1,920×1,080.

Gate: the playable scene reports separate CPU/GPU frame-time p50/p95/p99, avoids render-thread
meshing stalls under the agreed destruction load, and holds its frame budget at 1,920×1,080.

### Milestone 2 — structural physics

- bounded incremental topology analysis around changed voxels, foundation/authored anchors, and
  canonical detached-island descriptors (delivered as an isolated server-side primitive);
- revalidated rigid-body descriptors with fixed integer centre of mass, diagonal inertia, mass,
  bounds, canonical geometry, and stable identity (delivered);
- atomic static-world detachment and protocol-v3 body replication with independent fingerprints,
  hostile-input limits, and replica reconstruction (delivered);
- local-space body meshes produced by the bounded background worker, fixed-capacity GPU transform
  instances, body frustum culling, and world/shadow rendering (delivered);
- deterministic 60 Hz micrometre state, gravity, swept vertical collision against static voxels and
  exact body columns, stable stacking, wake propagation, sleeping, bounded sweep-and-prune with
  atomic overload rollback, protocol state replication, and batched GPU transform updates
  (delivered for axis-aligned downward contacts);
- persistent foundation and material constraint graph integrated into authoritative transactions;
- compression, tension, shear, and connection limits by material;
- local stress propagation after damage;
- unsupported island extraction (delivered for topology-changing voxel edits);
- rigid-body mass, centre of mass, and inertia derived from geometry (delivered);
- horizontal impulses, friction, restitution, rotation, clustering, and distance-based solver
  budgets.

Gate: destroying a load-bearing member produces a repeatable progressive collapse and never stalls a
60 Hz server tick in the agreed worst-case scene.

### Milestone 3 — real transport

- encrypted client authentication and session negotiation;
- unreliable sequenced deltas plus reliable snapshot/control channels;
- loss, duplication, reordering, latency, and bandwidth simulation;
- spatial interest management and per-client bandwidth budgets;
- join-in-progress and persisted-world recovery.

Gate: at least four processes remain synchronized under injected loss and jitter, and an intentionally
corrupted client is repaired from a bounded snapshot.

### Milestone 4 — FPS vertical slice

- high-quality first-person controller;
- projectile ballistics, penetration, ricochet, and material energy loss;
- one firearm and one explosive;
- spatial audio and destruction effects;
- dedicated server browser/session bootstrap;
- automated soak and adversarial command tests.

Gate: four-player match, persistent destruction, stable frame pacing, no authoritative divergence,
and a reproducible performance report.

## Non-goals for the spike

- claiming AAA visual quality before representative assets and lighting exist;
- synchronizing thousands of cosmetic particles as gameplay state;
- trusting client-side physics decisions;
- rebuilding operating-system windows, audio codecs, or GPU drivers;
- hiding missed performance targets behind average frame rates.
