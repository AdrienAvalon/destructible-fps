# Production roadmap

## Product contract

The destination is a photorealistic native multiplayer FPS with construction and weapon-dependent,
material-aware destruction. Terrain, buildings, props and constructed elements can be created,
damaged, fractured, detached and persisted while all players observe the same authoritative result.
The requirement-by-requirement definition of success is
[`game-contract.md`](game-contract.md). Visual realism, systemic destruction, low latency and broad
hardware support are separate budgets: none may silently consume another.

The engine remains purpose-built where that creates a durable advantage: sparse world storage,
destruction, structural analysis, replication, interest management, meshing, content cooking, and
performance tooling. It uses well-maintained platform libraries for operating-system windows, GPU
abstraction, audio codecs, cryptography, and device input. Rebuilding commodity drivers or codecs
would reduce reliability without improving the game.

“Everything is destructible” means every authored world surface has an explicit destruction model.
Simulation resolution is bounded by gameplay relevance. The server owns every permanent mutation;
clients may predict reversible motion and cosmetic debris only.

## Immediate priority — photorealistic renderer and industrial map

Current user direction: prioritize **visible native rendering and map quality**, ahead of the next
large fine weapon/authority expansion. The earlier Claude-helper/probe task is historical, not a
prerequisite to reopen. The full physics/destruction/multiplayer contract remains intact.
Do not turn completion of additional infrastructure primitives into a prerequisite for showing
meaningful visual progress. The point-ray probe is an inspection tool, not completed fine combat.

Next visual slice: a cohesive industrial courtyard and ruined building, using the supplied ruined
concrete/brick factory reference as art direction. Work at actual player height and architectural
scale: natural terrain transitions, layered architecture and broken edges, coherent material scale
and variation, rubble contacts, vegetation and a photographic outdoor/indoor light balance. Inspect
the existing light transport and content first; do not simply raise texture resolution or hide
silhouette defects under postprocessing. A generated reference is never a runtime screenshot.

Each visual increment needs an actual native before/after capture from fixed wide, approach and
close-fracture views, a moving-camera check, plus separately recorded CPU/GPU frame tails and
memory/geometry budgets on named hardware. Keep source asset provenance and reproducible cooking.
Damage remains a visual acceptance condition: no floating facade, sealed physical hole, detached
decoration, disappearing support or inconsistent shadow after the corresponding source changes.
Do not obtain realism by adding indestructible gameplay cover or colliders unrelated to visible
material. Fine weapon activation stays explicitly pending while this visual-first slice advances.

The user reconfirmed the ruined-factory image as the target, with the ambition to surpass it.
[`industrial-visuals.md`](industrial-visuals.md) separates that visual acceptance from intermediate
material-condition work and keeps the outstanding geometric/composition requirements explicit.

### Visual acceptance reset — 2026-09-06

The user again rejects the current image as non-photorealistic. Small fixes to individual fragments,
tiling or haze do not constitute the requested visual milestone. Close the in-flight haze validation,
then prioritize one cohesive, player-scale reference scene instead of another isolated cosmetic
increment. The full-resolution haze candidate was subsequently rejected on native cost/benefit
evidence; runtime was restored, as recorded in [the experiment report](occluded-haze.md).
Keep the existing prototype as a technical baseline, not as visual acceptance.

The [ruined-factory reference scene](reference-scene.md) now has supported broken storeys, grounded
collapse masses and an irregular physical soil/paving edge, separately from the frozen legacy
fixture. Initial native inspection still fails the visual gate: the rubble reads as stepped ramps.
The [oblique geometry increment](convex-inspection.md) introduces closed quantized slabs sharing
their source with rendered triangles, material rays and translating box contacts. The old stepped
apron is absent from this native scene, while its regression fixture is retained. This remains a
technical geometry milestone, not photorealistic acceptance: sparse authored pieces on congruent
supports still read as placed props. Next prioritize coherent multi-scale rubble, genuinely
irregular contacts/supports and surface detail in the complete reference composition, without
hiding coarse colliders or adding indestructible decorative gameplay cover.

The next scene must combine layered industrial architecture (slabs, columns, reveals and broken
material thickness), supported multi-scale rubble, coherent terrain transitions and vegetation,
and believable outdoor/interior illumination. Reuse verified scans and tools; choose new tools only
for a demonstrated production bottleneck. Visible solids remain the destruction/collision source.
Author a small complete area first; map breadth, generic editor work and optional AI features must
not consume this milestone. Do not promise that textures or postprocessing alone can supply it.

