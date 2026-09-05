# Performance evidence

Performance observations are point-in-time results tied to a command, scene, build, resolution, and
machine. They are not portable guarantees or substitutes for the later platform matrix.

## 2026-09-05 — adaptive repair and four-client trace replay

Source state: parent `f5ca8ce` plus the network-timing increment documented here. The graphical
client replaces fixed gap/retry constants with an integer Jacobson/Karels estimator. It starts with
100 ms of reordering grace and a 250 ms RTO, accepts only clean retained-delta repair samples, applies
Karn filtering after any retransmission, and exponentially backs off repeated requests. Reordering
grace is clamped to 50–500 ms and RTO to 100–2,000 ms. This timer remains entirely client-side and
does not enter the deterministic authority tick.

The process proxy can now replay a separate finite declarative trace for every direction/channel.
Each trace is capped at 256 steps, 128 pump ticks of delay and at most two deliveries per input, then
becomes clean. Four simultaneous clients replayed distinct loss, duplication, jitter and reordering
traces against the real 60 Hz process. Each issued exactly one repair, accepted one unambiguous RTT
sample, converged to the identical authoritative world, and observed exactly 19,806 delta bytes from
the server. The release run measured 16.510 ms RTT and a 100 ms RTO for each loopback client; offered
and delivered byte totals were equal across all four. The queue never overflowed. A separate real
Vulkan client deliberately discarded its first complete delta, repaired and presented both world
transactions in order with one 16 ms sample, drained its mesh work, and reported zero local transport
drops.

The complete promotion passed 151 library tests, ten binary tests and 50 integration tests in debug
and release with strict Clippy clean. Required release baselines measured destruction p99 0.402 ms,
8,192-voxel structural analysis plus promotion p99 4.503 ms, 1,024-body stacking p99 1.186 ms, and
snapshot total p99 10.467 ms. A real five-second RTX 4050 Vulkan smoke initialized the GPU in
2,076.3 ms, streamed all 128 chunks in 36.8 ms, and completed with GPU-total p99 0.256 ms, maximum
0.258 ms, and zero abandoned samples. Seeded stochastic impairment, transport pacing, congestion
control and the 32-headless-client load gate remain later work.

## 2026-09-05 — bounded inclined dynamic support

Source state: parent `2956991` plus the rotated-support increment documented here. Identity bodies
retain their exact canonical vertical-column path. A contact involving any rotated state instead
orders candidates by the minimum occupied proxy boundary, computes a rational vertical impact time
from swept occupied bounds, and refines it through at most 4,096 canonical pairs of the same
conservative fixed-point voxel proxies used by oriented lateral contact. An empty refinement rejects
the coarse overlap. Pair-budget exhaustion retains fail-closed support without inventing a torque.
The correction removes only downward penetration, zeros the vertical integration remainder, damps
existing angular motion, and permits deterministic sleep only once horizontal and angular velocity
are zero. Exact convex manifolds and full vertical impulse exchange remain later work.

The dedicated `rotated-stacks` release fixture dropped 1,024 concrete voxel bodies, fixed at a 45
degree Z inclination, into 256 four-body columns for 300 ticks. It observed 72,192 dynamic contacts,
never exceeded 768 broad-phase pairs, verified the exact conservative 1,414,222 micrometre vertical
proxy extent, and ended with all 1,024 states canonically asleep. Tick time measured p50 2.154 ms,
p95 4.660 ms, p99 4.795 ms and maximum 5.004 ms on this machine, below the 16.67 ms server interval.

The complete promotion passed 148 library tests, ten binary tests and 48 integration tests in debug
and release with strict Clippy clean. Required release baselines measured destruction p99 0.361 ms,
8,192-voxel structural analysis plus promotion p99 4.211 ms, unchanged axis-aligned 1,024-body
stacking p99 1.199 ms, and snapshot total p99 9.538 ms. A real five-second Vulkan smoke on the RTX
4050 initialized the GPU in 292.9 ms, streamed all 128 chunks in 35.3 ms, and completed with GPU-total
p99 0.800 ms, maximum 0.801 ms, and zero abandoned timestamp samples.

## 2026-09-05 — bounded live TLS identity reload

Source state: parent `ddc682b` plus the TLS-lifecycle increment documented here. Optional reload
intervals are limited to 5–3,600 seconds, run outside the fixed simulation tick, and require every
candidate chain to remain valid for at least one complete interval plus the 60-second expiry margin.
Each attempt rereads the fixed certificate and owner-only key paths through the existing type,
permission, size, PEM-cardinality, X.509 lifetime, and key-pair checks. The narrow endpoint capability
changes only future QUIC handshakes. A partial or invalid pair leaves both the active configuration
and monotonic shutdown deadline unchanged, while successful shorter-lived certificates correctly
shorten rather than silently extend that deadline. The validation clock is floored by startup wall
time plus monotonic elapsed time, so a later wall-clock rollback cannot grant extra validity.

A real-QUIC integration case proves that the old root is rejected for a new handshake after rotation,
the new root succeeds, and a session established under the old certificate remains usable. An
external-process case then replaces both files during a live run, observes exactly one successful
background reload, admits a second session under the new root, and applies its authoritative command
without dropping the first session. Focused tests cover interval bounds, the interval-plus-margin
boundary, wall-clock rollback, invalid pair rollback, and valid deadline replacement. The complete
promotion passed 146 library tests, ten binary tests and 48 integration tests in debug and release
with strict Clippy
clean. Release baselines measured destruction p99 0.359 ms, 8,192-voxel structural analysis plus
promotion p99 4.642 ms, 1,024-body stacking p99 1.151 ms, and snapshot total p99 9.890 ms. A cold
five-second RTX 4050 Vulkan smoke initialized the GPU in 2,085.1 ms, streamed all 128 chunks in
33.7 ms, and completed with GPU-total p99 0.241 ms, maximum 0.242 ms, and zero abandoned samples.
Automated certificate issuance and remote exposure remain separate gated work.

## 2026-09-05 — fail-closed OIDC authority refresh lifecycle

Source state: parent `fdf0b62` plus the authority-refresh increment documented here. The standalone
authority can retain its bounded static JWKS as bootstrap material while requiring trusted online
discovery before readiness. A valid complete discovered set replaces the bootstrap atomically and
starts a monotonic validity horizon of exactly three configured refresh intervals. Refresh work runs
outside the 60 Hz simulation loop. A request or validation failure retains both the previous set and
its previous deadline, exposes only a non-secret failure count, and cannot extend stale trust; the
authority terminates once that deadline is reached.

Focused configuration tests reject intervals below 60 seconds and relative private-root paths, and
prove that a rejected key set cannot advance the deadline. The seven-case external-process matrix
includes a real local HTTPS issuer with an explicit root: it replaces an intentionally wrong static
key before `READY`, admits a token signed only by the discovered key, and applies an authoritative
command. A separate issuer-mismatch case exits before `READY` without logging the configured issuer.
The complete promotion passed 141 library tests, ten binary tests and 46 integration tests in both
debug and release with strict Clippy clean. A real five-second Vulkan smoke on the RTX 4050 streamed
all 128 chunks in 35.6 ms and completed with GPU-total p99 0.884 ms, maximum 0.925 ms, and zero
abandoned samples. The required release baselines measured destruction p99 0.356 ms, 8,192-voxel
structural analysis plus promotion p99 4.211 ms, 1,024-body stacking p99 1.159 ms, and snapshot total
p99 9.298 ms. The authority remains loopback-only pending certificate renewal, platform ACLs,
production issuer/root provisioning, and the remote abuse/failure matrix.

## 2026-09-05 — bounded trusted OIDC discovery client

Source state: parent `a53fb98` plus the isolated discovery-client increment documented here. The
client accepts one exact parsed HTTPS issuer of at most 512 bytes, derives its standard well-known
path, requires TLS 1.2 or later, disables redirects and ambient proxies, and limits refresh intervals
to 60–3,600 seconds. Optional private roots are capped at 256 KiB and 16 certificates. Connection and
complete-request deadlines are three and five seconds. Discovery metadata is capped at 16 KiB, must
repeat the issuer byte-for-byte, and can select only a same-origin HTTPS JWKS endpoint; that response
is capped at 64 KiB. Streaming chunks are accounted before vector growth, so a missing or dishonest
`Content-Length` cannot bypass either ceiling. Public errors contain categories but no response body
or endpoint URL.

Three focused tests cover hostile issuer/metadata endpoints, a real local TLS exchange trusted by an
explicit root, and a chunked body one byte over the limit. `reqwest` 0.13.4 and `tokio-rustls` 0.26.5
are exactly pinned with default features disabled; the selected no-provider path preserves the
already current `rustls-webpki` 0.103.15 and the project's single rustls provider. The complete
promotion passed 138 library tests, ten binary tests and 44 integration tests in debug and release
with strict Clippy clean. A real five-second Vulkan smoke on the RTX 4050 completed with GPU-total
p99 0.946 ms, maximum 0.949 ms and zero abandoned samples. This client is deliberately not wired to
authority trust yet, and non-loopback exposure remains impossible.

## 2026-09-05 — complete-chain TLS lifetime gate

Source state: parent `7a1009b` plus the certificate-lifecycle increment documented here. Startup
parses all one-to-eight X.509 certificates after the existing bounded PEM and permission checks,
rejects malformed trailing data, refuses a chain that is not yet valid or already expired, and
requires at least 60 seconds remaining on every entry. The earliest wall-clock expiry is converted
once to a monotonic deadline; the standalone process stops before continuing to serve expired trust
material. Key/certificate compatibility remains independently enforced by rustls. The parser is the
exactly pinned `x509-parser` 0.18.1 with default features disabled and a single direct dependency
path.

Unit fixtures reject both past and future validity windows. A fifth external-process case installs
an expired but key-compatible identity, proves non-zero exit before `READY`, and observes only the
non-sensitive error category. The complete promotion passed 135 library tests, ten binary tests and
44 integration tests in debug and release with strict Clippy clean. A real five-second Vulkan smoke
on the RTX 4050 completed with GPU-total p99 0.241 ms, maximum 0.246 ms and zero abandoned samples.
The server remains loopback-only: certificate renewal, online OIDC discovery/JWKS refresh, Windows
service DACL validation, and the remote abuse matrix remain separate gates.

## 2026-09-05 — bounded oriented dynamic contact refinement

