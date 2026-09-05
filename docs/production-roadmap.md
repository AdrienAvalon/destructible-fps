# Production roadmap

## Product contract

The destination is a native first-person construction game in which terrain and buildings can be
created, fractured, detached, and persisted while several players observe the same authoritative
result. Visual realism, systemic destruction, low latency, and broad hardware support are separate
budgets: none may silently consume another.

The engine remains purpose-built where that creates a durable advantage: sparse world storage,
destruction, structural analysis, replication, interest management, meshing, content cooking, and
performance tooling. It uses well-maintained platform libraries for operating-system windows, GPU
abstraction, audio codecs, cryptography, and device input. Rebuilding commodity drivers or codecs
would reduce reliability without improving the game.

“Everything is destructible” means every authored world surface has an explicit destruction model.
Simulation resolution is bounded by gameplay relevance. The server owns every permanent mutation;
clients may predict reversible motion and cosmetic debris only.

## Non-negotiable budgets

| Domain | Shipping target | Promotion evidence |
|---|---:|---|
| Client frame time | 16.67 ms p99 at 1080p High; 8.33 ms p95 at 1080p Competitive | CPU and GPU timestamp captures, including active destruction |
| Server simulation | 60 Hz, 12 ms p99 with 32 nearby players | deterministic headless soak and worst-case collapse fixture |
| Local input | sampled/predicted at 120 Hz or display rate when higher | input-to-simulation trace |
| Destruction reaction | first authoritative visible result under 100 ms plus network latency | command-to-present trace |
| Gameplay traffic | 256 kbit/s sustained per player, 768 kbit/s bounded burst | packet capture under scripted combat |
| Ordinary datagram | at most 1,200 bytes | codec property tests and runtime counters |
| Join/repair memory | explicit bounded snapshot and reassembly queues | hostile-input tests and peak resident memory |
| World durability | no acknowledged mutation lost after process recovery | crash/restart and restore drills |

Targets are evaluated on named hardware and scene fixtures. Averages, empty scenes, and uncaptured
developer impressions are never release evidence.

## Delivery sequence

### Stage 0 — deterministic playable foundation (delivered)

- sparse 16³ multi-material chunks and integer destruction;
- atomic transactions with monotonically increasing sequence and 128-bit pre/post fingerprints;
- bounded 1,200-byte framing, out-of-order reassembly, replay and gap detection;
- server-to-client UDP loopback tests and repeatable destruction benchmark;
- fixed-step first-person movement, collision, rifle, explosive, crosshair, and Vulkan presentation;
- distance-prioritized initial streaming and boundary-aware background remeshing on shared immutable
  world snapshots;
- voxel ambient occlusion, directional shadows, fog, and filmic output;
- bounded CPU frame distributions and non-blocking real-GPU timestamp telemetry;
- conservative chunk-frustum culling with explicit draw counters.

Exit evidence: debug/release tests, strict Clippy, real-GPU smoke, benchmark, and actual captures.

### Stage 1 — observable production renderer

- real GPU timestamp queries and bounded CPU/GPU p50/p95/p99 telemetry (delivered; automated budget
  comparison remains);
- asynchronous initial meshing and distance-prioritized bounded bootstrap streaming (delivered;
  large-world residency streaming remains);
- conservative CPU chunk-frustum culling (delivered); hierarchical-Z occlusion, indirect drawing,
  and mesh-buffer arenas remain;
- energy-aware Cook-Torrance GGX direct lighting and explicit material metalness in the vertex
  contract (delivered); texture arrays, scanned albedo/normal/roughness/metalness maps, image-based
  lighting, and a full HDR pipeline remain;
- cascaded sun shadows, local lights, temporal anti-aliasing, and measured dynamic resolution;
- deterministic screenshot scenes and image-difference regression thresholds.

Exit gate: the representative breach scene stays within the client budgets while remeshing and
streaming, with no synchronous world meshing on the presentation thread.

### Stage 2 — structural destruction and rigid bodies

- deterministic bounded topology analysis around edits, foundation/authored anchors, canonical
  islands, mass, bounds, fingerprints, negative tests, and an 8,192-voxel benchmark (delivered as an
  isolated primitive);
