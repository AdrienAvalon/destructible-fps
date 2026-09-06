# Authoritative industrial workshop

This is the first authored industrial layout toward the visual reference, not a photorealistic
release. All solid building and terrain geometry is ordinary material state: the renderer derives
its surfaces from the same world used by collision, ray impacts, authority and snapshots. There is
no baked non-destructible building mesh hiding behind a voxel collision box.

## Play and inspect

```bash
cargo run --release --bin playable-demo -- --world industrial
cargo run --release --bin playable-demo -- --world industrial --showcase-intact --smoke-seconds 12
cargo run --release --bin playable-demo -- --world industrial --showcase-closeup --smoke-seconds 12
cargo run --release --bin playable-demo -- --world industrial --showcase-interior --smoke-seconds 12
cargo run --release --bin playable-demo -- --world industrial --lighting-stress --smoke-seconds 30
```

The first command is the freely playable intact scene. The others are fixed inspection cameras
(or the moving lighting-stress camera); they are not substitutes for a player-controlled traversal.
`--showcase` uses the wide industrial camera with the same real breach as `--showcase-closeup`.
Intact/interior views do not damage the map. The breach is an ordinary `DemoSession::fire` blast
at the front brick infill, crossing framed authority/replica validation before meshing.
`--world range` retains the old regression range; it remains the compatibility default. World
names are exactly `range` or `industrial`, never file paths. An explicitly selected world and
`--structural-lab` are mutually exclusive. Both presentation profiles `--msaa 1|4` remain available.

For the existing local network transports, select the map only at the authority:

```bash
cargo run --release --bin dedicated-server -- --world industrial --bind 127.0.0.1:40000
cargo run --release --bin multiplayer-demo -- --server 127.0.0.1:40000
# Alternatively, use the existing private, file-backed QUIC/TLS/OIDC launch configuration:
cargo run --release --bin secure-dedicated-server -- --config <private-server.json> --world industrial
```

QUIC clients retain the documented `--secure-server`, `--server-name`, `--ca-cert` and
`--credential-file` contract. No world flag or matching compiled scene is needed on a client:
every admitted client begins empty and installs the server snapshot. Neither transport is newly
exposed to the LAN/Internet by this increment. No credentials belong in command arguments or Git.

## Architecture and scale

The main concrete-framed hall has a flush ground slab, a raised roof monitor with real roof and
wall apertures, deeply recessed brick infill, a nine-cell-wide loading portal and a rear service
exit. A lower west workshop wing and taller rear service tower break up the silhouette. Doorways
connect the volumes at ground level. A separate four-post loading canopy has an explicit air gap
from the hall. A small timber barricade supplies a destructible material-response target indoors.
The foreground and existing spawn region are flat; bounded integer quarry banks sit outside the
authored footprint. There is no transparent-looking solid glass in the openings.

The current authoritative cell is **one metre**, not a centimetre-scale damage element. Concrete
sections, roof slabs and timber remain coarse; thin window framing, real reinforcement, fine
fracture surfaces, rubble, wet material blending and dressing are still missing. Lighting still
has coarse sky-visibility bands and no actual interior bounce. These limits remain visible in
the actual screenshots. A richer architectural layout does not remove them.

Every intact above-ground cell is connected by six-neighbour material to the foundation plane.
This is a topology invariant, not proof of calibrated load-bearing capacity. Removing any first
three canopy support bases leaves a connection; removing the fourth disconnects precisely the
remaining canopy cells, not an accidental wall or terrain anchor. Real bounded blasts additionally
create ordinary replicated rigid bodies, preserve every undestroyed cell and produce falling motion.
The optional elastic structural lab is not silently enabled or claimed calibrated for this map.
The existing "rifle" remains the prototype radial damage profile; the low-energy timber regression
tests cumulative material integrity, not realistic bullet ballistics.

## Client loading and invalidation

Snapshot validation/install is atomic and retains existing codec/authority limits. Before install,
the client additionally rejects more than its 512 pending-chunk budget and checks generation-counter
exhaustion. It then clears old visual geometry and schedules new chunk/body meshes through the
existing bounded worker. No full-map `mesh_chunk`/`mesh_body` loop runs on receipt of a snapshot.
The canonical snapshot can be acknowledged before all its geometry is visible; loading may briefly
show the sky while geometry arrives. A full loading UI and measured decode/install latency remain
separate work; this change does not move snapshot decoding itself off the network pump.

Each submitted mesh job carries the current world generation in the client job state. A replacement
snapshot advances that generation; late completions from the previous world are discarded instead
of reintroducing old chunks or a reused body ID. Within one generation, a changed world fingerprint
still requeues affected chunks through the existing logic. Only one worker job remains in flight.
Prediction/player-state processing waits for a valid world. Replacement clears interpolation and
old player visuals, but preserves pending local inputs and their sequence high-water mark. New
input production waits for successful reconciliation against server state on the installed world.
Discarding that history would risk reusing inputs still in flight when the snapshot arrived.

The graphical smoke now requires an actual applied delta after the latest snapshot. Merely replacing
an empty/other map no longer counts as destruction. It also requires completed remeshing and drained
pending mesh queues at exit. The `jobs_mesh` output counts **completed** jobs; `pending` instead
counts predicted player inputs awaiting acknowledgement.
`--smoke-resnapshot` (only with `--smoke-seconds`) requests a second snapshot after two seconds of
play, then requires both its installation and at least sixty subsequent acknowledged inputs.
Combined with `--smoke-drop-first-delta`, it also exercises retained-delta repair after the reload.

## Validation boundaries

Tests serialize the complete intact and collapsed maps at both 1,100/1,200-byte MTUs, install into
a different client world and compare full canonical contents and body state. A real UDP process test
selects the industrial preset, deliberately drops a snapshot fragment, repairs it and verifies the
replacement fingerprint. The actual sixteen server spawn definitions are exercised by the fixed-step
character controller through the hall entrance, without jumping or changing the map.

The external code review prompted stronger opposite-map replacement and spawn endpoint checks,
an explicit late-mesh completion decision test, and preservation of unacknowledged input history.
The existing atomic snapshot test also verifies a populated replica remains unchanged on refusal.
A pure prediction test replays delayed inputs on a replacement world without reusing their sequence;
the graphical QUIC smoke separately forces a second snapshot and requires subsequent authority ACKs.
These checks do not force every possible GPU/worker completion interleaving during replacement.

The pre-existing range generator body and helpers remain byte-identical to parent `6e7df1d`; preset
selection does not rewrite it. Ordinary gameplay/replication regressions, real Vulkan captures,
two graphical QUIC clients and separate CPU/GPU performance evidence are recorded in `performance.md`.
Cross-OS rendering, calibrated collapse, combat-scale profiling and photorealistic fidelity remain
unfulfilled release gates.