Source state: parent `0a6af47` plus the oriented dynamic-contact increment documented here. Swept
X/Z contacts keep the bounded coarse body broad phase, then refine every contact involving a rotated
state through canonical material-voxel pairs at the rational impact time. A canonical normalized
quaternion interpolation selects that time's orientation, and each voxel is represented by the same
conservative fixed-point rotated proxy as static collision. Empty proxy intersections
discard a coarse false positive; accepted intersections produce deterministic local contact
centroids and therefore an inertia-weighted angular response. The narrow phase tests at most 4,096
voxel pairs per body pair. Budget exhaustion fails closed by retaining coarse separation while
withholding contact torque whose lever arm was not proven.

The dedicated `rotated-dynamic-head-on` release scenario resets 1,024 two-voxel bars for 300 ticks.
Every tick resolves 512 deliberately off-centre oriented impacts, requires both bodies in every pair
to gain angular velocity, and verifies the canonical linear response. Across 153,600 body contacts it
measured tick p50 6.019 ms, p95 6.151 ms, p99 6.499 ms and maximum 7.311 ms on this machine, below
the 16.67 ms 60 Hz tick interval. Focused negative tests prove separated rotated voxel proxies are
rejected, pair work saturates at the fixed budget, and an accepted off-centre contact generates
bounded valid angular state.

The complete promotion passed 134 library tests, ten binary tests and 43 integration tests in debug
and release with strict Clippy clean. The required release baselines measured destruction p99 0.350
ms, 8,192-voxel structural analysis plus promotion p99 4.452 ms, 1,024-body stacking p99 1.158 ms,
and snapshot total p99 9.480 ms. A real five-second Vulkan smoke on the RTX 4050 completed with
GPU-total p99 0.721 ms, maximum 0.721 ms and zero abandoned samples. Surface-paced CPU/redraw p99
was 33.264/33.271 ms in that point-in-time run, so this smoke proves clean rendering and GPU headroom,
not a portable frame-pacing guarantee.

## 2026-09-05 — bounded angular collision sweep

Source state: parent `c11e972` plus the sampled-angular increment documented here. Each fixed tick
first consumes the canonical angular remainder, derives a conservative L1 arc bound from the body's
mass-centred corner radius, and divides the rotation into samples whose maximum travel is 0.25 m.
At most eight substeps and 262,144 static-cell tests are allowed per body tick. Every candidate
orientation checks the union of its previous and candidate per-voxel proxy bounds before commit;
intersection, coordinate overflow or budget exhaustion stops angular motion without partially
applying the unsafe sample. Resting contact correction remains below the impact-torque threshold and
cannot re-inject angular energy.

The dedicated `angular-sweep` release scenario resets 1,024 two-voxel bars at maximum angular speed
for 300 ticks, requires an identical accepted rotation from every body, and performs the complete
empty-world collision query. It measured tick p50 3.001 ms, p95 3.055 ms, p99 3.103 ms and maximum
3.542 ms on this machine. Focused tests prove exact deterministic subdivision, obstacle rejection
before overlap, fail-closed rejection when the radius would require more than eight substeps, and
continued completion of snapshot catch-up after moving debris settles. The complete promotion passed
130 library tests, ten binary tests and 43 integration tests in debug and release with strict Clippy
clean. The required 1,024-body stacking baseline measured p99 1.209 ms and maximum 1.226 ms. A real
five-second Vulkan smoke on the RTX 4050 reported GPU-total p99 0.801 ms, maximum 0.802 ms and zero
abandoned samples.

## 2026-09-05 — rotation-aware static collision proxy

Source state: parent `5eec27a` plus the physics increment documented here. Non-identity rigid bodies
now derive a conservative fixed-point AABB per rotated material voxel, sweep those bounded proxies
against static cells on all three translation axes, and reconstruct an off-centre local contact point
from the actual overlapped cell. The resulting normal impulse updates the existing diagonal-inertia
angular response only above the existing impact threshold; resting support correction cannot inject
fresh angular energy. Identity bodies retain the direct precomputed surface path. Broad-phase bounds
now rotate all eight body corners about the exact mass centre, so rotated extents cannot disappear
before narrow-phase work.

The dedicated `rotated-lateral-sweep` release scenario resets and drives 1,024 two-voxel bars into
separate walls for 300 ticks, requiring 307,200 static contacts, non-zero contact torque, and one
canonical result across every body. It measured tick p50 3.929 ms, p95 4.103 ms, p99 4.227 ms and
maximum 4.454 ms. The unchanged lateral sweep measured p99 0.848 ms and a 1,000-tick dynamic
head-on run measured p99 1.091 ms. All remain below the 16.67 ms 60 Hz tick budget on this machine.
Focused physics and end-to-end simulation tests pass, including rotated mass-centred bounds, rotated
broad-phase inclusion, static non-penetration and collision-generated torque. The final complete
promotion passed 127 library tests, ten binary tests
and 43 integration tests in debug and release with strict Clippy clean. The required 1,024-body
stacking rerun measured p99 1.162 ms and maximum 1.185 ms. A real five-second Vulkan smoke on the RTX
4050 reported GPU-total p99 0.247 ms, maximum 0.251 ms and zero abandoned samples.

## 2026-09-05 — graphical authenticated-QUIC integration

Source state: parent `7322e85` plus the graphical secure-transport increment documented here. The
same multiplayer presentation now selects either the strictly local legacy UDP adapter or the
file-backed QUIC/TLS client. QUIC receive work runs on a two-thread Tokio runtime behind a bounded
512-datagram queue; the window tick drains at most 64 payloads and exposes queue overflow in both the
title and smoke result. Secure CLI parsing rejects incomplete configuration and any legacy/secure
mixture. A local-only fixture generator creates short-lived owner-only material without printing its
private key or access token.

Two simultaneous ten-second release Vulkan smokes connected to the standalone
`secure-dedicated-server` using a freshly generated self-signed TLS identity and two distinct RS256
OIDC credentials. On the NVIDIA GeForce RTX 4050 Laptop GPU, both installed their encrypted initial
snapshot, rendered the other authenticated player and applied two authoritative world deltas. The
ordinary client moved 80,096,248 micrometres and completed two background mesh jobs. The impaired
client discarded the first complete delta, requested one exact repair, converged and completed the
combined mesh job. Both exited at authority tick 1,254 with `drops_transport=0`. The server observed
two admitted sessions, two commands, no admission failure and no rate limiting. The generated
credential material was removed immediately afterwards.

The legacy regression then ran two simultaneous ten-second release Vulkan clients. Both rendered
the remote player, installed their snapshots and applied two deltas. The impaired client discarded
the first complete delta, issued exactly one repair and converged; both reported
`drops_transport=0`. The complete debug and release promotion passed 121 library tests, ten binary
tests and 43 integration tests with strict Clippy clean. The required release baselines remained
bounded: destruction p99 0.352 ms, structural analysis plus promotion p99 4.527 ms, 1,024-body
physics tick p99 1.111 ms, and snapshot total p99 9.362 ms. A five-second local Vulkan smoke reported
GPU-total p99 0.782 ms, maximum 0.807 ms, and zero abandoned GPU samples.

## 2026-09-05 — reusable secure client boundary

Source state: parent `149089f` plus the secure-client increment documented here. Client setup bounds
the root PEM to 256 KiB/16 certificates and the private credential to 4 KiB, requires absolute paths,
enforces owner-only credential access on Unix, generates a system-random nonce, verifies TLS and the
game ALPN, then authenticates through the post-TLS reliable stream. Credential storage is zeroized
and neither command-line values nor errors contain it.

An end-to-end integration creates an ephemeral trusted identity and owner-only credential file,
starts the real secure authority, connects through the new file-backed client, proves a non-zero
session/server nonce, requests and receives an encrypted snapshot datagram, and observes clean
disconnect. The complete promotion passed 121 library tests, eight binary tests, and 43 integration
tests in debug and release; strict Clippy was clean.

## 2026-09-05 — graphical retained-delta recovery increment

Source state: parent `905a0ad` plus the graphical recovery increment documented here. Once a future
complete transaction is buffered, the client waits 100 ms then requests the exact expected sequence
at a maximum of four requests per second. Ordered application, atomic replica validation and stale
mesh-result rejection remain unchanged. The smoke harness now emits two authority mutations and can
drop every frame of the first until the second has arrived.

Two release Vulkan clients ran for ten seconds. The impaired client discarded sequence 1, buffered
sequence 2, sent one repair request, then applied both transactions contiguously and completed the
combined background remesh (`deltas_monde=2`, `reparations=1`, `jobs_mesh=1`). The other client
applied both live transactions and completed two mesh jobs. Both also installed initial snapshots,
rendered one remote player and moved more than one voxel before clean exit at authority tick 1,101.
The complete promotion passed 119 library tests, eight binary tests, and 42 integration tests in
debug and release; strict Clippy was clean.

## 2026-09-05 — server-side network explosion envelope

Source state: parent `c1c4f06` plus the authority-policy increment documented here. Before calling the
generic destruction primitive, the network authority now rejects zero/oversized radii, zero or
radius-incoherent energy, energy above 50,000 units, and targets farther than 120 metres from the
authoritative eye position. Squared distance uses saturating 128-bit arithmetic. Unit tests cover
both shipped weapon profiles, excessive energy and extreme range; an authority-core regression
proves the invalid command increments rejection rather than mutation.

Two release Vulkan clients then completed the full graphical smoke against the real release server.
Both installed snapshots, moved, rendered the other player, applied the policy-approved destruction
delta and completed its background mesh job before clean exit at authority tick 900. The complete
promotion passed 119 library tests, eight binary tests, and 42 integration tests in debug and release;
strict Clippy was clean.

## 2026-09-05 — Cook-Torrance material baseline

Source state: parent `20bd942` plus the PBR baseline documented here. The former Blinn-style highlight
was replaced by GGX distribution, Smith visibility, Schlick Fresnel and explicit diffuse/specular
energy sharing. A separate metalness scalar expands the packed vertex from 44 to 48 bytes; the Rust
layout is compile-time asserted and the wgpu locations were shifted coherently for body instances.
Steel uses 0.92 metalness while the dielectric materials remain zero. Surface-scale roughness noise
is bounded to avoid unstable highlights.

```bash
cargo run --release --bin playable-demo -- --showcase --smoke-seconds 8
```

