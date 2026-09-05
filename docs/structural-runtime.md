# Tick-driven structural lab

The coarse load-failure policy now runs automatically in the local playable session and the shared
transport-independent authority core. It remains opt-in with explicit synthetic constants. This
is a playable **physics laboratory**, not the final environment, calibrated weapon behaviour,
compressive crushing, large-map support or a photorealism milestone.

```bash
cargo run --release --bin playable-demo -- --structural-lab
cargo run --release --bin playable-demo -- --structural-lab --smoke-seconds 12
cargo run --release --bin dedicated-server -- --structural-lab --bind 127.0.0.1:0 --max-ticks 300
cargo run --release --bin structural-failure-benchmark -- --scheduled --iterations 20
```

In the interactive lab, capture the mouse and aim at the left root of the suspended wooden beam.
Press **F** for a synthetic partial test charge. It weakens the beam without directly removing it;
the background assessment then decides whether to sever it. Normal left/right/middle mouse actions
remain available. This extra probe does not redefine the ordinary rifle's current damage profile.
The floor remains present, detached matter falls and both significant bodies retain their material.
The title shows pending work, worker activity, committed cuts, lifetime failures and incomplete
coverage. The normal showcase and lab flags are mutually exclusive.

The graphical smoke starts intact, waits for an initial assessment and applies the same test charge
after one second. Before its requested deadline it requires exactly one structural cut, two rendered
and sleeping bodies, no outstanding meshing/structural work and no structural fault. GPU/window
initialization or tick replication failure now propagates to a nonzero smoke result. The charge's
actual trigger time is logged. Eight seconds passed locally; use twelve seconds for more scheduling
margin, not as a hardware-independent CI guarantee. The lab has 1,791 solid cells
in 12 chunks; its initially seeded cantilever domain has five free cells and one authored clamp.
The clamp does not imply that the solver assessed the entire floor or every other authored branch.

## Shared ownership and sequencing

Trusted startup code supplies `StructuralSimulationConfig` to `DemoSession`, `AuthorityCore` or
`SecureDedicatedServer`. It contains authored anchors, validated elastic/strength tables and an
explicit list of initial seeds. The normal demo and server still use their existing policy unless
configured. The secure API retains certificate verification, credential admission and loopback
policy; no wire message can enable the structural policy or choose a fracture result. The secure
process JSON/CLI does not yet expose an authored structural scene manifest.

Each accepted explosion or construction delta queues its changed cells and their six surviving
non-fixed neighbours. This includes partial integrity changes. The worker follows their **complete**
connected free domain to actual authored clamps; reaching the node cap fails with `DomainTooLarge`,
never an invented fixed boundary. A cut's surviving neighbours enqueue subsequent branches, so a
second overloaded support can fail on a later tick without a manual solve/commit call.

`AuthorityCore::complete_tick` applies queued commands, polls/commits structural work and then
advances rigid-body physics. Structural deltas use the same broadcast, retained-delta repair and
snapshot catchup paths as other transactions. Receive handlers only parse and enqueue. `DemoSession`
uses the same runtime, reverse-framed replication and existing mesh worker; its tick reports dirty
chunks and new body IDs before transforms are updated. Clients never run the floating solve.

The authority decides when a worker completion enters a tick. This does not promise identical
completion timing across machines or lockstep replay without recording the accepted transaction
history. Monotonic sequence and before/after fingerprints still define the single accepted order.

## Work bounds and explicit incomplete states

- One worker and one outstanding job, at most one result/commit and one submission per tick.
- A deduplicated FIFO of at most 8,192 seeds; at most 64 obsolete seed checks per tick.
- A stale completion cannot clear pending seeds; its seed moves to the tail rather than starving
  another structure. A current complete domain coalesces its redundant queued seeds.
- Complete extraction positions remain available even after numerical failure, so five seeds in
  the same out-of-range beam do not cause five identical numerical solves. This is still a failed
  assessment, not stable support. Incomplete extraction cannot subsume unknown coverage.
- The existing 512-chunk snapshot / 4,096-node / seven-fragment / body and protocol bounds remain.
  The new complete-domain position list is bounded additional memory, not a world-sized cache.
- A five-second **observed job latency** deadline requests cancellation and latches a stopped
  runtime. A late completion cannot mutate the world. The tick never waits, joins or spawns a
  replacement. This is not hard thread preemption: shutdown still owns and joins the worker;
  an actual uninterruptible engine bug would require process-level recovery.

`failed` is a lifetime count, `last_failed_seed` identifies its latest input and `last_error()`
retains the last exact failure. `incomplete()` includes failures as well as overflow/stopped flags;
the title therefore cannot report complete coverage merely because a failed queue became empty.
An ordinary solver,
configuration or capacity error is not retried continuously; subsequent real edits may enqueue new
assessment work. The counter does not automatically erase that earlier incomplete assessment.
Queue overflow latches `overflowed`, including after the accepted queue drains: discarded coverage
has not magically been restored. Worker death and deadline expiry latch `stopped`.

For this lab, explicit recovery is to stop the local session and start a freshly authored scene.
Low-level callers can construct a replacement runtime outside the tick path, with a complete
author-supplied seed set; that alone does not prove large-map coverage. The network core rejects
late/repeated policy replacement. Safe in-match reseeding from a persisted map, rescan/backpressure
recovery and load-domain decomposition remain implementation work; neither an empty queue nor a
process restart proves a saved match's structural consistency.

`observe_changes` does bounded neighbour work per supplied committed change, not constant work
independent of transaction size. Coalescing scans the bounded pending queue and commit preparation
still has bounded allocation/map costs. The scheduled benchmark reports these tick costs separately
from wall-clock first/second rupture latency at 60 Hz; it does not establish a sustained combat budget.

## Validation boundaries

The integration tests deliver the actual automatic fracture and first movement to two replicas at
both 256- and 1,200-byte MTUs. One client deliberately loses the creation transaction: its movement
waits in the ordered inbox until retained repair arrives. It then matches both bodies and the world;
360 physics ticks settle both bodies, and a third client's network snapshot/ack reconstructs them.
A separate real UDP adapter test drops creation, buffers movement and repairs through its actual
owned loopback sockets. The finite dedicated-process lab check additionally verifies readiness,
initial automatic assessment and clean exit. A real loopback QUIC test uses two authenticated connections and checks damage, automatic
creation and movement on the same shared core. The local session test checks dirty mesh notifications,
one-time body IDs and conserved 3,250 kg of timber after falling/settling.

Unit tests cover stale FIFO fairness, bounded skips/overflow (including stale requeue at exact
capacity), a deadline without slot reuse, canonical-order metadata, submit failure reporting,
complete failed-domain coalescing, and refusal of a domain beyond capacity without mutation.
Local and network startup-only guards are also tested. These tests do
not calibrate wood, crush a supporting cube, validate dynamic contact loads or establish a final
weapon/physics/art pipeline. Those limitations remain in [structural-failure.md](structural-failure.md).
