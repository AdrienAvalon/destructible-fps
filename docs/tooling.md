# Local production and profiling tools

These tools serve the photorealistic/destructible FPS goal. They are development tools, not shipped
game dependencies. The user's installation mandate covers tools needed for this project, including
future deliberate updates; it is not permission to alter Codex access, host security policy, graphics
drivers, infrastructure services, buy subscriptions, or publish captures. Prefer signed distribution
packages or reviewed upstream sources, bounded tests and reversible changes over automatic updates.

## Verified installation, 2026-09-05

| Tool | Installed version | Actual validation | Remaining integration |
| --- | --- | --- | --- |
| RenderDoc, MIT | 1.45 | Captured/replayed Vulkan frame 120 of the real breach demo: 204 draw calls, 7 textures, RTX 4050 Laptop | Extend capture scenarios as gameplay/graphics evolve |
| Blender, GPL | 5.2.1 LTS | CPU render; GLB export/reimport; dimensions 2×3×4 m, Y-up conversion, 12 triangles, UVs, PBR roughness verified | Production asset pipeline and engine GLB importer are not implemented |
| Tracy, BSD-3-Clause | 0.14.1 | Loopback capture, CSV export containing exactly 200 known zones; GUI opened the same trace | Optional Rust client instrumentation and protocol compatibility still need implementation/validation |
| Linux perf, GPL-2.0 | 7.2.2 | User-space task-clock, cycles and instructions measured on the actual structural-load benchmark | Symbol-rich diagnostic builds and per-function sampling |
| CMake / Ninja | 4.4.3 / 1.13.2 | Built all three Tracy tools from upstream sources | Build support only |

The receipt in [`tools/toolchain-observed.json`](../tools/toolchain-observed.json) records package
versions, source identities, resolved vendor heads and installed Tracy binary digests. It describes
this installation; it is not an enforced dependency resolver, signed attestation, or promise of
bit-identical builds on another compiler. No GPU driver, existing package or kernel was upgraded.
The package manager added the tools and their dependencies (about 2.73 GiB), using its ordinary
signature checks and pre/post Snapper hooks. No reboot, daemon, autostart, capability grant or sysctl
change was performed. The detailed local transaction history remains in `/var/log/pacman.log`.

`perf-7.2.2-1` returned 404 on the configured mirrors. The identical package and detached signature
were downloaded from the official Arch package archive. `pacman-key --verify` reported a good,
fully trusted package signature; `pacman -U` repeated verification and installed it normally. This
does not establish why mirrors no longer contained it. Do not add an `IgnorePkg` hold or weaken
signature verification. At future host updates, revalidate `perf` against the running kernel; an
exact version string match is not a universal requirement for the perf API.