The real RTX 4050 Vulkan run compiled the shader and rendered 128 chunks/43,924 exposed faces, one
rigid body and two instanced players. Initial GPU setup took 2,019.1 ms and streaming took 38.4 ms.
GPU-total time was 0.156 ms p50, 0.158 ms p95, 0.159 ms p99 and 0.172 ms maximum over 4,096 retained
samples, with zero dropped timestamp samples. CPU/redraw work was 6.806 ms p99. The final view
submitted 88 world draws and 130 shadow draws. The complete promotion passed 116 library tests,
eight binary tests, and 42 integration tests in debug and release; strict Clippy was clean.

## 2026-09-05 — off-thread graphical delta meshing increment

Source state: parent `d939452` plus the network meshing increment documented here. Applying a world
transaction no longer meshes changed geometry on the presentation thread. One bounded worker gives
new rigid-body geometry priority, then processes camera-prioritized immutable chunk snapshots. The
client caps pending changed chunks at 512, chunk jobs at 256, body jobs at 16, and aggregate body
geometry at 32,768 voxels. A result whose world fingerprint became stale is requeued instead of
uploaded.

Two release Vulkan clients ran simultaneously for nine seconds. Both installed snapshots, rendered
one remote player, applied the same destruction delta, completed one background mesh job, drained
their mesh queues, and exited successfully. They reached authority ticks 921 and 918 and moved
80,096,248 um and 22,700,000 um respectively. The complete promotion passed 115 library tests, eight
binary tests, and 42 integration tests in debug and release; strict Clippy was clean.

## 2026-09-05 — bounded local correction smoothing increment

Source state: parent `9b4ce19` plus the presentation-only correction increment documented here.
Reconciliation preserves camera continuity for finite offsets no larger than two metres, then halves
the residual every 80 ms independently of frame rate. Larger discontinuities snap immediately to
the authoritative prediction. Unit tests prove exact initial continuity, half-life decay, large-gap
snapping and invalid-float rejection.

Two release Vulkan clients ran simultaneously for nine seconds after this change. Sessions 1 and 2
both installed their initial snapshot, rendered one remote player, applied the shared destruction
delta, and moved 80,096,248 um and 22,700,000 um respectively before clean exit. The complete
promotion passed 115 library tests, eight binary tests, and 42 integration tests in debug and release;
strict Clippy was clean.

## 2026-09-05 — graphical late-join snapshot increment

Source state: parent `5a79f19` plus the graphical snapshot increment documented here. Every loopback
graphical admission now requests and atomically installs the bounded authoritative snapshot before
accepting world deltas. It resets the ordered sequence cursor, remeshes the union of former and new
chunk positions so empty chunks disappear, clears stale GPU rigid bodies, uploads the snapshot body
set, acknowledges installation, and then receives the server's ordered catch-up queue. A stalled
transfer requests fixed 64-fragment missing windows at bounded intervals.

The first release Vulkan client ran for 15 seconds and destructively changed the authority. A second
release Vulkan client was deliberately started five seconds later. It joined as session 2, rendered
the existing remote player, moved 22,700,000 um, installed the already-modified snapshot and exited
successfully without receiving the earlier live delta (`deltas_monde=0`, `snapshot_pret=true`). The
first client independently observed the live destruction delta and moved 80,096,248 um. The complete
promotion retained 115 library tests, six binary tests, and 42 integration tests in debug and release;
strict Clippy was clean.

## 2026-09-05 — two-client graphical destruction replication increment

Source state: parent `4488131` plus the graphical world-replication increment documented here. The
loopback client now feeds bounded delta fragments through `OrderedDeltaInbox` and `ClientReplica`,
remeshes every affected chunk boundary, uploads newly detached bodies, and advances replicated body
transforms. Rifle, explosive, and construction mouse inputs remain requests; only the server emits
permanent mutations. Late-join snapshot bootstrap and off-thread network remeshing remain later
gates.

Two release Vulkan clients ran simultaneously for nine seconds against the real release server.
They established sessions 1 and 2, each rendered the other player, moved 80,096,248 um and
22,700,000 um respectively, and each applied the same automatic authoritative destruction delta.
Both clients exited successfully only after observing movement and a world mutation.

## 2026-09-05 — two-client graphical loopback increment

Source state: parent `9752e28` plus the graphical loopback increment documented here. The legacy
development authority broadcasts player state through the same transport-independent core while
remaining restricted to loopback. `multiplayer-demo` loads the deterministic world, sends
camera-relative 60 Hz inputs, predicts locally, reconciles compact v2 states, advances the 100 ms
remote interpolation clock from packet receipt, and uploads all other sessions to the fixed avatar
instance arena. It refuses non-loopback targets and does not claim production transport security.

Two release clients were run simultaneously against the real release server for eight seconds with
different automatic sprint trajectories. They established distinct sessions 1 and 2, each rendered
one remote player, reached authority ticks 990 and 993, retained only two and zero inputs respectively,
and measured maximum horizontal displacements of 79,760,414 um and 22,700,000 um. Both Vulkan clients
exited successfully. A separate headless process regression starts the actual server binary, admits
two independent UDP sockets, moves one player, and requires both clients to observe both sessions,
the input acknowledgement, and positive authoritative displacement.

The complete promotion passed 115 library tests, six binary tests, and 42 integration tests in both
debug and release profiles; strict Clippy was clean.

## 2026-09-05 — instanced remote-player presentation increment

Source state: parent `cb4d7bc` plus the avatar presentation increment documented here. The renderer
allocates exactly 16 transform slots and one immutable six-face placeholder mesh. All remote players
share one world draw and one shadow draw regardless of count; the local session is omitted. A pure
transform test proves the rendered 0.6 m by 1.8 m bounds match the authoritative collider.

```bash
cargo run --release --bin playable-demo -- --showcase --smoke-seconds 8
```

The real RTX 4050 Vulkan smoke rendered two placeholder remote players, all 128 chunks and the one
detached body, then exited cleanly. Initial GPU setup took 2,057.3 ms and initial streaming 36.7 ms.
The bounded 4,096-frame window reported GPU-total p50 0.139 ms, p95 0.202 ms, p99 0.203 ms and maximum
0.215 ms with zero dropped timestamp samples. CPU/redraw p99 was 8.179 ms. The final view contained
86 visible chunks, one visible body and both players, with 88 world draws and 130 shadow draws; the
two players contributed only one draw to each pass.

The complete promotion passed 115 library tests, four binary tests, and 41 integration tests in
debug and release; strict Clippy was clean.

## 2026-09-05 — local prediction and exact reconciliation increment

Source state: parent `7633f75` plus the prediction increment documented here. Player-state protocol
v2 encodes bounded velocities as signed 32-bit micrometres per second and adds all three signed
fixed-step integration remainders. The maximum 16-player packet falls from 1,054 to 910 bytes, or
145,600 bit/s of payload at 20 Hz; eight maximum interpolation views now retain at most 7,280 encoded
payload bytes. Prior v1 packets and invalid position, velocity, remainder, session, ordering, flag,
size, and tick values are rejected before client state changes.

The controlled client simulates one contiguous input immediately and retains at most 128 unconfirmed
commands. Reconciliation accepts only a newer server tick for the same session with a monotonic
acknowledgement no higher than the last locally produced input. It restores position, velocity,
grounded state, input sequence and integration remainders, drops the acknowledged prefix, then
replays the remaining inputs through the same fixed-step character code. The operation uses a
candidate player and commits only after replay succeeds. Tests prove exact equality with an authority
while three newer inputs are replayed, atomic rejection of a wrong session/impossible ACK, and a
fail-closed full history. The secure QUIC integration performs prediction, receives the compact
state, reconciles it, and only then constructs a voxel.

The complete promotion passed 114 library tests, four binary tests, and 41 integration tests in
debug and release; strict Clippy was clean.

## 2026-09-05 — deterministic remote-player interpolation increment

Source state: parent `6c5b1f6` plus the interpolation increment documented here. The client retains at
most eight validated full views, targets six 60 Hz server ticks (100 ms) behind the newest state, and
interpolates fixed-micrometre position and velocity with integer arithmetic. World positions and
velocities are codec-bounded before their use in interpolation. Targets older than history or newer
than the latest packet clamp to a real authority sample; no unbounded extrapolation is performed.
Join and leave visibility changes only at the newer complete-view boundary.

The worst retained encoded payload is 8,432 bytes (eight maximum 1,054-byte packets), excluding
small container allocation metadata. History cannot grow with session duration or packet rate.
Coverage includes exact midpoint motion, join/leave boundaries, stale atomic rejection, capacity
eviction, invalid fractional time, delayed target selection, and interpolation between opposite
world bounds without arithmetic overflow. The complete promotion passed 111 library tests, four
binary tests, and 41 integration tests in debug and release; strict Clippy was clean.

## 2026-09-05 — bounded player-state replication increment

Source state: parent `b084d66` plus the player-state increment documented here. Authenticated motion
is sampled from the 60 Hz authority and broadcast as a complete latest-wins view at 20 Hz. The
versioned packet stays allocation-bounded and session-sorted, carries the server tick and last input
acknowledgement, and rejects partial, stale, replayed, duplicate, unordered, unknown-flag, and
oversized states before replacing the client view. Join, update, and leave counts are derived from
each accepted complete packet.

At 16 players the exact packet is 1,054 bytes. Twenty packets per second therefore represent
168,640 bit/s of application payload per client, excluding QUIC/IP overhead, beneath the 256 kbit/s
sustained gameplay target. A one-player packet is 79 bytes. The server sends at most one state
datagram per authenticated peer on a broadcast tick; it shares the global bounded egress accounting
but is prioritized before world commands and body physics. Spatial interest, state deltas and an
adaptive cadence remain later optimizations rather than unmeasured claims.

The secure integration test proves two authenticated QUIC clients receive the same two-player view,
then separately proves a moved player's replicated fixed position and acknowledged input before
construction. The complete promotion passed 108 library tests, four binary tests, and 41 integration
tests in debug and release; strict Clippy was clean.

## 2026-09-05 — authoritative player movement and construction reach increment

