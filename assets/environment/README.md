# Offline HDR environment

`overcast.iblz` contains actual HDR lighting, not the generated concept image. The source is
[Overcast Soil (Pure Sky)](https://polyhaven.com/a/overcast_soil_puresky), by Sergej Majboroda
(original) and Jarod Guest (sky edits), under [CC0](https://polyhaven.com/license). `source.json`
records the exact 1K RGBE URL, byte count, SHA-256, upstream MD5 and authoring toolchain. The source
stays under ignored `target/environment-source/`, never downloaded by the game. No new dependency.

## Reproduction

```bash
python3 tools/cook_environment.py --fetch  # retrieve a missing hash-pinned source and cook
python3 tools/cook_environment.py --check  # offline byte-for-byte comparison; no replacement
python3 -m unittest discover -s tools -p 'test_*.py'
cargo test environment::tests
cargo test --test material_projection -- --ignored --nocapture  # real Vulkan GPU required
cargo run --release --bin playable-demo -- --showcase-closeup
```

`--lock-source` is an explicit source-update operation, never a build hook. It resolves only the
named asset, validates upstream size/MD5 and HDR layout, and records SHA-256. The shared material
downloader restricts HTTPS origins, rejects cross-origin redirects before following them, removes
ambient proxies, sets a socket timeout and bounds response bytes. The HDR download/cache ceiling
is 2 MiB; the local manifest ceiling is 8 KiB. The pure Python importer accepts only the fixed
1,024×512 modern Radiance RLE layout, `-Y +X` orientation, a header at most 1 KiB, bounded nonempty
channel runs and exact payload termination. Old-RLE, XYZE, other dimensions/orientations,
exposure/primary/color-correction metadata and radiance outside finite nonnegative half-float
range are rejected. Authoring pins Python 3.14.7 and zlib 1.3.1.zlib-ng; mismatch fails explicitly.

## Lighting and layout

All maps are **linear RGBA16F**, alpha one, cube order +X, -X, +Y, -Y, +Z, -Z. Signed face axes
are defined by `cube_direction`. Longitude wraps and latitude clamps. Hammersley sample order and
half conversion are deterministic. Local reconstruction is byte-identical; other libm/platform
authoring implementations remain untested.
`--check` is currently a local reference-authoring check, not a portable multi-OS CI gate. The
reference machine/toolchain is recorded in `docs/performance.md`; ordinary builds use the committed
asset and do not recook it. A software GPU is not counted as the real-hardware validation above.

| Texture | Dimensions | Meaning |
| --- | --- | --- |
| Sky | six 256² faces, one level | HDR radiance for background and haze |
| Diffuse | six 16² faces, one level | cosine convolution **E/pi**, 512 samples/texel |
| Specular | six 64² faces, seven mips to 1² | normalized GGX convolution, 256 samples; roughness=mip/6 |
| BRDF | 64², one level | red/green split-sum coefficients, 256 samples; x=NdotV, y=roughness |

Specular bytes are **mip-major**, then face-major; upload selects that order explicitly. Mip zero
samples radiance directly. Perceptual roughness `r` becomes `alpha=r²`; IBL Schlick masking uses
`k=alpha/2`, not the direct-light remapping. The shader reflects the negative view direction around
the world-space perturbed normal, derives maximum LOD from the texture, applies `F0*A+B`, and
attenuates diffuse by integrated reflectance. Diffuse E/pi is applied once. These are single-scattering
split-sum approximations, not full multiscattering or reference path tracing. The
[Filament lighting reference](https://google.github.io/filament/Filament.md.html) documents diffuse
convolution and the prefiltered specular/BRDF separation.

The same world-space environment supplies sky, diffuse light, reflections and fog color. Source
radiance gain is 1; a shared fixed exposure 0.75 precedes the existing fitted tone curve/output
encoding. There is no auto-exposure, HDR display output or HDR render-target/postprocessing chain.
The glass material remains opaque; sky reflections are not refraction or transparency.

The pure-sky lower hemisphere becomes a **ground approximation**: upper cosine-convolved light
times RGB reflectance (0.12, 0.10, 0.08), blended across the lower 0.08 vertical direction. It does
not capture quarry geometry. The old sun is reduced to a small shadowed artistic fill RGB
(0.35, 0.33, 0.30), each channel below one quarter of the fixed source's mean upper-hemisphere RGB
value (1.43568). This is not an extracted/calibrated overcast sun. New environments require review
of the rig and exposure, not blind reuse of those constants.

## Bounds and evidence

The 1,096,466-byte pack expands to exactly **3,452,912 texel bytes (3.29 MiB)**. Its 32-byte header
is magic `AVIBL001` and six little-endian u32 fields (sky edge, diffuse edge, specular edge, specular
mips, LUT edge, decoded bytes), then SHA-256(header+decoded texels), then zlib. Dimensions are Rust
constants, never allocation requests from metadata. Compressed bytes are capped at decoded bytes
+65,536; inflation has the exact decoded ceiling, length/digest must match, and every half-float
texel is checked before upload. Negative/nonfinite channels or alpha other than one fail startup.
The digest detects corruption, not author identity or an attacker replacing code and assets.

Reviewed artifact SHA-256 (in addition to the source HDR hash in the manifest):

```text
030886d79cdf6e05d521ee1c6b5a90f432346a3f38e99e73a58789123724da1f  overcast.iblz
9d195286e438e9bdf36653a5d5993d28cb4a206becd049b072c8120fdb0a57aa  source.json
```

Decode/upload run once at startup. No source parsing, convolution or network fetch occurs in frame,
gameplay or network-receive paths. GPU byte counts exclude alignment/driver metadata; startup also
temporarily owns decoded CPU texels. No portable RSS/VRAM claim follows from layout arithmetic.

Python tests cover malformed RLE, HDR values, axes/edges, longitude seams/poles, constant radiance,
diffuse normalization and mirror/rough BRDF limits. The full RGBE decode also matched ImageMagick
7.1.2-30 Q16-HDRI linear float output exactly locally (max error zero). The explicit Vulkan test uses
production bindings/WGSL at asymmetric directions on every face, independent CPU half-float sky and
mirror checks, all seven specular mip uploads, grazing finiteness and zero-AO output, retaining the
prior normal/projection checks. Headless ordinary tests deliberately skip the GPU proof.

The renderer now adds [dynamic directional sky visibility](../../docs/sky-visibility.md) around
the camera. No local reflection probes, indoor bounce, fine contact shadows or TAA exist yet.
Vertex AO stays approximate, and the coarse sky mask does not restore reflected interior light.
Small probe faces and finite
sample counts can blur detail or introduce variance; PDF-selected source mips/multiscattering remain
work. This is a coherent lighting foundation, **not achievement of the photorealistic target**.
