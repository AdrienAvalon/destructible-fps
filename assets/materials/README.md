# Offline industrial material library

The runtime ships `industrial.pbrz`, not an AI-generated scene or a live asset downloader.
`sources.json` records the source author, public page, exact map URLs, dimensions, byte counts and
SHA-256 of every reviewed source. These five Poly Haven scans are [CC0](https://polyhaven.com/license).
The game code's license does not override those asset rights.

| Runtime layer | Scan | Physical tile (metres) |
|---|---|---|
| Soil | [Brown Mud Rocks 01](https://polyhaven.com/a/brown_mud_rocks_01) | 1.3 × 1.3 |
| Stone | [Rock Face 03](https://polyhaven.com/a/rock_face_03) | 2.7 × 2.7 |
| Wood | [Weathered Brown Planks](https://polyhaven.com/a/weathered_brown_planks) | 1.8 × 1.8 |
| Brick | [Brick Wall 001](https://polyhaven.com/a/brick_wall_001) | 3 × 3 |
| Concrete | [Concrete Floor 01](https://polyhaven.com/a/concrete_floor_01) | 2 × 2 |

These are the original 1K JPEG diffuse, OpenGL normal and roughness maps. The initial materials are
dielectric; steel/glass and damage overlays retain the shader's procedural path. No upstream
displacement map is claimed as actual geometry. These scans improve surface information, not the
coarse shape, physical reinforcement, indirect lighting or temporal stability of the scene.

## Reproduction

Ordinary Rust builds use the committed pack and need neither Python, ImageMagick nor network access
for assets. Authoring currently pins Python 3.14.7, ImageMagick 7.1.2-30 Q16-HDRI and zlib
1.3.1.zlib-ng in both the cooker and manifest. A toolchain mismatch fails rather than silently
changing the cooked asset. Updating that contract requires a new reviewed pack and numerical tests.

From the repository root:

```bash
# Optional first authoring fetch: only missing hash-pinned public inputs, cached under target/.
python3 tools/cook_materials.py --fetch
# Offline byte-for-byte reconstruction; does not replace the committed pack.
python3 tools/cook_materials.py --check
python3 -m unittest discover -s tools -p 'test_*.py'
# Production WGSL projection/normal helpers, executed on a real Vulkan GPU.
cargo test --test material_projection -- --ignored --nocapture
```

`--lock-sources` is a separate explicit source-update operation against the five named upstream API
records. It refreshes the manifest and pack; never use it as an automatic build step. Downloads
have fixed size/time ceilings, no ambient proxy and HTTPS-only same-origin redirects checked before
opening the next connection. All cached bytes must match SHA-256 before decoding. The decoder is
an authoring subprocess with memory, disk and execution-time limits, not part of the game.

## Channel and geometry contract

- Array A: sRGB albedo in RGB, linear perceptual roughness in A (`Rgba8UnormSrgb`; alpha is linear).
- Array B: linear tangent-space OpenGL normal in RGB, linear metalness in A (`Rgba8Unorm`).
- Both arrays: five layers, 1,024² base level, eleven mips down to 1², layer-major upload order.
- Color mips average linear light. Normals are renormalized at each level. Roughness averages its
  squared value and includes discarded normal variance to reduce distant specular shimmer.
- Undefined below-hemisphere source normals become flat; valid JPEG normals are normalized before
  packing. This is sanitization, not reconstructed geometric detail.
- Material coordinates remain object-local during debris motion. Projection uses signed
  right-handed frames, top-origin image rows with OpenGL normal conventions, and explicit fragment
  gradients computed before divergent material/axis branches. Flat maps preserve smooth normals.
- Mixed exact/derived mesh cells pin their surface vertex to a common lattice corner. This removes
  the visible cap-to-Surface-Nets gaps; it is not a proof of a collision-ready watertight manifold.

## Package and memory bounds

The pack contains an eight-byte `AVPBR001` magic, four little-endian u32 fields (edge, layer count,
mip count, decoded byte count), 32 SHA-256 bytes, five pairs of little-endian f32 physical tile
dimensions, and a zlib stream. The digest covers the fixed header, physical dimensions and decoded
payload. It detects corruption; it is not an asset-author signature. All dimensions are fixed by the
runtime; source metadata cannot request larger textures or decompression allocations.

The current pack is 36,320,919 bytes (34.64 MiB). Two complete RGBA8 arrays occupy 55,924,040 texel
bytes (53.33 MiB), excluding driver alignment/metadata. Decompression temporarily allocates the same
decoded size before GPU upload, with a hard output ceiling. These are layout bounds, not a claim
that process RSS or peak VRAM equals those numbers. Current CPU decode is about 194 ms on the local
release measurement machine. The roughly 11 ms upload telemetry measures CPU enqueue work, not GPU
transfer completion. Decoding and upload happen at startup, not on network receive or frame paths.

GPU block compression, larger material variety, asset residency, cross-material blending, calibrated
wetness and Metal/DX12 backend validation remain work. The explicit Vulkan test covers the actual
production WGSL projection frames, flat-map invariance and inverse-transpose instance normals;
ordinary headless test runs intentionally report it ignored and do not constitute GPU evidence.
