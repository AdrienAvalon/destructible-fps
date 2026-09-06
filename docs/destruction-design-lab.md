# Destruction experience: prioritized design experiments

Status: **proposed experiments, not implemented features or claims of unprecedented invention**.
Prioritize ideas that make the same physical world more believable, beautiful and tactically useful.
Keep expensive speculative simulation behind evidence gates. These experiments support the product
contract; they do not indefinitely expand the definition of a finished game.

## First candidates

| Priority / experiment | Player-visible value | Small proof and promotion gate |
| --- | --- | --- |
| P1: shared consequences of a breach | A new opening changes cover, light, sound propagation, visibility and NPC routes coherently | One wall, before/after breach, two clients: use the same geometry revision for gameplay queries; bound derived lighting/audio/navigation refresh and disclose stale results |
| P1: readable structural failure | Creaks, material shedding, visible deformation where supported and dust signal danger before a progressive collapse | Remove partial support in a small structure; cues reflect actual loads/failure state, not a scripted timer; compare native presentation to canonical physics and test safe escape options |
| P1: a persistent material history | Exterior dirt/weathering, fresh internal fracture, reinforcement, settled rubble and repairs form a believable damaged scene | Inspect one element intact/chipped/breached/collapsed/repaired in motion at close and far distances; check material scale, closure, lighting, topology and bounded save data |
| P2: useful construction under damage | Bracing, bridging, barricading or clearing rubble offers alternatives to shooting everything | One authorized rescue/route-repair sequence with resource costs, real support/contact effects and two-client persistence; no decorative brace that secretly does nothing |
| P2: an explanatory incident replay | Developers can find why a wall floated, a support failed or a shot passed through cover | Reproduce one recorded incident from a checkpoint; inspect geometry revision, load/contact evidence and accepted events; fail visibly if reproduction diverges |
| P2: generated stress playtests | Procedural maps expose bad seams, unreachable objectives and destruction spikes before players do | Fixed-seed map/weapon/support matrix, automated legal player traces, minimized failing case and native before/after capture; deterministic assertions independent of an agent's opinion |

Readable failure must not guarantee a warning interval for every sudden brittle break. Material
behavior and the actual load path remain primary. Cosmetic dust/chips can decorate the event, but
debris that blocks movement or bullets remains authoritative. Audio, vision and navigation may use
different bounded derived representations of the same geometry; this is not a mandate to run a
single expensive query for every pixel, sound ray and NPC every tick.

Persistent visual history must not become an unbounded list of decals. Use content/material state
and bounded derived detail, record significant physical changes canonically and preserve authored
scale across chunks and LOD transitions. A repaired wall should have the strength and appearance
of its actual repaired construction, not silently revert to an untouched perfect asset.

## Photorealism and playability are joint gates

For each experiment, inspect actual native views at player height, in motion, and at low grazing
light angles. Compare intact, damaged, collapsed and rebuilt states, indoor/outdoor transitions,
thin cut edges, rubble contacts, material variation and shadows. Include weapon/viewmodel scale,
sound and feedback in the eventual playable review, not just architectural fly-throughs.
Use real-world references as art direction; generated images do not demonstrate runtime quality.

Measure CPU/GPU frame tails, server tick tails, worst destruction latency, memory/VRAM, queue depth,
bandwidth and visual error in sustained activity. Optimize invisible/cosmetic work first; never
obtain a better FPS by silently changing bullet cover, collapse or client agreement. Future temporal
effects must test disocclusion after explosions and moving rubble: blur cannot conceal bad geometry.

Accessibility belongs in the same review: readable material/hazard cues, subtitles, remapping,
optional camera shake and reduced flashes. Competitive quality settings must not remove meaningful
concealment or create different collision/visibility rules. Cosmetic dust needs an explicit fairness
policy before it becomes tactical concealment; client particle density alone cannot decide it.

## Valuable but deferred

Fire/heat-driven weakening, smoke ventilation, water erosion/flooding, electricity networks and
weather-driven material changes could create richer cascades. They are not first-demo requirements:
each needs its own bounded physical model, counterplay, persistence and multiplayer tests. Prototype
one localized interaction only after the current solid-material destruction and renderer gates.
Do not simulate a complete planet or continuously generate new art during combat to satisfy these ideas.

## Experiment discipline

Each experiment records hypothesis, one representative fixture, owned acceptance IDs, hard resource
limits, native evidence and a keep/revise/defer decision. Measure simultaneous effects, not isolated
best cases. Counterfactual debugging runs against a disposable checkpoint fork; it must never rewrite
the active match or be presented as the actual historical cause. Minimize failures into seeded test
cases and preserve versioned inputs, not private player conversations.

Delivery remains [the production roadmap](production-roadmap.md): first close visual fracture seams,
integrate fine physical damage and demonstrate the industrial scene, then promote successful
experiments in small reversible increments. The [agent tools](agent-tooling.md) and
[optional adaptive director](adaptive-director.md) help exercise this game; neither substitutes for it.
