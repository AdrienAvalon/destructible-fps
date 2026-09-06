# Rejected full-resolution haze experiment

On 2026-09-06 a candidate based on parent `51342f6` was implemented, tested, measured and
**rejected for default integration**. Runtime shader and test files were restored byte-for-byte
to that parent. This report is evidence for the decision, not a delivered rendering feature.
The existing unoccluded haze limitation remains. The photorealistic reference remains unfulfilled.

## What was tested

The candidate reused existing world-space sky depth maps to estimate sky-lit air at four
camera-to-surface midpoints, with eight upper-hemisphere comparisons each (up to 32 additional
PCF lookups per shaded fragment). Exponential segment weights retained the old open-sky transfer
and artistic 92% cap. The homogeneous transmission relation follows
[PBRT's media treatment](https://www.pbr-book.org/3ed-2018/Volume_Scattering/Media);
the sky-only quadrature and cap were our approximations, not PBRT's volume integrator.
No depth passes, textures, geometry, authority, dependencies or resource caps were added.

Production-WGSL GPU tests verified zero added sky light in covered sealed rooms, retained surface
extinction, roof removal, moving/removing bodies, and illuminated outdoor air before an occluded
doorway endpoint. Binary CPU geometry rays supplied an independent reference, with <0.03 tolerance
for PCF edge footprints; scalar transfer tests used 1e-5. Other cases covered zero/negative density,
zero distance, tiny/extreme optical depth, visibility clamping and continuous 32–48 m coverage.
Long sealed halls outside coverage still acquired haze; narrow apertures and illumination
transitions remained under-sampled. These candidate-specific tests are in the saved experimental
patch, not the restored runtime test suite.

## Measurements that rejected promotion

Candidate standalone inspector SHA-256:
`77fef50ade1956bdae318b8af5fb7cb44b92a4a29f71980bf863d9da49e98e7a`.
Linux Vulkan release, 1440 × 900, 4x MSAA, exposure 0.75, i7-13700H, RTX 4050 Laptop.
Parent measurements are the prior sequential receipt in [polygon rubble](polygon-rubble.md).
Milliseconds, short uninstrumented runs; not sustained combat or cross-hardware qualification:

| View | Parent GPU p50/p95/p99 | Candidate GPU p50/p95/p99 | Candidate CPU with present p50/p95/p99 | Candidate RSS KiB |
|---|---|---|---|---|
| Fracture | 2.805 / 2.851 / 3.590 | 4.404 / 4.470 / 5.306 | 5.020 / 5.242 / 5.912 | 285088 |
| Approach | 2.705 / 2.754 / 3.527 | 4.298 / 4.338 / 5.047 | 4.896 / 5.065 / 5.650 | 283408 |
| Wide | 2.370 / 2.404 / 2.727 | 3.718 / 3.814 / 4.306 | 4.328 / 4.559 / 4.928 | 287228 |

A separate 15-second forced-refresh lighting stress run, with 16 synthetic remote players,
increased total GPU p50/p95/p99 from 2.413/3.088/3.115 to 3.689/4.386/4.417 ms.
The HDR pass increased from 1.569/2.206/2.229 to 2.767/3.454/3.482 ms. Both runs had zero
abandoned timestamp samples and no sky-cache hits. This is not a networked combat test.
Sequential clocks/thermal state are not controlled; the consistent approximately 53–59% median
increase across these observations is sufficient to reject this cost/benefit tradeoff, not a
universal slowdown coefficient.

Two owned native RenderDoc captures completed with Vulkan replay, frame 400 and fourteen textures:
approach `target/tooling/renderdoc-_p52o3qx` (302550413 bytes, 221 draws) and fracture
`target/tooling/renderdoc-oxi9b1wt` (307065300 bytes, 219 draws). Compared with parent approach
`renderdoc-1813af1w`, the actual image was only modestly darker inside. It still lacked detailed
architecture, dense believable rubble, natural terrain, vegetation and a convincing light balance.
The candidate did not deliver a meaningful scene-wide visual change for its measured cost.
The wide performance run passed, but wide capture and motion acceptance were deliberately not
pursued after rejection. No three-view/motion visual acceptance is claimed.

## Validation, review and recoverability

`/tmp/fps-occluded-haze-WAfijI/validate.sh` completed with exit 0 for the candidate: formatting,
strict all-target Clippy, debug/release all-target suites, targeted network/security/OIDC,
doctests, all normally ignored GPU tests explicitly in both profiles, 500-event destruction,
100 structural iterations, 1024-body/300-tick physics, 20 snapshot iterations, locked standalone
builds, industrial mesh benchmarks and all native smoke runs. Physical fingerprints, finish keys
and mesh vertex/index counts remained unchanged. Passing those tests did not override visual
and performance rejection.

Graphify guided ownership inspection and was refreshed. Claude analysis and runtime-diff review
both returned usable advisory results. The full runtime shader diff was reviewed; tests were
explicitly summarized, not represented as fully externally reviewed. Codex confirmed projection
and orthographic depth conventions in source. A fragment-output golden comparison and thin-wall
temporal coverage remained missing. No review or test proved photorealism.

The exact candidate shader/test diff is retained locally at
`/tmp/fps-occluded-haze-WAfijI/rejected-candidate.patch`, alongside logs; captures are excluded
from Git. These temporary artifacts are not a durable backup. Three older parent-of-parent
captures were moved intact into `target/tooling-archive-lHJXgO/`, not deleted, keeping the current
parent comparisons. No tools, drivers, privileges, active infrastructure or remote repositories
were changed. The next priority is the cohesive reference scene specified in the production
roadmap, not another full-resolution haze implementation.

## Restored baseline verification

After restoring source, `/tmp/fps-visual-reset-L9euWP/validate.sh` also completed with exit 0:
the same full workflow, 555 ordinary tests in each build profile, six normally ignored GPU tests
explicitly in each profile, two doctests, security/network checks, all benchmarks and native runs.
The rebuilt standalone inspector SHA-256 returned exactly to the parent receipt:
`27ff423cb65aec64f1e64b80607d5ad95eeff7ad079a818c088e356ff785285f`.
`git diff --exit-code -- src tests Cargo.toml Cargo.lock assets` confirmed no runtime/content diff.
The saved candidate patch also passes `git apply --check` against that restored source.

Restored GPU p50/p95/p99 were fracture 3.113/3.154/4.010 ms, approach 2.608/2.651/3.317 ms and
wide 2.370/2.410/2.745 ms, with peak RSS respectively 270020, 269956 and 269872 KiB. Restored
forced-refresh stress measured 2.688/2.873/2.906 ms total GPU and 1.816/1.975/2.004 ms HDR.
The repeated baseline is not numerically identical to the earlier observation, reinforcing the
clock/thermal caveat; it still confirms a materially lower cost than the rejected candidate.
Only documentation and the stricter scene-wide visual priority are promoted by this revision.
