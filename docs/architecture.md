# Architecture and promotion gates

## Product intent

The target is a photorealistic first-person shooter with persistent, material-aware destruction.
The world is authoritative on dedicated servers and every gameplay-relevant fracture is reproduced
for all interested clients. Performance claims are accepted only with captured frame, simulation,
network, memory, and worst-case destruction measurements.

"Everything is destructible" is implemented at bounded physical resolutions:

1. terrain uses a sparse volumetric representation and can form craters or tunnels;
2. load-bearing structures use a material constraint graph;
3. detached pieces become rigid bodies at a resolution selected by their gameplay relevance;
4. settled or distant debris is merged into static clusters;
5. dust, chips, and non-gameplay fragments are deterministic cosmetic effects.

This preserves believable outcomes without attempting an impossible atom-level simulation.

The current compact renderer uses an energy-aware Cook-Torrance direct-light model: GGX normal
distribution, Smith visibility and Schlick Fresnel share the sun contribution between diffuse and
specular lobes. Metalness is explicit in the 48-byte vertex contract rather than inferred from
colour or roughness; only steel is metallic in the current authored material table. Per-voxel
ambient occlusion and bounded procedural albedo/roughness variation provide the interim surface
detail. Texture arrays, measured material scans, normal maps and image-based lighting remain the
asset-quality gate; the procedural model is a physically coherent baseline, not photorealism.

## Runtime ownership

The dedicated server owns commands, damage, fracture, structural separation, rigid-body creation,
and persistent world state. Clients predict only reversible player and weapon motion. A client never
announces that a wall was destroyed; it requests an action and receives the resulting transaction.

Delta protocol v6 uses monotonically increasing sequences, independent 128-bit pre/post
fingerprints for the static world and active body set, bounded fragments, and before-state
validation. Detached body membership and integer dynamic state travel in separate canonical frames;
both are reconstructed and validated before any static-world write. Missing data stops application;
the UDP client first requests an exact retained transaction and receives a canonical snapshot when
that sequence has expired. Snapshot installation itself rejects a mismatched body/state set, invalid
high-water mark, non-canonical descriptor, static/body voxel overlap, or inconsistent world
fingerprint before replacing any replica state. Corrupt or stale data cannot partially mutate a
replica.

Runtime bodies use compact non-zero 64-bit IDs reserved monotonically by the server only when the
whole detachment transaction commits. Their canonical geometry retains a separate 128-bit
fingerprint, and the active-body fingerprint mixes entity ID, geometry, and dynamic state. A future
persistent server must durably store the ID high-water mark with its world snapshot before it may
restore and allocate another body.

Network explosion requests cross an additional authority policy before the generic destruction
primitive: radius is limited to eight voxels, energy is non-zero and capped by both `2,000 * r²` and
50,000 units, and the target voxel centre must be within 120 metres of the authoritative player's
eye. Distance is evaluated with saturating 128-bit integer squares, so hostile extreme coordinates
cannot overflow into acceptance. This prevents arbitrary remote world edits and oversized weapon
profiles. It is not yet server-side aim validation: view orientation, ray obstruction, fire cadence,
ammunition and lag-compensated hit history remain part of the complete combat gate.

