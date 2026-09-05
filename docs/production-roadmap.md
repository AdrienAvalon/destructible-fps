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
- physically based material table, texture arrays, normal/roughness/metalness maps, and HDR pipeline;
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
- atomic explosion/topology/body transactions and protocol-v3 body membership replication with
  independent body fingerprints, bounded reassembly memory, and hostile-input rejection
  (delivered);
- preserved-material body rendering through bounded off-thread local-space meshing and a
  fixed-capacity GPU transform arena (delivered);
- 60 Hz micrometre gravity, swept vertical static and body-column collision, stable stacking, wake
  propagation, deterministic sleeping, bounded sweep-and-prune with fail-closed overflow,
  protocol-v3 state updates, and batched GPU transforms (delivered for downward axis-aligned
  contacts);
- persistent authored and inferred support graph with material compression, tension, shear, and
  joint limits;
- incremental stress propagation restricted to affected graph islands;
- unsupported component extraction with mass, centre of mass, and inertia from voxel geometry;
- deterministic server rigid-body integration, collision islands, sleeping, and continuous collision
  detection for fast gameplay objects;
- debris relevance tiers: authoritative hazards, replicated coarse bodies, deterministic cosmetic
  fragments, and settled static clusters;
- player construction with server-validated placement, resource cost, support, and collision.

Exit gate: removing a load-bearing member causes a repeatable progressive collapse; the worst-case
fixture remains inside the 60 Hz server budget and converges bit-for-bit on replicas.

### Stage 3 — Internet multiplayer

- authenticated encrypted session negotiation and protocol-version agreement;
- unreliable sequenced gameplay channel plus reliable control, inventory, and snapshot streams;
- entity/component snapshots, acknowledgements, delta baselines, and bounded repair;
- spatial interest management for players, active fractures, projectiles, and persistent edits;
- client input prediction, server reconciliation, interpolation, and bounded lag compensation;
- deterministic loss/jitter/reorder/duplication simulator and multi-process test harness;
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

1. replace spawn-position body identity with a separate monotonic entity identity before allowing
   structures to be rebuilt and detached repeatedly at the same coordinates;
2. add horizontal velocity, material friction/restitution, and deterministic impulse response;
3. add angular state, inertia-driven impulses, conservative rotated bounds, and replicated
   orientation.

Each increment lands with focused tests, the complete repository validation suite, a real-GPU smoke,
updated evidence, and a coherent commit. A stage advances only when its exit gate is demonstrated.