Source inspection confirms that the Blender smoke exports/reimports a GLB, while the native game
still has no model-asset importer: installing Blender did not deliver a content-production bridge.
The material library contains five scan families, and the industrial world is authored integer
geometry with bounded refined patches. Use those facts to choose the implementation, not an
assumption that detailed assets can already be dropped into the renderer. Any needed authoring
bridge must deliver actual scene content and validate scale, bounds, provenance and physical
correspondence; an isolated importer smoke or a beautiful indestructible backdrop is not this
milestone. Conservative collision mismatches in the older smoothed coarse renderer remain an
explicit limitation in `visual-direction.md`, not precedent for silently bypassing the fine source.

Review actual native player-height stills and motion against the supplied factory reference at
comparable framing. Evaluate silhouette/scale, material detail/repetition, rubble contacts, terrain,
lighting and frame-time cost together. Keep the gate open if major categories still look like a
prototype; passing shader tests or a high FPS cannot substitute for visible quality. Generated
concept art is not an acceptable deliverable for this comparison. Broader hardware, multiplayer and
destruction qualification remain separate required gates, not implied by a beautiful static scene.

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

The production authoring scope also includes a reproducible, brief- and photograph-guided
[map generator](map-generation.md), sharing the game's destructible material geometry. Its recipe,
terrain, building, reference-analysis and export gates are tracked separately from the renderer.
The reusable [gameplay/scenario layer](gameplay-authoring.md) adds mode templates, objectives,
events and NPC encounters for solo, co-op and competitive use of the same authoritative core.
The [agent-callable production interface](agent-tooling.md) covers creating, validating, playing,
capturing, profiling and replaying content. A separate [optional adaptive director](adaptive-director.md)
can propose NPC goals, scenario branches and supported world events through server validation.
Its backend contract admits tested local models or explicitly enabled APIs, with a no-model fallback,
no implicit paid-cloud switch and no developer rights exposed to in-game actors. These are planned
requirements (TOOLS-02/03, MODE-01, SCEN-01, AI-01/02), not delivered integrations.
Additional [design experiments](destruction-design-lab.md) are ranked hypotheses with promotion gates,
not claims of new inventions or permission to postpone the native photorealistic destruction slice.

| Added requirement | Owning exit gate | Relationship to the next visual demo |
| --- | --- | --- |
| MODE-01, SCEN-01 | Stage 4: shared mode/scenario runtime; Stage 5 supplies reusable authoring | Do not block the ongoing renderer/destruction slice on a general editor |
| TOOLS-02 | Stage 5: seeded/reference-guided map authoring and playable export | Start after the existing industrial geometry integration |
| TOOLS-03 | Stage 5: actual agent connection, bounded jobs and inspectable evidence | A single existing capture operation can be exposed earlier |
| AI-01, AI-02 | Stage 5 adaptive extension, after Stage 4 NPC/scenario and recorded-event foundations | Optional to players, required for the agreed full product scope, not a prerequisite for the next visual demo |

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

- bounded exact fine/uniform surface extraction with canonical shared edge subdivisions, explicit
  output/work failures and precision bounds, immutable worker delivery and a real native four-stage
  inspection viewer, one-cell exact collar to the existing smooth terrain, and bounded incremental
  industrial-scene inspection (delivered; see `fine-rendering.md`); authoritative weapon/body/network
  activation and photoreal scene acceptance remain separate gates;

- real GPU timestamp queries and bounded CPU/GPU p50/p95/p99 telemetry (delivered; automated budget
  comparison remains);
- asynchronous initial meshing and distance-prioritized bounded bootstrap streaming (delivered;
  large-world residency streaming remains);
- conservative CPU chunk-frustum culling (delivered); hierarchical-Z occlusion, indirect drawing,
  and mesh-buffer arenas remain;
- energy-aware Cook-Torrance GGX direct lighting, explicit material identity and metalness in the
  vertex contract, five attributed scanned PBR materials in versioned offline-cooked texture arrays,
  normal-aware mips and explicit-gradient body-local triplanar projection, procedural steel/glass
  and cut layers, and a view-correct HDR sky with offline diffuse/GGX image-based lighting,
  split-sum BRDF and fixed shared exposure (delivered); GPU block compression, material blending,
  local reflections/bounced transport, and advanced postprocessing remain;