The first real transport slice now separates a generic `AuthorityCore<PeerId>` from its thin
nonblocking UDP process adapter. The core owns the world, sessions, commands, repairs, snapshots,
simulation, and retained replication state; it never owns or calls a socket. A fixed versioned
control codec admits source-bound development sessions, validates bounded explosion commands,
separates receive and simulation phases, and applies hard per-tick limits to ingress,
queued work, simulation, and egress. Complete deltas are released to clients only in contiguous
sequence order, including when UDP delivers later packets first. A process-level integration test
drives two independent sockets and proves identical world/body state and fingerprints. A second
test drops a whole sequence for one client, keeps later complete packets buffered, and recovers by
requesting the exact server-retained frames. Snapshot requests use a distinct control message; a
repair request for a future sequence cannot force snapshot work. The retention history, recovery
queue, repairs per tick, and shared send-attempt budget are all fixed. An expired-history miss is
observable and remains fail-closed while a canonical snapshot is framed below the MTU, hashed
against mixed/corrupt fragments, and paced at 16 frames per peer per tick. The client retains only
one four-MiB-bounded snapshot and reports missing fragments in fixed 64-bit windows. The server
retains the immutable encoded frames, retransmits only those selected fragments under the shared
egress budget, and waits for a matching snapshot-install acknowledgement. Live deltas are withheld
from that peer and their shared encoded buffers enter a per-transfer queue capped at 256 packets and
8 MiB. Only after the acknowledgement is that queue replayed in order before live delivery resumes.
A missing or overflowed catch-up delta stalls rather than silently skipping state. Process tests
cover selective repair of a lost snapshot fragment, explicit install acknowledgement, and a moving
body with post-snapshot catch-up.

The core takes a transport-specific application payload ceiling from 256 through 1,200 bytes and
encodes every delta and snapshot against it. Its ingress counter independently rejects work beyond
64 datagrams per tick even if an adapter drains too aggressively. Outbound delivery is a
nonblocking callback over an opaque ordered peer key, so UDP addresses are adapter state while QUIC
can use immutable connection IDs. An already-authenticated connection is admitted with a non-zero
unique session ID and an opaque 256-bit principal; a wire `Hello` cannot replace it. Disconnect,
legacy re-handshake, and idle expiry remove queued commands, recovery work, and snapshot state for
the old session before its peer key may be reused.

The reusable secure client boundary takes only an address, a validated TLS server name, and absolute
paths to a root-certificate PEM and application credential. It rejects root bundles beyond 256 KiB
or 16 certificates and credentials beyond the protocol's 4 KiB limit. On Unix the credential file
must deny every group/other permission. Credential bytes are trimmed only at their outside ASCII
whitespace, remain zeroizing memory, and never enter arguments or diagnostic values. A system CSPRNG
produces the non-zero client nonce; verified TLS/ALPN completes before the bounded reliable admission
stream, after which only size-checked encrypted datagrams are exposed to callers. The graphical
client owns a 512-datagram asynchronous receive queue so the window thread never waits on QUIC. It
drains at most the global per-tick receive budget and exposes every local overflow in its title and
smoke evidence; permanent world gaps still converge through exact retained-delta repair.

Authenticated character motion uses an independent player-state v2 datagram rather than entering
the ordered permanent-world transaction stream. Every third 60 Hz authority tick, the server emits
one complete session-sorted view at 20 Hz to every authenticated peer. The packet carries the server
tick, fixed-micrometre position, bounded compact velocity, exact integration remainders, grounded
state, and latest accepted input sequence for at most 16 players; its maximum encoded size is 910
bytes, below the secure 1,100-byte application
ceiling. A client atomically replaces its prior view only when the server tick advances, so loss does
not stall motion while replay, reordering, duplicate IDs, malformed flags, and partial views cannot
roll state backward. Absence from a newer complete view means the session left. At the cap, this
correctness baseline consumes 145.6 kbit/s of payload per client; interpolation, delta baselines,
and spatial interest remain required before the larger scale gate.

Remote rendering retains at most eight validated complete views and normally samples six server
ticks, or 100 ms, behind the newest tick. Position and velocity interpolation use fixed integers and
bounded world/motion inputs, avoiding frame-rate-dependent accumulation. A player leaving remains in
the older view until the newer boundary; a joining player appears exactly at that boundary. Targets
outside retained history clamp instead of extrapolating unbounded motion, while an old or malformed
packet leaves the whole interpolation history unchanged. The local controlled player instead
simulates each contiguous input immediately and retains at most 128 commands. A newer state restores
the exact position, velocity, grounded flag and integration remainders, removes the acknowledged
prefix, and deterministically replays the remaining commands. Wrong-session, stale,
impossible-acknowledgement and over-cap paths leave prediction unchanged. This simulation primitive
is proven through real QUIC. The graphical client preserves its pre-reconciliation camera position
for corrections up to two metres, exponentially halves that temporary offset every 80 ms, and snaps
larger discontinuities immediately. Prediction and collision remain on the exact reconciled state;
the offset is presentation-only and finite-input checked.

