# Opt-in coarse structural failure

This PHYS-01 increment connects actual equilibrium forces to server-owned static-to-body
transactions. It is deliberately not a claim of realistic building collapse, calibrated material
presets, automatic dedicated-server integration or a visual improvement. The existing game's
ordinary loop does not enable this optional policy yet.

## Ownership and lifecycle

Configure both `with_structural_materials` and `with_structural_strengths`, submit a trusted free
solid seed to `StructuralScheduler`, then poll a `CompletedStructuralJob`. The worker extracts the
complete clamped-boundary domain, solves equilibrium, assesses section demand and prepares fragment
descriptors. `commit_structural_failure` consumes the opaque completion and revalidates every
occupied/absent-chunk and configuration observation before an integer transaction can be created.
Changing elastic parameters, strengths or anchors invalidates earlier jobs. A cloned authority
cannot consume another authority's result. Current dynamic-body motion and unrelated edits in
existing unobserved chunks do not invalidate this static self-weight calculation.

`Ok(None)` means the configured, converged assessment found no over-threshold free section; it is
not a declaration that the whole structure is safe under all loads. An unconfigured policy, cancelled
or stale job, failed solve, descriptor failure and live capacity rejection are explicit errors.
In particular, `OutsideLinearRegime` is unresolved, never permission to cut automatically or quietly
label the structure supported. Automatic scheduling must expose failures/backlog and implement a
bounded retry/alternative strategy; repeated capacity rejection is not a valid final destruction
policy. No error counter or UI backlog has been integrated into the ordinary server yet.

The caller must schedule commits at authoritative tick boundaries and feed returned deltas into
its ordinary broadcast/retained-repair path. There is no new client command or client-selected
fracture outcome. The standalone benchmark and tests exercise this API explicitly; full process/
graphical gameplay integration remains subsequent work.

## Section model and explicit strengths

