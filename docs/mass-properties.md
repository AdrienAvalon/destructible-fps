# Exact remaining-material mass properties

`mass_properties.rs` integrates actual rectangular material volumes from both uniform `World` and
typed `RefinedWorld`. The same accumulator now supplies the real `RigidBodyDescriptor` constructor's
mass and centre of mass. A hole removes its exact volume and density contribution, while damage
integrity alone does not erase matter. Non-air integrity zero remains massive; air contributes
nothing. Sub-kilogram fragments retain a rational mass rather than rounding to a massless body.

This is a shared physical-property calculation, **not fine rigid-body activation**. The current
body membership, connectivity, contacts, renderer and live delta/snapshot codecs still represent
whole metre cells. They cannot accept a `RefinedWorld`. The new raw moments are derived from the
same whole-cell membership on both ends of the existing codec, not transmitted as a new protocol.
Fine topology, atomic static-to-fine-body transfer, full-tensor dynamics, fine body rendering and
transport remain required before removing that boundary.

## Exact integrals, explicit pivot

The integrals follow the usual definition of mass moments and the symmetric inertia tensor; see
[MIT Dynamics, lecture L26](https://ocw.mit.edu/courses/16-07-dynamics-fall-2009/dd277ec654440f4c2b5b07d6c286c3fd_MIT16_07F09_Lec26.pdf).
The rectangular integration and integer scaling below are this engine's implementation choices.

All geometry coordinates are local to the minimum selected metre cell, in lattice units with
`L=256` per metre. For a leaf `[a,b)` with density `rho`, define `w=rho*volume_lattice`. Accumulate:

```text
W     = sum(w)
F_i   = sum(w * (a_i + b_i))
Q_ii  = sum(4*w * (a_i^2 + a_i*b_i + b_i^2))
Q_ij  = sum(3*w * (a_i + b_i) * (a_j + b_j))
mass  = W / L^3 kg
COM_i = F_i / (2*W) local lattice units
```

The six `Q` entries retain second moments, including cross-axis terms. Subdivision of identical
density into differently damaged leaves leaves `W/F/Q` exactly unchanged. Selection order must be
canonical, and canonical leaf geometry prevents overlaps; this calculator does not accept arbitrary
overlapping user boxes as a public input. Work statistics are separate from physical properties.

The full tensor is exact **about its stated pivot**, not advertised as an exact tensor about an
unrepresentable rational centre. The default pivot is the nearest 1/256 millimetre to the exact
world centre, with half ties away from world zero; it is then converted to local coordinates.
Explicit pivots use the same units and must remain inside the selected cells' bounding box. This
keeps lever arms bounded. The 1/256 mm pivot precision is not the 1/256 m geometry resolution.

For local pivot `p` in 1/256 mm, the product second moment numerator about that pivot is
`T_ij = 1,000,000*Q_ij - 6,000*(p_i*F_j + p_j*F_i) + 12*W*p_i*p_j`.
Tensor diagonal numerators are `T_yy+T_zz`, etc.; off-diagonal entries are **negative** `T_xy`, etc.
All have the positive kg*m² denominator `12 * 256^3 * 256000^2`. No per-leaf rounding or floating
state is used. `ExactRatio` has private fields and no arbitrary-input constructor; its representation
is not necessarily reduced, so general fraction comparison must cross-multiply with appropriate
bounds rather than assuming structural equality means mathematical equality.

Direct exact-to-whole-mm COM rounding preserves the existing descriptor's nearest-mm, ties-away
semantics; it does not double-round through the finer pivot. A half tie translated across world
zero can change which side receives the quantized pivot. Tests cover that explicitly; exact mass
moments are translation covariant, but arbitrary rounded-pivot coordinates are not claimed to be
translation invariant at ties.

## Work, arithmetic and authority boundaries

| Resource | Hard limit |
| --- | --- |
| Selected occupied cells | 16,384 |
| Selected extent on each axis | 16,384 metres including the final cell |
| Total canonical leaf visits, including air | 131,072 |
| One source fine page | existing 8,192-leaf volume contract |

Count, order and extent are checked before integration. Each page is charged before its leaves are
visited; failure discards the unpublished candidate and never changes the source world. The
simultaneous leaf cap does not promise that all selected cells may independently reach their page
maximum. Tests accept sixteen 8,192-leaf pages and explicitly refuse one additional uniform cell.
Empty selections, empty selected cells, zero density, excess extent/work and arithmetic overflow
are distinct errors. No partial or saturated properties are returned. Integration needs no new
collection allocation; returned moments contain only fixed-size values.

The integer proof uses the full `u16` density range, not only current material densities: cell
count <=2^14, per-cell volume <=2^24, density <2^16, local coordinates <=2^22, local pivot <2^32.
Thus `W<2^54`, `F<2^77`, `Q<2^102`; the largest tensor expression intermediate remains below
2^124, inside signed i128. Global COM calculations use i32 world-page coordinates and remain
inside the same type before rounding. Accumulations and final tensor additions/multiplication
are checked; bounded intermediate leaf products use that explicit proof. A private adversarial
test fills 16,384 disjoint diagonal cells with density 65,535 across every maximum extent, then
evaluates both opposite-corner pivots. It also verifies explicit zero-density rejection before
changing the accumulator. Changing any of these bounds requires a new arithmetic proof.

Body import now applies the 16,384-cell ceiling even if a caller supplies a larger custom
`BodyLimits`; existing default/runtime limits are unchanged. Direct world import checks this
before describing the island. Fine mass integration alone proves neither connectedness nor
structural support: current coarse bodies retain their existing BFS connectivity check, and
fine body construction remains unavailable. Snapshot decode must still revalidate membership
and other authority state; physical moments are not authentication or permission to detach cells.

## Legacy angular response remains explicit

The live descriptor stores the new immutable raw moments and uses them for mass and COM. It still
derives `inertia_diagonal_kg_mm2` using its prior whole-cell intrinsic rounding and rounded-mm
pivot; angular impulses still use that approximation. Full tensor entries are available for the
next solver migration but are **not** silently substituted into the existing diagonal solver.
No fine mass is paired with a whole-cube collision proxy in the live authority. Old and new builds
should still be rebuilt together; unchanged packet layouts are not a general mixed-build promise.
Neither calibrated material strengths nor realistic progressive collapse follows from mass
integration alone. The remaining solver must handle cross-axis angular response and gyroscopic
terms, stable contact manifolds and work scheduling before fine-body activation.

## Validation and repeatable measurement

```bash
cargo test --lib mass_properties::tests
cargo run --release --bin mass-benchmark -- --iterations 100
```

Tests cover analytic cubes, thin slabs, bores, smallest lattice cubes, full material density,
integrity-only repartitioning, negative/extreme coordinates, exact signed half ties, and 64 fixed
seeded four-cubed material grids checked against an independent subcube-integral oracle. Component
checkpoint reconstruction yields identical physical moments. Actual coarse body import uses the
same properties as uniform/fine-specialized World queries; all ordinary body/network regression
tests remain required. A fine component checkpoint test is not live fine-body network evidence.

The benchmark repeatedly measures fixed uniform, maximum-leaf and bored sixteen-page worlds,
checking identical moments for identical physical occupancy and exactly one-quarter mass loss
for the bores. It also measures the actual 16,384-voxel body import, including canonical membership
preparation, connectivity, collision columns and derived metadata. Each scenario reports first,
p50/p95/p99/max and work counters. Construction of the fine fixture is outside its timed region;
body input cloning is inside the body-import region, destruction of the returned descriptor is
outside. First samples are not cache-flushed cold-start measurements. These are import/integral
costs, not frame time, a complete server tick or sustained fine-world combat.

## Contradictory review disposition

The Claude analysis/review verified the box-integral coefficients, pivot conversion, signed COM
rounding and i128 bounds. The final review received the complete new runtime module/tests and
complete body-constructor diff, excluding benchmark/docs and unchanged codecs/volume internals.
Its concerns about sending fine mass through old whole-cell body membership do not describe the
implemented type boundary: fine bodies are still refused, not downgraded. The new hard body cap
is documented on `BodyLimits`, with a test rejecting 16,385 cells even under a 20,000-cell override
before world lookups or connectivity work. Existing material data has no solid zero-density
entry; a test covers all seven solid IDs at zero integrity, and the explicit zero-density guard
protects future table changes. A conversion-overflow error was clarified rather than called zero
mass.

Codex verified the actual snapshot decoder reconstructs descriptors through
`RigidBodyDescriptor::from_replicated_voxels`, and snapshot installation independently rebuilds
and compares the complete descriptor. The out-of-order framed snapshot test now requires actual
bodies and explicitly compares raw mass properties and the derived full tensor after decoding.
These post-review checks were run locally, not presented as a second Claude review. World access
does clone a fine volume handle during the bounded two passes, but that handle uses `Arc<[Leaf]>`:
it does not copy 8,192 leaves. No extra temporary cell vector was introduced on that speculative
basis. Benchmarks measure the resulting traversal cost.

## Measured validation, 2026-09-06

Implementing tree based on `0df4df7`, Rust 1.97.1 release/fat LTO, Intel i7-13700H, Linux
7.2.2-1-cachyos. The final benchmarks ran sequentially after compilation/tests finished, with no
other intentional benchmark, unchanged/unpinned frequency policy and no cache flushing. Native
benchmark SHA-256: `6c03763363638c2750eed40b0e255de0d1f20bff3c695ac3e85f3f22d86b27c9`.

Milliseconds, 100 samples per fixed scenario:

| Scenario | First | p50 | p95 | p99 | Maximum |
| --- | --- | --- | --- | --- | --- |
| Uniform 16 cells | 0.00263 | 0.00158 | 0.00159 | 0.00164 | 0.00263 |
| Fine 16 pages / 131,072 leaves | 1.84227 | 1.81777 | 1.92163 | 1.96842 | 2.00495 |
| Bored 16 pages / 80 leaves | 0.00239 | 0.00198 | 0.00201 | 0.00212 | 0.00239 |
| Actual 16,384-voxel body import | 4.58959 | 3.95307 | 4.01967 | 4.12436 | 4.58959 |

The fine and uniform complete moments agree exactly at 10,400 kg. The bored case has 64 solid
leaves and weighs 7,800 kg. Child-process peak RSS via `getrusage` is 12,024 KiB, including fixture
and launch overhead, not allocation counts or isolated leaf payload. Raw properties occupy 192
bytes per descriptor, approximately 192 KiB for one copy at the current 1,024-body cap; retained
snapshots/replicas add their own copies. They are computed at import, not every physics tick.
No new heap collection is needed by the integrator. A separate user-space `perf stat` run took
575.49 ms task-clock; the raw per-PMU counters include CPU migration/scaling and are retained in
the artifacts, not summed into an invented whole-system counter. No privilege or host setting
was changed for profiling.

The existing 8,192-voxel structural fixture's promotion p50/p95/p99 is 1.745/1.785/1.832 ms. The
retained prechange executable measured 1.686/1.780/1.812 ms, with the same island fingerprint
`a9e84bc57ccb01a92095462009d56477`. That artifact's SHA-256 is
`92b5d336dcf93ebdbc79503f7206284ff4ed570098440ec6d02dcd4768abc37d`. It is not an independently
attested identical-toolchain commit A/B build. These observations do not establish a speedup;
the change adds physical information and a small measured promotion cost. Multi-millisecond
fine integration and large body import require bounded worker scheduling before fine activation,
not synchronous work on rendering or packet reception.

Final validation passes formatting, strict all-target Clippy, 476 ordinary tests in **each**
debug/release profile, all six actual Vulkan tests explicitly in each profile, two compile-fail
doctests and 22 offline tooling tests. The required targeted network, secure transport/authority/
process and OIDC suites pass. The new properties have 13 targeted tests; snapshot restoration
also explicitly verifies the new metadata after real framing/reassembly and canonical install.

Required destruction-500, structural-100, physics-1,024-bodies/300-ticks and snapshot-20 checks pass.
Destruction preserves both replicas at `e309309ed7c650fc3ac194e285e983b2` with 908 frames at
MTU 1,200 and 0.567 MiB payload. The unchanged stacking fixture reaches 1,024/1,024 sleeping
bodies, p99 tick 1.188 ms; snapshot total p99 is 13.777 ms for 1,181 frames/1.351 MiB. These
unchanged coarse workloads do not prove fine contacts, 32-player performance or durable saves.

The real native range and industrial breach smokes exited cleanly. The industrial view uses
RTX 4050 Laptop/NVIDIA 610.57.04, Vulkan, 1440×900, 4×MSAA, 50,723 faces after 208 fractured
coarse voxels. CPU frame-work p50/p95/p99 is 16.668/16.825/33.318 ms (includes presentation
pacing); GPU total is 2.501/4.224/4.247 ms with zero abandoned samples. This five-second view has
no rigid bodies and is not body-motion coverage; the separate structural-lab smoke supplies that
coverage. Neither is photorealism, fine rendering or sustained combat acceptance evidence.

The separate twelve-second native `--structural-lab` smoke also returned zero: one committed
structural failure, two GPU-rendered bodies, two sleeping bodies after the fall, no failed/stale
job, overflow, pending work or simulation error. This lab uses explicitly synthetic material
strengths, not a calibrated real building. Graphify update/doctor reported a fresh index with
zero dangling endpoints.

Raw local artifacts: `/tmp/fps-mass-properties-NPn0me` (temporary evidence, not production saves).
No package, live infrastructure, account, remote listener, profiler privilege or agent permissions
changed. The full game goal remains active, with fine rendering a next visible integration target.
