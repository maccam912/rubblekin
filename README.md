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

Your character name is remembered when you choose Join or local play and is filled in on the next start. You can change it in the join form; `--name` overrides the remembered value. The name is stored in `game/join-preferences.json`, alongside your persistent game data, so closing the launcher or updating the client keeps it. Direct client runs store this file in their working directory; Android uses app-private storage.

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

## Automatic crash reports

Release builds use hosted Sentry when the GitHub repository variable `SENTRY_DSN`
is configured. Android initializes reporting before the updater/game, captures
Java exceptions, native crashes and ANRs, and records Rust panic messages and
backtraces through JNI. Cached Android reports can upload on the next launch.
Desktop clients capture Rust panics/startup errors and native crashes through
Sentry's separate crash-reporter process. Desktop uploads currently require
connectivity when the crash occurs; durable offline retry is not implemented.
Reports include the exact source commit, operating system/device information,
graphics preset and whether the client was at the join screen or in a world.
Screenshots, replay and performance tracing are disabled.

For automatic native stack symbolication, configure these repository settings:

- Actions variables: `SENTRY_DSN`, `SENTRY_ORG`, and `SENTRY_PROJECT`.
- Actions secret: `SENTRY_AUTH_TOKEN`, with permission to upload debug files to the
  selected Sentry project. Keep this token out of client configuration and Git.

CI retains Rust release line tables and uploads the matching Android ELF,
Linux ELF, macOS dSYM or Windows PDB before packaging. Linux packages strip debug
sections from staged copies, retaining the build ID and original symbol inputs;
desktop packaging enforces the existing launcher's 512 MiB executable/download
limits. The APK contains Gradle's stripped library. Without the upload token,
crash reporting still works, but native traces may contain unresolved addresses.
Failed configured symbol uploads fail that build so it cannot publish a release
with silently missing symbols.

Source builds read `SENTRY_DSN` and `SENTRY_ENVIRONMENT` at build time. Desktop
also allows runtime overrides; setting `SENTRY_DSN=''` disables reporting. Builds
without a DSN remain usable without a Sentry account. Android uses the values
embedded in its APK and needs rebuilding when configuration changes.

To verify desktop reporting without sending events to the hosted project:

```sh
cargo build --locked -p rubblekin_client --example crash_report
python3 scripts/test_crash_reporting.py
```

This deliberately triggers a startup error, Rust panic and native abort in
separate processes, receiving the actual events/minidump at a local collector.
It also checks that absent/malformed DSNs leave startup working without uploads.
For a manual symbol upload, set the three private-build variables above and run
`python3 scripts/release/sentry_symbols.py --binary-dir target/release` (or pass
`--android --binary-dir android/app/src/main/jniLibs/arm64-v8a`). Node.js is
required for the pinned Sentry CLI. No upload token is embedded in the game.

## Automatic client releases

