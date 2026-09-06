# Visual direction and photoreal promotion gates

The simulation remains voxel-authoritative, but the final image must not expose a Minecraft-like
rendering model. Voxels describe occupancy, material, integrity, networking and fracture. They are
not the long-term surface representation. Rendering may derive richer meshes and cosmetic detail as
long as it preserves the authoritative boundary and stays within fixed asynchronous work budgets.

## Target

![Photoreal industrial target](concepts/photoreal-industrial-target-v1.png)

This image is an AI-generated art-direction reference, not a runtime capture or a promise that the
current engine already reaches this fidelity. It establishes the intended grounded scale: a damaged
European industrial/quarry site, layered concrete/brick/steel failure, damp terrain, restrained
vegetation, natural atmosphere, readable traversal and no visible voxel grid.

Generation prompt (built-in image generation):

> Photorealistic first-person view of a fully destructible modern industrial training site in a
> rocky temperate landscape; believable concrete, brick, steel, glass, earth and rubble; a
> physically plausible facade breach revealing rebar and layered material; late-afternoon
> overcast sun, soft global illumination, contact shadows, restrained vegetation and smooth natural
> terrain. Environment only, no people, weapons, vehicles, UI, text, logos or watermark. Avoid
> Minecraft, cubes, blocky terrain, toy proportions, saturated stylization and cinematic blur.

## Delivered foundation

- stable material identity crosses CPU meshing into the GPU without changing voxel, physics or wire
  formats;
- five CC0 Poly Haven scans supply brick, concrete, stone, soil and wood albedo/roughness/GL normals
  at their documented physical tile scales; steel/glass and cut overlays remain procedural;
- offline source hashes and pinned cooking tools reproduce a fixed embedded package; two 1,024²
  texture arrays have eleven linear-light, normal-aware mip levels and bounded trilinear filtering;
- explicit-gradient signed triplanar projection removes per-vertex plane changes on curved surfaces;
  surface-gradient blending preserves smooth geometric normals when the normal map is flat;
- detached bodies preserve their material projection while moving instead of sampling world-locked
  textures;
- screen-footprint fading reduces distant procedural shimmer;
- Cook-Torrance GGX consumes scanned or synthesized albedo, roughness, metalness and transformed
  surface normals, with inverse-transpose support for scaled instances;
- the sky reconstructs each view ray from the inverse view-projection matrix and shares its
  atmosphere with distance haze;
- one CC0 HDR environment drives sky, diffuse convolution, GGX roughness-prefiltered reflections
  and split-sum BRDF with shared exposure; offline cooking, bounded half-float packages and
  real-GPU orientation/mip tests replace the former fixed ambient colors;
- sixteen camera-near directional depth views now mask distant-sky diffuse and specular light
  using actual chunk/body/player geometry, with edit/pose invalidation, continuous coverage fade,
  hardware closed-room/moving-caster regressions and refresh-only GPU telemetry;
- bounded RGBA16Float frame targets preserve radiance until a 1x/4x spatial resolve, followed by
  one fixed exposure/tone/transfer stage and display-linear HUD composition; MSAA targets polygon
  edges, not normal-map/specular shimmer or temporal reconstruction;
- a fixed 3×3 PCF kernel softens the existing bounded 2,048² directional shadow map.
- soil and stone now use crack-free Surface Nets while authored brick, concrete, wood, steel and
  glass preserve exact architectural edges;
- each natural chunk uses a fixed 17³-cell cache, adaptive quad diagonals, bit-identical seams in
  all six directions, and conservative face/edge/corner invalidation after edits;
- brick and concrete below a fixed authoritative-integrity threshold reuse the bounded derived mesh
  to retreat static breach edges; mixed cells pin their derived vertex to the shared lattice corner
  so the unique exact-side cap actually meets the derived surface instead of leaving white gaps and
  detached facade ribbons; signed-direction ray regressions cover the joint;
- exact integer integrity is converted only after meshing into a render-only GPU damage ratio;
  deterministic aggregate, fracture, soot and crack synthesis changes damaged masonry and detached
  debris without adding a replicated field or altering collision;
- exposed thin brick and concrete cuts within a fixed six-voxel, same-material damage halo reuse the
  derived silhouette and carry a deterministic local depth in `[0, 1]`; the shader uses it to
  distinguish the outer shell, mineral core, aggregate chips and a sparse rusty reinforcement
  response without changing the authoritative voxel;
- topology edits invalidate a conservative two-voxel render neighborhood, extended to seven around
  masonry damage so a layered edge cannot remain stale across a chunk boundary; one isolated edit
  still reaches at most eight chunks and all meshing remains off the presentation thread;
- deterministic quarry banks make the smoother silhouette visible without changing the building,
  spawn, authoritative shot corridor, voxel fingerprints, physics, or network formats.

The generated concept is not shipped as a runtime texture. The game loads only the fixed local
cooked packs, never upstream JPEG/HDR images or live asset endpoints. See `../assets/materials/README.md`
and `../assets/environment/README.md` for provenance, reproduction and bounds. The initial scans still visibly repeat on
large surfaces; material blending, authored scale variation and proper geometry remain necessary.

## Promotion order

1. Hybrid surface extraction (terrain, coarse static masonry fractures and layered thin cut shading
   delivered): retain exact cubes for intact authored architecture, derive smooth crack-free soil
   and stone from the same voxel field, then add true sub-voxel fracture contours and extend the
   derived representation to detached bodies.
2. Layered destruction (render-only cut-depth foundation delivered): replace the procedural layer
   approximation with authored facade, reinforcement, insulation and interior geometry; generate
   bounded local rubble and dust from authoritative fracture inputs.
3. Asset/material pipeline (five scans, versioned arrays, bounded offline cooking and normal-aware
   mips delivered): add GPU block compression, material blending, larger libraries and residency/LOD
   tiers with deterministic fallbacks.
4. Lighting/post (offline image-based sky lighting, linear HDR frame, spatial MSAA and fixed shared
   exposure delivered): cascaded sun shadows, local reflection/bounce probes, advanced postprocessing, temporal anti-aliasing,
   contact refinement and quality tiers. Dynamic directional sky visibility is delivered near the
   camera; it is coarse and does not restore bounced interior light. See `sky-visibility.md`.
   See `hdr-display.md` for frame/color bounds and the distinction between spatial and temporal AA.
5. World dressing: instanced vegetation, decals, drainage/puddles, terrain blending, props and sound
   without making gameplay targets unreadable.

Every promotion must preserve asynchronous remeshing, authoritative fingerprints and protocol
bytes. Record GPU/CPU p50/p95/p99 on the representative breach scene; a prettier frame that exceeds
the fixed simulation/render budget or shimmers in motion does not pass.

Chunk streaming currently prioritizes render uploads from a complete immutable world snapshot; it
does not treat an absent render mesh as absent neighbor world data. Future partial world residency
must retain a neighbor halo or explicitly invalidate both sides when that data arrives.

The current natural and fractured masonry mesh is visual-only. Collision, ray impacts, and authority
deliberately retain the conservative voxel volume, so contacts can precede the smoothed surface near
a rounded or recessed edge. Exact transition faces are a rasterization closure, not yet a proof of a
single watertight manifold suitable for collision. That temporary mismatch is preferable to
weakening authority with an unvalidated floating-point collision proxy; a derived deterministic
collision representation remains a later promotion gate.
