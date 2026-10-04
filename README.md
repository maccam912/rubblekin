# Rubblekin

A first playable voxel valley: walk through editable terrain, build with a creative palette, join another player, and watch Moss forage or rest according to its own needs. The long-term game direction and decisions live in [DESIGN.md](DESIGN.md).

![The valley with the default Balanced shadows and terrain shading](artifacts/shadows-balanced.png)

This is a working prototype for trusted cooperative play. It has no accounts, public-server authentication, TLS, ownership claims, quests, combat, or complete economy.

## Run locally

Use Rust/Cargo compatible with the workspace's Rust 1.95 minimum, then run from the repository root:

```sh
cargo run --locked -p rubblekin_client
```

The first build compiles Bevy and takes longer than subsequent launches. The client opens a join screen with a server address and display name. Choose **Join server** to connect remotely or **Local world** to host at `127.0.0.1:7878` with the save `saves/valley.json`. Names are guest display names, not authenticated accounts. Failed connections return an error on the screen so you can correct the address and retry.

Use `--local` to go straight into a local world, or `--connect HOST:PORT --name NAME` to connect immediately. Balanced graphics is the default; use `--low` for the least expensive preset. Click the game to capture the mouse; press Escape to release it. Press F10 to return to the join screen.

Closing the host window saves and stops its server. To keep the world running after players leave, use the dedicated server below.

For a second client on the same computer:

```sh
cargo run --locked -p rubblekin_client -- --connect 127.0.0.1:7878 --name Visitor
```

For a fresh world, choose a different save file. A seed changes newly created worlds; existing saves retain their original seed.

```sh
cargo run --locked -p rubblekin_client -- --local --save saves/another-valley.json --seed 123
```

On macOS, the optional launcher script creates `artifacts/Rubblekin.app`:

```sh
sh scripts/macos-app.sh
open artifacts/Rubblekin.app
```

The launcher references this checkout's `target/debug/rubblekin` binary. It is a development convenience, not a standalone distributable app.

## Controls

| Input | Action |
| --- | --- |
| Click the game / Escape | Capture / release the mouse. |
| WASD | Move relative to the camera. |
| Mouse or arrow keys | Look while the mouse is captured. |
| Space / Shift | Jump / sprint. |
| Mouse wheel | Adjust third-person camera distance. |
| Left click / right click | Remove / place the targeted block. |
| Ctrl + hold mouse button | Repeat digging or building. |
| 1–6 | Grass, earth, stone, wood, brick, glass. |
| F; Q / E | Toggle creative flight; descend / ascend. |
| Tab | Show or hide Moss's needs, chosen action, reason, and target. |
| F2 | Cycle Low → Balanced → High → Low graphics. |
| H | Show or hide the controls panel. |
| F10 | Disconnect and return to the join screen. |
| F12 | Save a screenshot in `artifacts/`. |

Developer controls work only when the server allows them:

| Input | Action |
| --- | --- |
| F6 / F7 | Force Moss to forage / rest. |
| F8 | Clear the override and restore autonomous choices. |
| F9 | Set hunger to 85 and energy to 35 for testing. |
| `[` / `]` | Favor rest / restore equal forage and rest weights. |

The local auto-host enables these controls for every connected player. They are disabled by default on a dedicated server. On keyboards with media function keys, use the platform's function-key modifier if needed.

## Graphics

All presets include terrain corner shading to give blocks and recesses more depth. Press F2 to change quality while playing, or select a preset at startup:

| Preset | Startup option | Shadows and antialiasing |
| --- | --- | --- |
| [Low](artifacts/shadows-low.png) | `--low` | Terrain shading and inexpensive ground shadows under characters; no dynamic shadows or MSAA. |
| [Balanced](artifacts/shadows-balanced.png), default | `--balanced` | Nearby sun shadows: one 1024 × 1024 shadow map out to 32m camera depth, hardware 2×2 filtering, no MSAA. |
| [High](artifacts/shadows-high.png) | `--high` | Two 2048 × 2048 shadow maps out to 90m, Gaussian filtering, and 4× MSAA. |

For example, `cargo run --locked -p rubblekin_client -- --low` starts with Low selected. Balanced and High replace the character ground-shadow shapes with real sun shadows. The distant mountain scenery does not cast shadows. Shadow distance is bounded separately from the landscape view distance; the outer shadow boundary has a hard cutoff. These budgets limit rendering work, but performance on the children's computers still needs testing.

## Play over a LAN or run a dedicated server

To host from the game on a trusted LAN:

```sh
cargo run --locked -p rubblekin_client -- --local --bind 0.0.0.0:7878 --name Host
```

Other computers connect to the host's LAN address, for example:

