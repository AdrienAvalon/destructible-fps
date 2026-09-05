# Performance evidence

Performance observations are point-in-time results tied to a command, scene, build, resolution, and
machine. They are not portable guarantees or substitutes for the later platform matrix.

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
analysis, body promotion, protocol-v4 fragmentation, reordering, decode, reassembly,
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
result is comfortably below the 12 ms server-work target but still excludes horizontal impulses,
friction, restitution, rotation, interest filtering, serialization, socket I/O, and other gameplay
systems. Those costs require separate promotion evidence before the Stage 2 exit gate can pass.
