# Reusable FPS gameplay and scenario authoring

The project is a reusable FPS foundation, not a single fixed map or match. Its content tools must
support different settings, objectives, story scenarios, scripted events, NPCs, solo sessions,
cooperative play and competitive multiplayer. This extends the user's map-generation request.
It does not claim every idea is expressible without new gameplay systems or that this engine is
already a general-purpose game editor.

## Responsibility boundaries

| Layer | Owns | Must not do |
| --- | --- | --- |
| Engine | Rendering, physical geometry, destruction, networking, persistence, resource budgets | Hard-code a particular mission or grant authored content direct client authority |
| Gameplay systems | Characters, weapons, inventory, construction, teams, interactions, NPC action capabilities | Duplicate physics or bypass validation for scripted outcomes |
| Mode/scenario runtime | Objectives, conditions, event ordering, spawn requests, mission/team/player progress | Trust client-reported success, recursively run unlimited events, or execute arbitrary imported code |
| Authored content | Maps/recipes, characters, dialogue, objective graphs, reusable encounters, mode settings | Present an unsupported behavior as delivered just because a schema accepts its name |

A single-player session should use the same authoritative simulation locally, without requiring
a public listener or an external identity service. Cooperative and competitive sessions change
admission, participants and mode rules, not the meaning of physical destruction. A narrative event
that destroys a wall must go through a validated simulation action; all clients and later joins
must receive the resulting geometry, not merely a cosmetic animation of an intact obstacle.

## Scenario model

Use versioned, declarative, reference-validated data for objectives and events, backed by explicitly
implemented engine/gameplay capabilities. Examples include entering a zone, reaching an objective,
interacting with an object, a validated actor death, an elapsed mission timer or a structural failure.
Actions can request an encounter, update an objective, start attributed dialogue, change a door's
state or submit an authorized physical effect. They do not directly mutate arbitrary world memory.

Event identity, sequence, fired/active state and mission progress belong to authoritative save and
replication state. Test duplicate delivery, late join, reconnect, save/restore and conflicting events.
Bound nodes, references, pending actions, per-tick execution, timer counts and actor populations.
Detect cycles/repeated activation and surface overload explicitly rather than silently losing
important mission events or freezing the server. Maps and scenarios are data, not remotely supplied
native executables or unbounded scripts.

The optional [adaptive director](adaptive-director.md) may propose events and NPC goals through
this same runtime. Its local or remote model is never the simulation authority, and no model is
required for ordinary NPC movement, combat or a playable authored scenario. Development agents
use a separate [production tool interface](agent-tooling.md), not an unrestricted NPC endpoint.

NPC logic selects from bounded legal actions on the server. Navigation must react to destructible
cover, new openings, collapsing floors and construction. Rendering/animation and cosmetic speech
cannot define collision or gameplay success. Enemy, allied and neutral actors can share capabilities
while receiving different goals and policies; do not duplicate complete simulation implementations
for those roles. Introduce actor/NPC state, budgets and save/replication tests before claiming bots
or narrative characters work in multiplayer.

## Authoring workflow and acceptance

1. Generate or author a map using the [map-generation workflow](map-generation.md).
2. Select a game-mode template and place typed, identifiable entities, spawn points and triggers.
3. Author an objective/event graph and encounters using implemented capabilities.
4. Validate references, traversal, budgets, physical interactions and supported feature versions.
5. Inspect and play the same scenario in local solo, cooperative and competitive-compatible modes.
6. Test late join, interrupted events, player removal, save/restart and deterministic state repair.

A first end-to-end scenario should combine the actual FPS strengths: breach a supported structure,
open a route, cause an NPC encounter to react to the changed world, complete an objective, then
restore the scenario with its progress and destroyed geometry intact. Its scripted intent must not
fake weapon damage or bypass the physical model. Competitive modes need not reuse narrative
objectives, but should reuse the same underlying interaction and world-state systems.

Current implementation provides portions of the rendering, destruction, character and network
foundations. The generalized scenario runtime, reusable mode templates, complete NPC AI and the
editor workflow above are **not delivered yet**. Keep them on the product acceptance contract;
do not describe an inspection fixture with four prepared meshes as a scenario editor.