```sh
cargo run --locked -p rubblekin_client -- --connect 192.168.1.50:7878 --name Visitor
```

Replace the example address with the actual host address and allow TCP port 7878 through its firewall. This auto-host mode grants developer controls to everyone who connects.

For a server that keeps simulating without an open game window:

```sh
cargo run --locked -p rubblekin_server -- --bind 0.0.0.0:7878 --save saves/shared-valley.json
```

Join it from the connection screen or with the client's `--connect` option. Add `--allow-admin` only for a trusted development session; it authorizes **every** connected player, not named administrators. Ctrl+C or SIGTERM saves the simulation and exits cleanly.

Only one process can own a save file. Stop an auto-host before starting a dedicated server on the same port or save. Players receive new session identities and spawn positions when they reconnect; player accounts and persistent character positions are not implemented.

## Saves and containers

Accepted edits are saved before the server acknowledges them. The server writes a temporary file beside the save, syncs it, and atomically replaces the previous save. An OS lock on a sidecar file prevents concurrent writers. NPC state and simulation time are checkpointed every five seconds and on orderly shutdown. Corrupt or unsupported saves fail visibly and are left intact.

There is no downtime catch-up: simulation advances while the server runs, even with zero players, and resumes from saved time after a restart. Back up the JSON save before experimenting with new terrain-generation or save-format versions; automatic migration is not implemented.

[Dockerfile](Dockerfile) tests and builds only the headless server; it excludes the renderer and game assets. The runtime runs as UID/GID 10001. [deploy/kubernetes.yaml](deploy/kubernetes.yaml) provides one replica, a 1 GiB persistent volume claim, a `Recreate` rollout strategy, startup/readiness probes, `imagePullPolicy: Always`, and a private `ClusterIP` service. Review storage settings before using this standalone template.

```sh
docker build -t rubblekin-server:prototype .
docker run --rm -p 7878:7878 -v rubblekin-world:/data rubblekin-server:prototype
```

[Server image publishing](.github/workflows/server-image.yml) tests/builds Linux AMD64 and ARM64 images on their native GitHub runners on pull requests. A publication job assembles the multi-platform image only after both builds succeed. On `main` pushes that change server build inputs, or a manual run on `main`, it also publishes `ghcr.io/maccam912/rubblekin:latest` and an immutable `sha-<full commit>` tag using the workflow's `GITHUB_TOKEN`. Repository/package linkage is included in the image labels. The GHCR package must be public for anonymous cluster pulls; package visibility is separate from repository visibility.