[Checks and builds](.github/workflows/ci.yml) is the single entry point for branch pushes, pull requests, and manual runs. It selects the exact commits, then runs formatting, Python tooling tests, and Rust tests/Clippy on Linux x64. All four check jobs per commit run in parallel, subject to runner availability. Both the commit matrix and its check matrix use [GitHub's fail-fast cancellation](https://docs.github.com/en/actions/reference/workflows-and-actions/workflow-syntax#jobsjob_idstrategyfail-fast): one failed check cancels remaining checks and blocks publication. Independent builds can finish and cache useful work. Superseded branch/PR runs are cancelled; main pushes are retained for per-commit releases.

[Client builds](.github/workflows/client-release.yml) and any needed [server image builds](.github/workflows/server-image.yml) start alongside checks. Every desktop platform retains native package startup checks, launcher tests and actual client installation/launch checks; Android retains its unit tests and APK validation. Server builds retain container-native tests and stage image archives as workflow artifacts. **Every check for every selected commit must succeed before publication** through [client publication](.github/workflows/client-publish.yml) or [server publication](.github/workflows/server-publish.yml). Only the entry point calls these reusable workflows; none has a separate push/manual trigger that can bypass the gate. A client release still requires complete validated packages for that commit, and the server requires both architectures.

Rust 1.98.1 is pinned. Linux tests use the ordinary test profile with optimized dependencies and reduced debug output, avoiding release LTO; Clippy has its own compilation cache. Release builds retain their optimized profile and Windows static-CRT flags. Desktop builds emit the Rust library and executables; Android explicitly requests its shared library, avoiding redundant shared-library linking on desktop. `Swatinem/rust-cache` keeps dependency caches keyed by runner, target, compiler and profile inputs, pruning workspace artifacts and unused dependencies instead of storing another whole target tree for every commit. Parallel jobs restore previous completed caches independently. Cache keys omit compiler job count while builds retain their runner-specific limits. Linux/Android builds report workspace compiler-cache statistics; desktop builds and Linux tests upload compilation timings. Android's Gradle and pinned cargo-ndk caches are separate from Rust. Docker keeps native AMD64/ARM64 BuildKit caches and dependency layers for both test and release profiles. Cache misses cause ordinary compilation; final executable linking, test execution and package validation still run. The `rust-v3` prefix starts a fresh Rust cache namespace.

Clients are released for every new first-parent commit pushed to `main`, including multiple commits per push and documentation changes. Merge commits represent their merged branches; branch/PR commits do not publish. Commits predating the release tooling are skipped. A push supports at most 64 releasable commits (GitHub's 256-job client build matrix limit); larger batches and non-fast-forward pushes fail explicitly and can be released individually with **Checks and builds → Run workflow → commit** on `main`. That manual SHA receives the same complete checks and does not rebuild the server. Leave `commit` empty to check/build the current head and server. GitHub's explicit `[skip ci]` commit directive also skips the push workflow.

Each complete build publishes `client-<full SHA>` with four client ZIPs, four launcher ZIPs, `client-manifest.json`, and `SHA256SUMS`. Android-capable commits also include the APK; built-in-updater commits require a separate `android-manifest.json`, leaving the desktop manifest format unchanged. Android metadata records the actual APK version code, size, and SHA-256; the native updater also verifies package and signing identity before installation and rejects downgrades. Android unit tests run before APK packaging. Drafts stay hidden until every file is uploaded. Publication is serialized; commit order on `main`, rather than completion time, determines **Latest**. Once all checks pass, a later packaging/build failure for one commit does not discard other complete commits; a failed platform cannot publish a partial release. Reruns resume drafts and preserve published assets. Players read the public latest-manifest download and need no GitHub credentials.

The workflow normally uses `GITHUB_TOKEN`, granting contents write only to tag reservation/client publication and packages write only to server publication. Planning, checks and builds are read-only. A small reservation job creates immutable tags alongside checks, before the push head becomes historical; this publishes no release, and a failed check can leave a reserved tag without assets. GitHub can require **Workflows: write** for historical commits whose workflow files differ from current `main`. If such a rerun is rejected, create/push the exact lightweight `client-<SHA>` tag at that SHA with an authorized account, or configure an optional repository `RELEASE_TOKEN` secret with **Contents: write** and **Workflows: write**. That token is used only after an ordinary mutating request is rejected. Normal current-main publication needs no extra token.

Run tooling tests with Python 3.11+ using `python3 -m unittest discover -s scripts/release -v` and `python3 -m unittest discover -s scripts/ci -v`. `package.py` packages native binaries; `smoke.py` verifies extracted applications. Mac bundles must be packaged on macOS. Publication uses GitHub's documented `concurrency.queue: max`; older Actionlint versions may not recognize it.

## Run from source

Use Rust/Cargo compatible with the workspace's Rust 1.97.1 minimum, then run from the repository root:

```sh
cargo run --locked -p rubblekin_client
```

The first build compiles Bevy and takes longer than subsequent launches. The client opens a join screen with `rubblekin.oci.koski.co:7878` as the default server address and a display name. Choose **Join server** to connect remotely or **Local world** to host at `127.0.0.1:7878` with the save `saves/villages.json`. New saves generate an inhabited island; an existing save retains its original generator and terrain. Names are guest display names, not authenticated accounts. Failed connections return an error on the screen so you can correct the address and retry.

Use `--local` to go straight into a local world, or `--connect HOST:PORT --name NAME` to connect immediately. Balanced graphics is the initial desktop default; use `--low` for the least expensive preset. Joining reports actual connection and landscape-preparation stages with elapsed time. Cancel, Escape, or Android Back returns to the form after the current step and any local-world saving finish. Click the game to capture the mouse; press Escape to open the pause menu. Resume or Escape returns to play. Press F10 or choose **Leave world** to return to the join screen.

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

The client loads 50 cm editable voxels near the player or observer and draws distant terrain from the same geography. The default 48 m near-detail range loads at most 169 nearby chunks; the menu allows 24–1000 m (49–63,001 chunks). Two detail-generation jobs and one distant-mesh job are active at a time. The world uses a bounded column cache and sparse saved edits rather than storing billions of untouched blocks. New local worlds use a separate save path so the original valley stays intact:

```sh
cargo run --locked -p rubblekin_client -- --local
cargo run --locked -p rubblekin_client -- --local --observe
cargo run --locked -p rubblekin_client -- --local --save saves/valley.json
```

Export the actual generated geography as a shaded PPM map, with the spawn marked by a cream cross:

```sh
cargo run --locked -p rubblekin_core --example geography -- 42 /tmp/geography.ppm
```

New islands use GeographyV6, retaining the established landforms, village locations, main trails, and biomes: visibly distinct meadows, broadleaf woods, pine forest, dry scrub, desert, wet forest, tundra, alpine rock, beaches, and snow. Pine trees have taller tiered crowns, scrub is low and sparse, and meadows have fewer trees. V5 mixes slender aspens into broadleaf forest, broad cedars into pine forest, and spreading canopy trees into rainforest. Nearby voxels, simplified trees, and painted crown footprints use the same generated shapes and colors. River water and its carved bed share a channel profile, avoiding elevated water walls at tributaries and dry banks. Medium/far terrain uses smooth stitched heightmap triangles painted with an 8192² desktop map: biome colors, gentle relief, actual water, village footprints/fields, connecting trails, and individual tree-colored dots at the actual generated positions. Android retains 2048²; smaller device texture limits select a supported power-of-two size. The full 8192² mip chain uses about 341 MiB of GPU texture storage (about 21 MiB at 2048²); generation happens once when joining a world, and CPU pixels are released after upload. Filtered mip levels limit distant shimmer. Simplified trees cover your chosen 128–2500 m range, initially 128 m, while building silhouettes bridge to editable voxels within 128 m; fine grain stays on nearby blocks. Stepped fake voxel LOD and coarse square grain have been removed.

Existing worlds retain their recorded generator and saved edits, including V3 buildings and V4 regional homes, windmills, lookouts, and tree clearings. Villages and resources appear in GeographyV3–V6 worlds. V5 introduced tree varieties and sparse roadside ruins/waystones; loading a save from an earlier generator does not add or move them. Use `--generation v3` or `--generation v4` with a fresh save to reproduce those generators. V6 retains those sites and adds timber trail shelters and quarry yards. To try V6 without replacing an existing world:

```sh
cargo run --locked -p rubblekin_client -- --local --observe --generation v6 --save saves/scenic-island-v6.json
```

Current limits: drainage follows eight directions on the 64 m grid; water is static and does not simulate flowing through excavations or swimming. Individual edits appear only in the nearby detailed area; very distant terrain still uses a coarse surface, and vegetation beyond the chosen tree range is represented by crown-colored dots baked into the terrain texture, with no added tree meshes or draw calls. These dots describe generated trees; individual saved edits still appear only in detailed terrain. Village airships and the in-game world map are implemented below; gliders remain future work. Villages and trails are generated in GeographyV3–V6; live physical settlement expansion remains future work. Representative integrated-graphics measurements remain necessary. The [map-textured valley](artifacts/map-lod-valley.png), [closer view](artifacts/map-lod-near.png), [forest/coast](artifacts/map-lod-forest.png), and [exact 2048² atlas](artifacts/distant-map-atlas.png) show the approved medium/far rendering, enabled automatically in all graphics presets. Nearby [voxel gameplay](artifacts/map-lod-ground.png) and [ground shading](artifacts/terrain-readable-ground.png) are retained. Export the current exact atlas with `cargo run --locked -p rubblekin_client --example distant_map -- 42 /tmp/distant-map.ppm` (8192² by default; append `2048` for the fallback raster).

## Villages, resources, and residents

New local play uses `saves/villages.json`. The geography-first generator places a bounded network of established villages on dry, gentle land near reachable freshwater. Farming suitability, actual generated trees, and stone/clay/iron deposits contribute to scores over terrain-accessible catchments. Placement balances useful opportunities and spacing; the existing island and mountain landforms remain intact. Terrain-following dirt trails connect feasible sites; shallow wet crossings use fords or graded wooden causeways. Static water can overlap a few road surfaces by up to 0.20 m. Voxel cottages, storehouses, workshops, market stalls, and soil plots remain editable.

Each village starts with six residents and seeded stores; no chronological founding history is simulated. Residents have individual hunger and energy: they eat real village food, rest at home, and resume interrupted jobs with their cargo intact. Food shortages slow work without starvation or death. Farmers walk assigned crop rows, plant and tend crops, harvest ripe food, and carry it to stores. They retrace their field path for meals and sleep, then return to the interrupted work; that path and progress survive restart. Actual planted soil controls growth and yield; removing all planted soil destroys crop maturity. Other workers draw from bounded resource reserves, and traders carry actual surplus between villages while retaining food reserves. Stores, crop cycles, resident needs/routes/positions, cargo, and remaining reserves persist across restart and progress while the server runs with no clients. NPC resource reserves are a numerical estimate of accessible supply; their work does not remove visible ore blocks or fell trees. Player quarry work removes its actual designated loose pile blocks. Growth capacity is reported, but new residents/buildings are not yet created. Players can trade, carry paid deliveries, and do local field or workshop work as described below. Server downtime is not replayed.

Players, Moss, and village residents collide with each other using the shared character controller, including during creative flight. Joining players receive nearby unoccupied spawn positions. Resident movement passes around other people where space permits; blocked routes cannot produce goods remotely. The inspector shows hunger, energy, activity, and the reason for it, while farming and meals have visible hand motions. The read-only observer camera has no physical body.

The [village map](artifacts/villages-map.png) plots the actual seed-42 plan and resource scores; the [building catalog](artifacts/village-assets.svg) shows all thirteen generated building types, and the [tree catalog](artifacts/tree-silhouettes.svg) shows the six tree silhouettes. Both catalogs render actual source geometry and colors. The [native village view](artifacts/villages-native.png) shows the running client. Opening the inspector selects the character, block, or farm plot under the center dot, within 128 m and behind no nearer terrain. It keeps that target while its details update; close and reopen to select another. Farm plots show crop growth, harvest readiness, dimensions, and intact plant soil sites. Growth is currently one shared village crop cycle. V6 fields mix grain, leafy greens and root vegetable appearances, all harvested as the same Food. Harvested fields show bare soil; plants covered by solid blocks disappear from both rendering and aimed inspection. Small mallet/chisel and grain ties decorate existing intact workshop/storehouse fixtures in the nearby terrain batch. Blocks show material and cell coordinates, with building/village context and village stores at storehouses or markets. In read-only observer mode, press **V** to visit the next village; **R/Home** returns to spawn.

```sh
cargo run --locked -p rubblekin_client -- --local --observe --save saves/villages.json
cargo run --locked -p rubblekin_core --example settlements -- 42 /tmp/villages.json
cargo run --locked -p rubblekin_core --example village_asset_catalog -- /tmp/village-assets.svg
cargo run --locked -p rubblekin_core --example village_asset_catalog -- /tmp/tree-silhouettes.svg --trees
```

Existing saves remain available with `--save saves/geography.json` or `--save saves/valley.json`. Protocol v19 requires rebuilt matching clients and servers. This source update does not itself update the public test server.

## Trade, deliveries, and local work

Press **B** or **Cargo** to see your coins, goods, delivery, and nearby local work. Walk up to a village market entrance (within three meters) to buy or sell food, timber, stone, clay, and iron. Prices reflect local production and current stock. Choose one or five units; the panel shows the exact total before each purchase or sale. Transactions use the server’s current quote and save before confirmation. Village food and material reserves cannot be bought away.

You start with zero coins. Accept a delivery at a market to carry six sealed units to a neighboring village for **12 coins**. Deliver at the destination market, or return the parcel at its origin. Jobs have no expiry. Cargo holds **24 units**, including the sealed parcel; you can carry one delivery at a time. Ordinary walking and free airships transport cargo. Creative building materials remain separate from traded goods.

At cultivated fields or an intact workshop bench, choose **Start work** and remain nearby for **six seconds**. Field tending pays **2 coins** and plants or advances the village’s real crop cycle. At ripe fields with surplus village Food, **Harvest surplus crops** gathers up to **12 Food into your cargo**; sell it at a market to earn coins. Harvesting resets the shared crop cycle, so farmers and players compete for the same ripe crops. Damaged soil reduces the yield. Village Food must remain above its food reserve plus the maximum harvest, and all cargo must fit before starting or completing the work. Workshop maintenance pays **4 coins**, consuming **1 Timber and 1 Stone from village stock** while retaining its eight-unit reserve of each. The panel shows availability, distance, progress, and **Cancel work**. Moving away, losing access, leaving, or disconnecting cancels unfinished work; only completed effects and rewards persist. Unfinished work is not replayed offline. The [compact work panel](artifacts/local-work-touch.png) shows a confirmed field wage; the [harvest panel](artifacts/harvest-touch.png) shows real Food cargo. Nearby eligible work appears as a **B** hint or touch **Work** button. Your explorer faces the actual site and uses a hoe, hammer or empty harvest basket while server-confirmed work remains active. The [native harvest view](artifacts/work-tools-touch.png) shows the basket and leafy crops.

Beside a V6 quarry’s loose Stone pile, **Collect stone** removes one actual pile block after six seconds and adds **1 Stone to cargo**. Sell it at a village market for coins. Each pile has 50 finite blocks; walls, terraces and floors remain building scenery. A shared saved record prevents another payout if anyone rebuilds an extracted block. Existing creative materials stay unlimited and separate from goods. The [compact quarry work view](artifacts/quarry-work-touch.png) shows the completed cargo.

The panel has large buttons, keyboard selection with Tab/arrows and Enter, and mouse-wheel or touch scrolling. Its title, wallet, Close, quantity, and Refresh controls stay visible above the scrolling work, delivery, and trade body; a fixed footer explains scrolling. B, Escape, Android Back, or Close returns to play. Opening a panel stops your controls while the shared world keeps running. The HUD retains your active delivery destination.

Use the same **character name** to keep progress on this device. `player-profiles.json` in the game data directory stores private guest tokens scoped to the local save or remote address and character name. Back it up with your world save; losing the token loses access to that character’s progress. A different name creates a separate character. The server saves coins, cargo, job, and location in the world file; reconnecting also restores a moving airship position when applicable. Two sessions cannot use the same character simultaneously. Existing worlds gain markets without regenerating their terrain. Delivery/local-work rewards and sales create coins; purchases remove them. Village treasuries and a balanced monetary supply are outside this prototype.

V4–V6 villages keep their established doors, fields, residents, and trade routes while using timber cabins, pale masonry cottages, and steep-roof upland homes. Suitable sites add a farm windmill or a stair-access lookout joined to the village lane. These are editable voxel buildings with interiors; windmill sails are stationary. The [asset sheet](artifacts/village-assets.svg) shows the actual generated shapes, and the [native market](artifacts/player-market.png) shows a loaded delivery. The [windmill view](artifacts/village-windmill.png) records the native landmark. Difficult sites may omit the extra landmark.

Fresh V5 worlds also place a bounded set of roofless stone ruins and banded waystones beside existing trails. Walkable short spurs lead to open ruin courtyards or a marker and bench; these remain editable blocks. Placement avoids village fields, landings, steep/wet ground, and tree crowns. Main village and airship connections keep their established routes. Seed 42 has 17 roadside sites. The map marks them with violet **R/S** symbols, and aimed inspection identifies the actual structure or tree. The [native ruin courtyard](artifacts/trail-ruin.png) and [V5 world map](artifacts/world-variety-map.png) show the current content.

V6 worlds retain the V5 sites, open timber trail shelters and stone quarry workyards, and now add natural stone arches, standing-stone circles, hollow fallen giants, canvas traveller camps, ruined stair towers and abandoned kilns. Seed 42 has **282 roadside sites**. Short walkable spurs leave the through-trails; towers offer a viewing ledge and fallen trunks can be entered. Smaller cairns, trail benches, broken waycarts, survey posts, dead snags and split rocks fill gaps that reject larger footprints. Supported viewing decks fill raised-trail gaps, with timber piles preserving the hillside beneath. The [exploration catalog](artifacts/exploration-assets.svg) renders all thirteen new source assets. Their violet map dots reveal **A/S/F/T/O/K** symbols when zoomed in. Nominal walking gaps have a 95-second median and 106-second 90th percentile; 305 of308 seed42 gaps are within two minutes. Run `cargo run --locked -p rubblekin_core --example exploration -- 42 /tmp/exploration.json` for the actual route pacing and site coordinates. Their short approaches and quarry terraces are walkable; the yard’s cutting bench remains scenery while its loose pile supplies Stone work. The map adds **P/Q** markers for shelters and quarries. See the [native shelter](artifacts/trail-shelter.png), [quarry yard](artifacts/quarry-yard.png), and [V6 map](artifacts/v6-world-map.png). Sparse regional ferns, mushrooms, dry shrubs and tundra cushions replace selected grass tufts in the existing nearby terrain batch; they remain decorative and disappear when their supporting ground or headroom is edited. This pre-release content pass may revise generated scenery in existing saves; start a fresh world for review.

Geographic islands now have persistent rabbits and wolves. Rabbits hop and lower their heads to graze wild forage; wolves hunt rabbits, and both keep away from people. Hunger, food, breeding, age and physical migration gradually change populations while the server runs, including with nobody online. Village crops and player blocks remain untouched. Aim at an animal and press Tab to inspect activity, hunger and its habitat. The first pass bounds the island at256 animals and64 habitats; wild forage regrows in a saved habitat ledger. Near a wild plant, open **B: Cargo & work** and gather for six seconds to carry1 Food. Gathering reduces the same supply rabbits eat; sell Food at a village market. Berry bushes and mushrooms mix in woodlands; clover and strawberries mix in meadows, with herbs in shrubland. They visibly thin and regrow with that supply; aim and press Tab to inspect them. Ordinary decorative grass remains unchanged. Restart resumes saved ecology without simulating server downtime. See [native rabbits](artifacts/rabbits-native.png).

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
| 1–6 | Select a hotbar slot. |
| I (or C) | Open the creative inventory; choose a block, then click a hotbar slot or press 1–6 to assign it. I / Escape / Done returns to play. |
| F; Q / E | Toggle creative flight; descend / ascend. |
| Tab | Close inspection, or open it and select the target under the center dot. |
| G / N | Optional pilot conversation / next nearby pilot. |
| B | Open cargo, coins, deliveries, and nearby field/harvest/workshop work; trade beside a market. |
| F2 | Cycle Low → Balanced → High → Low graphics. |
| M | Open or close the world map; Escape also returns to play. |
| H | Show or hide the controls panel. |
| F10 | Disconnect and return to the join screen. |
| F12 | Save a screenshot in `artifacts/`. |

Touch **Inventory** opens the same catalog; tap a block, then the desired hotbar slot. The 153-block library includes nature, stone, masonry, eight woods with logs/planks/parquet, metals, and 18 colors each of concrete, wool and tiles. Browse categories/pages or click Search and type on desktop. Textured cube previews match the material patterns used in the world. Building materials are unlimited and do not spend cargo. Six hotbar choices persist in `creative-hotbar.json` beside other client preferences and survive launcher updates. Movement, looking and editing stop while the inventory is open, including its closing frame; the shared simulation continues. Observers remain read-only. While the map is open, **C** retains its map-center action.

Native previews: [desktop inventory](artifacts/creative-inventory-desktop.png), [colored blocks](artifacts/creative-inventory-colors.png), [compact touch inventory](artifacts/creative-inventory-touch.png).

Press **M** while playing or observing to open the north-up world map, or choose **Map** in the pause menu. It shows the actual island, rivers, trails, named towns, spawn, and your live position (camera position for observers). Town numbers match the directory, which gives your horizontal distance to each town and regional architecture where space permits. W/L badges identify windmills and lookouts. Fresh V5 worlds show violet R/S markers for trail ruins and waystones; V6 adds P/Q for shelters and quarry yards; larger layouts show the nearest roadside site and distance. Scroll or pinch over the map to zoom and drag with a mouse or one finger to pan. The visible **Zoom − / +**, **Center on you**, and **Whole world** buttons also work on touch screens; **C** and **R** retain the desktop shortcuts. **M**, **Escape / Android Back**, or **Return** closes it. Player controls stop while the shared world and airship travel continue. Legacy valleys have an overhead terrain map without towns. The geographic map represents generated terrain; individual block edits remain visible in the game nearby.

Developer controls work only for player sessions when the server allows them:

| Input | Action |
| --- | --- |
| Backquote / tilde (` / ~) | Open or close the admin command panel. |
| F6 / F7 | Force Moss to forage / rest. |
| F8 | Clear the override and restore autonomous choices. |
| F9 | Set hunger to 85 and energy to 35 for testing. |
| `[` / `]` | Favor rest / restore equal forage and rest weights. |

The local auto-host enables these controls for every connected player. Dedicated servers grant them to the player named exactly `maccam912` as an easter egg; other players need the server's `--allow-admin` setting. Names are freely chosen, so anyone using that exact name receives admin permissions. On keyboards with media function keys, use the platform's function-key modifier if needed.

## Admin commands

Press the **backquote / tilde key** (` / ~) in an admin-enabled player session to open the command panel. Type a command and press Enter; `wildlife` reports populations and nearby animal coordinates. Escape or the same key closes it. Up/Down recalls commands, Ctrl/Cmd+V pastes, and the mouse wheel scrolls replies. Your movement, building and gameplay shortcuts are blocked while the panel is open; the shared world keeps running.

| Command | Action |
| --- | --- |
| `help` | List every console command and its syntax. |
| `teleport X Y Z` | Move yourself to coordinates. |
| `teleport Ian X Y Z` | Move Ian to coordinates. |
| `teleport Violet` | Move yourself beside Violet. |
| `teleport Ian Violet` | Move Ian beside Violet. |

`tp` is an alias for `teleport`; a leading `/` is optional. Coordinates are **meters**, in X Y Z order, with Y the height of the player's feet. They must be inside the world and clear of terrain and characters. Named destinations choose clear space within three meters so players do not overlap; grounded destinations require a nearby supported floor. Names match in full, ignoring case; quote names containing spaces, for example `teleport "Ian Koski" Violet`. Missing or duplicate names produce a readable error.

The server grants admin commands and NPC developer controls to player sessions named exactly `maccam912` (case-sensitive), even without `--allow-admin`. Local hosting and `--allow-admin` grant them to every connected player. Observers remain read-only and still require the server-wide setting for admission. No per-user authentication is added. Teleporting clears velocity and preserves the player's creative-flight toggle. Coordinate teleports detach from airships; teleporting to a passenger places you on their moving deck. Teleporting invalidates pre-teleport movement inputs and requires matching rebuilt clients and servers.

## Ride village airships

Every GeographyV3–V6 village has an airship port and low landing berths connected by wooden gangways. Two-way services connect neighboring villages, with connections for farther trips. Each direction departs within three minutes and stops for 30 seconds; these are initial tuning defaults. Rides are currently free.

Walk or jump onto a landed airship to ride. Move, sprint, look around and jump with the ordinary controls while it carries you. Walk or jump off an edge at any time, including during flight; you return to ordinary falling. There are no boarding/exit menus or required conversations. The current prototype has no fall damage; gliders remain future work.

Optional **G / Pilot** asks a nearby pilot where they are going and when they leave. **N / Next pilot** selects another nearby pilot. An empty port displays the wait for its next departure.

NPC traders compare the complete airship trip, including approach and waiting, against walking to their destination. They carry their goods aboard and continue to the original destination before delivering; local jobs continue on foot. Journey state and schedules resume from saved simulation time after restart; server downtime is not replayed.

Airship platforms branch beside through-trails with clearance for the entire turning deck. Selected berth/ramp footprints clear generated tree crowns; player edits never move the landing. The pre-release October 8 generator changes can move older berths, so use a fresh world for review. Worlds without villages have no routes. Rebuild both client and server for protocol v19; this local source change does not update the public server.

Native macOS captures show the [voxel airship at its landing](artifacts/airship-port.png) and the [open deck in flight](artifacts/airship-onboard.png).

## Observe without a player avatar

Choose **Observe as admin** on the join screen, then **Local world** or **Join server**. Direct startup also works:

```sh
cargo run --locked -p rubblekin_client -- --local --observe
cargo run --locked -p rubblekin_client -- --connect 127.0.0.1:7878 --observe
```

Observer sessions require an admin-enabled server (`--allow-admin` on dedicated hosting). Local hosting enables this; the public test server has it disabled. The existing admin setting applies to everyone who can connect, so it is not per-user authentication.

The read-only camera creates no avatar and passes freely through terrain. WASD flies along the view, Q/E descends/ascends, mouse or arrows look, scroll changes speed from 2–64 m/s (initially 12), and Shift gives a 5× boost. **R or Home** returns to spawn and resets speed; **V** visits the next village in GeographyV3–V6. Tab closes inspection or opens it on the aimed character, block, or plot. F2 changes graphics, and F10 returns to the join screen, where you can switch back to **Play as explorer**.

Observers see live terrain edits, other players, and NPC activity. They cannot build or change NPC settings; the server enforces this even for custom clients. In geographic worlds, nearby detailed terrain follows the camera and distant landforms cover the full 32.768 km region. The old 160 × 160 m valley renderer remains available for legacy saves.

The current handshake uses **protocol v22**. Rebuild/restart both client and server together. Save version 8 records the terrain generator, village residents/stores, private player trading progress, shared consumed quarry/salvage cells, persistent wildlife/forage, and interrupted animal journeys. Wildlife home ranges change on physical arrival. Versions 1–7 load additively; earlier residents receive default needs and keep their jobs, goods, and terrain. Generation identifiers remain explicit, but pre-release scenery geometry may change in place under the October 8 save waiver. Original valleys remain a separate generator.

## Graphics

The sky has drifting clouds, pale blue at the horizon and deeper blue overhead, and a visible sun. The shared world runs a **40-minute day–night cycle: 20 minutes of day and 20 of night**, with gradual dawn and dusk. Sunset warms the sky, clouds and sunlight; Balanced/High shadows lengthen as the sun lowers. Shadow direction advances in small one-second steps to prevent continuous edge shimmer; the visible sun and color transitions remain smooth. Stars fade in at night while cool ambient and directional light keep the landscape readable. Low retains inexpensive contact shadows. Sky time resumes from the world's saved simulation clock, with no downtime catch-up.

Before loading a world, choose **Use minimum graphics** on the main menu to reset quality to Low, near detail to 24 m, and medium trees to 128 m, with dynamic shadows and antialiasing off. This saves immediately for the next launch and can help when expensive saved settings prevent entering a world. It is also available on Android.

All presets include terrain corner shading, darker ground sides, and a subtle top-edge cue at actual drops, so descending steps remain visible without sun shadows. Flat ground has no added edge outlines. Fine world-aligned grain fades below pixel size on nearby voxels; medium/far heightmaps use a generated map atlas. The embedded shader and atlas need no downloaded texture assets. Open **Escape** (desktop) or **Menu / Back** (Android) to choose quality, near-detail distance, medium tree distance, and shadow distance. All distances use meters. Drag a distance bar to preview a value and release to apply it; the − / + buttons retain step adjustment. Click a bar or select it with Tab, then use Left / Right for fine adjustment (8 m detail, 1 m trees/shadows), or Home / End for its minimum / maximum. Held keys preview the value and apply once released. **Reset distances** restores 48 m detail, 128 m trees, and the selected quality's shadow default without changing quality. Changes apply while the menu is open. Resume returns to play; Leave world returns to the join screen. Player controls stop while the menu is open, and the shared world continues running. F2 still cycles quality, and startup flags override the saved quality:

| Preset | Startup option | Shadows and antialiasing |
| --- | --- | --- |
| [Low](artifacts/shadows-low.png) | `--low` | Terrain shading and inexpensive ground shadows under characters; no dynamic shadows or MSAA. |
| [Balanced](artifacts/shadows-balanced.png), default | `--balanced` | Nearby sun shadows: one 1024 × 1024 shadow map out to 32m camera depth, hardware 2×2 filtering, no MSAA. |
| [High](artifacts/shadows-high.png) | `--high` | Two 2048 × 2048 shadow maps out to 90m, Gaussian filtering, and 4× MSAA. |

Near detail adjusts the editable voxel square around your player or observer from 24–1000 m in 8 m steps (default 48 m). These are approximate cardinal distances because loading follows chunk boundaries. Increasing this range adds memory and rendering work; the map-painted distant landscape stays visible. Legacy valleys already load their full terrain.

**Medium tree distance** controls how far simplified trees remain visible outside detailed terrain in geographic worlds: 128–2500 m (256–5000 blocks), default 128 m. Drag the bar for whole-meter values; the − / + buttons change it by 128 m, with the final increase reaching exactly 2500 m. Trees retain full generated density within the chosen range; increasing it adds generation, memory, and rendering work. Far building silhouettes keep their 128 m range; buildings inside the near square retain their placeholders until their detailed meshes arrive, and the map-painted landscape stays visible beyond the trees. The maximum is available for hardware experimentation; its frame rate is unverified.

Shadow distance adjusts the final sun-shadow cascade from 8–192 m. Low disables dynamic shadows and its distance controls; Balanced and High restore their 32 m / 90 m shadow defaults when selected. Changing quality retains your near-detail and tree distances. Shadows can only come from loaded terrain and visible characters; distant map terrain does not cast them. Preferences are saved in `graphics.json` beside the client's local data and survive leaving, rejoining, and restarting. Existing preferences without a tree-distance value keep the 256-block default.

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

Only one process can own a save file. Stop an auto-host before starting a dedicated server on the same port or save. Players receive new public session IDs when reconnecting. A private guest token restores trading progress and the saved position for that character on this server/world. This is local guest progress rather than an account/login system.

## Saves and containers

Accepted edits are saved before the server acknowledges them. The server writes a temporary file beside the save, syncs it, and atomically replaces the previous save. An OS lock on a sidecar file prevents concurrent writers. NPC state and simulation time are checkpointed every five seconds and on orderly shutdown. Corrupt or unsupported saves fail visibly and are left intact.

There is no downtime catch-up: simulation advances while the server runs, even with zero players, and resumes from saved time after a restart. The save records its terrain-generation version. Version-1 valley saves keep the original terrain when read and are written as version 7 with an explicit ValleyV1 generator. New islands use GeographyV6; existing GeographyV1–V5 islands keep their original terrain; a new client does not turn an existing valley into an island. Unknown save/generation versions fail visibly. Use a fresh save path to explore new geography.

[Dockerfile](Dockerfile) tests and builds only the headless server; it excludes the renderer and game assets. The runtime runs as UID/GID 10001. [deploy/kubernetes.yaml](deploy/kubernetes.yaml) provides one replica, a 1 GiB persistent volume claim, a `Recreate` rollout strategy, startup/readiness probes, `imagePullPolicy: Always`, and a private `ClusterIP` service. Review storage settings before using this standalone template.

```sh
docker build -t rubblekin-server:prototype .
docker run --rm -p 7878:7878 -v rubblekin-world:/data rubblekin-server:prototype
```

[Server image builds](.github/workflows/server-image.yml) test/build Linux AMD64 and ARM64 images on native runners alongside the shared checks when a main push or pull request changes server inputs, workflow files, or CI planning scripts. The architecture builds run in parallel and cancel each other on failure. They stage image archives without pushing to GHCR. [Server publication](.github/workflows/server-publish.yml) assembles the multi-platform image only after both builds and all shared checks succeed. Main pushes and manual main runs with an empty `commit` input publish `ghcr.io/maccam912/rubblekin` with immutable `sha-<full commit>` tags; pull requests never publish. Publication is serialized and promotes `latest` according to main's first-parent order, so a late older build cannot roll the server back. Repository/package linkage is included in the image labels. The GHCR package must be public for anonymous cluster pulls; package visibility is separate from repository visibility.

The actual OCI cluster configuration lives in [fleet-infra/apps/rubblekin](https://github.com/maccam912/fleet-infra/tree/main/apps/rubblekin). It uses the cluster's OCI block storage and shared ingress-nginx TCP load balancer on port 7878. The game uses raw TCP, so it needs a TCP forwarding entry, not an HTTP Ingress. Flux scans `latest` every five minutes and commits its new digest into the Deployment to trigger a rollout. `Always` checks the image when a container starts; it does not restart existing pods by itself. One `Recreate` replica ensures the old save writer stops before its replacement. The namespace and world PVC are retained when removing the Flux app and require deliberate manual deletion.

The public test server address is **`rubblekin.oci.koski.co:7878`**, prefilled on the join screen. Enter a display name and choose **Join server**, or connect directly:

```sh
cargo run --locked -p rubblekin_client -- --connect rubblekin.oci.koski.co:7878 --name Visitor
```

The October 4 native client and two-client socket checks passed over the original public IP (`147.224.165.110:7878`), including shared player state, ping, and logout removal. The public server disables the server-wide admin setting; the `maccam912` exception becomes available when this server change is deployed. OCI's security list permits TCP 7878 to the shared load balancer; its existing private rules cover forwarding to Kubernetes.

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
| [client](crates/client/src/lib.rs) | Bevy 0.20.0 rendering, input, prediction, camera, and inspection UI. |

For a block edit, read these in order:

1. [`edit_blocks`](crates/client/src/lib.rs) sends `ClientMessage::Edit` using the types in [protocol.rs](crates/core/src/protocol.rs).
2. [`handle_message` and `validate_edit`](crates/server/src/lib.rs) check rate, reach, line of sight, world bounds, and character occupancy.
3. The server updates [`World`](crates/core/src/world.rs), writes the [save](crates/server/src/persistence.rs), and broadcasts `ServerMessage::BlockChanged`, or replies with `Rejected`.
4. [`receive_network`](crates/client/src/lib.rs) applies the accepted edit; [`rebuild_chunks`](crates/client/src/terrain.rs) updates the affected meshes.

Transport is newline-delimited JSON over nonblocking TCP. There is no generic message bus or automatic ECS replication. The headless server currently uses a small standard-library loop; Bevy ECS can be introduced when the simulation earns that complexity. This transport and full-world snapshot approach are prototype choices, not a global-scale networking design.

Movement predicts each frame locally and sends the same numbered input and duration to the server. Server snapshots acknowledge completed inputs; [prediction.rs](crates/client/src/prediction.rs) replays newer inputs so delayed snapshots do not pull the player backward on release or step climbing. The server validates movement time, and prediction history is bounded. The current protocol requires matching client/server builds: restart both after updating. Existing valley saves remain compatible through the explicit legacy generator.

## Verify changes

```sh
cargo test --locked --workspace
cargo clippy --locked --workspace --all-targets -- -D warnings
cargo fmt --all -- --check
```

Tests include real localhost sockets, so the test environment must permit local networking. The October 6 repair pass verified 351 ordinary tests locally on macOS across the workspace run and final affected-crate reruns: 189 client, 59 core/trail, 67 server unit, 24 multiplayer and 12 launcher. The existing native package integration test remains opt-in; it previously verified a real client ZIP download, installation, bundle signature, and child process startup. Applicable release-tooling Python tests also passed earlier in this repair pass.

Geography regressions cover the actual raised-water junction, sampled channel banks, unchanged V1 landforms, substantial V2 biome regions, shared tree shapes, dry spawn, drainage, distant collision/edits, legacy-save compatibility, and retained generation identity after restart. Renderer checks cover bounded streaming/triangles, immediate trees in pending chunks, foliage colors at biome boundaries, stale edit jobs, deep cuts, distant edge alignment, heightmap cells without artificial terraces, and actual water/voxel placement. Existing tests cover joining, movement reconciliation, observer permissions, multiplayer replication, NPCs, persistence, and graceful shutdown. Workspace Clippy with warnings denied, formatting, and the locked offline native build pass. Village regressions cover resource-based placement, deterministic and legacy terrain, editable deposits/buildings, physical resident roundtrips on three seeds, every seed-42 trail in both directions, soil-dependent crops, real cargo conservation, blocked work, and village replication/persistence without connected players. Native macOS village views and observer visits were inspected; long-distance flight feel and lower-end performance remain unverified. The [CI pipeline](.github/workflows/ci.yml) checks pushed changes on Linux and verifies native release packages on Linux, Windows and macOS.

Capture a reproducible initial scene:

```sh
cargo run --locked -p rubblekin_client -- --local --screenshot artifacts/geography-ground.png --exit-after 40
```

`--screenshot` waits for a joined world, then lets the scene settle for eight seconds before capturing once. Leaving during that interval restarts it on the next join. `--exit-after` remains an absolute app-time deadline; if it wins before the image is saved, the log reports the missed capture. Increase or omit the deadline on slower machines. Keep the game window visible during capture; an occluded macOS window can produce an empty screenshot. F12 captures any current menu or view immediately. Screenshots document appearance, not frame-time behavior. Follow [the repeatable visual route](VISUAL_CHECKS.md) for isolated preferences, a fixed village/airship journey, and evidence to retain.

## Current limits and next feedback

- **World:** new islands span 32.768 km with 50 cm editable cells and bounded local detail. Distant terrain reflects generated geography; remote edits appear only in the nearby voxel region; medium/far heightmaps use the generated map texture, with simplified trees through the chosen 128–2500 m range. Building proxies outside near detail extend to 128 m; pending near chunks retain their buildings until detailed geometry is ready. Legacy 160 m valleys keep their original terrain.
- **Simulation:** Moss retains the original foraging loop. GeographyV3 adds up to 60 village residents with hunger/energy, active crop work, shared crop growth, stores, finite extraction reserves, and physical deliveries/trade. Local farm paths use actual terrain and character physics; player-built obstructions can still block residents. Player markets, coins, cargo, paid deliveries, and field/workshop jobs use those real stores and crops. Live settlement expansion, personal farm ownership, individual plant lifecycles, and flowing water remain future work.
- **Airships:** scheduled village shuttles, NPC passengers and pilot destination dialogue are implemented. Board physically, move on deck and walk/jump off anywhere. Routes, waits and passenger comfort need playtesting; gliding remains future work.
- **Building and ownership:** all players have unlimited materials and cooperative edit access. Claims and configurable offline property protection remain planned; no protection system is implemented. Moss does not destroy player structures.
- **Networking:** 32 connection cap and 100,000 edited-cell cap are defensive prototype limits. There are no accounts, transport encryption, hostile-client load tests, or public-server readiness claims. Slow or malformed peers are disconnected.
- **Compatibility and performance:** native macOS playtests exercised building/removal, NPC override/clear, a [second connected client](artifacts/two-client.png), and [restoring the edits after restarting](artifacts/restarted-world.png). The October 4 graphics playtest verified the full live Balanced → High → Low → Balanced cycle, nearby player/tree shadows, and the Low fallback; brief foreground HUD observations reached around 120 fps on an Apple M5 Pro. Focus and capture interruptions make these unsuitable for a frame-time comparison. Earlier October 3 samples at 1440 × 900 showed about 119–125 fps on the old low preset and 93–120 fps on the old high preset, with scene construction around 0.16–0.23 seconds. These are separate observations on a strong machine, not controlled benchmarks or a measured before/after speed comparison. Windows, Linux, the children's computers, representative integrated graphics, and performance during extensive building remain untested.

The next useful feedback is on village placement and appearance, visible resident work and trails, the first player farming/trade interaction, mountain and valley scale, and movement/camera feel. See [DESIGN.md](DESIGN.md) for the wider ambitions and unresolved choices.

The bundled Atkinson Hyperlegible font is distributed under its [SIL Open Font License](assets/fonts/OFL.txt).