Remote presentation owns one immutable six-face avatar mesh and a fixed 16-matrix GPU instance
arena. Each accepted interpolated view rewrites only the compact transforms, excludes the local
session, and renders all remaining players with one instanced world draw and one instanced shadow
draw. The placeholder dimensions exactly match the authoritative 0.6 m by 1.8 m character bounds.
This deliberately proves the data/GPU path before committing to a skinned character asset and
animation graph.

The graphical network harness selects either the legacy loopback adapter or authenticated QUIC/TLS
from one complete file-backed launch contract. It starts from the identical deterministic demo
world, obtains its session through the selected transport, sends camera-relative inputs at 60 Hz,
predicts the local collider, reconciles every newer authority view, samples remote players through
the delayed interpolation history, and uploads those transforms to the instanced renderer. It also
orders the permanent-world delta stream, validates every transaction
through `ClientReplica`, remeshes changed chunk boundaries, uploads newly detached rigid bodies, and
tracks their replicated transforms. Mouse actions send requests only; the authority chooses and
broadcasts the resulting mutation. A release smoke run requires both real Vulkan clients to apply
the same destruction delta. The loopback authority emits the same player-state and world-delta
packets as QUIC so the transport-independent simulation and client presentation can be exercised in
two real windows. Every admitted graphical client requests the current snapshot, assembles its
bounded hashed fragments, installs it atomically, resets the ordered delta cursor, rebuilds the union
of old and new chunk slots, clears stale GPU bodies, uploads the authoritative body set, acknowledges
installation, and only then accepts player mutations. Stalled transfers request only missing
64-fragment windows. A late-join release smoke proves that a client can arrive after destruction and
render the already-modified snapshot. Live delta remeshing now uses one bounded background worker:
changed chunks are capped at 512 pending entries, prioritized from the current camera, and submitted
in batches of at most 256; body batches retain the existing 16-body/32,768-voxel cap. Results from a
stale immutable world snapshot are never uploaded and their chunks are requeued against the newest
replica. Initial snapshot rebuild remains synchronous until the residency-streaming gate.
After bootstrap, a complete future delta starts a bounded reordering grace period. The graphical
client initially waits 100 ms and retransmits after 250 ms, then adapts both values from clean repair
round trips using integer Jacobson/Karels smoothing. RTO stays between 100 ms and two seconds,
reordering grace between 50 and 500 ms, and repeated requests back off exponentially. Karn filtering
excludes ambiguous responses after retransmission. An intentionally impaired Vulkan smoke drops the
complete first transaction, accepts the second, obtains the retained frames, releases both in order,
reports RTT/RTO evidence, and requires its mesh queues to drain before success. An expired retained
sequence can still move the client back through the same atomic snapshot path.
This does not relax exposure: the legacy client/server pair and current secure authority refuse
non-loopback operation. The graphical QUIC client is transport-ready for a remote address, but that
path remains unavailable until the authority's remote security gate is satisfied.

The dedicated-authority sessions described above are deliberately loopback-only and unauthenticated;
the snapshot hash is an integrity check, not a MAC, and that legacy transport provides no
confidentiality, identity, packet authenticity, or congestion control. It must not be exposed beyond
the developer machine.