The actual OCI cluster configuration lives in [fleet-infra/apps/rubblekin](https://github.com/maccam912/fleet-infra/tree/main/apps/rubblekin). It uses the cluster's OCI block storage and shared ingress-nginx TCP load balancer on port 7878. The game uses raw TCP, so it needs a TCP forwarding entry, not an HTTP Ingress. Flux scans `latest` every five minutes and commits its new digest into the Deployment to trigger a rollout. `Always` checks the image when a container starts; it does not restart existing pods by itself. One `Recreate` replica ensures the old save writer stops before its replacement. The namespace and world PVC are retained when removing the Flux app and require deliberate manual deletion.

The October 4 deployment is running, and native client/two-client socket checks through the cluster succeeded. The configured public endpoint is `147.224.165.110:7878`; public access is currently awaiting an OCI network-rule check because that port times out. Until that is resolved, use:

```sh
kubectl -n rubblekin port-forward service/rubblekin 17878:7878
```

Then enter `127.0.0.1:17878` and a display name on the join screen. Keep the forwarding command running while playing.

Resource requests are starting values, not measured production requirements. OCI provisioned its 50 GiB minimum volume for this app's 1 GiB request. Increasing replicas does not distribute a world; the save requires one writer.

## Git and assets

Install Git LFS before cloning, or run `git lfs install` followed by `git lfs pull` in an existing checkout. The seven curated screenshots linked in this documentation use LFS. The small bundled font and its license remain ordinary Git files. Generated screenshots, app bundles, logs, saves, build output, and local environment files are ignored. `Cargo.lock` is committed; CI and container builds use `--locked`. Server builds need no LFS assets.

## Follow a feature through the code

| Crate | Responsibility |
| --- | --- |
| [core](crates/core/src/lib.rs) | Seeded terrain, edited cells, shared character physics, and explicit wire types. |
| [server](crates/server/src/lib.rs) | Authoritative 20 Hz loop, validation, connections, NPC decisions, and saves. |
| [client](crates/client/src/main.rs) | Bevy 0.19.1 rendering, input, prediction, camera, and inspection UI. |

For a block edit, read these in order:

1. [`edit_blocks`](crates/client/src/main.rs) sends `ClientMessage::Edit` using the types in [protocol.rs](crates/core/src/protocol.rs).
2. [`handle_message` and `validate_edit`](crates/server/src/lib.rs) check rate, reach, line of sight, world bounds, and character occupancy.
3. The server updates [`World`](crates/core/src/world.rs), writes the [save](crates/server/src/persistence.rs), and broadcasts `ServerMessage::BlockChanged`, or replies with `Rejected`.
4. [`receive_network`](crates/client/src/main.rs) applies the accepted edit; [`rebuild_chunks`](crates/client/src/terrain.rs) updates the affected meshes.

Transport is newline-delimited JSON over nonblocking TCP. There is no generic message bus or automatic ECS replication. The headless server currently uses a small standard-library loop; Bevy ECS can be introduced when the simulation earns that complexity. This transport and full-world snapshot approach are prototype choices, not a global-scale networking design.

Movement predicts each frame locally and sends the same numbered input and duration to the server. Server snapshots acknowledge completed inputs; [prediction.rs](crates/client/src/prediction.rs) replays newer inputs so delayed snapshots do not pull the player backward on release or step climbing. The server validates movement time, and prediction history is bounded. Protocol v2 requires matching client/server builds: restart both after updating. Existing world saves remain compatible.

## Verify changes

```sh
cargo test --locked --workspace
cargo clippy --locked --workspace --all-targets -- -D warnings
cargo fmt --all -- --check
```

Tests include real localhost sockets, so the test environment must permit local networking. All 45 tests pass locally on macOS: 13 core, 16 client, and 16 server. Join-screen tests cover validation, failed connections, rapid text entry, and join/leave/rejoin/disconnect cleanup. They cover core geometry and physics, terrain meshing and client handshake, multiplayer edits and reconnects, saves, autonomous behavior, and graceful server shutdown. Movement regressions cover delayed stop acknowledgments, uneven-frame stairs/cliffs, real localhost prediction, exact-once execution, and server time/sequence validation. Terrain regressions also check corner shading, face winding, and affected chunks after edits. macOS/Linux shutdown tests send signals to child server processes they create. Workspace Clippy with warnings denied and formatting checks pass. The [Linux/Windows/macOS CI matrix](.github/workflows/ci.yml) is running on GitHub; live rendering and platform performance still require native playtests on each target.

Capture a reproducible initial scene:

```sh
cargo run --locked -p rubblekin_client -- --local --screenshot artifacts/valley.png --exit-after 12
```

`--screenshot` captures after eight seconds. Keep the game window visible during capture; an occluded macOS window can produce an empty screenshot. F12 captures the current view interactively. Screenshots document appearance, not frame-time behavior.

## Current limits and next feedback

- **World:** 160 × 160 meters of editable terrain with 50 cm cells. The distant mountain ring is scenery. All playable chunks are currently built up front; streaming and distant terrain LOD are future work.
- **Simulation:** one forager, three renewable logical berry patches, and tunable needs. Berry shrubs and water are decorative; there is no plant lifecycle, fluid simulation, settlement economy, or pathfinding around complex structures yet.
- **Building and ownership:** all players have unlimited materials and cooperative edit access. Claims and configurable offline property protection remain planned; no protection system is implemented. Moss does not destroy player structures.
- **Networking:** 32 connection cap and 100,000 edited-cell cap are defensive prototype limits. There are no accounts, transport encryption, hostile-client load tests, or public-server readiness claims. Slow or malformed peers are disconnected.
- **Compatibility and performance:** native macOS playtests exercised building/removal, NPC override/clear, a [second connected client](artifacts/two-client.png), and [restoring the edits after restarting](artifacts/restarted-world.png). The October 4 graphics playtest verified the full live Balanced → High → Low → Balanced cycle, nearby player/tree shadows, and the Low fallback; brief foreground HUD observations reached around 120 fps on an Apple M5 Pro. Focus and capture interruptions make these unsuitable for a frame-time comparison. Earlier October 3 samples at 1440 × 900 showed about 119–125 fps on the old low preset and 93–120 fps on the old high preset, with scene construction around 0.16–0.23 seconds. These are separate observations on a strong machine, not controlled benchmarks or a measured before/after speed comparison. Windows, Linux, the children's computers, representative integrated graphics, and performance during extensive building remain untested.

The next useful feedback is on movement and camera feel, building at this cell size, the visual direction, and whether Moss's actions are understandable. See [DESIGN.md](DESIGN.md) for the wider ambitions and unresolved choices.

The bundled Atkinson Hyperlegible font is distributed under its [SIL Open Font License](assets/fonts/OFL.txt).