- bounded promotion into revalidated authoritative body descriptors with integer-millimetre centre
  of mass and integer inertia (delivered);
- atomic explosion/topology/body transactions and protocol-v6 body membership replication with
  independent body fingerprints, bounded reassembly memory, and hostile-input rejection
  (delivered);
- non-zero server-monotonic 64-bit body IDs separated from canonical 128-bit geometry fingerprints,
  with checked reservation and rollback-safe exhaustion handling (delivered; persistent high-water
  storage remains a Stage 3 requirement);
- preserved-material body rendering through bounded off-thread local-space meshing and a
  fixed-capacity GPU transform arena (delivered);
- 60 Hz micrometre gravity, mass-weighted blast impulses, inertia-weighted off-centre angular
  response, canonical fixed-quaternion integration, rotation-aware per-voxel conservative static
  sweeps, collision-generated torque, material ground friction and normal restitution, vertical
  body-column collision, stable stacking, wake propagation, deterministic sleeping, bounded
  sweep-and-prune with fail-closed overflow, protocol-v6/snapshot-v2 state updates, and mass-centred
  GPU transforms with conservative rotated render bounds (delivered; static angular motion uses a
  radius-derived 0.25 m sample bound, at most eight substeps and 262,144 tested cells per body tick;
  over-budget or intersecting rotation stops, rotated bodies fail closed out of the legacy
  vertical-column solver);
- four-pass swept X/Z body contacts with rational time-of-impact overlap validation, bounded
  refinement through 4,096 canonical rotated per-voxel proxy pairs, deterministic contact centroids,
  inverse-mass separation, material restitution, off-centre angular impulse, momentum-preserving
  tangential friction, fail-closed coarse separation on pair-budget exhaustion, sleep wake-up, and
  deterministic short-chain propagation (delivered);
- persistent authored and inferred support graph with material compression, tension, shear, and
  joint limits;
- incremental stress propagation restricted to affected graph islands;
- unsupported component extraction with mass, centre of mass, and inertia from voxel geometry;
- exact convex dynamic-body contact manifolds, rotated vertical support, gyroscopic response, deeper
  collision-island convergence, and continuous multi-contact resolution between fast moving bodies;
- debris relevance tiers: authoritative hazards, replicated coarse bodies, deterministic cosmetic
  fragments, and settled static clusters;
- player construction with shared replay ordering, bounded static placement, per-session resource
  costs, face support, static occupancy, conservative dynamic-body exclusion, fingerprinted
  replication, six-metre server-player reach, conservative integer line of sight, a real
  authenticated-QUIC movement-plus-build test, and a playable wood-placement control (authority
  slice delivered; persistent inventory, recipes, removal and dynamic attachment remain);
- fixed-step authoritative character movement with bounded newest-input retention, independent
  input replay protection, stale-input expiry, gravity, jumping, static collision, fall recovery and
  session cleanup (server slice delivered); a 20 Hz, single-datagram, full-view state stream now
  replicates fixed position, compact velocity, integration remainders, grounded state and input
  acknowledgement for all 16 sessions;
  an eight-view integer interpolation history renders remote state at a bounded 100 ms delay with
  coherent joins and leaves; local prediction retains 128 contiguous inputs and atomically replays
  the unacknowledged suffix from exact server state; a shared instanced placeholder mesh renders up
  to 16 remote character bounds in one world and one shadow draw; a two-window loopback client wires
  input, prediction, reconciliation, interpolation, authoritative construction/destruction, static
  remeshing, rigid-body creation and body transforms to the real development process (slices
  delivered); graphical late-join snapshot bootstrap, missing-fragment repair, acknowledgement and
  ordered catch-up, bounded small-correction smoothing with safe large-discontinuity snapping, and
  bounded off-thread live-delta chunk/body meshing are delivered; initial-snapshot residency
  streaming, secure graphical transport, view authority and dynamic-body contact remain.

Exit gate: removing a load-bearing member causes a repeatable progressive collapse; the worst-case
fixture remains inside the 60 Hz server budget and converges bit-for-bit on replicas.

### Stage 3 — Internet multiplayer