A test-only source-bound UDP proxy injects either its fixed regression profile or a finite
declarative trace independently for every direction/channel. A trace holds at most 256 steps, limits
delay to 128 pump ticks and deliveries to zero, one, or two, then becomes clean so recovery remains
provable. The queue is capped at 2,048 datagrams and 2 MiB, rejects datagrams above the application
MTU before allocation, and accounts received and delivered bytes by control, delta, snapshot, and
unknown channel. One process test repairs a completely lost first delta while later packets are
buffered; another repairs lost snapshot fragments and a lost first install ACK. A four-client test
replays distinct loss/jitter/duplication traces, obtains one clean adaptive RTT sample per client,
proves identical replicas and exact equality of offered server delta bytes. This is deterministic
fault injection and a fairness regression, not an Internet congestion algorithm or a substitute for
authenticated transport.

The secure runtime now wires the transport-independent authority core to Quinn/rustls TLS 1.3. A
server certificate is verified against explicit client roots and the `destructible-fps/1` ALPN. The
first bidirectional stream carries one
versioned opaque credential after TLS; an injected synchronous verifier maps it to an opaque
server-owned principal. The verifier performs bounded offline work and must never trust a
caller-supplied display identity. Client nonce, unique server session ID, and server nonce bind the
admission result to that connection. The implementation does not expose or use 0-RTT because gameplay
commands are not replay-safe.

Admission has a five-second internal deadline. Credentials are limited to 4 KiB; the
application-owned encoded and received buffers are securely zeroized on every exit path, credential
contents are never included in error text, and the verifier borrows them only for the duration of its
call. Transport-library and caller-owned memory remain governed by their respective lifecycles. Each
endpoint permits one bidirectional stream, no unidirectional streams, 16-KiB stream / 32-KiB
connection receive windows, 128-KiB send and datagram buffers, and at most 32 pending server
connections with 512 KiB total pending data. Gameplay payloads are limited to 1,100 bytes before send
and immediately after receive, reserving space below the project's 1,200-byte UDP target for QUIC and
IP overhead. QUIC supplies transport encryption, integrity, loss recovery for streams, congestion
control, and connection migration; application command IDs and authoritative sequencing remain
necessary for semantic replay protection.

Handshake and credential admission tasks run outside the fixed simulation tick and are capped at 32
concurrent attempts. The server reserves monotonically increasing non-zero session IDs in accept
order and samples each server nonce from the operating system CSPRNG. Successful admissions enter a
64-event control queue; authenticated gameplay enters a separate 256-datagram / 281,600-byte queue.
The tick drains controls first and at most 64 gameplay payloads, then advances repair,
commands, physics, and replication. A connection may submit at most 240 datagrams in one fixed
one-second window; protocol violations close immediately, while 32 consecutive full-queue drops
close a sender that keeps applying backpressure. The authority retains at most 16 active sessions.

The first concrete verifier accepts only RS256 access tokens against a pre-provisioned JWKS no larger
than 64 KiB and 32 keys. Every key needs a bounded unique `kid`, cannot declare non-signing use or
operations, and has a 2,048-to-4,096-bit canonical RSA modulus with exponent 65,537. The token header
cannot redirect key lookup; embedded `jwk`, `jku`, critical, encryption, and compression parameters
are rejected. Signature, exact HTTPS
issuer, exact audience, required `exp/iss/aud/sub`, optional `nbf`, `iat`, minimum remaining validity,
and a maximum 15-minute issued lifetime are enforced. A 4,096-entry fail-closed cache consumes each
bounded `jti` once until expiry. This deliberately requires a fresh credential after a failed or
disconnected admission. The internal 256-bit principal is a domain-separated SHA-256 digest of the
pinned issuer and validated subject; display names never become identity.

JWKS replacement validates the complete candidate before one write-lock swap, so a bad refresh keeps
the last accepted keys. Verification performs no DNS, HTTP, or file access and holds the key lock only
long enough to clone the selected immutable key. The standalone process loads a bounded static set
whose declared remaining validity must be between one minute and 24 hours. The verifier rejects new
admissions at expiry and the process stops rather than running on stale identity data.