Source state: parent `1d714cf` plus the player-authority increment documented here. Control protocol
v3 adds a fixed 27-byte player-input message with a separate monotonic sequence. Each authenticated
session retains only its newest unit-bounded world-space movement intent and advances exactly once
per 60 Hz authority tick, independent of datagram count. Inputs expire after 15 ticks; fixed
micrometre position, velocity and division remainders make gravity, acceleration, jumping, bounded
terminal velocity, swept static-voxel contact resolution and fall recovery repeatable. Sixteen
deterministic, distinct spawn slots are recycled only after disconnect, and admission fails closed
when every slot intersects static geometry. Idle expiry releases player, replay and ephemeral
construction accounting together.

Construction now requires a six-metre server-owned player context. Before commit, the authority
rejects overlap with every connected player AABB and performs a bounded integer voxel traversal from
the authoritative eye to the target. Edge and corner ties check every crossed neighbor
conservatively, so an arbitrary axis order cannot permit corner clipping. The real authenticated-QUIC
test sends a player input, observes server motion, and then places a voxel from that state.
Player-state replication, client prediction
and reconciliation, view authority, dynamic-body character contacts and persistent identity-backed
inventory remain separate increments.

```bash
cargo run --release --bin character-benchmark -- --ticks 10000
cargo run --release --bin construction-benchmark -- --iterations 200
```

The fixed movement fixture processed 160,000 player steps across 10,000 16-player ticks in 72.994 ms
on this machine: 2,191,957 player steps/s, with aggregate-tick p50 6.835 us, p95 9.551 us, p99
13.551 us, and maximum 52.258 us. With canonical context, reach, all-player overlap and line-of-sight
checks enabled, the construction fixture processed 51,200 placements in 12.773 ms: 4,008,342
placements/s, with per-placement p50 0.179 us, p95 0.194 us, p99 0.207 us, and maximum 4.640 us.
These fixtures isolate authority work and do not include network scheduling or future player-state
broadcast.

The complete promotion passed 104 library tests, four binary tests, and 41 integration tests in both
debug and release; strict Clippy was clean.

The eight-second RTX 4050 Vulkan smoke initialized in 263.4 ms, streamed all 128 chunks in 34.3 ms,
kept the single body asleep, and shut down with zero dropped GPU timestamp samples. GPU-total p50
was 0.184 ms, p95 0.189 ms, p99 0.194 ms, and maximum 0.197 ms. Redraw cadence was compositor-bound
and included a 33.366 ms p99, so this short window is retained as a renderer regression gate rather
than a frame-pacing claim.

## 2026-09-05 — server-authoritative construction increment

Source state: parent `f8d38d9` plus the construction increment documented here. A build request joins
explosions in one per-session monotonic command sequence, but commits only when its material is
solid, all coordinates stay within the fixed world bound, the target is empty, one static face
supports it, no conservative dynamic-body AABB overlaps it, and the session has enough of its fixed
512-unit budget. Material costs range from one to six units. Every refusal occurs before world tick,
sequence, fingerprint, replay high-water mark, or resources change.

Control protocol v2 transports the fixed 35-byte request over both UDP and authenticated QUIC. A
successful placement is an ordinary canonical one-change world delta, so existing fragmentation,
retention, repair, snapshot catch-up, fingerprint validation, and client remeshing need no parallel
state path. The playable client exposes wood placement on middle click and runs the request through
the same encode, reverse-order reassembly, replica validation, and dirty-chunk scheduling used by
destruction.

New coverage proves atomic policy failures, cross-command replay rejection, resource charging,
dynamic-body exclusion, exact control-codec bounds, invalid material and prior-version rejection,
in-process replica convergence, bounded authority-core broadcast, and one real OIDC-independent
credential-authenticated QUIC construction transaction. The complete promotion passed 92 library
tests, four binary tests, and 41 integration tests in both debug and release; strict Clippy was clean.

```bash
cargo run --release --bin construction-benchmark -- --iterations 200
```

The release fixture performed 51,200 supported placements over 200 fresh authoritative worlds in
9.063 ms: 5,649,173 placements/s, with per-placement p50 0.096 us, p95 0.188 us, p99 0.199 us, and
maximum 110.540 us on this machine. It isolates validation and commit cost; network scheduling,
render remeshing, persistent inventory storage, and authoritative player reach are outside this
microbenchmark.

The eight-second RTX 4050 Vulkan regression smoke streamed all 128 chunks in 35.7 ms, left the one
dynamic body asleep, and shut down cleanly with zero dropped GPU timestamp samples. GPU-total p50
was 0.189 ms, p95 0.194 ms, p99 0.195 ms, and maximum 0.206 ms. This confirms that the construction
input and updated HUD preserve the renderer gate; it does not exercise an automated placement in the
windowed client.

## 2026-09-05 — authoritative angular-state increment

Source state: parent `a75db12` plus the angular-state increment documented here. Bodies now carry a
canonical integer quaternion scaled by 1,000,000, world-space milliradian angular velocity capped at
12 rad/s, and three fixed-step remainders. Each 60 Hz step uses integer quaternion composition and
renormalization. An impulse applied at the body voxel nearest the blast is transformed around the
mass centre, evaluated against the body-space diagonal inertia, and transformed back to produce
angular velocity. Supported bodies use bounded angular damping so they can deterministically sleep.

Protocol v6 and snapshot v2 reject prior layouts and carry orientation, angular velocity, and
remainders through the body fingerprint. The renderer converts only the already validated quaternion
to a GPU transform, rotates around the mass centre, and derives a conservative world AABB from all
eight transformed corners for frustum culling. Physics collision geometry remains axis-aligned: this
increment does not claim oriented voxel collision, contact-generated torque, or gyroscopic response.

Focused tests cover canonical quaternion rejection, bit-identical 120-step integration, an
inertia-weighted off-centre impulse, snapshot/delta version rejection, end-to-end angular replication,
mass-centred rendering, and rotated render bounds. The complete promotion passed 87 library tests,
four binary tests, and 40 integration tests in debug and release; strict Clippy was clean.

```bash
cargo run --release --bin physics-benchmark -- --bodies 1024 --ticks 1000 --scenario dynamic-head-on
cargo run --release --bin snapshot-benchmark -- --iterations 100
cargo run --release --bin playable-demo -- --showcase --smoke-seconds 8
```

The 1,024-body dynamic fixture resolved all 512,000 contacts with tick p50 0.961 ms, p95 0.983 ms,
p99 1.010 ms, and maximum 1.178 ms. The representative snapshot used 859 frames / 0.983 MiB and
completed at p50 8.438 ms, p95 8.547 ms, p99 8.636 ms, and maximum 9.412 ms. Both remain below the
12 ms server-work target on this machine.

The real RTX 4050 Vulkan smoke completed with one of one bodies asleep, 1,483 CPU and 1,481 GPU
samples, and no dropped GPU timestamp. GPU-total p50 was 0.193 ms, p95 0.199 ms, p99 0.203 ms, and
maximum 0.210 ms; GPU initialization took 2,086.5 ms and the 128-chunk bootstrap stream 37.3 ms. The
short run validates the new mass-centred transform and clean shutdown, not final 1080p performance.

## 2026-09-05 — four-pass contact and friction increment

Source state: parent `62519ab` plus the bounded contact-iteration increment documented here. The
lateral solver now performs at most four deterministic passes over the already capped 8,192-pair
broad phase and stops early when a pass changes no constraint. This allows a right-to-left impact to
propagate through a short chain despite the canonical left-to-right pair order. Dynamic Coulomb
friction reduces relative tangential velocity by a value bounded from the normal impulse while
reconstructing both velocities from their shared momentum; changed-axis integration remainders are
discarded.

New tests prove bit-identical four-body reverse-order propagation with residual penetration below
one eighth of a voxel, and an equal-mass glancing contact whose 10 m/s tangential slip converges to a
shared 5 m/s velocity without momentum loss. The exported `MAX_BODY_SOLVER_PASSES` makes the work
ceiling inspectable. The complete promotion passed 82 library tests, four binary tests, and 40
integration tests in debug and release.

```bash
cargo run --release --bin physics-benchmark -- --bodies 1024 --ticks 1000 --scenario dynamic-head-on
```

The 1,024-body fixture still resolved exactly 512 independent contacts per sample and all 512,000
expected contact resolutions. Across 1,000 samples, tick p50 was 0.944 ms, p95 0.980 ms, p99
0.994 ms, and maximum 1.014 ms. The usual independent-pair path performs one productive pass and one
empty early-exit pass. This remains below the 12 ms server-work target; voxel-exact contact, rotation,
and convergence of large constraint islands remain outside this result.

## 2026-09-05 — bounded lateral body-contact increment

Source state: parent `66741fe` plus the isolated-pair dynamic-contact increment documented here.
The solver uses the existing bounded swept broad phase, compares rational entry times without
floating point, requires strict overlap on both orthogonal axes at contact time, separates the final
coarse bounds in inverse proportion to mass, exchanges normal momentum using material restitution,
and wakes impacted sleeping bodies. Pair traversal and tie-breaking are deterministic.

Unit coverage includes equal-mass 120 m/s head-on impacts on both X and Z, unequal wood/steel impact
with wake-up and exact separation, and a negative case where swept bounds meet only at a corner. This
is a bounded single pass over at most 8,192 broad-phase pairs. It is not yet a voxel-exact, iterative,
rotating, or frictional dynamic-body solver.

The complete promotion passed 80 library tests, four binary tests, and 40 integration tests in both
debug and release.

```bash
cargo run --release --bin physics-benchmark -- --bodies 1024 --ticks 1000 --scenario dynamic-head-on
```

The fixture resets outside each timed sample, then resolves 512 independent simultaneous wood/wood
head-on contacts among 1,024 active bodies. Across 1,000 samples it resolved all 512,000 expected
contacts, with 512 maximum broad-phase pairs: tick p50 0.806 ms, p95 0.831 ms, p99 0.856 ms, and
maximum 0.886 ms. A separate 300-tick vertical stack regression remained within the 12 ms target at
p50 0.494 ms, p95 1.119 ms, p99 1.133 ms, and maximum 1.166 ms while all 1,024 bodies settled at
their exact expected heights.

## 2026-09-05 — three-axis rigid-body response promotion

Source state: parent `d9468ff` plus the physics and protocol-v5 increment documented here. The
promotion suite passed 77 library tests, four binary tests, and 40 integration tests in debug and
release. New cases cover exact XYZ impulse conversion, sleep wake-up, bit-identical fixed-step
repetition, symmetric 120 m/s lateral wall impacts across negative coordinates without tunnelling,
upward ceiling impact, grounded friction to sleep, mass-weighted mixed-material response, bounded
horizontal wire states, and an off-centre blast replicated for 30 ticks without a body-fingerprint
divergence.

