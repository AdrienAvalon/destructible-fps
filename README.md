# Destructible FPS prototype

Technical spike for a photorealistic, server-authoritative multiplayer FPS whose terrain and
structures can ultimately be destroyed. This repository starts with the correctness and
performance foundations instead of presenting a scripted visual demo as if it were a game engine.

## First playable demo

The Linux demo now combines the authoritative core with a real-time first-person client:

- compact 16³ voxel chunks (2 bytes per voxel);
- material-dependent damage using deterministic integer arithmetic;
- atomic world transactions with a rolling 128-bit state fingerprint;
- replay protection and strict server sequencing;
- delta fragmentation below a 1,200-byte network MTU;
- out-of-order frame reassembly;
- a real nonblocking UDP dedicated-server process, fixed versioned control handshake, and
  two-client process-level loopback synchronization test;
- a transport-independent bounded authority core keyed by opaque peer IDs, with authenticated
  principal binding, stale-session cleanup, configurable application payload ceilings, and a thin
  legacy UDP adapter;
- bounded recent-delta retention and prioritized exact retransmission: a process test deliberately
  drops a complete sequence, buffers later deltas, requests repair, and proves convergence;
- canonical four-MiB-bounded world/body snapshots, paced transfer, selective 64-bit fragment repair
  windows, atomic install acknowledgement, and retained-delta catch-up before live delivery;
- a bounded deterministic UDP impairment proxy exercising latency, jitter, loss, duplication,
  reordering, delta repair, fragment repair, and acknowledgement retry against the real process;
- a TLS 1.3 QUIC authority adapter with certificate verification, game-specific ALPN, bounded
  post-TLS credential admission, connection-bound principals, cryptographic server nonces,
  monotonic session IDs, and 1,100-byte encrypted gameplay datagrams;
- a bounded offline Keycloak-compatible OIDC verifier with RS256/JWKS key policy, exact
  issuer/audience and time validation, atomic key rotation, one-use `jti` replay defense, and stable
  issuer/subject-derived principals;
- a standalone secure authority process with bounded JSON/PEM/JWKS loading, strict Unix key-file
  permissions, forced static-JWKS expiry, graceful interruption, and no credential-valued arguments;
- strict caps on incomplete packets, fragments, and retained bytes to prevent
  reassembly-memory exhaustion;
- atomic structural separation: detached voxels leave the static world and become bounded,
  server-owned body descriptors in the same transaction;
- protocol-v6 body assignments, three-axis translation, canonical fixed-quaternion orientation, and
  bounded angular velocity using compact server-monotonic 64-bit entity IDs, independent 128-bit
  geometry fingerprints, pre/post body fingerprints, and full client-side connectivity, material,
  mass, identity, and state revalidation;
- control-protocol-v2 construction requests with replay protection, fixed per-session resources,
  material costs, bounded coordinates, face-support and occupancy checks, conservative dynamic-body
  exclusion, and ordinary fingerprinted world deltas shared by local, UDP, and authenticated QUIC
  clients;
- immediate detection of packet gaps and replica divergence;
- a repeatable end-to-end benchmark using a multi-material test building;
- a safe Vulkan renderer on `wgpu`, selecting the high-performance adapter;
- face-culled chunk meshes, distance-prioritized asynchronous initial streaming, and bounded
  background remeshing limited to chunks whose visible boundary changed;
- bounded off-thread body meshing in local space, with fixed-capacity GPU instance transforms,
  independent body frustum culling, and participation in both world and shadow passes;
- 60 Hz server-authoritative body motion in deterministic micrometre units, mass-weighted blast
  impulses, inertia-weighted off-centre angular response, canonical fixed-quaternion integration,
  three-axis swept static collision, material ground friction and normal restitution, vertical
  voxel-column body collision, four-pass coarse swept X/Z body contacts with mass-weighted impulse
  exchange and tangential friction, stable stacking, wake propagation, sleeping, bounded
  sweep-and-prune broad phase, atomic overload rollback, and replicated GPU transforms about the
  mass centre;
- a 120 Hz fixed-step first-person controller with gravity, jumping, collision, and mouse look;
- server-authorized rifle and explosive impacts rendered from the replicated world;
- per-vertex voxel ambient occlusion, a 2,048² directional shadow map, procedural material
  shading, distance fog, single-transfer tone mapping, and a crosshair;
- non-blocking real-GPU timestamp queries and bounded CPU/GPU p50/p95/p99 frame telemetry;
- conservative per-chunk camera-frustum culling with visible and submitted draw counters.

This is a **first playable engineering slice**, not a photorealistic or production multiplayer
game. Oriented voxel collision, contact-generated torque, gyroscopic response, deeper
constraint-island convergence, progressive structural stress, authenticated remote-authority
integration, trusted OIDC discovery/JWKS provisioning and certificate lifecycle, adaptive
retransmission and congestion control, audio, asset-quality PBR, temporal anti-aliasing, and
large-world residency streaming remain explicit later gates.

The first server-side structural pipeline is now integrated. A deterministic bounded topology
analyzer finds components adjacent to voxel edits, follows foundation or authored anchors, and emits
canonical detached-island proofs. The server revalidates each proof, derives integer-millimetre mass
properties, removes its voxels from the static world, and replicates the new body atomically. The
body is rendered from its preserved material voxels and receives a deterministic material-weighted
blast impulse. It sweeps all translation axes against static voxels, applies bounded restitution and
ground friction, stacks on exact vertical body columns, and sleeps after a deterministic rest
interval. Four bounded passes separate coarse swept X/Z body bounds, exchange normal velocity from
mass and restitution, reduce tangential slip without losing linear momentum, and propagate a short
contact chain. A nearest-voxel blast application point additionally generates deterministic angular
velocity through the diagonal inertia tensor; protocol-v6 deltas and snapshot-v2 transfers replicate
the canonical quaternion, and the GPU rotates the mesh around its mass centre. Collision geometry is
still axis-aligned, so oriented voxel contact, contact torque, and full constraint-island convergence
are not claimed yet.