- bounded linear RGBA16Float scene targets, explicit 1x/4x spatial MSAA, radiance resolve before
  fixed exposure/tone mapping, exact output transfer and unexposed display-linear HUD (delivered);
  temporal antialiasing, specular stability, HDR-monitor output and photographic exposure remain;
- dynamic distant-sky visibility from sixteen bounded directional depth maps, actual rendered
  static/moving casters, cache invalidation, continuous coverage fade and refresh-only timings
  (delivered near the camera); higher-quality visibility, real bounce and large-world coverage remain;
- 3×3 PCF sun-shadow filtering (delivered); cascaded sun shadows, local lights, temporal
  anti-aliasing, and measured dynamic resolution remain;
- hybrid chunk meshing with fixed-cache Surface Nets for soil/stone, exact authored architecture,
  six-direction seam proofs, adaptive diagonals and corner-aware remeshing (delivered for static
  terrain), plus integrity-thresholded derived brick/concrete breach edges, a fixed render-only GPU
  damage channel, unique exact-side transition ownership, and bounded cross-chunk damage-halo cuts
  with local-depth shell/core/aggregate/reinforcement shading and pinned mixed-cell junctions
  (delivered as a coarse static fracture pass); true sub-voxel layered geometry, smooth detached
  fracture surfaces and mesh-buffer deduplication remain;
- deterministic screenshot scenes and image-difference regression thresholds.

Exit gate: the representative breach scene stays within the client budgets while remeshing and
streaming, with no synchronous world meshing on the presentation thread.

### Stage 2 — structural destruction and rigid bodies

- deterministic bounded topology analysis around edits, foundation/authored anchors, canonical
  islands, mass, bounds, fingerprints, negative tests, and an 8,192-voxel benchmark (delivered as an
  isolated primitive);
- bounded promotion into revalidated authoritative body descriptors with integer-millimetre centre
  of mass and integer inertia (delivered);
- shared exact rectangular-volume mass/first/second moments for uniform and typed fine World
  storage, rational sub-kilogram fragments and full tensors about explicit bounded pivots, with
  actual coarse body mass/COM using the same accumulator (delivered; see `mass-properties.md`);
  fine body membership/topology/transport, full-tensor angular response and fine graphical
  destruction remain integration gates, not delivered by this calculation;
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
  over-budget or intersecting rotation stops; inclined vertical support uses physical-bottom
  ordering and rational-time refinement through at most 4,096 conservative voxel-proxy pairs, with
  fail-closed support on pair-budget exhaustion);
- four-pass swept X/Z body contacts with rational time-of-impact overlap validation, bounded
  refinement through 4,096 canonical rotated per-voxel proxy pairs, deterministic contact centroids,
  inverse-mass separation, material restitution, off-centre angular impulse, momentum-preserving
  tangential friction, fail-closed coarse separation on pair-budget exhaustion, sleep wake-up, and
  deterministic short-chain propagation (delivered);
- bounded six-DOF Timoshenko beam equilibrium, canonical matrix-free iteration slices, explicit
  convergence/failure, analytical signed-axis and rigid-motion tests, and partial-support-loss
  benchmarks (foundation delivered); bounded complete free-component extraction with actual
  clamped-solid boundaries, copy-on-write chunk snapshots, authority/configuration/occupied-and-air
  observation validation, explicit cancellation and one-slot server worker scheduling (callable
  adapter delivered); explicit brittle section strengths, root-moment-aware endpoint demand,
  mass-preserving one-cell severance and component preparation on the worker, and revalidated
  atomic static-to-body protocol transactions (opt-in coarse adapter delivered); shared bounded
  FIFO dirty-domain ticks, stale requeue/coalescing, explicit errors/deadline, ordinary retained
  network repair and playable lab remeshing (delivered and opt-in; large-map decomposition and
  in-match coverage recovery, calibrated strengths, crushing geometry, nonlinear/contact load
  response and the complete authored gameplay sequence remain);
- persistent authored and inferred support graph with material compression, tension, shear, and
  joint limits;