The discovery client derives the standard well-known path from one parsed issuer, requires
TLS 1.2 or later with platform or explicitly bounded private roots, disables redirects and ambient
proxies, and uses three-second connection plus five-second total deadlines. Discovery metadata is
limited to 16 KiB, must repeat the exact issuer, and may point only to an HTTPS JWKS endpoint on the
same scheme/host/port. JWKS is limited to 64 KiB. Both known and chunked bodies are counted before
growth, and only JSON/JWK Set media types are accepted. When configured, an initial online refresh
must validate and atomically replace the bootstrap set before readiness. A bounded background task
then refreshes outside the fixed simulation tick. A successful complete-set swap grants exactly
three refresh intervals of monotonic validity; a failure leaves the previous keys and deadline in
force, so repeated failure eventually closes the authority rather than extending stale trust.

The process configuration is bounded to 16 KiB and rejects unknown fields, links, relative credential
paths, non-regular files, certificates over 256 KiB or eight entries, private keys over 64 KiB, and
JWKS over 64 KiB. It requires exactly one PEM private key, verifies key/certificate compatibility
through rustls, and parses the complete X.509 chain before opening the endpoint. Every certificate
must be currently valid with more than 60 seconds remaining. The final minute is reserved rather
than served: the earliest expiry minus that margin becomes a monotonic process safety deadline. A
reload policy also requires one complete watcher interval before that deadline. The static JWKS
bootstrap likewise requires more than one minute and reserves that final minute from its declared
absolute trust horizon. On Unix the
standalone process refuses UID 0, the private key must belong
to the service UID, and public trust inputs may belong only to root or that UID; their existing mode
constraints remain mandatory. The immediate parent of each path must be an equally owned,
non-writable, non-link directory. Credential content is never accepted through argv or printed.
Windows remains loopback-only while installer-owned DACL validation is designed.

Optional TLS reload retains only the fixed certificate/key paths, a bounded interval, and a shared
monotonic deadline. Its worker performs the complete startup validation again before giving a narrow
endpoint capability a new bounded server configuration. Quinn applies that identity only to future
handshakes, so active authenticated sessions continue normally. Mismatched or partially replaced
files never reach the endpoint, and failure leaves the prior deadline unchanged; external issuance
cannot silently turn into indefinite stale-certificate service. Certificate validation uses the
greater of observed wall time and startup wall time plus monotonic elapsed time, preventing a clock
rollback followed by reload from extending an old identity.

Real loopback QUIC tests cover a valid encrypted datagram exchange, an untrusted certificate, an
invalid application credential, a stalled admission deadline, a mismatched nonce echo, oversized
payloads in both directions, and 20 independent sequential sessions. A second real-QUIC integration
suite admits two clients, routes one destructive command through the authority, and verifies that
both receive the same canonical transaction. It also proves that invalid credentials never enter
authority state and that an over-rate session closes before simulation work. Five standalone-process
tests exercise real OIDC admission and command application, invalid-token rejection without a world
mutation, remote-bind rejection, expired-certificate rejection, and private-key permission rejection
before readiness. Two additional process cases prove that trusted HTTPS discovery replaces an
intentionally wrong bootstrap key before accepting a real OIDC command, while mismatched metadata
exits before readiness without disclosing the endpoint. Production issuer/root provisioning,
automated certificate issuance, reliable snapshot/control streams, OS-specific secret ACL validation,
and completion of the partially executable
[`non-loopback attack matrix`](remote-exposure-gate.md) remain required before remote exposure. Both
the direct runtime API and the validated file policy remain fail-closed to loopback.

An eighth standalone process case replaces the certificate and key files while one authenticated
session is active, waits for the bounded worker, then connects and applies a command through the new
certificate. The pre-rotation connection remains established and the final lifecycle counters prove
one attempted and successful reload with no failure.

