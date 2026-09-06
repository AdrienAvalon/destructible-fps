# Directional rifle slice

The left mouse button now submits a directional rifle intent, not a small radial explosion.
Both native demos use it; R requests reload, and the window title displays magazine/reserve and
reload status. Right click remains the **experimental radial blast**. This increment is a gameplay
and authority foundation, not calibrated firearm physics, a finished combat loop or photorealism.

## Authority and material response

`RifleCommand` carries only a nonzero monotonic command ID and three signed 16-bit aim components.
The authority uses its fixed-micrometre player eye, normalizes the direction again, and fixes the
range at 120 metres. There is no client origin, hit position, range or energy in the rifle request.
The local standalone adapter quantizes its camera; this does not make its local camera an
anti-cheat boundary. Network commands always obtain the eye from the server character state.

The fixed ray visits at most 512 cell intervals/contacts, with rational integer face ordering,
an inward-rounded micrometre endpoint, simultaneous tied-face advancement and conservative
zero-length edge/corner contacts, including tied crossings on a parallel grid plane. It computes
the complete bounded visit list before any mutation.
Distinct faces rounding to the same micrometre retain a blocking contact instead of skipping the
grazed cell. The regression fixes an explicit near-corner miss, not just mathematically exact ties.
A shot starting inside material evaluates that material. Exact seam contacts can stop a shot
without damaging a cell; continuous coplanar walls instead receive the positive-length owner hit.
These are conservative grid rules, not a finite-radius projectile collision mesh.

Each positive interval costs work according to its length, material and remaining cell integrity.
The finite 750-unit budget can pass through a broken glass cell and reach wood behind it. Three
axial shots remove a pristine full-length wood cell, while adjacent untraversed cells retain their
integrity. Longer oblique intervals cost more; brick, concrete and steel resist more than wood.
Unused sub-integrity work is absorbed when a cell stops the shot, not converted into an extra hit.
**All work/resistance constants are fictional game units, not joules or measured ballistic data.**
Rounding up each integrity unit's cost can make full-cell work exceed the nominal material reference.

One integrity value still represents a **whole one-metre cell**. Repeated hits at different places
on that same cell share its damage. A short chord can eventually remove the entire cell, not a
bullet-sized tunnel. True sub-cell cavities, surface hardness, strain rate, fragmentation, spall,
ricochet, projectile travel time/drop, recoil and weapon-specific measured data remain future work.

Existing significant bodies stop the ray using their rotated conservative fixed AABB, expanded by
two micrometres for rotation rounding. A body contact inside a static cell conservatively blocks
before that cell's damage. This can overestimate cover in empty regions of an irregular body;
fine body collision, body damage, penetration and rifle momentum transfer are not implemented.

The shared atomic damage transaction validates detachment/body/protocol limits and rolls back
all voxel edits on refusal. A rifle-severed support promotes the remaining component to a body
at its ordinary spawn state, **without an explosive impulse**. The existing 60 Hz body simulation
then owns its motion. This is topological support removal; it does not silently enable or calibrate
the separate opt-in elastic structural lab on the full industrial map.

## Ammunition, wire and lifecycle

The fixed server simulation clock owns a 30-round magazine, 90 reserve rounds, six-tick minimum
shot interval and 120-tick reload (100 ms and two seconds at 60 Hz). Geometry transaction ticks
cannot accelerate cadence. The server computes a candidate weapon state and installs it only
after the complete synchronous world/body transaction succeeds. Thirty-two queued shot requests
in one tick cannot all spend ammunition or mutate the map; promotion failure spends none.
An accepted miss spends a round and emits an empty ordered delta; reload emits another empty delta.

Control protocol **v4** adds exact 28-byte rifle and 22-byte reload requests. Existing v3 clients
must be rebuilt along with the server; old control versions are rejected, not silently interpreted.
The world delta and snapshot codecs are unchanged. Shared command ordering covers rifle, reload,
construction and the legacy debug explosion request. Truncation, extra bytes, zero aim and foreign
sessions are rejected before authoritative mutation.

A separate exact 49-byte `DFWS` v1 personal status packet is sent at 20 Hz, within the ordinary
bounded outbound budget. It contains session, simulation tick, accepted command high-water mark,
magazine, reserve, next shot tick and reload deadline. This is 980 application payload bytes per
second per player, **excluding QUIC/UDP/IP overhead**. The client accepts only its current session,
ignores older/duplicate ticks and rejects malformed bounds or regressed command acknowledgement.
It is read-only HUD state, never a client-authorized outcome; the server rejects reflected packets.

Ammo/cadence is **ephemeral per-session authority state**, not part of the world's persistent
fingerprint or snapshot. Ordinary snapshot repair does not reset server ammo; disconnect/re-admission
does. Persistent inventory, reconnect identity policy and authoritative combat replay are unfulfilled
gates. Commands still use lossy gameplay datagrams, without a new reliable resend/acceptance UI;
lost status refreshes at the next broadcast. No weapon pickup/refill, player health, death or audio
is implied by these counters.

The legacy explosion RPC still accepts a client-selected centre/radius/energy under its existing
envelope. It is a debug/prototype capability, **not a production weapon authorization policy**.
Neither development UDP nor the secure authority is newly exposed outside loopback by this lot.

## Repeatable validation and profiling

```bash
cargo test --lib ballistics::
cargo test --test ballistics --test rifle_network
cargo run --release --bin rifle-benchmark -- --iterations 500
```

The benchmark resets identical small fixtures outside the timed region. Each sample executes three
shots, framing/reassembly and two replicas at 1,100/1,200-byte MTUs. Wood, glass/wood, steel and
support detachment are separate cases. It reports first sample, p50/p95/p99/max and exact payload,
frame/change/body counts, and fails on a changing workload or replica divergence. It is not a full
server tick, render frame, high-body-count worst case or allocation profiler.

For real graphical validation, run an industrial authority and two clients with their ordinary
documented transport options, adding `--smoke-rifle --smoke-seconds 10` to each. The odd session
fires three times through the loading bay at the timber barricade, reloads, fires again and then
moves. Actions are spaced from their actual send and gated on the shooter's previous authoritative
acknowledgement; slow loading cannot compress them into an invalid burst. The other observes.
One client can additionally use `--smoke-resnapshot --smoke-drop-first-delta`. Success requires
actual post-snapshot geometry deltas, repaired loss, drained mesh queues, subsequent acknowledged
movement, the missing targeted timber cell with its neighbour intact, and personal ammo 29/87
versus 30/90.
This scenario requires the industrial map and both session parities; an unmatched map or single
observer must not be interpreted as coverage. See `performance.md` for actual run evidence.
