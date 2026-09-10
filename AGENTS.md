# Destructible FPS engineering rules

## Scope and intent

Unreal Engine 5.8.2 is the production direction since 2026-09-07 for the photorealistic multiplayer
FPS with persistent material-aware destruction. Read the current README and
`docs/checkpoints/2026-09-10-unreal-presentation.md` before resuming. This published snapshot contains
the Unreal presentation and native captures; the migration sources remain in the local development
checkout and are not yet included here. Preserve the Rust prototype as a technical reference and
regression corpus; do not expand its renderer by default. The Rust-specific rules and commands below
apply to that preserved code, not to an assumed Unreal port. A visually attractive demo is not
evidence that multiplayer or structural destruction works.

## Resuming work

Read `REPRISE.md`, then the latest dated receipt in `docs/checkpoints/`, before continuing after a
context loss. Confirm current Git/source state rather than treating historical measurements as live
evidence. Preserve the distinction between the playable coarse FPS, the fine/convex inspection viewer,
planned features and the unachieved photorealistic target. Update the handoff and evidence with each
material checkpoint; never depend on a temporary path or private conversation to reproduce a claim.

## Load-bearing invariants

- The dedicated server is authoritative for damage, fracture, structural separation, rigid bodies,
  inventory, and persistent world state. Clients submit bounded commands, never outcomes.
- Gameplay destruction uses fixed-step, deterministic inputs. Do not introduce floating-point state
  into the replicated voxel transaction without a cross-platform determinism test or server-only
  ownership and explicit quantization.
- Every replicated transaction carries monotonic sequencing and pre/post state fingerprints. A gap,
  stale before-state, malformed fragment, or wrong final fingerprint fails closed before partial
  mutation and requests a bounded snapshot repair.
- Untrusted network allocations and collections are bounded. Keep ordinary application datagrams at
  or below 1,200 bytes until path-MTU discovery is designed and tested.
- `unsafe` Rust is forbidden by default. A future Vulkan boundary may use a small isolated crate with
  documented invariants, validation-layer tests, and a safe public API; do not weaken the workspace
  globally.
- Physics and rendering work never runs synchronously on the network receive path. Expensive chunk
  meshing and structural analysis must be scheduled with explicit budgets and backpressure.
- Cosmetic debris is not authoritative gameplay state. Synchronize fracture inputs and significant
  bodies; derive bounded cosmetic effects locally.

## Performance evidence

Never describe a subsystem as optimized from an average FPS or an empty-scene benchmark. Record:

- target hardware, resolution, build profile, scene, and commit;
- CPU and GPU p50/p95/p99 frame times separately;
- server tick p95/p99 and worst-case destruction spikes;
- resident memory, allocation counts, generated geometry, bandwidth, loss, and correction rate;
- warm and cold runs, with a sustained scene that does not become cheaper as it is destroyed.

Changes that improve throughput but violate replica correctness, bounds, or frame pacing are
regressions. Benchmark baselines are evidence, not hard-coded pass thresholds across unrelated
hardware.

## Validation

Run before committing:

```bash
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test --all-targets
cargo test --release --all-targets
cargo test --test network
cargo test --test secure_transport
cargo test --test secure_authority
cargo test --test secure_server_process
cargo test oidc::tests
cargo run --release --bin destruction-benchmark -- --events 500
cargo run --release --bin structural-benchmark -- --iterations 100
cargo run --release --bin physics-benchmark -- --bodies 1024 --ticks 300
cargo run --release --bin snapshot-benchmark -- --iterations 20
cargo run --release --bin playable-demo -- --smoke-seconds 5
```

The graphical smoke check requires an active Linux Wayland or X11 session with a Vulkan-capable
adapter. Treat a headless skip as missing coverage, not success.

Add a regression test near every corrected parser, synchronization, determinism, or bounds defect.
Use explicit fixed seeds for reproducible simulation tests. Do not commit `target/`, raw GPU/video
captures, or profiling output. Small selected native screenshots explicitly requested for a
documented checkpoint may live in `docs/screenshots/` with provenance and build identity; label them
separately from generated art-direction concepts and do not capture the user's desktop or secrets.

## Repository hygiene

Keep commits small and coherent using `type(scope): description`. Do not add a large dependency to
save a few lines: state its runtime role, maintenance cost, license, platform support, and measured
benefit. Pin toolchain and dependency versions once external crates are introduced. Never embed
service credentials, signing keys, or distribution tokens in the game repository.
