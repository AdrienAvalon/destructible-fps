# Industrial roof ruin — native geometry increment

This continues the [ruined-factory visual target](industrial-visuals.md), not its final acceptance.
The fine inspector now has an irregular opening above the left bay and thinner concrete roof
sections. All four snapshots share this authored roof; their stage names describe the low wall
patch only. This is not a roof collapse caused by the player's weapon.

## Geometry and limits

`mesh/fine/fixture/roof.rs` profiles the main sheet at y=14 and raised cap at y=18 to an actual
25 cm thickness. Full y=14 strips remain below the raised walls beginning at y=15. An open notch
spans x∈[-19,-10), reaching the front boundary z=17 from a deterministic irregular rear edge.
Cell coordinates are inclusive in the main sheet's [-20,20]×[-16,16] loops; physical cuts are
half-open. Profiles use 16/256 m strips, not a displaced renderer surface or different collider.

The initial support test caught a mistaken assumption: the projecting beam in z=16 has AIR below,
unlike the z=15 masonry row. Its central portion is now absent, with only two 50 cm stubs connected
laterally to the existing side columns. The ragged brick row rests on the unchanged y=12 lintel.
Window frames and columns below the roof are preserved. These positive-area contacts and connected
sheets are **topological evidence, not structural-load calibration**. Section sizes, cantilevers,
reinforcement, stress-dependent failure and mass-conserved debris remain work. No newly authored
roof rubble is presented as the missing slab's conserved mass.

Authoring requires exact expected uniform source material, preserves the input world and tick,
and only removes material. Its 1,455 changes are replayed in six sorted transactions of at most
256 changes; the largest contains 647 before/after leaves under the unchanged 32,768 limit.
The temporary authoring list is bounded at 2,048 changes. All ordinary world, worker and resident
limits remain unchanged; no authority/network behavior or material/shader/lighting setting changes.

Tests cover source refusal, preserved occupied cells outside the exact envelopes, raised-wall
contacts, lateral stub roots, full lintel contact, exact 250,000 µm ray chords through both sheets
and a genuinely open skylight. Flood fills on the authored 16-unit footprint lattice find a single
connected component for each retained roof sheet; the main sheet touches a continuous corner post
to the ground. Subtractive same-material edits reconstruct the exact result through bounded replay.
All-stage full-versus-dirty mesh-byte checks cover the complete 132-chunk production fixture.

The old normal-only geometry oracle now lives in a private `cfg(test)` fixture constructing the
original masonry and rubble directly. Its four pre-treatment checksums were not regenerated.
This replaces the earlier integration-test trick of removing newly added window cells; there is
no production legacy switch. The current production fixture is still fully tested with its roof
and windows present. Normal-fitting acceptance still applies only to the eight original shards.

## Validation receipt — 2026-09-06

Parent `0079fa9`, standalone inspector SHA-256:
`b4ad0f2dd46308b803ac9bdbee2ccef4f9e355234a2bec241469fd6609d0a6d4`.
`/tmp/fps-industrial-roof-07Za35/validate.sh` ran fail-fast and finished with exit 0: formatting,
strict all-target Clippy, 531 ordinary tests in each of debug/release, targeted network/secure
transport/authority/process/OIDC tests, two compile-fail doctests, all six explicit real-GPU checks
in both profiles, the four required destruction/structure/physics/snapshot benchmarks, twenty fine
extractions per stage, and five native smoke runs. Graphify update/doctor passed; conclusions were
checked in source and actual tests, not inferred from its index or Claude's verdict.

Initial whole scene: 58,915 quads, 251,730 vertices, 440,910 indices. Content tests keep headroom
below 400,000 vertices/1,200,000 indices; the inspector's actual limits remain 524,288/1,572,864.
There are 1,801 fine pages initially, 1,798 after the low breach, and 9,783 leaves in that final
snapshot. Final dirty replacement: 57,288 vertices, 127,110 indices and 5,154,867 total work over
eight one-chunk jobs. Maximum job work across all stages is 1,476,726, below 4,194,304. Total work
is not a single-job budget. Source preparation is outside extraction and frame timings.