The solver remains axis-aligned. It integrates all three translation axes at 60 Hz, retains each
Euclidean division remainder, and sweeps the six canonical body silhouettes against static voxels.
Static contact combines mass-weighted body and surface coefficients, suppresses sub-0.5 m/s
micro-bounces, and applies deterministic ground friction. Newly detached bodies receive a bounded
momentum budget proportional to blast energy and mass-weighted fragmentation; this response is
deterministic gameplay calibration, not a claim of real-world explosive-energy units. Protocol v5
separates these semantics from v4 peers that rejected horizontal state.

Sequential release evidence on the same machine was:

```bash
cargo run --release --bin physics-benchmark -- --bodies 1024 --ticks 300
cargo run --release --bin physics-benchmark -- --bodies 1024 --ticks 300 --scenario lateral-sweep
```

| Fixture | p50 | p95 | p99 | max |
|---|---:|---:|---:|---:|
| Authoritative protocol-v5 destruction event | 0.012 ms | 0.221 ms | 0.362 ms | not reported |
| Structural analysis plus six-surface body promotion | 4.221 ms | 4.401 ms | 4.553 ms | 4.707 ms |
| 1,024-body physics tick | 0.468 ms | 0.882 ms | 0.903 ms | 0.932 ms |
| 1,024-body four-cell lateral sweep | 0.519 ms | 0.536 ms | 0.550 ms | 0.634 ms |
| Representative snapshot round trip | 8.366 ms | 8.449 ms | 9.773 ms | 9.773 ms |

The destruction fixture sustained 21,526 events/s and emitted 865 frames / 0.520 MiB while creating
32 active bodies with initial state updates. The physics fixture ended with all 1,024 bodies asleep,
768 maximum broad-phase pairs, 8,192 static contacts, and 68,096 vertical body contacts. The
separate lateral fixture resets outside each timed sample, then makes every body sweep four cells
into its own wall; all 307,200 expected contacts resolved. The three-axis solver remains well below
the 12 ms server-work target. Rotation, lateral dynamic-body response, interest filtering, and full
socket scheduling are still excluded from this promotion.

A five-second Vulkan regression smoke on the NVIDIA GeForce RTX 4050 Laptop GPU completed with 358
CPU and 356 GPU samples, zero timestamp drops, one visible body returned to sleep, and GPU-total
p99 0.698 ms (max 0.749 ms). Initial GPU setup took 314.5 ms and the 128-chunk bootstrap stream took
47.7 ms. CPU frame-work p99 was 33.462 ms under presentation pacing; the short run proves clean
render integration, not the final sustained 1,920×1,080 client budget.

## 2026-09-05 — standalone secure authority promotion

Source state: commit `6900d38` plus the standalone configuration/process increment documented here.
The complete suite passed 71 library tests, four binary tests, and 39 integration tests in both debug
and release. Its four external-process cases completed in 1.20 seconds: a real RS256 OIDC credential
admitted a QUIC session and one authoritative destruction command; an invalid credential produced no
simulation work; a non-loopback bind and a group-readable Unix private key both failed before
readiness. The process accepts only a configuration path in argv and its captured output contained no
test credential.

The launch path bounds configuration to 16 KiB, certificate input to 256 KiB/eight entries, private
key input to 64 KiB/one entry, and JWKS input to 64 KiB/32 validated keys. Static key validity is
limited to 60–86,400 seconds and converted once into a monotonic deadline. This is configuration and
security evidence, not a networking throughput result; non-loopback exposure remains disabled.

Sequential release baselines remained within the existing targets: destruction p99 0.354 ms at
21,502 events/s, structural combined p99 4.158 ms (max 4.232 ms), 1,024-body physics p99 0.866 ms
(max 0.900 ms), and snapshot total p99 9.434 ms. Replicas converged, all simulated bodies slept, and
the representative snapshot remained 859 frames / 0.983 MiB.

A five-second Vulkan smoke on the NVIDIA GeForce RTX 4050 Laptop GPU completed with 379 CPU and 377
GPU samples, zero timestamp drops, CPU-work p99 17.019 ms, GPU-shadow p99 0.089 ms, GPU world/HUD p99
0.110 ms, and GPU-total p99 0.220 ms. Initial GPU setup took 270.1 ms and the 128-chunk initial stream
took 40.2 ms. The secure process is not on the renderer path; this run guards against an unrelated
regression rather than attributing graphical performance to it.

## 2026-09-05 — dedicated-process transport promotion

Source state: parent `ce27090` plus the dedicated transport change documented in this section.

`cargo test --all-targets` and `cargo test --release --all-targets` each passed 45 library tests,
three binary tests, and 18 integration tests. The new integration test starts the actual release or
debug dedicated-server child process, negotiates two independent loopback UDP clients, submits one
bounded command, drains fragmented deltas in sequence, and verifies identical static world, body
geometry, dynamic state, ID high-water mark, and fingerprints. Its release execution took 0.19 s;
that wall time is functional process-level evidence, not a latency or throughput benchmark.

The transport has explicit safety ceilings of 16 peers, 64 received datagrams, 256 queued commands,
32 simulated commands, and 4,096 attempted outbound datagrams per server tick. Complete out-of-order
client packets retain at most 16 packets and 8 MiB in addition to the existing bounded fragment
assembler. These are overload bounds, not the final 256-kbit/s per-player bandwidth policy; interest
management, acknowledgements, snapshot repair, and per-client budgets remain required.

The next transport increment retains at most 64 encoded delta packets and 8 MiB, admits at most 64
queued repair requests, and serves at most 16 before new simulation each tick under the same 4,096
send-attempt ceiling. A second process test discards every initial frame of sequence 1 for one client,
buffers a complete future packet without applying it, requests the missing sequence, and finishes
with identical replicas. This proves bounded short-gap retransmission; it does not cover expired
history, sustained loss, congestion control, or final per-client bandwidth policy.

The complete post-change promotion passed 46 library tests, three binary tests, and 19 integration
tests in both debug and release; the two process transport tests completed together in 0.21 s in
release. Destruction, structural, and 1,024-body physics p99 were respectively 0.365 ms, 4.123 ms,
and 0.878 ms. The repeated Vulkan smoke completed cleanly with zero dropped GPU samples and
0.194 ms GPU-total p99. CPU frame-work p99 was 16.890 ms in that run because surface/presentation
pacing clustered around 16.7 ms, versus 11.729 ms in the immediately preceding run; this variance
needs the later sustained capture and does not establish a renderer regression or a shipping-budget
pass.

The full promotion rerun produced the following point-in-time results:

| Fixture | p50 | p95 | p99 | max |
|---|---:|---:|---:|---:|
| Authoritative destruction event | 0.013 ms | 0.243 ms | 0.389 ms | not reported |
| Structural analysis plus body promotion | 4.024 ms | 4.195 ms | 4.285 ms | 4.295 ms |
| 1,024-body physics tick | 0.456 ms | 0.878 ms | 0.905 ms | 0.939 ms |
| Vulkan CPU frame work | 1.039 ms | 8.561 ms | 11.729 ms | 15.566 ms |
| Vulkan GPU total | 0.136 ms | 0.145 ms | 0.189 ms | 0.198 ms |

The five-second Vulkan showcase ran at 1,440×900 on the RTX 4050 Laptop GPU, completed 2,616 GPU
samples with zero drops, rendered 90/128 chunks and one replicated body, and exited cleanly.

## 2026-09-05 — bounded snapshot repair baseline

Source state: commit `90ec6d0` plus the selective snapshot-repair change documented here.

Command:

```bash
cargo run --release --bin snapshot-benchmark -- --iterations 20
```

The representative world was damaged first so the snapshot contained 71,249 static voxels, one
detached body, and its moving fixed-point state. Each canonical snapshot occupied 0.983 MiB across
859 application datagrams at the 1,200-byte MTU.

| Measurement | p50 | p95 | p99 | max |
|---|---:|---:|---:|---:|
| Server encode and frame | 3.817 ms | 3.891 ms | 4.439 ms | 4.439 ms |
| Client reassemble and decode | 4.356 ms | 4.516 ms | 4.724 ms | 4.724 ms |
| Client semantic validate and install | 0.303 ms | 0.308 ms | 0.320 ms | 0.320 ms |
| End-to-end in-memory work | 8.476 ms | 8.659 ms | 9.483 ms | 9.483 ms |

The encoder has a four-MiB payload ceiling; the client retains only one transfer. The server emits
at most 16 snapshot frames per peer per tick and retains at most 256 catch-up packets or 8 MiB per
transfer. Encoded snapshot and delta buffers are reference-counted so retransmission, same-tick
snapshot clients, catch-up queues, and the global short-gap history share bytes instead of cloning
them. Snapshot work requires its own session-bound control request; a future delta-repair sequence
is rejected as a snapshot trigger. After paced emission the server retains the transfer until a
matching install acknowledgement and serves only fragments selected through fixed 64-bit missing
windows under the same egress budget. A transfer that exceeds its packet/byte catch-up budget stalls
fail-closed and requires a throttled newer snapshot.

An initial 64-frame/tick experiment caused the loopback receiver to miss 37 of 864 accepted
datagrams; reducing the burst to 16 delivered the same initial-world snapshot completely in 58
server ticks. The process tests additionally discard one snapshot frame, selectively request that
single fragment, install and acknowledge the completed snapshot, then create a moving authoritative
body and prove ordered catch-up before returning that client to live deltas. This is reliable
loopback milestone evidence, not congestion-controlled Internet transport; deterministic impairment,
acknowledgement retry/timeout policy, and remote transport remain later gates.

The final promotion passed 54 library tests, three binary tests, and 25 integration tests in both
debug and release. The unchanged destruction, structural, and 1,024-body physics fixtures reported
0.359 ms, 4.235 ms, and 0.878 ms p99 respectively. The five-second Vulkan smoke on the RTX 4050
Laptop completed 2,376 GPU samples with zero drops: CPU frame-work p99 was 12.725 ms and GPU-total
p99 was 0.208 ms at 1,440×900.

## 2026-09-05 — deterministic impaired-UDP baseline