- separate nonblocking UDP dedicated-server process, fixed protocol-version handshake,
  source-bound development sessions, hard ingress/queue/simulation/egress budgets, ordered delta
  inbox, and a two-client process integration test (delivered for loopback only);
- count-and-byte-bounded recent-delta history, bounded repair queue, prioritized exact
  retransmission, a process test that deliberately loses one sequence, and graphical future-gap
  detection/retry proven by a real Vulkan client that discards a whole transaction (delivered);
- canonical four-MiB-bounded snapshots, MTU-safe framing, one-transfer client assembly, paced
  per-peer emission, fixed-window selective retransmission after a lost fragment, acknowledged
  atomic install, shared-buffer per-transfer catch-up capped at 256 packets/eight MiB, and ordered
  return to live delivery (delivered for loopback);
- bounded TLS 1.3 QUIC configuration, server-certificate validation, post-TLS opaque credential
  admission, connection-bound principal, admission timeout, and encrypted datagram tests (delivered
  and wired into the authority runtime); reusable file-backed client bootstrap with bounded root/token
  loading, strict token permissions, cryptographic nonce and end-to-end encrypted snapshot test is
  delivered, while graphical event-loop wiring remains;
- offline RS256 access-token verification with strict JWKS key policy, exact issuer/audience and time
  validation, bounded one-use `jti` cache, atomic rotation, and issuer/subject-derived 256-bit
  principal (delivered); a TLS-only, no-redirect/no-proxy discovery client with bounded private
  roots, exact issuer, same-origin JWKS, deadlines and response ceilings is also delivered, while
  atomic process refresh wiring remains;
- transport-independent authority state keyed by opaque peer IDs, authenticated principal binding,
  bounded core-owned ingress and egress, transport-sized delta/snapshot framing, and legacy UDP
  behavior preserved by process tests (delivered);
- 32-task bounded concurrent admission, 64-event control and 256-datagram gameplay queues,
  cryptographic server nonces, monotonic session allocation, per-session ingress limits, and
  two-client secure-authority convergence (delivered in-process over real QUIC sockets);
- player-state protocol v2 with a sorted 16-player cap, 910-byte maximum packet, 20 Hz latest-wins
  broadcast, input acknowledgement, stale/replay rejection, disconnect removal, and real two-client
  QUIC convergence, plus a 100 ms bounded deterministic remote interpolation buffer and exact
  128-input local reconciliation and a fixed-capacity instanced remote-player GPU path (delivered;
  live loopback graphical movement and permanent-world delta wiring delivered; secure QUIC graphical
  wiring and spatial delta baselines remain); graphical late-join snapshot, ordered catch-up and
  bounded correction smoothing are delivered for loopback;
- standalone secure authority with bounded configuration/credential files, exact PEM cardinality,
  Unix permission checks, complete-chain X.509 lifetime preflight, monotonic TLS/static-JWKS expiry,
  signal-aware shutdown, and a five-case external-process matrix (delivered for loopback; online
  refresh, automated renewal, platform ACLs, and remote policy remain);
- unreliable sequenced gameplay channel plus reliable control, inventory, and snapshot streams;
- broader entity/component snapshots, acknowledgements, delta baselines, and bounded repair;
- spatial interest management for players, active fractures, projectiles, and persistent edits;
- graphical client prediction/reconciliation and bounded lag compensation;
- server-side network explosion radius, radius-scaled energy, and 120-metre authoritative-player
  range policy (delivered); view-ray, obstruction, cadence, ammunition, and lag-history validation
  remain;
- configurable stochastic and trace-replay impairment plus a 32-client process harness;
- fixed-profile bounded loss/jitter/reorder/duplication proxy with delta/snapshot repair, ACK retry,
  and per-channel byte accounting (delivered for one real client and one server process);
- rate limits, command validation, allocation limits, and fuzzed packet decoding.

Exit gate: at least 32 headless clients sustain the traffic and tick budgets under the agreed network
impairment profile; four graphical clients remain synchronized through join, damage, and repair.

### Stage 4 — complete FPS loop

- data-driven weapons, recoil, reload, ballistics, penetration, ricochet, and material energy loss;
- health, armour, inventory, resources, building tools, death, respawn, and match rules;
- server-side hit validation and replayable authoritative combat timeline;
- animation graph, first/third-person rigs, inverse kinematics, camera feedback, and accessibility
  options;