- incremental stress propagation restricted to affected graph islands;
- unsupported component extraction with mass, centre of mass, and inertia from voxel geometry;
- exact convex dynamic-body contact manifolds, full vertical impulse exchange, gyroscopic response,
  deeper collision-island convergence, and continuous multi-contact resolution between fast moving
  bodies;
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
  roots, exact issuer, same-origin JWKS, deadlines and response ceilings is also delivered and wired
  to mandatory pre-readiness plus periodic atomic refresh with monotonic stale-key shutdown;
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
  reserved 60-second TLS and static-JWKS shutdown margins, signal-aware shutdown, and a fifteen-case
  external-process matrix including trusted discovery, hostile issuer mismatch, trust-outage
  shutdown, admission saturation, oversized-datagram rejection, multi-session queue pressure, and
  authenticated reconnect cycling;
- offline LAN policy contract and checker with a bounded schema, exact private address/interface/UDP
  port, certificate and issuer identity, canonical source ranges, compile-time runtime-limit parity,
  bounded observability, rollback ownership, and a maximum 24-hour lifetime (delivered without a
  remote bind capability), plus bounded read-only POSIX/Windows host enumeration proving the exact
  operational interface/address/index/private-prefix assignment, and an offline exact-SAN,
  ordered-chain, single-reviewed-CA proof covering the full policy lifetime plus the TLS safety
  margin, with optional strict private-key correspondence and TLS 1.3 signing-capability proof
  (delivered for candidate files; live policy review, installed runtime identity and the other
  environment proofs remain);
  bounded certificate/key hot reload changes future handshakes without disrupting current sessions
  (delivered for loopback; automated issuance, platform ACLs, production issuer/root provisioning,
  and remote policy remain);
- unreliable sequenced gameplay channel plus reliable control, inventory, and snapshot streams;
- broader entity/component snapshots, acknowledgements, delta baselines, and bounded repair;
- spatial interest management for players, active fractures, projectiles, and persistent edits;
- graphical client prediction/reconciliation and bounded lag compensation;
- server-side network explosion radius, radius-scaled energy, and 120-metre authoritative-player
  range policy (delivered); view-ray, obstruction, cadence, ammunition, and lag-history validation
  remain;
- configurable stochastic impairment plus a 32-client process harness (finite bounded trace replay
  is delivered);
- bounded loss/jitter/reorder/duplication proxy with delta/snapshot repair, ACK retry, per-channel
  offered/delivered byte accounting, four simultaneous traced clients, exact server-egress fairness,
  and Karn-filtered bounded adaptive RTT/RTO estimation (delivered for real loopback processes);
- rate limits, command validation, allocation limits, and fuzzed packet decoding.

Exit gate: at least 32 headless clients sustain the traffic and tick budgets under the agreed network
impairment profile; four graphical clients remain synchronized through join, damage, and repair.

### Stage 4 — complete FPS loop

- directional fixed-point rifle intent from the authoritative eye, finite magazine/reserve,
  fixed-tick cadence/reload, material/chord work, coarse penetration and personal weapon HUD
  (delivered as the [rifle slice](ballistics.md)); data-driven weapons, recoil, sub-cell cavities,
  dynamic-body damage, ricochet, calibrated ballistics and production explosive policy remain;
- health, armour, inventory, resources, building tools, death, respawn, and match rules;
- server-side hit validation and replayable authoritative combat timeline;
- animation graph, first/third-person rigs, inverse kinematics, camera feedback, and accessibility
  options;
- spatial audio, occlusion, reverberation zones, destruction layers, and voice integration boundary;
- bots able to traverse, attack, build, and re-plan after topology changes.

Exit gate: a complete four-player match can start, finish, restart, and persist its intended world
changes with no manual repair or authoritative divergence. MODE-01 and SCEN-01 additionally require
the same core interactions in local solo/co-op, one authored NPC/objective sequence, bounded event
execution and late-join/save-restore progress. Model-driven adaptation is a separate Stage 5 gate.

### Stage 5 — world and asset toolchain

- versioned material, weapon, structure, biome, and game-mode schemas;
- bounded seeded map recipes and brief/reference-guided generation with explicit scale/uncertainty;
- scenario graph/entity authoring, reusable mode templates and NPC encounter definitions backed by
  implemented capabilities, with event budgets, replicated progress and save/restart acceptance;
- optional adaptive NPC/scenario direction above ordinary fixed-step gameplay, with bounded filtered
  observations, typed proposals, deterministic server policy and recorded accepted-decision replay;
  prove the no-model fixture first, then two interchangeable backends, including local inference;
- explicit participant opt-in, provider-down fallback, disabled-mode behavior and fixed disclosed
  competitive rules; measure concurrent inference/combat costs before enabling adaptation;