Source state: commit `f43bdc9` plus the bounded test proxy and process scenarios documented here.
The fixed profile delays every datagram by 2–6 proxy pump ticks, reorders deliveries, drops every
53rd snapshot fragment once, duplicates every 47th snapshot fragment once, drops every frame of
delta sequence 1 once, duplicates delta sequence 2 once, and discards the first snapshot install
ACK. No pseudo-random source or wall-clock seed participates in those decisions.

Five repeated delta runs and three repeated snapshot runs converged. The representative delta run
dropped all eight frames of the first transaction, delivered a later duplicate out of order, then
recovered from retained history. It delivered 20 delta datagrams / 11,134 bytes, observed seven
reordered deliveries, and peaked at nine queued datagrams / 8,400 bytes. The snapshot run received
880 source datagrams, deliberately dropped 17, injected 19 duplicates, and delivered 882 datagrams /
1,058,196 bytes after selective repair. It delivered 20 control datagrams / 548 bytes after dropping
the first ACK, observed more than 670 reordered deliveries in representative runs, and peaked at 32
queued datagrams / 38,400 bytes. Both scenarios had zero proxy queue drops and stayed below the
2,048-datagram / 2-MiB hard queue limits.

These figures account application datagrams at the proxy boundary; they exclude UDP/IP/Ethernet
headers. They demonstrate bounded deterministic repair on loopback, not throughput, fairness, RTT
estimation, or congestion behavior on a real network.

## 2026-09-05 — authenticated QUIC session baseline

Source state: commit `2a7d1d7` plus the isolated secure-session change documented here. The transport
uses released Quinn 0.11 with rustls/ring, TLS 1.3, explicit trust roots, game-specific ALPN, one
post-TLS reliable admission stream, and encrypted unreliable gameplay datagrams. Production identity
verification and dedicated-authority integration are intentionally outside this measurement.

Twenty independent sequential loopback connections were measured in the optimized release test.
Every iteration performed a fresh TLS handshake and one application admission; no resumption or
0-RTT was used.

| Measurement | p50 | p95 | p99 |
|---|---:|---:|---:|
| Server-authenticated TLS handshake | 0.926 ms | 1.077 ms | 1.449 ms |
| Post-TLS credential admission | 0.176 ms | 0.308 ms | 0.378 ms |
| Combined connection and admission | 1.066 ms | 1.231 ms | 1.827 ms |

The application credential is bounded to 4 KiB; the application-owned encoded and received buffers
are securely zeroized on every exit path, without claiming control over caller or transport-library
memory. Admission has an internal five-second deadline. The QUIC transport caps concurrent streams
and all configured receive/send/datagram windows; the server admits at most 32 pending connections
and 512 KiB of pending handshake data. Encrypted gameplay payloads are rejected above 1,100 bytes in
either direction to leave transport-header margin below the 1,200-byte application-UDP target.

Six real-socket integration tests also reject an untrusted certificate, invalid credential, stalled
admission, mismatched nonce echo, and oversized send/receive datagrams. The deliberate stalled-peer
test consumes its full five-second timeout. These measurements establish bounded local session
setup and negative behavior, not WAN latency, remote denial-of-service resilience, or a production
authentication deployment.

The complete promotion passed 56 library tests, three binary tests, and 31 integration tests in both
debug and release. Destruction, structural promotion, 1,024-body physics, and snapshot end-to-end
p99 were 0.351 ms, 4.227 ms, 0.884 ms, and 9.365 ms respectively. The five-second Vulkan smoke on
the RTX 4050 Laptop completed 2,549 GPU samples with zero drops: CPU frame-work p99 was 12.633 ms and
GPU-total p99 was 0.198 ms at 1,440×900. These unchanged fixtures remained within their current
milestone budgets; the secure-session code is not on their hot paths yet.

## 2026-09-05 — bounded offline OIDC verification baseline

Source state: commit `ae79f31` plus the OIDC verifier change documented here. One hundred distinct
pre-signed RS256 access tokens were verified sequentially in the optimized release test. Key
generation and token signing were outside the timed loop; each timed sample included bounded header
decode, local `kid` lookup, AWS-LC signature verification, exact standard-claim validation, custom
time/length policy, SHA-256 principal derivation, replay-cache pruning, and `jti` insertion.

| Measurement | p50 | p95 | p99 | max |
|---|---:|---:|---:|---:|
| Offline RS256 token admission | 0.016 ms | 0.018 ms | 0.021 ms | 0.039 ms |

The JWKS input is capped at 64 KiB / 32 keys; RSA work is limited to 2,048–4,096-bit signing keys with
exponent 65,537. Tokens remain under the secure-session 4-KiB bound, may be issued for at most 15
minutes, must have at least ten seconds remaining, and consume one of 4,096 bounded replay entries
until expiry. Negative tests reject weak/duplicate/malformed keys, non-HTTPS configuration, HMAC
algorithm substitution, unknown `kid`, wrong issuer/audience, expired or overlong tokens, altered
signatures, and repeated `jti`; rotation is a validate-before-swap operation. This is CPU-cost and
policy evidence, not proof of OIDC discovery freshness or end-to-end server integration.

The complete promotion passed 62 library tests, three binary tests, and 31 integration tests in both
debug and release profiles. On the same release build, destruction p99 was 0.341 ms for 500 events,
structural-simulation combined p99 was 4.209 ms over 100 iterations, 1,024-body physics p99 was
0.881 ms over 300 ticks, and the snapshot round-trip p99 was 9.320 ms over 20 iterations. Two
consecutive five-second GPU smoke tests completed without dropped timestamp samples. The first was
surface-paced at 376 samples (CPU-work p99 16.971 ms, GPU-total p99 0.799 ms); the immediate repeat
collected 2,245 samples (CPU-work p99 13.258 ms, GPU-total p99 0.194 ms). The verifier is not on the
render hot path, so the presentation/clock variance must still be characterized by a sustained
capture before it can be attributed to this change.

## 2026-09-05 — transport-independent authority baseline

Source state: commit `2e8231f` plus the authority-core refactor documented here. The generic core is
monomorphized over an opaque peer key and emits through a nonblocking callback; it no longer owns a
socket. Its protocol framing ceiling is selected once at construction and its own counter rejects
ingress beyond 64 datagrams per simulation tick. A focused test drove a real destructive command
through an authenticated peer at the QUIC-sized 1,100-byte ceiling and verified every emitted frame.

The promotion passed 65 library tests, three binary tests, and 31 integration tests in both debug and
release profiles. A deliberately parallel benchmark pass was discarded as comparative evidence due
to CPU/build-cache contention. The subsequent sequential release pass measured destruction p99 at
0.384 ms for 500 events, structural combined p99 at 4.235 ms over 100 iterations, 1,024-body physics
p99 at 0.842 ms over 300 ticks, and snapshot round-trip p99 at 9.401 ms over 20 iterations. Replica
fingerprints remained identical and all 1,024 bodies slept.

The five-second Vulkan smoke on the NVIDIA RTX 4050 Laptop initialized in 299.9 ms and streamed all
128 chunks in 35.9 ms. It collected 2,019 GPU samples with zero drops at 1,440×900: CPU frame-work
p99 was 14.014 ms, GPU shadow p99 0.090 ms, GPU world/HUD p99 0.103 ms, and GPU-total p99 0.210 ms.
The renderer does not call the authority-core transport adapter; this smoke is a regression gate, not
evidence that the networking refactor improved rendering.

## 2026-09-05 — secure authority runtime baseline

Source state: commit `5287248` plus the QUIC authority adapter documented here. Admission and
datagram reception run asynchronously outside simulation. The adapter bounds concurrent admission
tasks at 32, its control channel at 64 events, gameplay buffering at 256 × 1,100-byte payloads, core
ingress at 64 payloads per tick, active peers at 16, and each peer at 240 received datagrams per
fixed one-second window. Thirty-two consecutive full gameplay-queue writes close the offending
connection. Outbound datagrams are copied once into Quinn-owned storage and remain subject to the
core's shared 4,096-attempt tick budget and Quinn's 128-KiB send buffer.

Four real-QUIC adapter tests passed in 0.10 seconds in the optimized profile: two authenticated
clients received the same destructive authority transaction, an invalid credential never created a
core session, a 241-datagram burst was closed by the per-session limiter before simulation work, and
a wildcard bind was rejected without a production exposure policy. The full promotion passed 67
library tests, three binary tests, and 35 integration tests in both
debug and release. The sequential release baselines were destruction p99 0.403 ms, structural
combined p99 4.303 ms, 1,024-body physics p99 0.856 ms, and snapshot round-trip p99 9.550 ms. Replica
fingerprints remained equal and all simulated bodies slept.

Two consecutive five-second Vulkan regression smokes completed with zero timestamp drops but exposed
presentation/clock variance. The first collected 369 GPU samples (CPU-work p99 33.445 ms, GPU-total
p99 0.212 ms); the repeat collected 401 (CPU-work p99 17.001 ms, GPU-total p99 0.814 ms). The secure
authority is not on the renderer path, and these short surface-paced runs are recorded rather than
misattributed; a sustained capture remains required for render-performance conclusions.

## 2026-09-05 — Stage 1 telemetry baseline

Command:

```bash
cargo run --release --bin playable-demo -- --showcase --smoke-seconds 8
```

Environment and scene:

- NVIDIA GeForce RTX 4050 Laptop GPU, proprietary NVIDIA driver, Vulkan backend;
- 1,440×900 client viewport;
- 71,304 solid voxels, 128 allocated/rendered chunks, 43,912 exposed faces;
- 2,048² directional comparison shadow map;
- release profile with fat LTO and one code-generation unit;
- bounded window containing the most recent 4,096 completed samples.

| Measurement | p50 | p95 | p99 | max |
|---|---:|---:|---:|---:|
| Redraw interval | 1.049 ms | 6.638 ms | 8.292 ms | 10.652 ms |
| CPU frame work | 1.045 ms | 6.631 ms | 8.285 ms | 10.646 ms |
| GPU shadow pass | 0.061 ms | 0.078 ms | 0.079 ms | 0.079 ms |
| GPU world and HUD pass | 0.065 ms | 0.085 ms | 0.086 ms | 0.086 ms |
| GPU total from shadow start to world/HUD end | 0.136 ms | 0.174 ms | 0.176 ms | 0.191 ms |

The four-slot asynchronous GPU readback ring dropped zero samples. CPU frame work deliberately
includes surface acquisition, command encoding, queue submission, and presentation, but not GPU
completion. GPU values come from native pass timestamp queries multiplied by the adapter timestamp
period. The scene is still small and uses simple materials; these results establish instrumentation,
not the final photorealistic content budget.

