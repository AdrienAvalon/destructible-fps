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

The initial delta protocol uses monotonically increasing sequences, 128-bit pre/post fingerprints,
bounded fragments, and before-state validation. Missing data stops application and requests a
snapshot. Corrupt or stale data cannot partially mutate a replica.

## Planned engine layers

### Milestone 1 — Vulkan visual slice

- Linux and Windows window/input abstraction;
- Vulkan device selection that prefers the discrete GPU;
- bindless material tables and physically based shading;
- chunk meshing off the render thread;
- GPU frustum and occlusion culling;
- HDR output, temporal anti-aliasing, and measured dynamic resolution;
- a benchmark capture for the RTX 4050 Laptop at 1,920×1,080.

Gate: a controllable camera renders the test range at a stable frame pace, reports CPU/GPU timings,
and regenerates only chunks touched by destruction.

### Milestone 2 — structural physics

- foundation and support graph;
- compression, tension, shear, and connection limits by material;
- local stress propagation after damage;
- unsupported island extraction;
- rigid-body mass, centre of mass, and inertia derived from geometry;
- sleep, clustering, and distance-based solver budgets.

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
