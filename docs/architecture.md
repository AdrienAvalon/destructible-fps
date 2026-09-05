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

Delta protocol v4 uses monotonically increasing sequences, independent 128-bit pre/post
fingerprints for the static world and active body set, bounded fragments, and before-state
validation. Detached body membership and integer dynamic state travel in separate canonical frames;
both are reconstructed and validated before any static-world write. Missing data stops application;
the UDP client first requests an exact retained transaction and receives a canonical snapshot when
that sequence has expired. Snapshot installation itself rejects a mismatched body/state set, invalid
high-water mark, non-canonical descriptor, static/body voxel overlap, or inconsistent world
fingerprint before replacing any replica state. Corrupt or stale data cannot partially mutate a
replica.

Runtime bodies use compact non-zero 64-bit IDs reserved monotonically by the server only when the
whole detachment transaction commits. Their canonical geometry retains a separate 128-bit
fingerprint, and the active-body fingerprint mixes entity ID, geometry, and dynamic state. A future
persistent server must durably store the ID high-water mark with its world snapshot before it may
restore and allocate another body.

The first real transport slice runs the authority in a separate nonblocking UDP process. A fixed
versioned control codec admits source-bound development sessions, validates bounded explosion
commands, separates receive and simulation phases, and applies hard per-tick limits to ingress,
queued work, simulation, and egress. Complete deltas are released to clients only in contiguous
sequence order, including when UDP delivers later packets first. A process-level integration test
drives two independent sockets and proves identical world/body state and fingerprints. A second
test drops a whole sequence for one client, keeps later complete packets buffered, and recovers by
requesting the exact server-retained frames. Snapshot requests use a distinct control message; a
repair request for a future sequence cannot force snapshot work. The retention history, recovery
queue, repairs per tick, and shared send-attempt budget are all fixed. An expired-history miss is
observable and remains fail-closed while a canonical snapshot is framed below the MTU, hashed
against mixed/corrupt fragments, and paced at 16 frames per peer per tick. The client retains only
one four-MiB-bounded snapshot and reports missing fragments in fixed 64-bit windows. The server
retains the immutable encoded frames, retransmits only those selected fragments under the shared
egress budget, and waits for a matching snapshot-install acknowledgement. Live deltas are withheld
from that peer and their shared encoded buffers enter a per-transfer queue capped at 256 packets and
8 MiB. Only after the acknowledgement is that queue replayed in order before live delivery resumes.
A missing or overflowed catch-up delta stalls rather than silently skipping state. Process tests
cover selective repair of a lost snapshot fragment, explicit install acknowledgement, and a moving
body with post-snapshot catch-up.
These sessions are deliberately loopback-only and unauthenticated; the snapshot hash is an integrity
check, not a MAC, and the transport provides no confidentiality, identity, packet authenticity, or
congestion control. It must not be exposed beyond the developer machine.

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
  bounds, canonical geometry fingerprint, and independent monotonic entity identity (delivered);
- atomic static-world detachment and protocol-v4 body replication with independent fingerprints,
  compact 64-bit IDs, hostile-input limits, and replica reconstruction (delivered);
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

- separate nonblocking UDP authority, versioned control handshake, source-bound development
  sessions, bounded queues and per-tick work, ordered client delivery, and a real two-client
  process test (delivered for unauthenticated loopback only);
- exact short-gap repair from a count-and-byte-bounded delta history, prioritized before new
  simulation, with deliberate whole-sequence loss and convergence coverage (delivered);
- canonical bounded snapshots, corruption rejection, paced transfer, selective bitmap repair after
  a deliberately lost fragment, acknowledged atomic install, and retained-delta catch-up during
  active body motion (delivered for loopback);
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
