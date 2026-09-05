# Structural load solver foundation

`src/elasticity.rs` adds an actual six-degree-of-freedom equilibrium calculation toward PHYS-01:
loads redistribute through surviving connections instead of treating every ground-connected
component as infinitely strong. `src/structural_jobs.rs` now extracts authoritative world domains
and runs immutable calculations on a bounded worker. The server exposes an explicit scheduling/
revalidation API; the normal game loop does not automatically schedule or commit structural load
failures yet. The opt-in [coarse failure adapter](structural-failure.md) can now prepare a
mass-preserving fracture and commit it as an ordinary replicated transaction. The numerical kernel
does not animate deformation or establish that realistic progressive collapse is already playable.

## Mechanical representation

Each supplied voxel-centre node has three translations, three rotations, mass, integrity, elastic
modulus, Poisson ratio, optional external forces/moments and an explicitly clamped boundary flag.
Cardinal neighbours share beam elements with axial extension, torsion and both coupled bending
planes. The six-DOF lattice approach and harmonic adjacent-material moduli are motivated by
[Hiller and Lipson's 2014 model](https://www.creativemachineslab.com/uploads/6/9/3/4/69340277/dynamicsimulation.pdf),
especially equations 1–12. We do not use its dynamic integrator or claim its full nonlinear model.

Unlike the initial Euler-Bernoulli proposal, the implemented bending blocks include shear
deformation. Their closed-form two-node Timoshenko coefficients follow the
[numgeo beam formulation](https://j-machacek.github.io/numgeo/theory/elements/beam.html), using a
square-section shear correction of 5/6. With cell spacing `L` and minimum endpoint integrity fraction
`f`, the effective area is `L² f`, the second moment is `L⁴ f² / 12`, and the Saint-Venant square
torsion constant is `0.1406 L⁴ f²`, replacing the initial polar-area-moment approximation `2I`.
The square-bar [AutoFEM validation example](https://autofem.com/examples/torsion_of_a_beam_with_the_squ.html)
provides the torsion coefficient. Damage does not remove occupied mass.

The force law is linear elastic and isotropic. This remains a lattice approximation, not a
calibrated volumetric solid model: wood grain, masonry mortar, unilateral contact, steel yield,
reinforcement, plastic hinges, buckling, restrained warping and dynamic loading are
not represented. Benchmark elastic constants and masses are explicit test inputs, not validated
game-material presets. Real material calibration must precede gameplay failure thresholds.

The implementation refuses a converged solution outside its conservative small-deformation range:
any rotation over 0.1 rad or relative translation across a bond over 0.1 cell. It cannot be used to
carry a large collapse through this linear model. Detached-body dynamics remain a separate owner.

## Units, solver and limits

Input/output translations are metres, rotations radians, loads newtons and moments newton-metres.
Internally `q = [translation, L * rotation]` and `b = [force, moment / L]` give all residual components
force units. Matrix-free Jacobi-preconditioned conjugate gradients use canonical node/bond order.
Acceptance recomputes the true residual `b - Kq`, rather than trusting a recursively updated one;
its norm must satisfy `max(absolute_tolerance_n, relative_tolerance * initial_load_norm)`.

The hard envelope is 4,096 nodes, fewer than three cardinal bonds per node, 512 total iterations and
at most 32 iterations per `advance` call. Positions must be unique and sorted, all physical inputs
finite and range checked, and every connected component must reach an explicitly clamped node.
An unattached component is not stabilized with artificial springs. Full anchors carry all applied
force/moment, including their own gravity load, in the reported support reactions.

`ElasticModel::new` owns bounded model construction; `ElasticJob::new` allocates bounded scratch.
`advance` performs no allocations and reads no mutable world state. The iteration cap is a work
bound, not a wall-clock deadline; model construction and `finish` also cost time and belong off the
network/presentation paths. Batching changes scheduling, not floating-point operation order.
These `f64` values are local analysis scratch, not new replicated state or a promise of bit-identical
results across operating systems. The explicit server adapter converts validated outcomes into canonical
integer transactions; clients must never independently decide fracture from these floats.

Invalid call budgets leave the job unchanged. Numerical failure, total-budget exhaustion and a
linear-range violation latch an error; `finish` exposes no partial result. `Pending`, `Unresolved`
and `Supported` must remain distinct in any future gameplay adapter. A failed solve must not silently
classify a building as safe, destroy it by default, or become hidden indestructibility. Domain
decomposition, bounded backlog, stronger preconditioning and explicit failure handling are promotion
requirements before general gameplay integration.

The numerical model knows only its supplied mechanical domain. The authoritative adapter below
owns completeness and stale-state checks. Omitted voxels are never implicitly fixed anchors.
Large connected worlds exceed this first cap; passing the small fixtures is not evidence of
whole-map structural support.

## Authoritative domains and immutable jobs

An explicitly configured `StructuralMaterials` provides finite, kernel-compatible Young's moduli
and Poisson ratios for all seven solid material IDs. There is no synthetic default disguised as
material calibration. Mass comes from the same material density as significant fragments and uses
the world's one-metre cell spacing. The current job applies self-weight only, with the same gravity
as body dynamics; damaged occupied voxels retain all mass. Blast impulses, contact forces from
resting bodies, temperature and material fatigue are not included in this static analysis.

The caller submits a trusted server-selected **free solid seed**, not a client-specified fragment.
The worker traverses the whole face-connected free component and reads all its cardinal neighbours,
including air. Actual clamped solid neighbours are included but not traversed through. Finding the
first foundation does not terminate the search as it can for simple connectivity. Authored anchors
without a solid voxel never support the component. Coordinates use checked neighbour arithmetic.
Empty seeds, fixed seeds, invalid integrity, unanchored components, oversized domains and solver
failures remain explicit errors, not successful partial support classifications.

This is a complete **clamped-boundary problem**, not all matter connected through soil. Another
free branch across an infinitely clamped node does not change this domain's displacement, but its
load is not in the reported reactions. A shared foundation's bearing check would require separate
aggregation of every contributing domain and counting its own weight only once. These reactions
must not be treated as total soil/foundation capacity. Flexible foundations require including their
coupled free nodes, not reusing this rigid-boundary simplification.

`World` now shares immutable dense chunks with `Arc`; cloning copies map metadata, not every
4,096-cell payload. The first actual write to a shared chunk copies that chunk; no-op writes do not
copy or retire it. Wire data, fingerprints, tick semantics and logical dense-payload statistics are
unchanged. Logical payload bytes do not measure shared physical RSS.

Submission checks backpressure before capture. It accepts at most 512 resident chunks and limits
historical hash-table capacity metadata to twice that cap. Reclaiming entries alone does not shrink
a `HashMap`, so the retained high-water bound prevents a once-large map from silently making snapshot
work unbounded. An over-limit history remains an explicit rejection until a later bounded residency/
compaction design handles it; no synchronous full-map rebuild is hidden on the caller's path.
Up to 4,096 authored anchor positions are copied as bounded configuration metadata.

The worker retains an observation for every read chunk. An occupied observation holds the actual
immutable chunk allocation, not a wrapping revision counter. Changing data, removing/recreating
the chunk, or changing data then rolling it back cannot revalidate the old pointer while it is
retained. An absent observation holds a vacancy epoch and requires continued absence. Chunk
creation/reclamation changes that epoch, preventing empty/occupied/empty ABA. This deliberately
conservative global epoch can also invalidate an absent-boundary job when a distant chunk is
created or reclaimed. Remote writes in **existing unobserved chunks** do not invalidate it. Fine
grained absence histories/fair dirty-domain scheduling remain necessary before heavy construction
churn can be claimed starvation-free.

Every result also retains an authority/configuration identity. Changing anchors or materials,
or cloning the authority, creates a new identity. A result from another instance cannot be used
even if voxel bytes and fingerprints match. `AuthoritativeServer::structural_result` revalidates
all these observations and returns a borrowed solution: inspecting it keeps that authority
immutably borrowed. This is not a mutation permit. `commit_structural_failure` repeats validation
before promoting the worker's coarse fracture plan into the existing integer protocol.

The scheduler has one thread and **one outstanding slot**, including a completed-but-unconsumed
result. Repeated submissions return `Busy`; they cannot build an unbounded backlog. Explicit
cancellation retires even a result already waiting in that slot, and polling drains it before reuse.
Work checks cancellation per traversal node and between eight-iteration solve slices. Drop closes
both channels, requests cancellation and joins the thread. Model construction and final force
extraction are bounded but not preemptible; cancellation is not a hard wall-clock deadline.
`WorkerStopped` is terminal for that scheduler instance. Its owner must report the failure and
replace the scheduler; no automatic panic retry hides a deterministic solver defect or promotes
an incomplete result. Material mapping is exhaustive, so adding an enum variant requires an
explicit mapping/calibration decision instead of silently extending a worker-side array index.

Read observations are capped separately at `7 * 4096 + 1`, including absent neighbouring chunks;
they need not be fewer than the resident chunk count. At most the snapshot's 512 dense chunks
can be retained, initially about 4 MiB of logical voxel payload; configuration, map/node/bond
metadata and numerical scratch add to this. This is not a measured total-memory budget, nor a cap
on results that a caller deliberately retains after consuming the scheduler slot.

## Validation and progression

```bash
cargo test --lib elasticity::tests
cargo test --lib structural_jobs::tests
cargo run --release --bin structural-load-benchmark -- --iterations 20
cargo run --release --bin structural-jobs-benchmark -- --iterations 20
```

Tests compare axial compression, torsion, shear-aware cantilever deflection and pure bending moments
against analytical values on all signed axes at three cell scales. Additional cases check zero
forces under rigid translation/rotation, stiffness symmetry/diagonals, eccentric force and moment
balance, gravity, support removal, damaged and disparate-material compliance, integrity 1, exact
node bounds, extreme coordinates, invalid inputs/options, budget latching, large deformation and
identical results under different solver batch sizes. A heterogeneous three-dimensional lattice
checks global force/moment balance and internal/external work at three cell scales, including loads
on the anchors. The benchmark adds a 64-cell column, 64-cell
cantilever, 64×32 wall and the same wall with only two foundation supports remaining. It requires
convergence, force balance and analytical long-cantilever agreement, not merely a timed exit.

Next promotion work remains explicit:

1. Extend the explicit complete-domain worker to fair automatic dirty-domain scheduling and bounded
   decomposition/residency for real maps. Preserve shared-clamp reaction semantics and stale-state
   rejection under distant new-chunk churn, not only edits in existing chunks.
2. Calibrate strength and direction-dependent material response beyond the new explicit isotropic
   brittle section envelope; add crushing geometry and contact feedback while preserving mass.
3. Integrate automatic worker scheduling outside receive/presentation paths and broadcast/retain
   the new atomic coarse fracture transactions; never treat unresolved jobs as stable support or
   automatic collapse. Load continuation/nonlinear handling must cover over-range configurations.
4. Exercise weak wood, explosive wall breach and overloaded remaining supports in the same two-client
   playable sequence, including repair/late join and repeated failure/rebuild. Validate multi-OS,
   sustained latency and memory before claiming realistic synchronized collapse.