## Screenshots

![First-person approach to the intact test building](docs/screenshots/01-approach.png)

![Server-authoritative explosive breach](docs/screenshots/02-authoritative-destruction.png)

![Showcase orbit around the multi-material building](docs/screenshots/03-orbit-interior.png)

## Run

```bash
cargo test --all-targets
cargo test --test network
cargo test --test secure_transport
cargo test --test secure_authority
cargo test --test secure_server_process
cargo test oidc::tests
cargo run --release --bin destruction-benchmark -- --events 500
cargo run --release --bin structural-benchmark -- --iterations 100
cargo run --release --bin physics-benchmark -- --bodies 1024 --ticks 300
cargo run --release --bin physics-benchmark -- --bodies 1024 --ticks 300 --scenario lateral-sweep
cargo run --release --bin physics-benchmark -- --bodies 1024 --ticks 1000 --scenario dynamic-head-on
cargo run --release --bin snapshot-benchmark -- --iterations 20
cargo run --release --bin playable-demo
```

The legacy dedicated-server process is intentionally restricted to loopback. Its socket has been
separated from the reusable authority core and its real two-client path is exercised by
`cargo test --test network`; do not expose that binary to a LAN or the Internet. The separate secure
runtime now connects QUIC admissions and encrypted datagrams to the same authority core, and
`cargo test --test secure_authority` proves a two-client destructive transaction. The
`secure-dedicated-server` process loads certificate paths and a time-bounded OIDC JWKS from a
fail-closed file configuration. It still rejects non-loopback binds pending trusted online key
refresh, certificate lifecycle checks, and the remote-exposure test gate. The secure boundary tests
cover TLS certificate rejection, application-credential rejection, admission timeout, nonce
mismatch, datagram bounds, invalid credentials, and per-session rate limiting. A separate offline
OIDC verifier validates pre-provisioned JWKS. Four external-process tests prove valid OIDC command
admission, invalid-token rejection without simulation work, remote-bind refusal, and Unix private-key
permission refusal. Trusted discovery/refresh, certificate provisioning, reliable control/snapshot
streams, and remote deployment policy are still required.
For local protocol development the legacy authority can be started directly:

```bash
cargo run --release --bin dedicated-server -- --bind 127.0.0.1:40000
```

The authenticated authority accepts only one non-secret argument: an absolute configuration path.
See [`docs/secure-server.md`](docs/secure-server.md) and
[`config/secure-server.example.json`](config/secure-server.example.json):

```bash
cargo run --release --bin secure-dedicated-server -- \
  --config /absolute/path/to/secure-server.json
```

The benchmark includes server-side destruction, encoding, deliberate frame reordering, decoding,
reassembly, client application, and final server/client verification. It is not a renderer-only
microbenchmark.

### Controls

- click the window to capture the pointer;
- `ZQSD` or `WASD` to move, `Shift` to sprint, and `Space` to jump;
- left click for a localized rifle impact;
- right click for a larger explosive blast;
- middle click to place a full-integrity wood voxel on the targeted supported face;
- `Escape` releases the pointer; press it again to quit.

For a non-interactive graphics check that exits automatically:

```bash
cargo run --release --bin playable-demo -- --smoke-seconds 5
```

For a reproducible orbit around an already damaged authoritative world (useful for screenshots):

```bash
cargo run --release --bin playable-demo -- --showcase
```

The window title reports FPS, frame time, solid voxel count, cursor state, and the most recent
authoritative destruction result. An auto-terminating smoke run prints bounded redraw cadence,
CPU frame-work, GPU shadow, GPU world/HUD, and GPU-total distributions. Point-in-time measurements
are recorded in [`docs/performance.md`](docs/performance.md).

## Runtime dependencies

All versions are pinned in `Cargo.toml`. `wgpu` provides a safe Vulkan abstraction, `winit` owns
Linux window/input integration, `glam` supplies SIMD-friendly camera math, `bytemuck` performs
checked POD uploads, and `pollster` bridges one-time GPU initialization. `quinn`, `rustls`, `ring`,
`tokio`, and `bytes` provide the portable asynchronous QUIC/TLS foundation; `zeroize` protects the
application-owned temporary credential buffers from compiler-elided clearing. `rcgen` exists only in
tests to create ephemeral loopback identities. `jsonwebtoken`, AWS-LC, `serde`, and `serde_json`
provide maintained RS256/JWK and bounded claims parsing while repository code owns the strict OIDC
policy, replay cache, and principal mapping. These dependencies are permissively licensed upstream
and replace fragile platform-specific boilerplate; game rules, destruction, replication, admission
policy, meshing, controller, and shaders remain repository-owned.

## Engineering targets

- 60 Hz authoritative simulation for nearby gameplay;
- 120 Hz local input and weapon prediction;
- 1,200-byte application frames to avoid IP fragmentation;
- GPU-driven Vulkan renderer with explicit frame budgets;
- no full chunk transfer during ordinary destruction;
- periodic snapshots only for joining or repairing a detected gap;
- scalable interest management rather than broadcasting the whole world.

The runtime architecture is documented in [`docs/architecture.md`](docs/architecture.md). The
complete path from this engineering slice to a distributable game, including measurable promotion
gates and performance budgets, lives in [`docs/production-roadmap.md`](docs/production-roadmap.md).