A ninth process case leaves the valid chain unchanged across one watcher interval. The public-chain
fingerprint matches, the endpoint and exact monotonic deadline remain untouched, and distinct
installed/unchanged counters expose whether an external provisioner actually rotated the identity.

A tenth process case starts from a deliberately short but admissible certificate, corrupts the
private-key file only after readiness, observes repeated reload failures, and proves that the real
server exits unsuccessfully at the monotonic safety deadline with no installed or unchanged reload.
No production clock override or test-only configuration path is involved.

An eleventh process case gives the static JWKS bootstrap a 66-second absolute trust horizon. The
server reserves the final minute, performs no unconfigured refresh, and exits unsuccessfully at the
resulting monotonic safety deadline. Live discovery-outage expiry against a disposable production-
shaped realm remains a separate deployment proof.

A twelfth external-process case completes 32 concurrent TLS handshakes without sending application
hellos, observes refusal of the 33rd connection, and still completes all 240 server ticks with no
admitted session or simulation traffic. A thirteenth admits one OIDC client, injects a 1,101-byte
datagram through raw QUIC, and proves connection closure plus a protocol-rejection counter before
the payload reaches the authority core.

A fourteenth fills all 16 authority slots and concurrently offers up to 240 maximum-sized malformed
datagrams per session. The 256-event shared queue reports pressure, at least one abusive session is
closed after 32 consecutive drops, already queued data is either rejected with the closed session or
decoded as malformed, and no command is applied. A fifteenth performs 32 complete authenticated
connect/disconnect cycles and finishes with exact admission/disconnection parity and no leaked active
session.

The LAN deployment policy is intentionally a separate offline module. It parses a 16 KiB-bounded,
versioned JSON contract and can describe only one short-lived RFC1918 or IPv6 ULA deployment. It
rejects wildcard, public, loopback and IPv4-mapped IPv6 addresses; non-canonical, overlapping or
mixed-family firewall ranges; wildcard certificate names; unsafe OIDC URLs; and any runtime limit
that differs from the compiled authority ceilings. Its type has no endpoint or server constructor,
and the secure launch configuration neither imports nor references it. A later promotion must add a
distinct capability that proves the declared interface/address relation, certificate SAN, exact
issuer, firewall state, ACLs and monotonic expiry before calling a non-loopback binder.

The adjacent host-attestation module takes the policy in the safe direction only: it reads a single
bounded interface snapshot and proves an exact, operational and uniquely indexed name/address/prefix
assignment. It has no socket, endpoint, authority or configuration dependency. Duplicate address
ownership, an absent/down interface, a missing index, an unsafe prefix or more than 256 address
records fails closed. This point-in-time proof must be repeated immediately before any future bind
and monitored afterward; it does not attest firewall, OIDC or service ACL state.

The sibling certificate-attestation module also points only from the policy toward public evidence.
It accepts an eight-entry/256 KiB maximum canonical PEM chain and exactly one 256 KiB-bounded,
self-issued reviewed CA. It rejects duplicate, embedded, unrelated or out-of-order certificates;
requires a single literal policy DNS SAN plus explicit server-auth usage; verifies the cryptographic
path with rustls/webpki; and requires every certificate to remain valid through policy expiry plus
the same 60-second margin reserved by the runtime. Unix reads inherit the integrity ownership,
parent-directory and `O_NOFOLLOW` contract, while Windows DACL proof remains outstanding. The proof
contains length-prefixed SHA-256 fingerprints but the CLI emits only a boolean. This module has no
private-key, resolver, socket, endpoint or server dependency and cannot promote the policy.

## Planned engine layers

### First playable slice — delivered

- Linux window and raw first-person input through `winit`;
- discrete-GPU preference with a safe `wgpu` Vulkan backend;
- deterministic multi-material test range rendered as face-culled chunk meshes;
- fixed-step movement, gravity, jump, voxel collision, ray targeting, and crosshair;
- rifle and explosive actions crossing the same server command, 1,200-byte fragmentation,
  out-of-order reassembly, fingerprint validation, and client-replica path covered by tests;
