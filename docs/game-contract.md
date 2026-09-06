# Game objective and acceptance contract

Deliver a beautiful, photorealistic, native multiplayer first-person shooter with construction and
persistent, material-aware destruction across Linux, Windows and macOS. The specialized engine must
keep responsive gunplay, believable physics, synchronized outcomes and bounded resource consumption
while the environment is being damaged and rebuilt. This expands the active project goal; none of
the requirements below is satisfied merely by documenting it or passing unrelated tests.

## The intended playable experience

A player enters a credible industrial/quarry environment, moves and aims naturally, shoots through
appropriate cover, breaches buildings with explosives, weakens supports until structures collapse,
and builds or repairs useful structures. Other players see and interact with the same changes. The
world remains visually coherent before damage, during fragmentation, after settling and after a
server restart. The generated concept in `visual-direction.md` supplies art direction; only native
runtime images and gameplay recordings demonstrate delivered image quality.

## Requirements that define completion

| ID | Requirement | Acceptance evidence |
|---|---|---|
| FPS-01 | Complete responsive FPS controls, weapons, aiming, recoil, ammunition, reload, impacts, readable feedback and spatial audio | A packaged client plays a complete scripted combat sequence; input latency and presentation inspected |
| DEST-01 | Every in-bounds physical terrain, building, prop and constructed element has a damage/fracture response; boundaries and cosmetic-only effects are explicitly identified | Content validator enumerates every asset; representative tests damage each physical asset family without hidden invulnerable fallbacks |
| DEST-02 | Different weapons cause different plausible damage based on energy, angle, penetration, material and thickness | Fixed fixtures compare shots through wood, glass, brick, reinforced concrete and steel; range, ammunition and server fire cadence enforced |
| DEST-03 | Explosive effects depend on distance, occlusion, material, structural supports and available energy | Sheltered/exposed fixtures, multi-material walls and large charges show distinct bounded damage, impulses, fragments and dust |
| PHYS-01 | Progressive structural failure, detached-body collisions, stacking, mass, angular response and stable settling remain credible under combat | Multi-storey support failure, intersecting bodies, blast chains and repeated collapse/build fixtures; no persistent penetration, energy growth or lost topology |
| BUILD-01 | Players construct, modify and repair within server-approved resources and placement constraints | Two-client build/breach/rebuild loop; collision, inventory, support, obstruction and persistence tests |
| VIS-01 | Photoreal material scale, geometry, lighting, atmosphere, vegetation, rubble, reflections and weapon presentation meet the visual reference direction | Native indoor/outdoor and damaged/intact captures, plus moving-camera review without conspicuous voxel steps, projection seams, repetitive noise or temporal shimmer |
| SYNC-01 | Gameplay destruction, significant debris, player interaction and construction converge across clients; cosmetic effects never alter authority | Two, four and 32-client combat traces with loss, reordering, late join, repair, reconnect and state fingerprints |
| PERF-01 | Responsive play persists during the expensive events, not only after debris sleeps | Sustained active-combat CPU/GPU p50/p95/p99, command-to-present latency, server ticks, peak RAM/VRAM, allocations, bandwidth and queue depth on named hardware |
| SEC-01 | Remote play rejects forged identity, replayed/out-of-range commands, unauthorized world edits and resource-exhaustion inputs | Authenticated server tests, hostile-input budgets, independent code review, dependency provenance and release exposure gate |
| SAVE-01 | Every acknowledged persistent edit survives tested crash/restart and backup restoration | Mutation journal/checkpoint recovery and restore drills with client rejoin and canonical state verification |
| OS-01 | Installable native clients and servers work on Linux, Windows and macOS with documented hardware tiers | Clean-machine installation, match, save/restore, update and rollback on each actual OS/backend |
| TOOLS-01 | Content authoring, material cooking, map validation, profiling and distribution are reproducible and usable | Documented commands recreate a map, offline asset package and tested release from versioned sources |
| TOOLS-02 | A reproducible map generator supports metric recipes, seeds, composable terrain/buildings and brief or photograph-guided authoring with explicit uncertainty | Regenerate identical geometry from a recipe; inspect a reference-guided map; reject invalid/oversized input; load, damage, save and restore the generated world |
| TOOLS-03 | Versioned agent-callable tools support content creation, validation, native play/capture, profiling, replay and comparison without widening host or runtime authority | Actual connection discovers and runs implemented operations; verify structured evidence, bounded jobs, cancellation, stale/idempotent publication, scoped artifacts and failure reporting |
| MODE-01 | A reusable FPS foundation separates engine, gameplay capabilities, authored content and mode rules for solo, cooperative and competitive sessions | The same world interactions use the same authoritative physics locally and with multiple clients; mode templates change rules without copying or weakening simulation |
| SCEN-01 | Authorable objectives, events and NPC encounters react to the destructible world and retain consistent progress | Play an authored breach/encounter/objective sequence; verify NPC reaction, bounded event execution, duplicate suppression, late join and save/restart of both geometry and scenario state |
| AI-01 | Optional adaptive direction changes NPC goals, scenario branches and supported world events through server-validated proposals while the game remains playable without inference | Opt-in native scenario with malformed/stale/duplicate proposal rejection, filtered observations, provider-down fallback, recorded-decision replay and matching multi-client/save-restore state |
| AI-02 | Interchangeable tested local or remote model adapters preserve the same gameplay contract, privacy rules and resource budgets | Run one conformance scenario on two backends; prove capability rejection, shared-resource frame/tick budgets, local-only egress, no implicit paid fallback and explicit disable |

