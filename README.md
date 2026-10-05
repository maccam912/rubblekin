# Rubblekin

A playable voxel world: explore a 32.768 km island with mountain ranges, eroded valleys, rivers, lakes, and climate-based biomes; build with a creative palette, join another player, and watch Moss forage or rest according to its own needs. The long-term game direction and decisions live in [DESIGN.md](DESIGN.md).

![Generated island geography; the small cream cross marks spawn](artifacts/geography-map.png)

This is a working prototype for trusted cooperative play. It has no accounts, public-server authentication, TLS, ownership claims, quests, combat, or complete economy.

## Download and play

Download a **rubblekin-launcher** ZIP from [the latest release](https://github.com/maccam912/rubblekin/releases/latest), extract it, and open the launcher. No Rust installation is needed. Keep the launcher: it checks GitHub on each start, downloads the newest complete client release, and opens the game automatically. Its window shows update progress and offers Retry or **Play installed version** if an update fails and a verified client is installed. The first launch needs internet access.

| Computer | Launcher archive |
| --- | --- |
| Windows x64 | `rubblekin-launcher-x86_64-pc-windows-msvc.zip` |
| Linux x64 | `rubblekin-launcher-x86_64-unknown-linux-gnu.zip` |
| Apple Silicon Mac | `rubblekin-launcher-aarch64-apple-darwin.zip` |
| Intel Mac | `rubblekin-launcher-x86_64-apple-darwin.zip` |

On macOS, move **Rubblekin Launcher.app** to Applications. Bundles are ad-hoc signed but not Developer ID signed or notarized; the first browser-downloaded launch may require **System Settings → Privacy & Security → Open Anyway**. Windows builds are unsigned and may show SmartScreen. On Linux, use `chmod +x rubblekin-launcher` if your archive tool drops executable permissions. Linux builds use Ubuntu 22.04 and need desktop X11/Wayland libraries, OpenGL for the launcher, and a working Vulkan driver for the game. Native playtests on Windows/Linux and representative integrated graphics remain necessary.

| OS | Data directory |
| --- | --- |
| Windows | `%LOCALAPPDATA%\rubblekin` |
| Linux | `$XDG_DATA_HOME/rubblekin`, default `~/.local/share/rubblekin` |
| macOS | `~/Library/Application Support/rubblekin` |

New local village worlds use `game/saves/villages.json`; existing `game/saves/geography.json` and `game/saves/valley.json` worlds remain available with explicit `--save` paths. All are retained through updates; screenshots use `game/artifacts`, and the latest client log is `logs/client.log`. Downloads use `clients/<target>/<commit>`, retaining the current and previous installed version for each architecture. Updates verify SHA-256, stage a complete installation, then atomically change the selected version. They do not modify saves. Existing checkout saves are not moved automatically; with the game stopped, copy one into `game/saves` and select it with `--save` if desired.

```sh
rubblekin-launcher --headless -- --low --connect rubblekin.oci.koski.co:7878
rubblekin-launcher --offline
rubblekin-launcher --data-dir /path/to/rubblekin-data
cargo run --locked -p rubblekin_launcher
```

Client arguments go after `--`; relative client paths resolve inside `game`. Headless mode fails visibly when an update cannot complete; `--offline` explicitly chooses the installed client. The launcher updates the client, not itself; manifest format 1 must remain compatible with previously downloaded launchers. Client ZIPs are also available for direct use without automatic updates.

## Android client

Android builds produce `rubblekin-client-aarch64-linux-android.apk` for ARM64 devices. The package declares Android 8.0+ and Vulkan 1.0 as requirements; these are build requirements, not a guarantee of performance on every device. Install the APK from an Android-capable client release, allowing installation from your download app when Android requests it. The updater is built into the game APK, so there is one Rubblekin app icon.

Opening Rubblekin checks for updates. Choose **Update** when a newer build is available, or **Play installed** to open the game immediately, including offline. **Retry** checks again after a connection failure. The first in-app update may open Android's **Allow from this source** setting for Rubblekin; return to the app after allowing it, then confirm the system installer. Canceling an update keeps the installed game available. Older APKs without the updater need one manual installation of a newer APK.

Play in landscape: use the left thumbstick to move and swipe the world on the right to look. On-screen buttons provide jump, sprint, creative flight and vertical movement, digging/building, materials, and a menu with inspection, camera/graphics controls, and leaving the world. Movement and looking support separate fingers. The join screen opens Android's keyboard for server address and display name. Android initially uses Low graphics; your menu choices are saved. Android's Back button opens or closes the pause menu in a world; while typing, it dismisses the keyboard first.

Local worlds use app-private storage, separate from installed binaries. Updating an APK with the same application ID and signing key preserves saves; uninstalling the app removes its local data. The prototype APK uses a shared development signing key so local and CI builds can update each other. Production/store signing is not configured.

To build from source, install Java 21 and the Android SDK with platform 36, build-tools 36.0.0, and NDK 28.2.13676358. Set `ANDROID_HOME` to the SDK directory; macOS defaults to `~/Library/Android/sdk`. Then:

```sh
rustup target add aarch64-linux-android
cargo install cargo-ndk --version 4.1.2 --locked
python3 scripts/build_android.py
```

The script builds optimized Rust, packages the GameActivity application with the pinned Gradle wrapper, and writes `target/android/rubblekin-client-aarch64-linux-android.apk`. Add `--install --device SERIAL` to update a connected device with `adb`; omit `--device` when only one device is connected. The ARM64 Android 15 emulator has verified keyboard entry, gameplay rendering, server-authoritative movement/look, digging/building, flight, menus, and local village-world saves. See [Android gameplay](artifacts/android-local.png) and [keyboard layout](artifacts/android-keyboard.png). Physical-device frame rate, memory use, thermals, and touch comfort still need testing.

Preview the touch interface on desktop with `cargo run --locked -p rubblekin_client -- --touch`. This helps inspect layout and single-pointer interaction; it does not replace Android or multitouch testing.

## Automatic client releases

[Checks and builds](.github/workflows/ci.yml) is the single entry point for branch pushes, pull requests, and manual runs. It selects the exact commits, then runs formatting, Python tooling tests, and release-profile Rust tests/Clippy on Linux x64, Windows x64, and both Mac architectures. All ten check jobs per commit run in parallel, subject to runner availability. Both the commit matrix and its check matrix use [GitHub's fail-fast cancellation](https://docs.github.com/en/actions/reference/workflows-and-actions/workflow-syntax#jobsjob_idstrategyfail-fast): one failed check cancels the remaining checks and blocks all builds in that run. Superseded branch/PR runs are cancelled; main pushes are retained for per-commit releases.

After **every check for every selected commit succeeds**, [client releases](.github/workflows/client-release.yml) and any needed [server image builds](.github/workflows/server-image.yml) start independently in parallel. These are reusable workflows with no separate push/manual trigger that can bypass checks. Client package startup/install smoke tests and container tests remain in the build stage because they validate the built packages and container environment.

Client tests and builds share cached Cargo downloads and compilation, using identical runner images, targets, Rust 1.98.1, release profile, and Windows static-CRT flags. Builds first look for the same commit's cache, then compatible earlier caches; separate check/build suffixes let completed builds save additional work without overwriting an immutable cache. Clippy has its own cache. Docker keeps separate AMD64/ARM64 BuildKit caches because its Debian environment cannot safely reuse the native client compilation. Caches are an optimization: misses or eviction cause normal compilation, and final executable linking/packaging is still required. Change the `rust-v2` cache namespace in both workflows if compiler flags or cache layout change.

Clients are released for every new first-parent commit pushed to `main`, including multiple commits per push and documentation changes. Merge commits represent their merged branches; branch/PR commits do not publish. Commits predating the release tooling are skipped. A push supports at most 64 releasable commits (GitHub's 256-job client build matrix limit); larger batches and non-fast-forward pushes fail explicitly and can be released individually with **Checks and builds → Run workflow → commit** on `main`. That manual SHA receives the same complete checks and does not rebuild the server. Leave `commit` empty to check/build the current head and server. GitHub's explicit `[skip ci]` commit directive also skips the push workflow.

Each complete build publishes `client-<full SHA>` with four client ZIPs, four launcher ZIPs, `client-manifest.json`, and `SHA256SUMS`. Android-capable commits also include the APK; built-in-updater commits require a separate `android-manifest.json`, leaving the desktop manifest format unchanged. Android metadata records the actual APK version code, size, and SHA-256; the native updater also verifies package and signing identity before installation and rejects downgrades. Android unit tests run before APK packaging. Drafts stay hidden until every file is uploaded. Publication is serialized; commit order on `main`, rather than completion time, determines **Latest**. Once all checks pass, a later packaging/build failure for one commit does not discard other complete commits; a failed platform cannot publish a partial release. Reruns resume drafts and preserve published assets. Players read the public latest-manifest download and need no GitHub credentials.

The workflow normally uses `GITHUB_TOKEN`, granting contents write only to tag reservation/publication jobs. Planning and checks are read-only. A small reservation job creates immutable tags alongside checks, before the push head becomes historical; this publishes no release, and a failed check can leave a reserved tag without assets. GitHub can require **Workflows: write** for historical commits whose workflow files differ from current `main`. If such a rerun is rejected, create/push the exact lightweight `client-<SHA>` tag at that SHA with an authorized account, or configure an optional repository `RELEASE_TOKEN` secret with **Contents: write** and **Workflows: write**. That token is used only after an ordinary mutating request is rejected. Normal current-main publication needs no extra token.

Run tooling tests with Python 3.11+ using `python3 -m unittest discover -s scripts/release -v` and `python3 -m unittest discover -s scripts/ci -v`. `package.py` packages native binaries; `smoke.py` verifies extracted applications. Mac bundles must be packaged on macOS. Publication uses GitHub's documented `concurrency.queue: max`; older Actionlint versions may not recognize it.

## Run from source

Use Rust/Cargo compatible with the workspace's Rust 1.95 minimum, then run from the repository root:

```sh
cargo run --locked -p rubblekin_client
```

The first build compiles Bevy and takes longer than subsequent launches. The client opens a join screen with `rubblekin.oci.koski.co:7878` as the default server address and a display name. Choose **Join server** to connect remotely or **Local world** to host at `127.0.0.1:7878` with the save `saves/villages.json`. New saves generate an inhabited island; an existing save retains its original generator and terrain. Names are guest display names, not authenticated accounts. Failed connections return an error on the screen so you can correct the address and retry.

Use `--local` to go straight into a local world, or `--connect HOST:PORT --name NAME` to connect immediately. Balanced graphics is the initial desktop default; use `--low` for the least expensive preset. Click the game to capture the mouse; press Escape to open the pause menu. Resume or Escape returns to play. Press F10 or choose **Leave world** to return to the join screen.

Closing the host window saves and stops its server. To keep the world running after players leave, use the dedicated server below.

For a second client on the same computer:

```sh
cargo run --locked -p rubblekin_client -- --connect 127.0.0.1:7878 --name Visitor
```

For a fresh world, choose a different save file. A seed changes newly created worlds; existing saves retain their original seed.

```sh
cargo run --locked -p rubblekin_client -- --local --save saves/another-island.json --seed 123
```

On macOS, the optional launcher script creates `artifacts/Rubblekin.app`:

```sh
sh scripts/macos-app.sh
open artifacts/Rubblekin.app
```

The launcher references this checkout's `target/debug/rubblekin` binary. It is a development convenience, not a standalone distributable app.

## Geography and world scale

New worlds use a 32.768 km square region, matching the physical extent of Veloren’s default map. The user confirmed the generated island shape on October 4; this pass retains it. Seeds 42, 43, and 123 currently produce peaks around 2.2 km above sea level. A global 513 × 513 plan at 64 m spacing defines mountain ranges, coasts, catchments, rivers, lakes, temperature, moisture, and biomes. An independent 48-pass erosion model incises channels, transports/deposits sediment, weathers steep slopes, and cuts spillways through some enclosed basins. [Veloren’s world-generation guide](https://book.veloren.net/players/world-generation.html) is the scale reference; this is Rubblekin’s own generator.

The client loads 50 cm editable voxels near the player or observer and draws distant terrain from the same geography. The default 48 m near-detail range loads at most 169 nearby chunks; the menu allows 24–96 m (49–625 chunks). Two detail-generation jobs and one distant-mesh job are active at a time. The world uses a bounded column cache and sparse saved edits rather than storing billions of untouched blocks. New local worlds use a separate save path so the original valley stays intact:

```sh
cargo run --locked -p rubblekin_client -- --local
cargo run --locked -p rubblekin_client -- --local --observe
cargo run --locked -p rubblekin_client -- --local --save saves/valley.json
```

Export the actual generated geography as a shaded PPM map, with the spawn marked by a cream cross:

```sh
cargo run --locked -p rubblekin_core --example geography -- 42 /tmp/geography.ppm
```

New islands use GeographyV3, retaining GeographyV2 landforms and biomes: visibly distinct meadows, broadleaf woods, pine forest, dry scrub, desert, wet forest, tundra, alpine rock, beaches, and snow. Pine trees have taller tiered crowns, scrub is low and sparse, and meadows have fewer trees. River water and its carved bed share a channel profile, avoiding elevated water walls at tributaries and dry banks. Medium/far terrain uses smooth stitched heightmap triangles painted with a 2048² stylized map: biome/canopy patches, gentle relief, actual water, village footprints/fields, and connecting trails. Filtered mip levels limit distant shimmer. Nearby tree/building silhouettes bridge to editable voxels within 128 m; fine grain stays on nearby blocks. Stepped fake voxel LOD and coarse square grain have been removed.

Existing islands retain GeographyV1 or GeographyV2, including their saved edits and terrain. Villages and resources are added only to new GeographyV3 worlds; existing worlds are not silently converted. To try the revised generator without replacing an existing world:

```sh
cargo run --locked -p rubblekin_client -- --local --observe --save saves/detailed-island.json
```

Current limits: drainage follows eight directions on the 64 m grid; water is static and does not simulate flowing through excavations or swimming. Individual edits appear only in the nearby detailed area; very distant terrain still uses a coarse surface, and distant vegetation is represented by map color. Airships, gliders, and an in-game world map remain future work. Villages and trails are now generated in GeographyV3; live physical settlement expansion remains future work. Representative integrated-graphics measurements remain necessary. The [map-textured valley](artifacts/map-lod-valley.png), [closer view](artifacts/map-lod-near.png), [forest/coast](artifacts/map-lod-forest.png), and [exact 2048² atlas](artifacts/distant-map-atlas.png) show the approved medium/far rendering, enabled automatically in all graphics presets. Nearby [voxel gameplay](artifacts/map-lod-ground.png) and [ground shading](artifacts/terrain-readable-ground.png) are retained. Export the exact atlas with `cargo run --locked -p rubblekin_client --example distant_map -- 42 /tmp/distant-map.ppm`.

## Villages, resources, and residents

New local play uses `saves/villages.json`. The geography-first generator places a bounded network of established villages on dry, gentle land near reachable freshwater. Farming suitability, actual generated trees, and stone/clay/iron deposits contribute to scores over terrain-accessible catchments. Placement balances useful opportunities and spacing; the existing island and mountain landforms remain intact. Terrain-following dirt trails connect feasible sites; shallow wet crossings use fords or graded wooden causeways. Static water can overlap a few road surfaces by up to 0.20 m. Voxel cottages, storehouses, workshops, market stalls, and soil plots remain editable.

Each village starts with six residents and seeded stores; no chronological founding history is simulated. Residents have individual hunger and energy: they eat real village food, rest at home, and resume interrupted jobs with their cargo intact. Food shortages slow work without starvation or death. Farmers walk assigned crop rows, plant and tend crops, harvest ripe food, and carry it to stores. They retrace their field path for meals and sleep, then return to the interrupted work; that path and progress survive restart. Actual planted soil controls growth and yield; removing all planted soil destroys crop maturity. Other workers draw from bounded resource reserves, and traders carry actual surplus between villages while retaining food reserves. Stores, crop cycles, resident needs/routes/positions, cargo, and remaining reserves persist across restart and progress while the server runs with no clients. Resource reserves are a numerical estimate of accessible supply; working a deposit does not yet remove its visible ore blocks or fell its trees. Growth capacity is reported, but new residents/buildings are not yet created. There is no currency, player harvest/trade menu, or replay of server downtime.

Players, Moss, and village residents collide with each other using the shared character controller, including during creative flight. Joining players receive nearby unoccupied spawn positions. Resident movement passes around other people where space permits; blocked routes cannot produce goods remotely. The inspector shows hunger, energy, activity, and the reason for it, while farming and meals have visible hand motions. The read-only observer camera has no physical body.

The [village map](artifacts/villages-map.png) plots the actual seed-42 plan and resource scores; the [asset catalog](artifacts/village-assets.svg) shows the generated voxel geometry. The [native village view](artifacts/villages-native.png) shows the running client. The nearby inspector shows the village's scores, water distance, stores, growth, and a resident's current activity. In read-only observer mode, press **V** to visit the next village; **R/Home** returns to spawn.

```sh
cargo run --locked -p rubblekin_client -- --local --observe --save saves/villages.json
cargo run --locked -p rubblekin_core --example settlements -- 42 /tmp/villages.json
cargo run --locked -p rubblekin_core --example village_asset_catalog -- /tmp/village-assets.svg
```

Existing saves remain available with `--save saves/geography.json` or `--save saves/valley.json`. Protocol v6 requires rebuilt matching clients and servers. This source update does not itself update the public test server.

## Controls

| Input | Action |
| --- | --- |
| Click the game / Escape | Capture the mouse / open or close the pause menu. |
| WASD | Move relative to the camera. |
| Mouse or arrow keys | Look while the mouse is captured. |
| Space / Shift | Jump / sprint. |
| Mouse wheel | Adjust third-person camera distance. |
| Left click / right click | Remove / place the targeted block. |
| Ctrl + hold mouse button | Repeat digging or building. |
| 1–6 | Grass, earth, stone, wood, brick, glass. |
| F; Q / E | Toggle creative flight; descend / ascend. |
| Tab | Show or hide the nearby village/resident inspector, or Moss's inspector elsewhere. |
| F2 | Cycle Low → Balanced → High → Low graphics. |
| H | Show or hide the controls panel. |
| F10 | Disconnect and return to the join screen. |
| F12 | Save a screenshot in `artifacts/`. |

Developer controls work only for player sessions when the server allows them:

| Input | Action |
| --- | --- |
| F6 / F7 | Force Moss to forage / rest. |
| F8 | Clear the override and restore autonomous choices. |
| F9 | Set hunger to 85 and energy to 35 for testing. |
| `[` / `]` | Favor rest / restore equal forage and rest weights. |

The local auto-host enables these controls for every connected player. They are disabled by default on a dedicated server. On keyboards with media function keys, use the platform's function-key modifier if needed.

## Observe without a player avatar

Choose **Observe as admin** on the join screen, then **Local world** or **Join server**. Direct startup also works:

```sh
cargo run --locked -p rubblekin_client -- --local --observe
cargo run --locked -p rubblekin_client -- --connect 127.0.0.1:7878 --observe
```

Observer sessions require an admin-enabled server (`--allow-admin` on dedicated hosting). Local hosting enables this; the public test server has it disabled. The existing admin setting applies to everyone who can connect, so it is not per-user authentication.

The read-only camera creates no avatar and passes freely through terrain. WASD flies along the view, Q/E descends/ascends, mouse or arrows look, scroll changes speed from 2–64 m/s (initially 12), and Shift gives a 5× boost. **R or Home** returns to spawn and resets speed; **V** visits the next village in GeographyV3. Tab toggles the nearby village/resident inspector or Moss's inspector elsewhere, F2 changes graphics, and F10 returns to the join screen, where you can switch back to **Play as explorer**.

Observers see live terrain edits, other players, and NPC activity. They cannot build or change NPC settings; the server enforces this even for custom clients. In geographic worlds, nearby detailed terrain follows the camera and distant landforms cover the full 32.768 km region. The old 160 × 160 m valley renderer remains available for legacy saves.

The NPC-needs handshake uses **protocol v7**. Rebuild/restart both client and server together. Save version 3 records the terrain generator and village residents/stores; earlier version-3 residents receive default needs and keep their jobs, goods, and terrain. Original version-1 valleys and version-2 worlds load with their original terrain and upgrade save metadata without changing their generator.

## Graphics

All presets include terrain corner shading, darker ground sides, and a subtle top-edge cue at actual drops, so descending steps remain visible without sun shadows. Flat ground has no added edge outlines. Fine world-aligned grain fades below pixel size on nearby voxels; medium/far heightmaps use a generated map atlas. The embedded shader and atlas need no downloaded texture assets. Open **Escape** (desktop) or **Menu / Back** (Android) to choose quality, near-detail distance, and shadow distance. Changes apply while the menu is open. Resume returns to play; Leave world returns to the join screen. Player controls stop while the menu is open, and the shared world continues running. F2 still cycles quality, and startup flags override the saved quality:

| Preset | Startup option | Shadows and antialiasing |
| --- | --- | --- |
| [Low](artifacts/shadows-low.png) | `--low` | Terrain shading and inexpensive ground shadows under characters; no dynamic shadows or MSAA. |
| [Balanced](artifacts/shadows-balanced.png), default | `--balanced` | Nearby sun shadows: one 1024 × 1024 shadow map out to 32m camera depth, hardware 2×2 filtering, no MSAA. |
| [High](artifacts/shadows-high.png) | `--high` | Two 2048 × 2048 shadow maps out to 90m, Gaussian filtering, and 4× MSAA. |

Near detail adjusts the editable voxel square around your player or observer from 24–96 m in 8 m steps (default 48 m). These are approximate cardinal distances because loading follows chunk boundaries. Increasing this range adds memory and rendering work; the map-painted distant landscape stays visible. Legacy valleys already load their full terrain.

Shadow distance adjusts the final sun-shadow cascade from 8–192 m. Low disables dynamic shadows and its distance controls; Balanced and High restore their 32 m / 90 m shadow defaults when selected. Changing quality retains your near-detail distance. Shadows can only come from loaded terrain and visible characters; distant map terrain does not cast them. Preferences are saved in `graphics.json` beside the client's local data and survive leaving, rejoining, and restarting.

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

There is no downtime catch-up: simulation advances while the server runs, even with zero players, and resumes from saved time after a restart. The save records its terrain-generation version. Version-1 valley saves keep the original terrain when read and are written as version 3 with an explicit ValleyV1 generator. New islands use GeographyV3; existing GeographyV1/V2 islands keep their original terrain; a new client does not turn an existing valley into an island. Unknown save/generation versions fail visibly. Use a fresh save path to explore new geography.

[Dockerfile](Dockerfile) tests and builds only the headless server; it excludes the renderer and game assets. The runtime runs as UID/GID 10001. [deploy/kubernetes.yaml](deploy/kubernetes.yaml) provides one replica, a 1 GiB persistent volume claim, a `Recreate` rollout strategy, startup/readiness probes, `imagePullPolicy: Always`, and a private `ClusterIP` service. Review storage settings before using this standalone template.

```sh
docker build -t rubblekin-server:prototype .
docker run --rm -p 7878:7878 -v rubblekin-world:/data rubblekin-server:prototype
```

[Server image publishing](.github/workflows/server-image.yml) runs only after the shared checks gate passes. It tests/builds Linux AMD64 and ARM64 images on native runners when a main push or pull request changes server inputs, workflow files, or CI planning scripts. The architecture builds run in parallel and cancel each other on failure. A publication job assembles the multi-platform image only after both builds succeed. Main pushes and manual main runs with an empty `commit` input publish `ghcr.io/maccam912/rubblekin` with immutable `sha-<full commit>` tags; pull requests never publish. Publication is serialized and promotes `latest` according to main's first-parent order, so a late older build cannot roll the server back. Repository/package linkage is included in the image labels. The GHCR package must be public for anonymous cluster pulls; package visibility is separate from repository visibility.

The actual OCI cluster configuration lives in [fleet-infra/apps/rubblekin](https://github.com/maccam912/fleet-infra/tree/main/apps/rubblekin). It uses the cluster's OCI block storage and shared ingress-nginx TCP load balancer on port 7878. The game uses raw TCP, so it needs a TCP forwarding entry, not an HTTP Ingress. Flux scans `latest` every five minutes and commits its new digest into the Deployment to trigger a rollout. `Always` checks the image when a container starts; it does not restart existing pods by itself. One `Recreate` replica ensures the old save writer stops before its replacement. The namespace and world PVC are retained when removing the Flux app and require deliberate manual deletion.

The public test server address is **`rubblekin.oci.koski.co:7878`**, prefilled on the join screen. Enter a display name and choose **Join server**, or connect directly:

```sh
cargo run --locked -p rubblekin_client -- --connect rubblekin.oci.koski.co:7878 --name Visitor
```

The October 4 native client and two-client socket checks passed over the original public IP (`147.224.165.110:7878`), including shared player state, ping, and logout removal. Dedicated-server admin controls are disabled. OCI's security list permits TCP 7878 to the shared load balancer; its existing private rules cover forwarding to Kubernetes.

For cluster-local troubleshooting, you can also forward the service:

```sh
kubectl -n rubblekin port-forward service/rubblekin 17878:7878
```

Then enter `127.0.0.1:17878` on the join screen. Keep the forwarding command running while playing.

Resource requests are starting values, not measured production requirements. OCI provisioned its 50 GiB minimum volume for this app's 1 GiB request. Increasing replicas does not distribute a world; the save requires one writer.

## Git and assets

Install Git LFS before cloning, or run `git lfs install` followed by `git lfs pull` in an existing checkout. The curated screenshots linked in this documentation use LFS. The small bundled font and its license remain ordinary Git files. Generated screenshots, app bundles, logs, saves, build output, and local environment files are ignored. `Cargo.lock` is committed; CI and container builds use `--locked`. Server builds need no LFS assets.

## Follow a feature through the code

| Crate | Responsibility |
| --- | --- |
| [core](crates/core/src/lib.rs) | Seeded terrain, edited cells, shared character physics, and explicit wire types. |
| [server](crates/server/src/lib.rs) | Authoritative 20 Hz loop, validation, connections, NPC decisions, and saves. |
| [client](crates/client/src/lib.rs) | Bevy 0.19.1 rendering, input, prediction, camera, and inspection UI. |

For a block edit, read these in order:

1. [`edit_blocks`](crates/client/src/lib.rs) sends `ClientMessage::Edit` using the types in [protocol.rs](crates/core/src/protocol.rs).
2. [`handle_message` and `validate_edit`](crates/server/src/lib.rs) check rate, reach, line of sight, world bounds, and character occupancy.
3. The server updates [`World`](crates/core/src/world.rs), writes the [save](crates/server/src/persistence.rs), and broadcasts `ServerMessage::BlockChanged`, or replies with `Rejected`.
4. [`receive_network`](crates/client/src/lib.rs) applies the accepted edit; [`rebuild_chunks`](crates/client/src/terrain.rs) updates the affected meshes.

Transport is newline-delimited JSON over nonblocking TCP. There is no generic message bus or automatic ECS replication. The headless server currently uses a small standard-library loop; Bevy ECS can be introduced when the simulation earns that complexity. This transport and full-world snapshot approach are prototype choices, not a global-scale networking design.

Movement predicts each frame locally and sends the same numbered input and duration to the server. Server snapshots acknowledge completed inputs; [prediction.rs](crates/client/src/prediction.rs) replays newer inputs so delayed snapshots do not pull the player backward on release or step climbing. The server validates movement time, and prediction history is bounded. Protocol v6 requires matching client/server builds: restart both after updating. Existing valley saves remain compatible through the explicit legacy generator.

## Verify changes

```sh
cargo test --locked --workspace
cargo clippy --locked --workspace --all-targets -- -D warnings
cargo fmt --all -- --check
```

Tests include real localhost sockets, so the test environment must permit local networking. The map-texture implementation verified 146 ordinary tests locally on macOS: 40 core, 60 client, 34 server, and 12 launcher. The existing native package integration test remains opt-in; it previously verified a real client ZIP download, installation, bundle signature, and child process startup. Release-tooling Python tests were verified during earlier release work and were not rerun for the rendering change.

Geography regressions cover the actual raised-water junction, sampled channel banks, unchanged V1 landforms, substantial V2 biome regions, shared tree shapes, dry spawn, drainage, distant collision/edits, legacy-save compatibility, and retained generation identity after restart. Renderer checks cover bounded streaming/triangles, immediate trees in pending chunks, foliage colors at biome boundaries, stale edit jobs, deep cuts, distant edge alignment, heightmap cells without artificial terraces, and actual water/voxel placement. Existing tests cover joining, movement reconciliation, observer permissions, multiplayer replication, NPCs, persistence, and graceful shutdown. Workspace Clippy with warnings denied, formatting, and the locked offline native build pass. Village regressions cover resource-based placement, deterministic and legacy terrain, editable deposits/buildings, physical resident roundtrips on three seeds, every seed-42 trail in both directions, soil-dependent crops, real cargo conservation, blocked work, and village replication/persistence without connected players. Native macOS village views and observer visits were inspected; long-distance flight feel and lower-end performance remain unverified. The [Linux/Windows/macOS CI matrix](.github/workflows/ci.yml) and client release workflow check pushed changes.

Capture a reproducible initial scene:

```sh
cargo run --locked -p rubblekin_client -- --local --screenshot artifacts/geography-ground.png --exit-after 18
```

`--screenshot` captures after eight seconds. Keep the game window visible during capture; an occluded macOS window can produce an empty screenshot. F12 captures the current view interactively. Screenshots document appearance, not frame-time behavior.

## Current limits and next feedback

- **World:** new islands span 32.768 km with 50 cm editable cells and bounded local detail. Distant terrain reflects generated geography; remote edits appear only in the nearby voxel region; medium/far heightmaps use the generated map texture, with nearby silhouettes through 128 m. Legacy 160 m valleys keep their original terrain.
- **Simulation:** Moss retains the original foraging loop. GeographyV3 adds up to 60 village residents with hunger/energy, active crop work, shared crop growth, stores, finite extraction reserves, and physical deliveries/trade. Local farm paths use actual terrain and character physics; player-built obstructions can still block residents. Live settlement expansion, prices/currency, player farming/trade interactions, individual plant lifecycles, and flowing water remain future work.
- **Building and ownership:** all players have unlimited materials and cooperative edit access. Claims and configurable offline property protection remain planned; no protection system is implemented. Moss does not destroy player structures.
- **Networking:** 32 connection cap and 100,000 edited-cell cap are defensive prototype limits. There are no accounts, transport encryption, hostile-client load tests, or public-server readiness claims. Slow or malformed peers are disconnected.
- **Compatibility and performance:** native macOS playtests exercised building/removal, NPC override/clear, a [second connected client](artifacts/two-client.png), and [restoring the edits after restarting](artifacts/restarted-world.png). The October 4 graphics playtest verified the full live Balanced → High → Low → Balanced cycle, nearby player/tree shadows, and the Low fallback; brief foreground HUD observations reached around 120 fps on an Apple M5 Pro. Focus and capture interruptions make these unsuitable for a frame-time comparison. Earlier October 3 samples at 1440 × 900 showed about 119–125 fps on the old low preset and 93–120 fps on the old high preset, with scene construction around 0.16–0.23 seconds. These are separate observations on a strong machine, not controlled benchmarks or a measured before/after speed comparison. Windows, Linux, the children's computers, representative integrated graphics, and performance during extensive building remain untested.

The next useful feedback is on village placement and appearance, visible resident work and trails, the first player farming/trade interaction, mountain and valley scale, and movement/camera feel. See [DESIGN.md](DESIGN.md) for the wider ambitions and unresolved choices.

The bundled Atkinson Hyperlegible font is distributed under its [SIL Open Font License](assets/fonts/OFL.txt).
