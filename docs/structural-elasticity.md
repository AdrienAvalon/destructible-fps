# Structural load solver foundation

`src/elasticity.rs` adds an actual six-degree-of-freedom equilibrium calculation toward PHYS-01:
loads redistribute through surviving connections instead of treating every ground-connected
component as infinitely strong. This is currently an offline-tested foundation for future server
jobs. Neither `AuthoritativeServer` nor a graphical client invokes it yet. It does not fracture a
voxel, animate deformation, or claim that the requested progressive collapse is already playable.

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
`f`, the effective area is `L² f`, the second moment is `L⁴ f² / 12`, and the current torsion
constant is approximated by twice that second moment. Damage does not remove occupied mass.

The force law is linear elastic and isotropic. This remains a lattice approximation, not a
calibrated volumetric solid model: wood grain, masonry mortar, unilateral contact, steel yield,
reinforcement, plastic hinges, buckling, dynamic loading and accurate square-section torsion are
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
results across operating systems. A future authority converts validated outcomes into canonical
integer transactions; clients must never independently decide fracture from these floats.

Invalid call budgets leave the job unchanged. Numerical failure, total-budget exhaustion and a
linear-range violation latch an error; `finish` exposes no partial result. `Pending`, `Unresolved`
and `Supported` must remain distinct in any future gameplay adapter. A failed solve must not silently
classify a building as safe, destroy it by default, or become hidden indestructibility. Domain
decomposition, bounded backlog, stronger preconditioning and explicit failure handling are promotion
requirements before general gameplay integration.

The model knows only its supplied mechanical domain. It cannot detect omitted neighbouring world
geometry or stale world revisions: the future domain builder must prove completeness, include
actual boundary support/load contributions and retain a revision/fingerprint for revalidation.
Omitted voxels are never implicitly fixed anchors. Large connected worlds exceed this first cap;
passing the small fixtures is not evidence of whole-map structural support.

## Validation and progression

```bash
cargo test --lib elasticity::tests
cargo run --release --bin structural-load-benchmark -- --iterations 20
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

1. Extract complete affected mechanical domains from authoritative voxel/material snapshots; prove
   boundary contributions and stale-result rejection under concurrent destruction/building.
2. Calibrate strength and direction-dependent material response; derive fracture candidates from
   compression, tension, shear, bending and torsional demand, preserving significant fragment mass.
3. Schedule bounded immutable jobs outside receive/presentation paths, then revalidate and commit
   integer damage, structural separation and significant body assignments atomically.
4. Exercise weak wood, explosive wall breach and overloaded remaining supports in the same two-client
   playable sequence, including repair/late join and repeated failure/rebuild. Validate multi-OS,
   sustained latency and memory before claiming realistic synchronized collapse.