- distance-prioritized initial chunk streaming plus boundary-aware bounded background remeshing,
  both using shared immutable snapshots and 16-chunk initial batches;
- per-vertex voxel ambient occlusion, a 2,048² comparison shadow map, procedural lighting,
  roughness, surface variation, fog, and filmic tone mapping with exactly one display transfer;
- bounded CPU frame distributions and non-blocking GPU timestamp readback with a fixed four-slot
  ring and graceful capability fallback;
- conservative camera-frustum chunk culling with explicit resident, visible, world-draw, and
  shadow-draw counters;
- auto-terminating real-GPU smoke mode.

Gate evidence: the release smoke test created a Vulkan surface on the RTX 4050 Laptop GPU, validated
the WGSL pipelines, uploaded the complete representative world, presented continuously, and exited
cleanly. This is point-in-time developer-machine evidence, not a portable FPS guarantee.

### Milestone 1 — production-grade Vulkan visual slice

- Linux and Windows window/input abstraction;
- bindless material tables and physically based shading;
- large-world streaming beyond the bounded local bootstrap window;
- hierarchical-Z occlusion culling and indirect drawing beyond the delivered CPU chunk frustum;
- HDR output, temporal anti-aliasing, and measured dynamic resolution;
- a benchmark capture for the RTX 4050 Laptop at 1,920×1,080.

Gate: the playable scene reports separate CPU/GPU frame-time p50/p95/p99, avoids render-thread
meshing stalls under the agreed destruction load, and holds its frame budget at 1,920×1,080.

### Milestone 2 — structural physics

- bounded incremental topology analysis around changed voxels, foundation/authored anchors, and
  canonical detached-island descriptors (delivered as an isolated server-side primitive);
- revalidated rigid-body descriptors with fixed integer centre of mass, diagonal inertia, mass,
  bounds, canonical geometry fingerprint, and independent monotonic entity identity (delivered);
- atomic static-world detachment and protocol-v6 body replication with independent fingerprints,
  compact 64-bit IDs, hostile-input limits, and replica reconstruction (delivered);
- local-space body meshes produced by the bounded background worker, fixed-capacity GPU transform
  instances, body frustum culling, and world/shadow rendering (delivered);
- deterministic 60 Hz micrometre state, gravity, mass-weighted blast impulse, inertia-weighted
  off-centre angular response, canonical fixed-quaternion integration, rotation-aware per-voxel
  conservative static sweeps, collision-generated torque, material ground friction and normal
  restitution, exact vertical body columns, stable stacking, wake propagation, sleeping, bounded
  sweep-and-prune with atomic overload rollback, protocol-v6/snapshot-v2 state replication, and
  mass-centred GPU rotation with conservative rotated render bounds (delivered; static angular motion
  is sampled at a radius-derived maximum travel of 0.25 m with at most eight substeps and 262,144
  tested cells per body tick; over-budget or intersecting rotation stops; inclined vertical body
  support uses occupied physical-bottom ordering plus rational-time refinement through at most 4,096
  conservative voxel-proxy pairs, and fail-closed support on budget exhaustion);
- four-pass X/Z dynamic contact from swept coarse body bounds, rational orthogonal-overlap validation
  at time of impact, bounded refinement through at most 4,096 canonical pairs of conservative
  rotated per-voxel proxies, deterministic contact centroids, inverse-mass separation, material
  restitution, off-centre angular impulse, momentum-preserving tangential friction, impact wake-up,
  fail-closed coarse separation on pair-budget exhaustion, and deterministic short-chain propagation
  (delivered);
- control-protocol-v3 static construction with shared command replay ordering, per-session resource
  budgets, deterministic material costs, coordinate/occupancy/face-support validation, conservative
  dynamic-body exclusion, six-metre authoritative-player reach, conservative integer line of sight,
  fingerprinted delta replication, and local plus authenticated-QUIC execution (delivered);