- structured agent/human tool operations with scoped artifacts, asynchronous bounded jobs,
  cancellation and atomic candidate publication; optional MCP connection verified end to end with
  actual native output, not merely registered configuration;
- seeded automated play/stress fixtures and full gameplay incident replay, separate from GPU frame
  replay; compare canonical decisions/state without asking an AI to regenerate the same answer;
- importer/cooker for standard source assets with deterministic derived artifacts and content hashes;
- native world editor for voxel sculpting, modular construction, support visualization, lighting,
  spawn/navigation markup, and play-in-editor;
- automatic collision, LOD, texture compression, shader permutation, and package generation;
- validator for missing references, excessive budgets, unsupported structures, and incompatible
  content versions;
- migration tools for saved worlds and authored content.

Exit gate: a new map can be authored, validated, cooked, hosted, joined, and restored using only
documented tools and source assets (TOOLS-02). TOOLS-03 requires actual agent discovery/invocation,
bounded job cancellation and native artifact readback. The adaptive extension passes AI-01/02 only
with the director's recorded-decision, consent, authority and resource fixtures on two backends,
including local inference and no-model fallback; merely installing a runner does not pass.

### Stage 6 — photorealistic environments

- calibrated physically based materials and physically plausible sun, sky, exposure, and atmosphere
  (scanned material/procedural atmosphere foundation, hybrid natural terrain and coarse
  integrity-driven masonry fracture plus layered cross-section shading delivered; calibration,
  authored geometry, material blending and calibrated photographic exposure remain; the linear HDR
  working frame and fixed exposure/SDR display transform are delivered);
- terrain blending, decals, vegetation, weather, water, particles, volumetric dust, and destruction
  residue;
- scalable indirect lighting/reflections with explicit quality tiers and stable temporal behavior;
- photogrammetry-friendly capture pipeline with aggressive runtime virtualization and LOD;
- art direction and readability review so realism never hides players, hazards, or build affordances.
- coherent intact/damaged/collapsed/repaired material presentation and shared breach consequences
  for lighting, sound and traversal, promoted from the bounded design experiments;
- temporal disocclusion, surface closure, rubble contact and competitive/accessibility checks across
  quality tiers; visual detail reductions must not alter authoritative cover or structural behavior.

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

The immediate priority is the playable visual/destruction experience requested by the user:

The first authored industrial layout is now playable with `--world industrial`, including actual
material geometry, intact/breach/interior inspection views, tested canopy separation and server-only
map selection. Initial/replacement snapshot meshes use the bounded worker and reject late jobs from
old map generations; resynchronization retains input history. This advances the layout/loading
foundation of VIS-01 and SYNC-01, not photorealism, calibrated collapse or the performance release gate.
See [`industrial-world.md`](industrial-world.md) and the dated performance evidence.

1. validate the delivered scanned PBR material slice, bounded offline cooking and stable surface
   projection against moving runtime views; follow with material blending and GPU compression;
   compare actual intact/breached images and frame distributions (VIS-01, TOOLS-01, PERF-01);
2. implement weapon-specific penetration/explosion fixtures and credible structural/fracture behavior
   in the same two-client build/breach loop (FPS-01, DEST-01/02/03, PHYS-01, BUILD-01, SYNC-01);
3. advance authored industrial content, lighting, temporal stability, rubble/dust and first-person
   presentation until the runtime scene approaches the visual reference (VIS-01, FPS-01).

Prepare authoring and agent interfaces alongside these slices only where they reuse an implemented
engine capability. Start with one real inspection/capture operation, then gameplay replay, seeded
map recipes and authored scenarios. Adaptive live AI follows the no-model scenario and validation
gates; model installation, cloud spending and infrastructure changes are not prerequisites for the
next visual demo. Keep the design-lab backlog subordinate to the priorities above.

Before exposing a non-loopback server, automated certificate issuance, platform service permissions,
production OIDC trust provisioning and the [`remote attack/failure matrix`](remote-exposure-gate.md)
remain mandatory. Full contact manifolds, vertical impulse exchange, gyroscopic response, seeded
network impairment profiles, pacing/congestion control and the 32-client load gate remain tracked
under their owning stages. None is removed from the final game contract.

Each increment lands with focused tests, the complete repository validation suite, a real-GPU smoke,
updated evidence, and a coherent commit. A stage advances only when its exit gate is demonstrated.
