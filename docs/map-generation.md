# Generative map authoring contract

The requested production tool must create playable, destructible worlds from an authored brief or
real-world reference photographs, not merely attractive background images. This requirement adds
to the full game goal; the native fine inspection is not a replacement for the map generator.

## Authoring inputs

- A versioned recipe: seed, metric dimensions, elevation range, biome/material palette, buildings,
  roads, landmarks, spawn/traversal constraints and explicit work/memory budgets.
- A natural-language brief translated into a reviewable recipe. Keep the generated recipe and seed
  so changes are reproducible and can be compared, rebuilt, tested and undone.
- Optional photographs with source/provenance and user annotations for scale and known viewpoints.
  Distinguish **inspired reconstruction** from measured reconstruction. One image cannot establish
  unseen geometry or absolute dimensions; do not label inferred geometry as a faithful survey.

The deterministic generator consumes the recipe, not a model's unbounded code. Reference analysis
may propose recipes, terrain masks and reusable building descriptions; those remain candidates
until schema, size, topology, traversal and runtime-budget checks accept them. Never execute code
embedded in a downloaded map or asset. Image parsing/cooking needs pixel, file-size, decode-memory
and time limits, attribution and format validation before it is admitted to a production pipeline.

## Shared engine representation

Terrain generation, authored architecture and player construction must converge on the same
material geometry used by destruction, structural support, collisions, meshing, snapshots and
network repair. Use coarse storage for uniform regions and bounded fine geometry for gameplay-
relevant thin sections. A height field is useful for an initial landscape, but not a substitute for
caves, overhangs, multilayer buildings and freely destructible volumes.

The current industrial map is hand-authored integer code. `RefinedWorld`, exact static queries,
fine surface extraction and the ongoing hybrid-terrain integration are foundations. A procedural
or photograph-driven generator, editable recipe format, production export and full runtime
admission are **not yet delivered**.

## Delivery sequence and evidence

1. Seeded bounded terrain recipes: hills/valleys, plateau/forecourt masks and configurable dimensions;
   deterministic hashes, strict negative-input tests and material-volume budget checks.
2. Composable metric building/road primitives with meaningful wall/slab/column layers and explicit
   structural anchors; validate real collision, navigation clearances and destruction semantics.
3. Native preview, camera bookmarks and persistent map export/import with schema/version checks,
   checksums and controlled file paths; identical source must reproduce identical map state.
4. Brief/reference-driven recipe authoring, keeping uncertainty and scale assumptions visible.
   Reconstruct from additional viewpoints or measured data when genuine fidelity is requested.
5. Content dressing and scalable LOD/residency; prove material/geometry identity through actual
   multiplayer joins, repairs, destruction and durable saves. Measure complete rendered scenes,
   not just generator throughput or generated concept images.

The desired outcome is a reusable map-creation workflow: generate, inspect, adjust constraints,
rebuild, test, then publish a validated playable map. Do not silently promote recipes which exceed
runtime limits, and do not turn unknown photographic regions into claimed measured geometry.