`SectionStrength` supplies positive finite tension, compression and shear capacities in Pa for each
of the seven solid material IDs, in the range 1–1e12 Pa. There is no default. The initial criterion
is an isotropic, brittle maximum-stress envelope, not wood grain, masonry joints, a steel yield
surface, fatigue, crack-energy propagation or buckling. The
[USDA Wood Handbook's mechanical-properties chapter](https://research.fs.usda.gov/treesearch/62244)
distinguishes wood's directional behaviour and effects such as growth features; one generic Wood
enum is insufficient to claim a calibrated material. Benchmark profiles are synthetic inputs.

For the same square section as the solver, let `f=min(integrities)/255`, `b=L sqrt(f)`, `A=L² f`
and `I=L⁴ f²/12`. At each bond end:

- signed axial stress is `N/A`, positive in tension;
- the extreme bending contribution is `(|Mu|+|Mv|) b/(2I)`;
- transverse shear is bounded by `1.5 hypot(Vu,Vv)/A`;
- square torsional peak shear is `|T|/(0.208 b³)`.

The sum of absolute bending moments is the maximum at a **square corner**, not a circular-section
resultant `hypot(Mu,Mv)`. Using the weaker endpoint's integrity for the whole bond follows the
current shared-section stiffness model; it does not resolve independent half-voxel damage shapes.

Tension/compression use the positive/negative axial extreme plus bending. Transverse and torsional
shear peaks are added conservatively even though their maxima need not coincide. Both bond-end
envelopes are considered for each free endpoint's material. This includes the root bending moment
of a cantilever, rather than losing it because the root node is fixed. A fixed node itself is not
severed by this local assessment: total foundation bearing and other branches behind a shared
clamp require a larger coupled problem. Explosions can still remove those authored supports.

The [AutoFEM square-bar example](https://autofem.com/examples/torsion_of_a_beam_with_the_squ.html)
supplies the `0.208` stress and `0.1406` stiffness coefficients. The solver now uses the square's
Saint-Venant torsion constant rather than `2I`, removing an approximately 18.5% stiffness excess.
The 50 mm, 1.5 m, 1000 N.m test recomputes twist from the stated E/nu/J: 0.0220199 rad. It does
not use the displayed 0.022168 rad, which differs from the formula with those inputs. Stress recovery gives
38.461538 MPa for its stated torque and width. Restrained warping remains outside this lattice.

The greatest nonnegative demand/capacity ratio is quantized downward to per-mille units and
saturated at `u32::MAX`. A candidate requires a value greater than 1000, leaving an explicit
quantization deadband near the threshold. Exactly one free voxel with maximum quantized demand is
selected; position order breaks ties. All floats remain worker scratch. No floating displacement,
stress or client-computed decision enters the replicated transaction.

## Matter-preserving coarse severance

The chosen voxel becomes its own significant body with unchanged material, integrity and mass.
The remaining domain is partitioned into face-connected components; those without a surviving
clamped node become separate bodies. Supported components remain static. Cutting one degree-six
cell gives at most seven pieces and at most 4,096 moved voxels. Connectivity, integer mass/inertia
and collision-descriptor construction happen on the worker using the immutable snapshot.
Every accepted cut consumes at least one live body slot and one entity ID even when no other
component detaches. A regression executes 1,024 real cuts, then confirms that the next cut is
explicitly refused without partial mutation. Bounded debris merging/reclamation remains necessary
for a long-running match; the body cap cannot be treated as permanent indestructibility.

Commit checks live body and voxel capacity, per-body limits, non-overlapping memberships, checked
ID/sequence/tick reservation and protocol framing before mutating anything. Geometry fingerprints
are independent of the temporary worker IDs; actual monotonic IDs are assigned from current live
state. Thus a different explosion can legitimately allocate bodies while the job is running.
Failure does not consume IDs, player command replay state or construction inventory. The resulting
ordinary protocol-v6 delta moves static voxels to body membership, with both pre/post fingerprints.

Bodies start in their original integer pose, at rest. There is no invented explosive kick or
injected elastic energy. This dissipative approximation does not simulate release of strain energy,
pre-fracture deformation or moving/resting-body loads on the remaining structure. Each subsequent
rupture needs a fresh solve; detached bodies are not recursively re-fractured by this policy.

Crucially, the failed cell still has its **whole coarse cube collision shape**. This models severed
connections, not compressive crushing: a broken cube may still carry contact load. A vertical
column therefore does not prove credible crushing/collapse just because it changed to dynamic
bodies. Sub-voxel fragments, crushed volume, reinforcement and coupled contact/structural loads
remain required before PHYS-01 is complete. The verified falling case is an unsupported cantilever,
not a claim that every overloaded building now collapses realistically.

## Validation

```bash
cargo test --lib structural_failure
cargo test --lib elasticity::tests
cargo test --test structural_failure
cargo run --release --bin structural-failure-benchmark -- --iterations 20
```

The end-to-end cantilever test first accepts an intact beam, then applies a partial explosion,
rejects its stale pre-blast job, commits a fresh stress-driven severance and reconstructs the
fragmented delta. Two replicas conserve all mass, track the two falling bodies for 360 fixed ticks
and agree after settling at the floor; a late snapshot and a duplicate transaction are also checked.
Other tests cover another live blast consuming IDs, deleting an anchor, editing another domain
cell, cancellation/configuration identity, live capacity and counter exhaustion, rollback-free
rejection, root moment recovery, all signed stress axes, integrity 1/0 and nonfinite inputs.

The large benchmark performs two explicit solves/commits on a brick panel with two weak supports.
Its stiff `E=1e11 Pa` fixture is synthetic and selected to stay within the linear model. The initial
`E=1e9 Pa` variant exceeded the linear range and was correctly refused; it is not counted as success
or hidden by changing the solver tolerance. Handling that real modelling limitation remains a
promotion requirement. Timings distinguish worker preparation from atomic commit and do not
measure network delivery latency or sustained active-combat performance.
