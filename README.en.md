[Français](README.md) · **English**

<div align="center">

# Destructible FPS

**Explore a ruined factory. Build a world that remembers every impact.**

An FPS in development with **Unreal Engine 5.8.2**, focused on persistent destruction.
The current milestone is a first-person walkthrough of an industrial environment on the Linux development workstation.

[Watch the video](#video) · [Screenshots](#unreal-screenshots) · [Progress](#project-status) · [Explore the repository](#explore-the-repository) · [Next steps](#next-steps)

[![Production direction: Unreal Engine 5.8.2](https://img.shields.io/badge/engine-Unreal%20Engine%205.8.2-313131?style=flat-square&logo=unrealengine&logoColor=white)](#project-status)
[![Milestone: local FPS walkthrough](https://img.shields.io/badge/milestone-local%20FPS%20walkthrough-8b7cf6?style=flat-square)](#video)
[![Published code: Rust prototype](https://img.shields.io/badge/published%20code-Rust%20prototype-2496ed?style=flat-square&logo=rust&logoColor=white)](docs/rust-prototype.md)
[![Code license: UNLICENSED](https://img.shields.io/badge/code%20license-UNLICENSED-lightgrey?style=flat-square)](#rights-and-attributions)

<img src="docs/screenshots/2026-09-08-unreal-marble-overview.png" alt="Native Unreal Engine capture from September 8, 2026: a ruined factory, rubble, wet ground and cliffs." width="880">

*Native Unreal Editor view, September 8, 2026. The 3D environment was generated with World Labs / Marble and rendered with Cesium for Unreal.*

</div>

## Video

<div align="center">

<a href="docs/videos/2026-09-10-marble-walk.mp4"><img src="docs/videos/2026-09-10-marble-walk-preview.gif" alt="Native animated preview of the Marble Walk session. Open the full video." width="640"></a>

[**Watch the full walkthrough · 32 s · 720p · MP4**](docs/videos/2026-09-10-marble-walk.mp4)

</div>

Recorded on **September 10, 2026** in the Unreal GameViewport on Linux, this silent video
shows a camera pan, a walk of approximately **6.3 m**, a jump and landing, and a return
to the starting point. The animated preview above uses four seconds from the same session.

Close-up detail still needs work. This is a prototype walkthrough, without destruction
or multiplayer. The [session evidence](docs/checkpoints/2026-09-10-unreal-video.md)
documents the route, capture process and validation limits.

## Project status

**Unreal Engine 5.8.2 has been the production direction since September 7, 2026.**
The Rust prototype remains a reference for simulation and networking. The images on this
page show the Unreal work from September 8, now presented in this repository.

The latest milestone, **Marble Walk**, lets a player explore an industrial scene in first
person inside the Linux editor. Rendering uses a collection of volumetric points
(*Gaussian splats*) from the Marble world; a separate collision mesh makes walking possible.

<details>
<summary><strong>View the test details and remaining work</strong></summary>

| Area | What works in Unreal | What still needs development or validation |
|---|---|---|
| **Industrial environment** | Scene imported, saved, reopened and rendered natively with Cesium | Close-up detail, visual stability in motion and GPU budget |
| **First-person walkthrough** | Character and camera, keyboard/mouse input, jump and landing, reset to the start; a 6.36 m route tested | Full terrain coverage, sustained keyboard input and automatic recovery after a fall |
| **Collision** | Separate source mesh, ground contacts and route positions cross-checked | Open mesh; not every obstacle or map boundary has been tested |
| **Destruction and construction** | Goals and technical references preserved in the Rust prototype | Combat, material-specific damage, collapse and consistent changes to the Unreal environment |
| **Multiplayer and distribution** | Server authority and persistence requirements preserved | Unreal replication, a playable package, other machines and other operating systems |

</details>

The current milestone is a **local technical walkthrough**, with imperfect close-up rendering.
The Rust prototype's destruction features have not yet been demonstrated in Unreal.
The [progress checkpoint and capture evidence](docs/checkpoints/2026-09-10-unreal-presentation.md)
describe the tests performed and the limits of this publication.

## Unreal screenshots

These images come from native sessions on **September 8, 2026**. The two views below
were captured in the **GameViewport during Play In Editor**, with the character and HUD.
The PNG files are published unchanged, with no retouching or additional image generation.

<table>
  <tr>
    <td width="50%" align="center"><strong>At the start of the walkthrough</strong></td>
    <td width="50%" align="center"><strong>At the foot of the building</strong></td>
  </tr>
  <tr>
    <td><a href="docs/screenshots/2026-09-08-unreal-marble-walk.png"><img src="docs/screenshots/2026-09-08-unreal-marble-walk.png" alt="Native Unreal GameViewport capture at the start of Marble Walk, with the prototype's crosshair and HUD." width="100%"></a></td>
    <td><a href="docs/screenshots/2026-09-08-unreal-marble-walk-close.png"><img src="docs/screenshots/2026-09-08-unreal-marble-walk-close.png" alt="Native capture after a 6.36-meter walk: facade and rubble seen up close, with detail that is still blurred and distorted." width="100%"></a></td>
  </tr>
  <tr>
    <td>After resetting to the start with R.</td>
    <td>Close-up detail still needs improvement.</td>
  </tr>
</table>

Click an image to open it at its original size.

The 3D environments were generated with **World Labs / Marble**, then integrated and
captured in Unreal. The [image manifest](docs/screenshots/2026-09-08-unreal-marble-manifest.json)
records their origin, resolution and SHA-256 hashes.

## Explore the repository

The commands are for authorized users; see the [reuse conditions](RIGHTS.md#english).

| Looking for… | Start here |
|---|---|
| **Unreal progress** | [Milestone and capture provenance](docs/checkpoints/2026-09-10-unreal-presentation.md) · [Video evidence](docs/checkpoints/2026-09-10-unreal-video.md) |
| **Code you can run today** | [Rust prototype guide](docs/rust-prototype.md): FPS demo, local multiplayer and geometry inspection on Linux/Vulkan |
| **Technical decisions** | [Prototype simulation and networking architecture](docs/architecture.md) |

**The published Unreal content is limited to the presentation, captures and their provenance.**
The migration sources, launcher and local playtest assets are not included yet.
Cloning this repository lets you explore the Rust prototype; no playable Unreal package is available.

The Rust prototype preserves the simulation, destruction and server authority references.
Its commands and older screenshots are collected in its guide. Its capabilities do not
establish that those features have been ported to Unreal.

<details>
<summary><strong>Launch the Unreal walkthrough on the prepared development workstation</strong></summary>

The local map is named `MarbleWalk_v1`. On the workstation where the engine, plugins
and assets have already been prepared, the existing command opens this playtest directly:

```bash
python3 tools/unreal.py editor --playtest marble-walk --x11
```

Click **Play**, then click inside the game view to give it focus.

| Configured control | Action |
|---|---|
| `ZQSD` / `WASD` / arrow keys | Walk |
| Mouse | Look around |
| `Space` | Jump |
| `R` | Return to the start |
| `Esc` / `Shift+F1` | Stop PIE / release the cursor |

Tests exercised W, Space, R and the mouse; not every keyboard layout was tested.

These instructions apply to the local development checkout. The `tools/unreal.py` file
is not included in this GitHub publication.

</details>

## Next steps

These steps describe the planned work; they have not been delivered yet.

1. **Make the walkthrough reliable**: test a longer route, obstacles, falls and controls.
2. **Measure the scene**: CPU/GPU time, memory use and rendering stability while moving.
3. **Build a small destructible area**: distinct materials, with collision and visuals updated together.
4. **Connect simulation and Unreal multiplayer**: server authority, persistence and client convergence.
5. **Prepare a testable release**: packaging, installation and validation on a clean machine.

## Rights and attributions

The published original code remains **UNLICENSED**, with [rights reserved](RIGHTS.md#english).
Reuse and commercial exploitation require prior written permission, with compensation
agreed for commercial exploitation of the proprietary content.
Third-party dependencies and assets retain their own licenses, including the Poly Haven materials
and environment under CC0. The [Rust guide](docs/rust-prototype.md#droits-et-attributions)
links to their sources and attributions.

The Unreal captures credit World Labs / Marble for world generation and Cesium for Unreal
for rendering it in Epic's engine. Publishing these views does not redistribute the Epic engine,
installed plugins or source files of the generated world.
The [provenance note](docs/checkpoints/2026-09-10-unreal-presentation.md#publication-et-attributions)
documents the terms checked for these captures.