Native release Vulkan, 1440×900, 4× MSAA, exposure 0.75; Linux 7.2.2-1-cachyos, Rust 1.97.1,
i7-13700H, RTX 4050 Laptop 6,141 MiB, NVIDIA 610.57.04. Measurements ran after compilation without
RenderDoc or screen recording, with normal engine GPU timestamps. Clocks and compositor pacing are
not controlled; these short tests do not establish a speedup, sustained combat or multi-OS budgets.

| Native view | CPU with present p50 / p95 / p99 (ms) | GPU p50 / p95 / p99 (ms) | Peak RSS (KiB) |
| --- | --- | --- | --- |
| Industrial fracture, 12 s | 3.097 / 3.257 / 3.579 | 2.489 / 2.515 / 2.940 | 269,112 |
| Industrial approach, 12 s | 3.011 / 3.172 / 3.442 | 2.397 / 2.444 / 2.730 | 268,652 |
| Industrial wide, 12 s | 2.192 / 2.414 / 2.627 | 1.583 / 1.602 / 1.820 | 268,692 |
| Small exact fixture, 8 s | 1.357 / 1.524 / 1.663 | 0.761 / 0.775 / 0.780 | 263,616 |

Every industrial run displayed and probed all four stages, with 3,906/4,013/5,490 GPU samples and
zero dropped queries respectively. Bootstrap took about 285–358 ms; replacements 28–43 ms.

Claude supplied one analysis and one targeted runtime review; the full production roof module was
provided, but only selected test source and summaries, not a complete test-diff review. Its useful
distinction between connectivity and load support is preserved above. The suggested single-job
transaction issue is addressed by six actual bounded transactions, not a larger cap. The tests and
sources independently prove the equal-tick replay contract and connected sheets; a review comment
about an unseen test is not evidence that the test is absent. The stub shapes remain authored
concrete with no promise that a future calibrated solver would keep them attached.

## Native visual evidence

The three matching before views are from `0079fa9`: `target/tooling/renderdoc-zmj7dvlf/` (wide),
`renderdoc-x3m4m1h7/` (approach), `renderdoc-h8iebesu/` (fracture), each `breach-thumbnail.png`.
All after captures use the same camera/light/material settings and the final standalone binary
above; they are actual replayed RenderDoc 1.45/Vulkan frame-400 captures with 14 textures, not
generated illustrations. The wide opening and the visible sky through the approach window improve
the silhouette, but the building remains overly regular and the courtyard sparse. This does not
pass the photorealistic ruined-factory gate.

| After view | Thumbnail under `target/tooling/` | Capture bytes / draws |
| --- | --- | --- |
| Wide | `renderdoc-_r7ggpz1/breach-thumbnail.png` | 295,325,610 / 245 |
| Approach | `renderdoc-4xg63vm2/breach-thumbnail.png` | 303,650,472 / 221 |
| Fracture | `renderdoc-ua8lc2px/breach-thumbnail.png` | 305,676,155 / 219 |

All three captures were inspected and their owned game smokes completed. The wide capture preceded
the full validation run; its executable hash matches the final standalone build after that run.

The finite motion harness was reused in `target/roof-motion-eqnKur/`, targeting only the new game's
PID-verified X11 window inside the existing network/PID sandbox. It recorded six seconds of bounded
orbit/zoom (90 frames, 1440×900 at 15 FPS) and required the 20-second native smoke to complete.
`motion.mp4` and extracted frames 5/25/75 were retained; those frames were inspected for real camera
movement and consistent attached roof/window geometry. This remains a motion spot-check, not a
complete high-refresh shimmer test. Screen-recording/XWayland timings are excluded from the table.
Both owned processes exited normally; the isolated wrapper completed with exit 0. No new tool,
network access, global input hook or permission change was needed.

Two completed older captures (`renderdoc-cg9_31h0`, `renderdoc-yqp_fdjh`) and the earlier failed
replay evidence `renderdoc-a6w79zsn` were moved intact into `target/tooling-archive-lHJXgO/` to
respect the active retention budget. Nothing was deleted and no limit was raised; the old failed
replay remains a failure, not validation evidence. These local archives are not off-machine backups.
Active tool evidence is about 1.8 GiB, the recoverable archive 4.9 GiB, and the new bounded motion
artifacts 8.7 MiB.