## 2026-09-05 — asynchronous initial meshing

The release showcase smoke initialized the Vulkan device and pipelines without performing CPU
meshing on the presentation thread. A dedicated worker then meshed the 128 chunks in
distance-prioritized batches of 16 from one shared immutable world snapshot. The renderer reached
all 43,912 exposed faces in 32.4 ms after the game runtime started and still completed the five-second
GPU smoke with zero dropped timestamp samples. The initial pending set is explicitly capped at 512
chunks; larger future worlds require the Stage 1 residency streamer rather than an unbounded queue.

## 2026-09-05 — camera-frustum culling

The five-second release showcase kept 90 of 128 resident chunks in the camera frustum and submitted
90 world draw calls. All 128 chunks remained in the directional shadow pass deliberately: geometry
outside the camera can still cast a visible shadow. GPU frame p99 was 0.201 ms in this run, but the
scene is too small and run-to-run variance too large to attribute a speedup. This result proves the
culling decision and counters; the later representative-scene gate will establish performance impact.

## 2026-09-05 — authoritative destruction baseline

Command:

```bash
cargo run --release --bin destruction-benchmark -- --events 500
```

The representative multi-material scene completed 500 server-authoritative destruction, structural
analysis, body promotion, then-current protocol-v4 fragmentation, reordering, decode, reassembly,
client-application, and final-verification cycles at 21,524 events/s. Event latency was 0.012 ms
p50, 0.229 ms p95, and 0.354 ms p99, or 2.1% of one 60 Hz frame budget. The run produced 838
application datagrams (0.515 MiB), fractured 13,009 voxels, detached 1,361 voxels into 32 active
bodies, and ended with identical static-world and body-set state on server and client.
Against the immediately preceding protocol-v3 run of the same deterministic fixture, compact IDs
reduced output from 847 to 838 datagrams and from 0.527 to 0.515 MiB (about 2.3%) without changing
the simulated result.

## 2026-09-05 — structural island extraction

Command:

```bash
cargo run --release --bin structural-benchmark -- --iterations 100
```

The fixture severs a single connector below a concrete slab of 8,192 voxels. Each analysis validates
the canonical after-state, searches only components adjacent to the edit, proves the slab has no
foundation path, computes mass/bounds, and reproduces the same 128-bit island fingerprint.

| Measurement | Result |
|---|---:|
| Combined throughput | 250 analyses and promotions/s |
| Topology analysis p50 | 2.569 ms |
| Topology analysis p95 | 2.605 ms |
| Topology analysis p99 | 2.624 ms |
| Body promotion p50 | 1.425 ms |
| Body promotion p95 | 1.466 ms |
| Body promotion p99 | 1.491 ms |
| Combined p50 | 3.995 ms |
| Combined p95 | 4.058 ms |
| Combined p99 | 4.117 ms |
| Combined max | 4.225 ms |

The promotion step revalidates the read-only island proof, canonical material voxels, six-neighbour
connectivity and geometry fingerprint before computing fixed-unit centre of mass and diagonal
inertia. Runtime identity is a separate checked server-monotonic 64-bit value. The combined result
is below the 12 ms server-work target on this fixture. Static-world detachment and replication are
integrated separately in the end-to-end benchmark; fixed-step collision performance is recorded
below, while progressive stress remains a later promotion gate.

## 2026-09-05 — replicated body rendering

Command:

```bash
cargo run --release --bin playable-demo -- --showcase --smoke-seconds 5
```

The deterministic showcase first severed a fragile support to guarantee one replicated body, then
breached the main facade. Body geometry was built by the same single-queue bounded background worker
as chunk geometry, in local coordinates, and uploaded into a fixed 1,024-instance transform arena.
The Vulkan run reported 1/1 body visible, 90/128 chunks visible, 91 world draws, and 129 shadow draws.
With fixed-step motion enabled, GPU total was 0.150 ms p50, 0.176 ms p95, 0.177 ms p99, and 0.180 ms
maximum across 1,987 completed samples, with zero dropped timestamp samples. The body reached the
static ground and the smoke gate reported 1/1 body sleeping. The scene is intentionally small: this
validates the body shader, upload, culling, state replication, and draw paths, not the final
active-body rendering budget.

## 2026-09-05 — fixed-step body simulation

Command:

```bash
cargo run --release --bin physics-benchmark -- --bodies 1024 --ticks 300
```

The fixture starts the full active-body limit in 256 four-body columns above a static voxel floor. It
exercises integer gravity, swept static queries, exact voxel-column body contacts, bottom-up stacking,
wake-aware sleep, and bounded sweep-and-prune candidate generation. Every column must finish at the
four exact canonical heights or the benchmark fails.

| Measurement | Result |
|---|---:|
| Tick p50 | 0.410 ms |
| Tick p95 | 0.831 ms |
| Tick p99 | 0.848 ms |
| Tick max | 0.885 ms |
| Maximum updated bodies | 1,024 |
| Maximum broad-phase pairs | 768 |
| Static contact resolutions | 7,680 |
| Body contact resolutions | 60,416 |
| Final sleeping bodies | 1,024/1,024 |

The broad phase reports saturation only on the 8,193rd candidate; the complete tentative tick is
then discarded, so overload cannot commit a partial or tunnelling-prone result. This core-solver
result was comfortably below the 12 ms server-work target. At this earlier solver state it still
excluded horizontal impulses, friction, restitution, rotation, interest filtering, serialization,
socket I/O, and other gameplay systems; the newer protocol-v5 promotion near the top of this file
supersedes the first three exclusions.

## 2026-09-05 — remote-exposure gate rehearsal

Source state: parent `1cc749e` plus the file-policy and test-stability increment documented here.
The authority remains deliberately loopback-only. Its remote promotion criteria are now a named
attack/failure matrix rather than an implicit checklist. IPv4 wildcard, private IPv4, IPv6 wildcard,
and IPv4-mapped IPv6 configurations all fail before trust material is used. Enabling both OIDC
discovery and TLS reload cannot implicitly grant a remote bind. On Unix, trust inputs are opened with
kernel `O_NOFOLLOW` and `CLOEXEC` before the opened regular-file descriptor, size, and permissions are
validated; this removes the final-component symbolic-link replacement window between path inspection
and open.

The first optimized full-suite run exposed a timing-only test flake: a player-state datagram could be
queued to QUIC immediately before the test blocked its sole runtime thread waiting for receipt. The
test driver now yields after the authoritative broadcast. The exact release case then passed ten
consecutive repetitions, followed by the complete debug and release suites.

| Promotion evidence | Result |
|---|---:|
| Library tests | 153 passed debug; 153 passed release |
| Binary tests | 10 passed debug; 10 passed release |
| Integration tests | 50 passed debug; 50 passed release |
| Destruction p99, 500 events | 0.373 ms |
| Structural analysis + promotion p99, 8,192 voxels | 4.268 ms |
| Physics p99, 1,024 bodies | 1.180 ms |
| Snapshot encode + decode + install p99 | 9.496 ms |
| Vulkan GPU total p99, RTX 4050 | 0.231 ms |
| Vulkan timestamp samples dropped | 0 |

The remaining blockers are automated certificate issuance with expiry-outage proof, disposable
production-shaped OIDC provisioning, handle-level Linux/Windows service ACL checks, concurrent
hostile external load, and a reviewed exact-interface deployment policy. This increment therefore
improves the future LAN gate without opening a socket beyond loopback.

## 2026-09-05 — observable TLS renewal progress

Source state: parent `5636d3f` plus the TLS outcome and asynchronous-test stabilization increment.
The reload controller fingerprints only the bounded public DER chain with incremental SHA-256. A
validated unchanged chain now returns `Unchanged`, leaves both the endpoint and exact monotonic
deadline untouched, and increments a dedicated process counter. A coherent new chain returns
`Installed`; an invalid pair still changes no state. The private key is parsed only for rustls
compatibility validation and is neither fingerprinted nor logged.

The new standalone-process case observed exactly one unchanged check and zero installations. The
live-rotation process case observed exactly one installation and zero unchanged checks while its old
session remained connected. During promotion, two unrelated asynchronous tests exposed premature
measurement barriers. The secure-authority driver now yields after queuing a player-state broadcast,
and the four-client trace test drains server egress for 250 ms after replica convergence before
asserting byte fairness. Each corrected case passed ten debug and ten release repetitions before the
complete suites.

| Promotion evidence | Result |
|---|---:|
| Library tests | 153 passed debug; 153 passed release |
| Binary tests | 10 passed debug; 10 passed release |
| Integration tests | 51 passed debug; 51 passed release |
| Destruction p99, 500 events | 0.385 ms |
| Structural analysis + promotion p99, 8,192 voxels | 4.280 ms |
| Physics p99, 1,024 bodies | 1.213 ms |
| Snapshot encode + decode + install p99 | 9.291 ms |
| Vulkan GPU total p99, RTX 4050 | 0.234 ms |
| Vulkan timestamp samples dropped | 0 |

This makes stalled certificate automation distinguishable from effective renewal, but it does not
pretend to issue certificates. The authority remains loopback-only until the external provisioner,
expiry-outage rehearsal, platform ACLs, hostile-load matrix, and reviewed private-network policy are
all proven.

## 2026-09-05 — Unix service identity and trust ownership

Source state: parent `ab9d53c` plus the Unix account-policy increment. The file-configured standalone
authority now rejects effective UID 0 before reading its configuration. Every opened trust input must
belong to either root or the effective service UID; the private key is stricter and must belong
exactly to the service UID. Existing no-follow, regular-file, size, and mode checks still run on the
opened descriptor. This prevents a writable directory from substituting a foreign-owned JWKS or
configuration file that merely has non-writable mode bits.

The policy is Unix-specific and leaves the server loopback-only. Rootless local fixture and real
process tests pass under UID 1000. Windows remains blocked pending installer-owned DACL and reparse
point validation; parent-directory ownership and replacement rights also remain an explicit gate.