- spatial audio, occlusion, reverberation zones, destruction layers, and voice integration boundary;
- bots able to traverse, attack, build, and re-plan after topology changes.

Exit gate: a complete four-player match can start, finish, restart, and persist its intended world
changes with no manual repair or authoritative divergence.

### Stage 5 — world and asset toolchain

- versioned material, weapon, structure, biome, and game-mode schemas;
- importer/cooker for standard source assets with deterministic derived artifacts and content hashes;
- native world editor for voxel sculpting, modular construction, support visualization, lighting,
  spawn/navigation markup, and play-in-editor;
- automatic collision, LOD, texture compression, shader permutation, and package generation;
- validator for missing references, excessive budgets, unsupported structures, and incompatible
  content versions;
- migration tools for saved worlds and authored content.

Exit gate: a new map can be authored, validated, cooked, hosted, joined, and restored using only
documented tools and source assets.

### Stage 6 — photorealistic environments

- calibrated physically based materials and physically plausible sun, sky, exposure, and atmosphere;
- terrain blending, decals, vegetation, weather, water, particles, volumetric dust, and destruction
  residue;
- scalable indirect lighting/reflections with explicit quality tiers and stable temporal behavior;
- photogrammetry-friendly capture pipeline with aggressive runtime virtualization and LOD;
- art direction and readability review so realism never hides players, hazards, or build affordances.

Exit gate: representative indoor, outdoor, construction, and collapse scenes pass image-quality,
temporal-stability, and frame-budget reviews on every quality tier.

### Stage 7 — persistence and operations

- append-only world mutation journal, transactional checkpoints, compaction, and schema migration;
- dedicated server configuration, discovery, moderation, backups, metrics, logs, and crash reports;
- idempotent recovery after process, machine, or storage interruption;
- admin permissions and audit trail separated from gameplay authority;
- soak tests covering long-lived worlds, repeated collapse/build cycles, and reconnect storms.

Exit gate: a seven-day accelerated soak survives injected crashes and restores every acknowledged
persistent mutation without unbounded storage, memory, or latency growth.

### Stage 8 — multi-OS and hardware scale

- Vulkan on Linux, Direct3D 12 on Windows, and Metal on macOS through the same safe `wgpu` contract;
- keyboard/mouse and controller mappings, high-DPI windows, multiple refresh rates, and ultrawide;
- capability-derived presets, shader/pipeline caches, and graceful fallback paths;
- CI builds and native smoke machines for each supported OS and GPU vendor;
- signed packages, differential updates, save compatibility, and crash-safe rollback.

Exit gate: clean machines install, host or join, play a scripted match, update, and roll back on every
supported OS. Cross-compilation alone is not platform validation.

### Stage 9 — security, scale, and release

- authoritative anti-cheat invariants, anomaly evidence, moderation workflow, and privacy boundaries;
- dependency provenance, reproducible release artifacts, signed manifests, and vulnerability response;
- regional server load, denial-of-service budgets, matchmaking, and capacity alarms;
- accessibility, localization, settings migration, onboarding, telemetry consent, and support tools;
- closed alpha, performance beta, content beta, release candidate, and rollback rehearsals.

Exit gate: every release requirement has a reproducible artifact or test result, all critical defects
are resolved, and launch/rollback ownership is documented.

## Immediate execution queue

The next three bounded increments are:

1. connect the delivered trusted OIDC discovery client to bounded atomic JWKS refresh, then add
   automated certificate renewal, Windows service DACL checks, and a remote attack/failure matrix
   before enabling an explicit non-loopback policy;
2. extend the delivered oriented per-voxel dynamic contact with rotated vertical support, continuous
   multi-contact manifolds, gyroscopic response, and deeper collision-island convergence;
3. extend the fixed impairment profile into configurable trace replay and congestion tests for at
   least four clients, with RTT estimation, adaptive retransmission, and bandwidth fairness.

Each increment lands with focused tests, the complete repository validation suite, a real-GPU smoke,
updated evidence, and a coherent commit. A stage advances only when its exit gate is demonstrated.