Reusable content and scenario boundaries are described in [`gameplay-authoring.md`](gameplay-authoring.md).
They are acceptance requirements, not a claim that generalized NPC AI or a scenario editor already exists.
The same distinction applies to [agent-callable tools](agent-tooling.md) and the optional
[adaptive director and backend interface](adaptive-director.md). These additions do not replace
the native photorealism, material destruction, structural physics or multiplayer gates.

"Everything destructible" requires meaningful physical outcomes. A small projectile may mark steel
without penetrating it, while sufficient cumulative damage or an appropriate explosive can sever
it. Reinforcement must affect resistance and failure, not just appear as a shader stripe. A texture
change alone does not satisfy volumetric damage. Debris that blocks traversal or bullets is gameplay
state; only insignificant chips, dust and other non-colliding detail may remain client-derived.

The simulation may use bounded spatial precision, sleep, merging, distance-dependent simulation and
asynchronous work. Those optimizations must preserve cover, traversal, structural outcomes and
multiplayer agreement. Overload handling must remain observable and cannot silently replace the
promised destruction with indestructibility. Deterministic integer state and server ownership remain
the foundation until a replacement has stronger validated evidence.

Concrete acceptance scenarios from the user's intent:

- Ordinary rounds repeatedly hit the same wood panel: localized holes and loss of remaining section
  accumulate; a thin panel becomes penetrable before a thick timber does, and weakened supports can
  fail. Spreading shots over the panel does not have the same effect as concentrating them.
- An explosive charge breaches a masonry wall: charge energy, distance, placement, cover, wall
  thickness and reinforcement determine the breach and fragments. A sheltered surface receives a
  different load from an exposed surface. Removing a critical support can cause a later collapse.
- A building loses most foundations to an explosion but remains geometrically connected: gravity
  redistributes its load onto the surviving supports. Material strength, remaining section, span and
  reinforcement determine overload and progressive failure. A weak final support must not hold an
  arbitrarily heavy building solely because graph connectivity still exists. With all support gone,
  the structure falls, collides, fragments as appropriate and settles coherently on every client.
- Two clients watch and interact with both events: cover, traversable openings, significant debris
  and the resulting structures agree, including after a late join and save/restore.
- The same scene is inspected at eye height and in motion: believable wood fibers, broken masonry
  cross-sections, fragment shape, lighting, dust and sound are visible in the native game. A generated
  illustration or a texture applied to a coarse cubic fracture does not pass the final visual gate.

## Performance and security gates

The measurable budgets in `production-roadmap.md` remain targets: 1080p High at 16.67 ms client p99,
1080p Competitive at 8.33 ms p95, and 60 Hz server simulation below 12 ms p99 with 32 nearby players.
The RTX 4050 Laptop is the currently available measurement machine, not evidence for other hardware.
Each baseline names its build, display resolution, power/clock conditions and scene. Test sustained
shots, explosions, collapse and rebuilding; cold/warm startup and passive orbit runs are supplemental.
Track visual quality alongside performance so lowering quality cannot silently satisfy a budget.

Native code, third-party libraries, authored or scanned assets and local tools may all be used when
their measured benefit, license, provenance and maintenance cost are understood. Improve the engine
where it serves the game; use mature libraries for cryptography, platform and media boundaries.
Validate Claude's external review findings independently. Never send secrets to a review model or
grant a client authority over gameplay outcomes.

## Next playable milestones

1. Visual material slice: scanned PBR inputs, stable projection, full mip chains and reproducible
   cooking, with a real before/after breach view and CPU/GPU evidence.
2. Destruction slice: weapon-specific penetration and explosions, progressive structure failure,
   load redistribution after partial foundation loss, credible fracture geometry, rubble and dust;
   two clients complete the same build/breach sequence.
3. Environment slice: an authored industrial scene with believable scale, lighting, shadows,
   vegetation, reflections, sound and first-person weapon presentation; temporal image review.
4. Sustained multiplayer slice: 32-client active-combat load, bounded streaming, congestion control,
   crash persistence and a reviewed remote-play deployment.
5. Distribution slice: native installs, cross-OS matches, hardware quality tiers, release security,
   save compatibility, update and rollback drills.

These are parallel product concerns, not permission to postpone all visual/gameplay work until every
infrastructure feature is complete. Every implementation lot identifies the requirement it advances,
delivers inspectable behavior and records remaining gaps. The goal stays active until every row has
current authoritative evidence across its full scope.