- fixed-step authoritative player position, velocity, gravity, jumping, static voxel collision,
  bounded terminal velocity and swept contact resolution, newest-input retention, independent replay
  sequencing, stale-input expiry, 16 distinct recyclable spawn slots, disconnect cleanup, and
  packet-rate-independent simulation (delivered as the server movement slice);
- persistent foundation and material constraint graph integrated into authoritative transactions;
- compression, tension, shear, and connection limits by material;
- local stress propagation after damage;
- unsupported island extraction (delivered for topology-changing voxel edits);
- rigid-body mass, centre of mass, and inertia derived from geometry (delivered);
- exact convex dynamic-body contact manifolds, full vertical impulse exchange, gyroscopic response,
  deeper constraint-island convergence, clustering, and distance-based solver budgets.
- player-state replication, client prediction and reconciliation, authoritative view/weapon state,
  persistent inventories, recipes, removal tools, material selection UI, dynamic-body character
  contact, and dynamic-body attachment for construction.

Gate: destroying a load-bearing member produces a repeatable progressive collapse and never stalls a
60 Hz server tick in the agreed worst-case scene.

### Milestone 3 — real transport

- separate nonblocking UDP authority, versioned control handshake, source-bound development
  sessions, bounded queues and per-tick work, ordered client delivery, and a real two-client
  process test (delivered for unauthenticated loopback only);
- exact short-gap repair from a count-and-byte-bounded delta history, prioritized before new
  simulation, with deliberate whole-sequence loss and convergence coverage (delivered);
- canonical bounded snapshots, corruption rejection, paced transfer, selective bitmap repair after
  a deliberately lost fragment, acknowledged atomic install, and retained-delta catch-up during
  active body motion (delivered for loopback);
- bounded deterministic latency/jitter/loss/duplication/reordering injection across real sockets,
  including delta recovery, selective snapshot recovery, ACK retry, and per-channel byte evidence
  (delivered for loopback tests);
- bounded TLS 1.3 QUIC transport and post-TLS credential admission (delivered and wired to both the
  authority and graphical client; remote operational exposure remains gated);
- bounded offline RS256/JWKS OIDC validation, atomic rotation, replay cache, stable principal mapping,
  bounded process configuration, and trusted pre-readiness plus periodic refresh (delivered;
  production issuer/root provisioning remains);
- transport-independent bounded authority core, opaque peer IDs, authenticated principal binding,
  transport-sized framing, and a regression-preserving loopback UDP adapter (delivered);
- bounded async admission/gameplay queues, monotonic session IDs, CSPRNG server nonces, per-session
  ingress limiting, lifecycle cleanup, and two-client QUIC authority convergence (delivered);
- unreliable sequenced deltas plus reliable snapshot/control channels;
- configurable stochastic and trace-replay network simulation beyond the delivered fixed profile;
- spatial interest management and per-client bandwidth budgets;
- join-in-progress and persisted-world recovery.

Gate: at least four processes remain synchronized under injected loss and jitter, and an intentionally
corrupted client is repaired from a bounded snapshot.

### Milestone 4 — FPS vertical slice

- high-quality first-person controller;
- projectile ballistics, penetration, ricochet, and material energy loss;
- one firearm and one explosive;
- spatial audio and destruction effects;
- dedicated server browser/session bootstrap;
- automated soak and adversarial command tests.

Gate: four-player match, persistent destruction, stable frame pacing, no authoritative divergence,
and a reproducible performance report.

## Non-goals for the spike

- claiming AAA visual quality before representative assets and lighting exist;
- synchronizing thousands of cosmetic particles as gameplay state;
- trusting client-side physics decisions;
- rebuilding operating-system windows, audio codecs, or GPU drivers;
- hiding missed performance targets behind average frame rates.