Primary references: [RenderDoc](https://github.com/baldurk/renderdoc),
[Tracy 0.14.1](https://github.com/wolfpld/tracy/releases/tag/v0.14.1),
[Tracy manual](https://github.com/wolfpld/tracy/blob/v0.14.1/manual/tracy.md),
[Blender](https://www.blender.org/).

## Repeatable checks

From the repository root, with the Linux desktop session active:

```bash
cargo build --locked --release --bin playable-demo
python tools/tooling_smoke.py renderdoc
python tools/tooling_smoke.py blender
python tools/tooling_smoke.py tracy
python -m unittest discover -s tools -p 'test_*.py'
shellcheck tools/tracy_viewer.sh
```

Each run creates a private, unique directory under ignored `target/tooling/`, prints its location
and requires an explicit JSON success artifact. Exit code alone, missing display, unsupported GPU,
an empty capture or a missing tool is not success. Python exceptions inside qrenderdoc can leave
its process exit code zero, so checking the result artifact is essential. Blender runs with factory
settings, automatic file scripts disabled and a nonzero Python-error exit code. Only the checked-in
test scripts and self-generated assets run; do not use this as a sandbox for hostile `.blend` files,
addons or downloaded captures.

The launcher uses Bubblewrap with private network/PID namespaces, a read-only filesystem view,
explicit writable output, a private `/tmp` and real GPU/compositor access. It passes a small
environment allowlist, not service tokens, proxy credentials or shell/Python startup customizations.
The filesystem view still exposes readable host files and local display IPC; this is not a
confidentiality boundary against malicious code. The network namespace has only loopback and is
not the host's loopback: no test listener can expose a capture service on the host LAN. The Tracy
test additionally checks the compiled client's loopback-only TCP listener and verifies that no
listener survives the capture. No remote replay server or global injection is started.

Each subprocess phase has a 60-second deadline. Cleanup kills the owned group; the PID namespace
also reaps children that create another group. A regression test exercises a detached descendant
and timeout. The smoke limits each output file to 256 MiB, captures one GPU frame, and refuses a
new run if retained evidence exceeds 2 GiB or 2,000 entries. Existing evidence is never automatically
deleted; archive selected old runs explicitly before continuing. A scene too large for these
budgets fails rather than silently raising them.

RenderDoc 1.45 as packaged here does not support `VK_KHR_wayland_surface`. Its smoke unsets
`WAYLAND_DISPLAY` for the game and uses the local XWayland Unix socket, without changing the user's
desktop or normal game backend. The qrenderdoc helper itself runs offscreen; the game and replay
use real Vulkan, not a headless substitute. Analytics and automatic update checks are disabled in
the smoke's private UI configuration and in this installation's new qrenderdoc UI configuration.

The two successful initial capture runs produced approximately 217 and 226 MiB `.rdc` files and
real breach thumbnails, inspected locally. Capture overhead reached hundreds of milliseconds for
the instrumented frame. These timings are **not** uninstrumented release performance, and the
current demo is **not photorealistic**. A Blender render is not an in-engine screenshot either.
These tool checks used the already built release game executable with SHA-256
`5d1b481a5a8d1432a1f75bd7294b4e9979bdf736923f969e0c3ecaf1138ef00f`; they do not validate
the independent structural-job changes pending in the working tree. The normal engine validation
matrix still applies before committing that separate lot.

## Tracy viewer and source build recipe

`~/.local/bin/tracy-profiler` points to [`tools/tracy_viewer.sh`](../tools/tracy_viewer.sh), not directly
to the raw profiler. It opens trusted saved traces in an isolated network namespace, with only
`target/tooling/` writable. The pinned configuration disables the LLM assistant and onboarding
achievements. The wrapper prefers local XWayland and an in-memory settings backend; host desktop
settings stay read-only. It is an offline viewer, not a live connection to a separately launched
host game. Use the coordinated private-namespace smoke for a live capture.

```bash
tracy-profiler target/tooling/<tracy-run>/smoke.tracy
```

Raw binaries live in `~/.local/opt/tracy-0.14.1/bin/`; capture and CSV CLI symlinks also exist in
`~/.local/bin/`. There is deliberately no capture daemon. Installing a newer version must not
replace these binaries while a capture is in progress. Retain the old version until the new
client/capture/viewer trio passes its smoke. Do not activate upstream LLM services or LAN discovery.

The sources used here are `~/.cache/destructible-fps-tracy-0.14.1`. To reproduce the initial build
on a compatible machine, first install/review distribution dependencies, then:

```bash
git clone --depth 1 --branch v0.14.1 https://github.com/wolfpld/tracy.git \
  "$HOME/.cache/destructible-fps-tracy-0.14.1"
fps_tracy_source="$HOME/.cache/destructible-fps-tracy-0.14.1"
test "$(git -C "$fps_tracy_source" rev-parse HEAD)" = 30997d5ca6bb632cc10807a1da8a6d3de0aeeb3c
cmake -S "$fps_tracy_source/profiler" -B "$fps_tracy_source/build-profiler" -G Ninja \
  -DCMAKE_BUILD_TYPE=Release -DLEGACY=ON -DNO_ISA_EXTENSIONS=ON -DNO_LTO=ON \
  -DCPM_SOURCE_CACHE="$fps_tracy_source/deps"
cmake --build "$fps_tracy_source/build-profiler" --target tracy-profiler --parallel 4
```

Repeat for `capture`/`tracy-capture` and `csvexport`/`tracy-csvexport` with their own build
directories, omitting `LEGACY`. These configure steps fetch upstream-specified dependencies;
they are not offline operations despite the offline viewer. Before promoting a rebuild, compare
the resolved vendor Git heads with the receipt and review upstream's applied patches. The upstream
tag is a lightweight Git tag; its identity was compared via official HTTPS Git and the checked-out
object, not represented as a locally verified signed tag. Install only the selected binaries into
the versioned user prefix and preserve the upstream license. No root source build, AUR recipe,
`curl | sh`, mutable branch installation or automatic self-updater is needed.

The standalone Tracy client is built only for its smoke with `TRACY_ENABLE`, `TRACY_ON_DEMAND`,
`TRACY_ONLY_LOCALHOST`, `TRACY_NO_BROADCAST`, `TRACY_NO_CODE_TRANSFER`, `TRACY_NO_SYSTEM_TRACING`
and `TRACY_NO_SAMPLING`. The Rust engine currently has no Tracy dependency; an eventual optional
client must be reviewed, version-compatible and disabled in distributed/default builds.

## CPU profiling without loosening host protections

```bash
cargo build --locked --release --bin structural-load-benchmark
perf stat -e task-clock:u,cycles:u,instructions:u -- \
  target/release/structural-load-benchmark --iterations 20
```

Only the launched process and its descendants are measured, in user space. Keep
`kernel.perf_event_paranoid=2` and `kernel.kptr_restrict=2`; do not run whole-system collection or
grant profiler capabilities. Kernel activity is outside this report's coverage. Do not compare
results collected during compiler/tool installation load with a quiet baseline.

Release currently strips symbols. For future callgraph work, retain symbols in a separate ignored
build, without modifying the shipping release profile:

```bash
CARGO_TARGET_DIR=target/profiling CARGO_PROFILE_RELEASE_DEBUG=1 \
  CARGO_PROFILE_RELEASE_STRIP=none cargo build --locked --release --bin structural-load-benchmark
```

That symbol-rich build and per-function sampling remain follow-up work, not validated by the
initial counter smoke. Keep diagnostic binaries and all traces out of shipping archives and Git.

## Review and validation boundary

The advisory Claude analysis reinforced the explicit listener, lifecycle and provenance checks.
Its final review invocation failed without a usable result; it is not counted as approval. Local
source review, ten passing Python tool/material tests, ShellCheck, `py_compile`, `git diff --check`,
repeated real GPU replay, Blender round trips and Tracy capture/CSV verification are the available
evidence. The configured Tracy GUI was also opened on the saved trace, visually inspected and
closed through its normal window-manager quit request with exit code zero. No tool process or
host-side capture listener remained afterwards. This is a tooling validation, not the engine's
full regression matrix or a security audit of upstream profiler implementations.
