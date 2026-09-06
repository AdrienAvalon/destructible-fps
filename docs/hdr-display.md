# Linear HDR frame and spatial antialiasing

This stage advances VIS-01 and PERF-01: preserve scene radiance until a spatial resolve, then apply
the display transform once. It does not change geometry, material resistance, simulation, commands,
replication, physics or snapshots. MSAA improves polygon-edge coverage; it is not temporal AA,
specular/shadow antialiasing, supersampled materials, bounced light or proof of photorealism.

## Image contract

The world and sky now output linear, non-negative radiance into `Rgba16Float`, not display-mapped
8-bit color. Shared WGSL clamps finite values to 65,504 and tests IEEE exponent bits to replace
NaN/Inf with zero before they can contaminate a resolved pixel. This is a defensive display
boundary, not a substitute for validated simulation/asset inputs or a proof that all shader
intermediates are well-defined on every backend.

Spatial sampling is explicitly 1× or 4×. Color and `Depth32Float` use identical sample counts;
both guaranteed device-format features and actual adapter capabilities must support the request.
Four samples require a supported HDR resolve. An unsupported count/format is an error, never an
unreported quality fallback. There is no per-sample shading forced in the world shaders; MSAA
resolves coverage, not all high-frequency material details.

The format/sample contracts follow the [wgpu format-feature API](https://docs.rs/wgpu/30.0.1/wgpu/struct.TextureFormatFeatures.html)
and [WebGPU render-pass resolve rules](https://gpuweb.github.io/gpuweb/#render-pass-color-attachment).

At 4×, the HDR color attachment resolves to a single-sample HDR texture inside the scene pass.
The multisample color and depth are discarded after that pass; only resolved radiance is needed
by presentation. At 1× the scene directly targets the same single-sample texture. A full-screen
presentation pass fetches the corresponding texel without filtering or scaling, applies the fixed
scene exposure (0.75, one Rust constant) and the existing ACES-style fit, then performs the output
transfer once. This remains SDR presentation of an HDR working image, not HDR-monitor support,
automatic exposure, bloom or a calibrated photographic camera.

The crosshair is analytically composited once in display-linear color **after** exposure/tone
mapping and **before** transfer. Its alpha is not tone-mapped or blended in encoded color; its
crossing is not double-blended. sRGB attachments perform hardware encoding; non-sRGB attachments
receive the matching piecewise sRGB transfer in WGSL, replacing the former approximate gamma 2.2.
The old world/sky display function, display fields in scene globals and separate crosshair pipeline
are removed. This small HUD solution is not a general text/UI compositor.

There is no temporal history, camera jitter or delayed resolve: an accepted changed mesh/pose
replaces the visible scene immediately under the existing bounded meshing schedule. MSAA alone
does not remove specular sparkle, directional-visibility bands or lighting leaks.

## Bounds and lifecycle

`FramePlan` validates non-zero dimensions against the actual device limit and an 8,388,608-pixel
ceiling before target creation or configuration mutation. No automatic resolution reduction is
used. 3,840×2,160 fits; an unsupported initial target fails explicitly. Later oversized window
requests are rejected before allocation and request restoration of the last valid window size,
without changing MSAA. Window-manager refusal to honor that restoration remains platform-dependent.

| Target storage | 1× | 4× |
| --- | ---: | ---: |
| Single-sample HDR | 8 B/pixel | 8 B/pixel |
| Multisample HDR | — | 32 B/pixel |
| Matching scene depth | 4 B/pixel | 16 B/pixel |
| Total | 12 B/pixel | 56 B/pixel |

At 1,440×900 these targets occupy 15,552,000 / 72,576,000 texel bytes; at the pixel ceiling, the
4× set is 448 MiB. These are target texel counts, **not** whole-process or whole-GPU limits.
The 64 MiB sky visibility, material/environment textures, geometry, swapchain and driver overhead
are additional. During a valid resize, old and candidate sets may coexist (up to two target sets).
Nonblocking GPU polling defers reconfiguration until prior submissions have drained. Only the latest
requested size is retained, and no new frame is submitted while it is pending; successive resizes
therefore do not retain an unbounded chain of in-flight targets. The event/network loops continue,
but presentation pauses until the queue is idle. Allocation failure/OOM
is not recovered automatically merely because dimensions passed validation.

Zero-size window events clear pending work without allocating; equal-size reconfiguration reuses
targets after queue drain. Busy polling is not fatal; both clients propagate unexpected polling
errors. Rejected oversized dimensions produce a deduplicated diagnostic and unmaximize/restore
request, not application exit. Surface recreation keeps the current presentation format only
if the candidate surface still supports it, instead of silently keeping incompatible pipelines.
Actual surface loss and other operating systems require separate platform validation.

## Reproduction and promotion evidence

```bash
cargo run --release --bin playable-demo -- --msaa 1 --showcase-closeup --smoke-seconds 12
cargo run --release --bin playable-demo -- --msaa 4 --showcase-closeup --smoke-seconds 12
cargo run --release --bin playable-demo -- --msaa 4 --lighting-stress --smoke-seconds 30
cargo run --release --bin playable-demo -- --msaa 4 --structural-lab --smoke-seconds 8
cargo test --lib render::display -- --ignored --nocapture
```

The requested default is 4×, gated for this increment on the representative scene and synthetic
moving-caster stress at GPU p95 ≤8.33 ms and p99 ≤16.67 ms on the named RTX 4050 Laptop. Record
1× alongside it and CPU wall separately; these are not a 1080p tier, CPU frame-pacing or cross-OS
release certification. If 4× misses this gate, retain 1× as default until the cost is resolved.
Both standalone and multiplayer clients default to 4× and accept only `--msaa 1` or `--msaa 4`;
these local presentation settings do not alter authority or wire state.

Eight timestamp queries split sun depth, sky visibility, HDR scene/resolve and display/HUD; total
includes all four. Sky refresh flags remain attached to their matching bounded readback slots.
The standalone telemetry window now retains at most 16,384 samples per metric, with explicit sample
counts; this covers the full measured thirty-second stress runs, not arbitrary longer sessions.
Missing timestamp capability is reported, not interpreted as zero GPU cost. Instrumented captures,
shader tests and compilation must not overlap the quiet timing runs.

Hardware fixtures use production frame targets, resolve settings, shared color helpers and display
shader. A per-sample bright/dark pattern proves that average radiance is tone-mapped, rather than
averaging already tone-mapped samples. Readback checks HDR >1, half-float range, nonfinite guards,
sRGB/UNORM agreement, unexposed HUD composition, two odd target sizes and a cleared next frame.
Another hardware fixture renders a diagonal triangle without sample-rate shading: 1× has binary
edge coverage while 4× must produce fractional resolved edge pixels.
Existing sky/material shader fixtures retain their linear illumination/projection assertions;
they were not tests of the old fragment-level display transform.
Pure resize-policy regressions cover zero/equal/oversized/deferred/replacement transitions. A real
owned XWayland window additionally exercises valid resizes, oversize rejection/restoration and
continued rendering. Its temporary override-redirect flag bypasses KWin's screen-size clamp only
for that disposable PID-selected window, so the oversize request actually reaches the application.
Neither test injects real device loss or proves other window managers.

See `performance.md` for actual runs, captures, resource observations and unfulfilled quality gates.