| Promotion evidence | Result |
|---|---:|
| Library tests | 154 passed debug; 154 passed release |
| Binary tests | 10 passed debug; 10 passed release |
| Integration tests | 51 passed debug; 51 passed release |
| Destruction p99, 500 events | 0.344 ms |
| Structural analysis + promotion p99, 8,192 voxels | 4.318 ms |
| Physics p99, 1,024 bodies | 1.191 ms |
| Snapshot encode + decode + install p99 | 9.459 ms |
| Vulkan GPU total p99, RTX 4050 | 0.247 ms |
| Vulkan timestamp samples dropped | 0 |

## 2026-09-05 — Unix trust-parent confinement

Source state: parent `95dfdf7` plus the immediate-parent policy. Before opening any configured trust
file, the authority now requires its immediate parent to be a real, non-link directory, owned by root
or the service UID, with no group/world write bits. A real mode-0770 fixture is rejected before its
configuration bytes are read. Combined with owner validation on the opened file and kernel
`O_NOFOLLOW`, directory substitution by an unrelated account no longer passes merely because the
replacement file itself is read-only.

| Promotion evidence | Result |
|---|---:|
| Library tests | 155 passed debug; 155 passed release |
| Binary tests | 10 passed debug; 10 passed release |
| Integration tests | 51 passed debug; 51 passed release |
| Destruction p99, 500 events | 0.355 ms |
| Structural analysis + promotion p99, 8,192 voxels | 4.583 ms |
| Physics p99, 1,024 bodies | 1.231 ms |
| Snapshot encode + decode + install p99 | 9.373 ms |
| Vulkan GPU total p99, RTX 4050 | 0.226 ms |
| Vulkan timestamp samples dropped | 0 |

This completes the current Unix file and immediate-directory ownership gate. Windows DACL/reparse
validation, automated issuance with expiry-outage evidence, and hostile external load still block
any non-loopback policy.

## 2026-09-05 — concurrent QUIC admission saturation

Source state: parent `b42498b` plus the hostile-admission integration case. Thirty-two concurrent
real QUIC/TLS connections complete their server-authenticated handshake and deliberately withhold
the bounded application hello. While those admission tasks remain occupied, a 33rd connection is
refused within two seconds. The authority reports exactly one refusal, keeps zero active sessions,
and performs zero received-datagram, command, player-simulation, or outbound work throughout the
test. Every connection remains bounded by the existing five-second admission deadline and transport
memory ceilings.

The exact case passed ten consecutive debug and ten consecutive release repetitions before the
complete promotion. This is a same-process real-socket adversarial rehearsal; reconnect storms and
malformed/queue-pressure campaigns driven by separate hostile processes remain required before LAN
exposure.

| Promotion evidence | Result |
|---|---:|
| Library tests | 155 passed debug; 155 passed release |
| Binary tests | 10 passed debug; 10 passed release |
| Integration tests | 52 passed debug; 52 passed release |
| Destruction p99, 500 events | 0.385 ms |
| Structural analysis + promotion p99, 8,192 voxels | 4.412 ms |
| Physics p99, 1,024 bodies | 1.224 ms |
| Snapshot encode + decode + install p99 | 9.364 ms |
| Vulkan GPU total p99, RTX 4050 | 0.232 ms |
| Vulkan timestamp samples dropped | 0 |

Graphify was refreshed after the new test path and reported 1,982 nodes, 5,601 post-build edges,
zero unverified code nodes, and zero modified, added, deleted, or excluded freshness entries.

## 2026-09-05 — fail-closed TLS renewal outage

Source state: parent `78e9dae` plus the certificate safety-deadline increment. The final 60 seconds
of the earliest certificate's X.509 lifetime are no longer served. Startup requires strictly more
than that margin, or more than one complete reload interval plus the margin when the watcher is
active. Both initial load and coherent rotation convert the remaining wall-clock validity into a
monotonic safety deadline. Unchanged or invalid reloads retain the exact existing deadline.

A real standalone process starts with a 72-second certificate and five-second reload interval. Only
after its `READY` line, the test replaces the private-key input with invalid provisioner output.
Every watcher attempt fails; no identity is installed or classified as unchanged. The process emits
its bounded final counters, closes the authority, and exits unsuccessfully at the safety deadline,
before the certificate enters its reserved final minute. The exact case passed three consecutive
debug and three consecutive release runs without a production clock override or test-only server
configuration.

| Promotion evidence | Result |
|---|---:|
| Library tests | 156 passed debug; 156 passed release |
| Binary tests | 10 passed debug; 10 passed release |
| Integration tests | 53 passed debug; 53 passed release |
| Destruction p99, 500 events | 0.358 ms |
| Structural analysis + promotion p99, 8,192 voxels | 4.293 ms |
| Physics p99, 1,024 bodies | 1.170 ms |
| Snapshot encode + decode + install p99 | 9.427 ms |
| Vulkan GPU total p99, RTX 4050 | 0.247 ms |
| Vulkan timestamp samples dropped | 0 |

Graphify reported 1,989 nodes, 5,623 post-build edges, zero unverified code nodes, and a fully fresh
index. This proves the TLS outage half of the trust-expiry gate. Automated CA issuance and the
equivalent OIDC stale-key process outage remain separate blockers to private-network exposure.

## 2026-09-05 — static OIDC trust safety deadline

Source state: parent `970cbd7` plus the static-JWKS trust-window increment. Static configuration now
requires strictly more than 60 seconds and reserves its final minute from service, matching the TLS
safety policy. The earlier point is converted to a monotonic deadline at startup and is shared by
credential admission and process shutdown. Successful online discovery remains intentionally
different: it atomically replaces the key set and grants the existing bounded horizon of three
refresh intervals.

A real standalone process receives a 66-second static JWKS horizon, emits `READY`, performs no
unconfigured refresh activity, then exits unsuccessfully at the resulting safety deadline. The exact
case passed three consecutive debug and three consecutive release executions. The upper 24-hour
bound and the now-strict lower boundary are covered independently.

| Promotion evidence | Result |
|---|---:|
| Library tests | 157 passed debug; 157 passed release |
| Binary tests | 10 passed debug; 10 passed release |
| Integration tests | 54 passed debug; 54 passed release |
| Destruction p99, 500 events | 0.473 ms |
| Structural analysis + promotion p99, 8,192 voxels | 4.499 ms |
| Physics p99, 1,024 bodies | 1.241 ms |
| Snapshot encode + decode + install p99 | 9.502 ms |
| Vulkan GPU total p99, RTX 4050 | 0.239 ms |
| Vulkan timestamp samples dropped | 0 |

The five-second Vulkan smoke was GPU-clean but its presentation/CPU redraw p99 was 33.316 ms, with a
16.626 ms median, because a small number of frames crossed two display intervals. The GPU maximum
was only 0.247 ms, so this is recorded as a presentation-cadence observation rather than attributed
to the trust-deadline change. Graphify reported 1,993 nodes, 5,638 post-build edges, zero unverified
code nodes, and a fully fresh index.

## 2026-09-05 — external-process admission and datagram abuse

Source state: parent `0bc1490` plus two hostile-client process cases and expanded terminal counters.
The standalone server now carries refused connections, TLS handshake failures, gameplay-queue drops,
and protocol rejections from each bounded network tick into saturating process totals. The final
`STOP` line exposes those non-secret totals without endpoints, principals, or credentials.

The first case launches the real server process, completes 32 concurrent TLS connections while
withholding every application hello, and confirms that the 33rd connection is refused. The process
still completes exactly 240 fixed ticks with one refusal, zero handshake failures, zero admitted
sessions, and no inbound, outbound, or command work. The second case authenticates a real OIDC
session and injects a 1,101-byte raw QUIC datagram. It observes connection closure, exactly one
protocol rejection, and zero payloads or commands delivered to the authority. Each exact case passed
three consecutive debug and three consecutive release executions.

| Promotion evidence | Result |
|---|---:|
| Library tests | 157 passed debug; 157 passed release |
| Binary tests | 10 passed debug; 10 passed release |
| Integration tests | 56 passed debug; 56 passed release |
| Destruction p99, 500 events | 0.358 ms |
| Structural analysis + promotion p99, 8,192 voxels | 4.461 ms |
| Physics p99, 1,024 bodies | 1.177 ms |
| Snapshot encode + decode + install p99 | 9.358 ms |
| CPU frame-work p99, RTX 4050 smoke | 13.995 ms |
| Vulkan GPU total p99, RTX 4050 | 0.234 ms |
| Vulkan timestamp samples dropped | 0 |

Graphify reported 2,000 nodes, 5,656 post-build edges, zero unverified code nodes, and a fully fresh
index. External reconnect cycling and multi-session queue pressure remain before the hostile-load row
can be closed.

## 2026-09-05 — multi-session queue pressure and reconnect cycling

Source state: parent `80f5c83` plus two final hostile-client process cases and authority rejection
counters in the terminal summary. The pressure case fills all 16 authority slots, then concurrently
offers up to 240 maximum-sized malformed datagrams per session against the fixed 256-event gameplay
queue. More than 256 datagrams are accepted by QUIC, the queue reports at least 32 drops, and at least
one abusive connection is closed. Inputs already queued at that boundary may be decoded as malformed
or rejected after their session closes; both outcomes are bounded rejection paths. No command reaches
the simulation and no rate-limit path is mistaken for queue pressure.

The reconnect case performs 32 sequential OIDC-authenticated connection cycles against the real
standalone process. It finishes with exactly 32 admissions, exactly 32 disconnections, zero active
sessions, zero refusals, zero handshake or admission failures, and zero commands. The exact queue
case passed five consecutive debug and five consecutive release executions; the reconnect case passed
three consecutive debug and three consecutive release executions before complete promotion.

| Promotion evidence | Result |
|---|---:|
| Library tests | 157 passed debug; 157 passed release |
| Binary tests | 10 passed debug; 10 passed release |
| Integration tests | 58 passed debug; 58 passed release |
| Destruction p99, 500 events | 0.353 ms |
| Structural analysis + promotion p99, 8,192 voxels | 4.581 ms |
| Physics p99, 1,024 bodies | 1.241 ms |
| Snapshot encode + decode + install p99 | 9.318 ms |
| CPU frame-work p99, RTX 4050 smoke | 7.481 ms |
| Vulkan GPU total p99, RTX 4050 | 0.241 ms |
| Vulkan timestamp samples dropped | 0 |

Graphify reported 2,002 nodes, 5,673 post-build edges, zero unverified code nodes, and a fully fresh
index. This closes the deterministic loopback hostile-load matrix. The same campaign remains required
through the reviewed LAN interface and firewall profile before non-loopback exposure.
