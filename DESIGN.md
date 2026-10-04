# Rubblekin game design and project history

Last updated: 2026-10-04

This is the living design document and handoff for future conversations. It records the intended game, confirmed choices, proposed implementation, open questions, and work actually completed. Rubblekin is the working name taken from the project folder; the final name has not been decided.

## Current state

A first playable prototype is implemented in a Rust workspace with core, server, client, and launcher crates. The native Bevy 0.19.1 client opens a geographic island or a saved prototype valley, supports third-person movement and a creative material palette, connects to an authoritative server, and displays one autonomous forager with inspection and developer controls. The user approved the simple starting architecture, a forager as the first NPC, and unrestricted materials for this prototype.

The October 4 client-distribution work adds per-commit GitHub releases and a separate launcher that downloads and starts the latest client. Implementation defaults are first-parent commits pushed to `main`, Windows/Linux x64 and both Mac architectures, a small graphical progress/retry window, and explicit cached play if an update fails. The user authorized committing and publishing this implementation on 2026-10-04. Client installations are versioned and verified with SHA-256; local game data is stored separately and retained through updates. The launcher uses a stable version-1 manifest and does not replace itself. [README.md](README.md#download-and-play) documents downloads, data locations, and release operation. Remote release verification is recorded in the work log below.

The October 4 CI reorganization uses one `Checks and builds` entry point: parallel, fail-fast checks for every selected commit gate both client releases and server images. Client tests and builds share compatible Rust compilation caches; Clippy and Docker use separate caches. Main pushes retain per-commit releases, while superseded branch/PR runs cancel. Both release and image publication preserve commit order when updating their latest version. These workflow changes are locally validated, and the user authorized pushing them on 2026-10-04. The first GitHub run confirmed fail-fast cancellation and blocked both builds when a macOS ARM64 test exposed a wall-clock assumption. That test now waits for persisted idle progress; its replacement run and cache speedups remain to be verified.

The user approved Balanced graphics by default with a Low fallback on 2026-10-04. The client now has stronger terrain corner shading in every preset, bounded nearby sun shadows in Balanced, and optional longer shadows with antialiasing in High. F2 cycles Low → Balanced → High → Low; startup flags select the same presets.

The HUD text was enlarged on 2026-10-04 at the user's request for more comfortable reading. Labels use 13–14 px, controls/status/notices use 16 px, and the inspector body uses 18 px, with wider inspector and material slots to accommodate them.

The join screen defaults to `rubblekin.oci.koski.co:7878` at the user's request on 2026-10-04. This applies to direct client and launcher startup; players can edit the address or choose local play.

The October 4 geography pass implements a much larger finite world at Veloren’s default physical scale: 32.768 km across, with mountain ranges around two kilometers high. The user explicitly requested mountains, erosion, valleys, and travel distances resembling Veloren. New worlds use a seeded global geography plan with drainage, erosion, lakes, rivers, climate, and biomes; detailed editable terrain is loaded around the player or observer, with distant terrain sampled from the same landforms. An island is the initial implementation default; the user has not separately confirmed its boundary shape. Airships and gliding remain unimplemented.

The independent admin camera is now implemented. On 2026-10-04 the user approved a read-only observer session with no avatar, available on local/admin-enabled servers. Select **Observe as admin** on the join screen or pass `--observe`; the camera has free flight, adjustable speed, a Shift boost, and R or Home to return to spawn. It receives live terrain, player, and NPC updates; the server rejects all observer movement, building, and NPC-control requests. The camera streams nearby terrain in geographic worlds and retains the original rendering for legacy valleys.

The server persists terrain edits, NPC state, and simulation time. Real socket tests verify multiplayer replication, rejection, reconnection, and restart behavior. Earlier native macOS playtests placed wood and brick, removed a placed block, exercised and cleared the NPC rest override, connected a second native client, and restored edits/NPC state after restart. Join-screen checks confirmed clipboard entry, connection-error recovery, local play, and returning with fields preserved. After the geography pass, all 89 ordinary workspace tests pass on macOS: 24 core, 31 client, 22 server, and 12 launcher; the existing packaged-launcher test remains opt-in. Workspace Clippy with warnings denied, formatting, and the locked offline native build pass. The new native ground view was inspected; long-distance flight feel and performance on representative lower-end hardware remain unverified.

The October 4 movement fix replaces held-key sampling and pullback on stopping with numbered movement commands and acknowledged-input replay. Client and server now simulate identical input durations, addressing accumulated movement drift and inconsistent step climbing. Automated tests verify delayed acknowledgments, stopping, uneven frames, stairs/cliffs, and real localhost reconciliation. Subjective movement feel still needs the user's next playtest. The geography handshake now uses protocol v4 and requires matching rebuilt clients and servers. Save version 2 records the terrain generator; version-1 saves load with the original valley generator.

The user requested smoother camera follow on 2026-10-04 and approved implementation. The third-person camera now interpolates its follow point so instant terrain step-height changes transition smoothly in the view. Mouse look and zoom remain immediate, with terrain obstruction checked after smoothing. Smoothing the visible player remains a possible follow-up experiment.

The client has only been run on macOS so far. Windows, Linux, and representative integrated graphics have not been playtested. Git is initialized, and the user approved the public `maccam912/rubblekin` repository on 2026-10-04. A server-only ARM64 Docker image now builds and passes its Linux core/server tests and a two-client runtime/save/restart smoke test. GitHub Actions builds AMD64 and ARM64 images and publishes `latest` plus immutable commit tags to GHCR. The OCI cluster app in `~/dev/fleet-infra/apps/rubblekin` is running at `147.224.165.110:7878`; two-client socket checks and a native macOS client connection passed over the public endpoint. Live publishing/deployment verification is recorded in the work log below.

[README.md](README.md) contains commands, controls, file pointers, and known limits. Geography evidence is in [geography-map.png](artifacts/geography-map.png) and [geography-ground.png](artifacts/geography-ground.png). Earlier graphics evidence is in [shadows-balanced.png](artifacts/shadows-balanced.png), [shadows-low.png](artifacts/shadows-low.png), and [shadows-high.png](artifacts/shadows-high.png). October 3 evidence remains in [prototype.png](artifacts/prototype.png) with the former high preset, [two-client.png](artifacts/two-client.png), and [restarted-world.png](artifacts/restarted-world.png) with the former default low preset. The next design feedback should concern movement/camera feel, 50 cm building scale, the visual direction, and whether the forager's choices are legible. Representative lower-end hardware remains the next important performance check.

For a new conversation, read this document and `AGENTS.md`, inspect the actual project state, and continue from the latest decisions and work log. Recheck external technical details when implementation requires them.

## The game we want to make

Build an approachable voxel sandbox RPG with grand landscapes, direct construction, and independent inhabitants. A player can find a place in the world through simple farming and trade, building, exploration, or eventually dungeon adventures and fighting monsters. Quests provide direction for players who want it.

In ordinary gameplay, the player is an embodied person in the world. They can cooperate with others and influence inhabitants, but do not control society as a disembodied god. Activities reminiscent of Dwarf Fortress may happen through the inhabitants' own decisions. A separate admin mode may operate without a player avatar, as described below.

The main inspirations are:

- Veloren's terrain, sense of scale, and detailed voxel appearance.
- Minecraft's ability to place blocks, build, and reshape the landscape.
- Dwarf Fortress's interacting systems and the stories they create, with a more approachable player experience.
- Rust's persistent shared world, with less punishment for being logged out.

This is a greenfield game. Existing games provide inspiration, not obligations to reproduce their mechanics or architecture.

## Confirmed direction

### Player roles and accessibility

Players choose how they participate. Farming should eventually contribute to the economy, while combat and dungeon exploration provide other paths. Cooperation and quests matter. Third person is the primary camera direction; adding a first-person option remains open.

Avoid player hunger timers, inventory juggling, and recipe memorization. This does not settle whether food, storage, crafting, or NPC needs exist; those systems should support meaningful activity without turning routine play into chores.

Progressively reveal complexity. Players should be able to participate in a living world without understanding every system that sustains it.

### Movement and camera feel

Smooth the third-person camera by interpolating (lerping) toward its follow position instead of instantly moving to each new player position. On 2026-10-04, the user reported that climbing terrain feels discontinuous when the character steps up a level and the camera immediately jumps with it, then approved implementation. [follow_camera.rs](crates/client/src/follow_camera.rs) now smooths the eye anchor on all axes with an exponential, time-based lerp: a stationary terrain step reaches 95% of its new height in about 170 ms. This is an initial tuning choice awaiting subjective playtesting. Mouse look and zoom remain immediate. The final camera position is constrained by a ray from the actual player eye, so a lagged anchor cannot carry it through terrain. A fresh camera starts at the player; offsets over four meters snap to avoid sweeping across large corrections. Follow state belongs to the camera entity and resets when leaving/rejoining. The independent observer camera retains direct motion.

Also consider interpolating the rendered player position to soften the character's visible step upward. This is a possible experiment, not a confirmed requirement. The recommended approach is to keep visual smoothing separate from authoritative movement and collision, preserving immediate input response and correct terrain interactions. Playtest whether any added visual lag is acceptable before adopting player smoothing.

### Terrain and building

Players can place and remove blocks directly, including carving through a mountain. Terrain edits should be persistent parts of the world. Editable cells around 25–50 cm, combined with smaller decorative details, match the desired fidelity. The prototype uses 50 cm cells as an initial baseline; a 25 cm comparison has not been built, and the final cell size remains open.

The user prefers Veloren's grand, believable terrain to Minecraft's terrain. On 2026-10-04 the user requested implementation of the full geography pass, including mountains and erosion, and selected Veloren’s mountain, valley, and travel scale as the reference. The implementation uses a 32.768 km square region and roughly two-kilometer peaks, matching the physical scale of Veloren’s bundled default map. A 513 × 513 global plan at 64 m spacing establishes landforms and drainage before exploration. An irregular island is the initial boundary default, pending specific shape feedback; exact geographic tuning remains subject to playtesting.

The geography implementation generates coastline, mountain ranges, drainage basins, rivers, lakes, and climate first, then samples detailed 50 cm terrain as needed. It uses an independent implementation of noise-based landforms, water-driven incision and sediment transport, and slope weathering. Detailed terrain and distant views share the same geography. This supersedes the earlier proposal-only status of global planning with locally loaded detail. The generation algorithm is versioned with saved edits: existing valley saves retain their original geometry, and future generator changes must introduce a new version or an explicit migration.

The user approved a creative palette with unrestricted materials for the first prototype. Direct placement and removal, repeated edits, and creative flight are implemented. More capable tools similar to a built-in WorldEdit are desirable; their scope, availability, and priority remain undecided.

### Travel and exploration

Long-distance transportation should make a large world enjoyable to explore. The user's children enjoy riding Veloren's airships, seeing the landscape from high above, and jumping off with a glider to investigate interesting places. Scenic passenger travel and the freedom to leave a ride for local exploration are desired experiences in Rubblekin.

Airships are the motivating example, with other forms of transportation also welcome. The design should support looking around during a journey and gliding down to explore. Vehicle types, routes, boarding, control by players or NPCs, travel times, glider handling, and landing consequences remain open. These features are not implemented, and their implementation order has not been selected.

### Admin observation

Provide an admin mode with a free-flying camera that can operate independently of any player avatar. An administrator should be able to inspect the world without creating, possessing, or following a character, and move at high speeds to cover large distances quickly. This is separate from passenger travel, gliding, and the prototype's existing character-based creative flight.

The independent camera and fast movement are confirmed requirements. The user approved read-only observer sessions on local/admin-enabled servers on 2026-10-04: no avatar, block editing, or NPC overrides, with WASD, mouse look, vertical movement, and a speed boost. The implemented camera flies through terrain independently of character physics; Q/E move vertically, mouse or arrows look, scroll adjusts base speed from 2–64 m/s (12 m/s initially), Shift multiplies speed by five, and R or Home returns to spawn. These exact speed values and the reset key are implementation defaults for playtesting. The inspector still shows Moss's needs, decisions, and target.

Observer admission reuses the existing server-wide `allow_admin` setting; it is enabled for local hosting and disabled on the public test server. This is not per-user authentication. The session creates no player avatar, never sends predicted character input, and cannot mutate terrain or NPC settings even through crafted protocol requests. The distinction between session identity/mode and player existence introduced in protocol v3 remains in v4. Legacy valleys still construct their small terrain up front. Geographic worlds load a bounded set of detailed chunks around the camera, with coarser terrain spanning the island. R or Home returns to spawn. Representative target-hardware performance and subjective flight/loading feel remain to be verified.

### Independent inhabitants and an evolving world

Inhabitants should have their own needs and wants. They can refuse requests, leave settlements, start businesses, and pursue their own lives. Relationships, factions, tribes, and kingdoms should be able to evolve over time.

Ecology matters: animals eat plants, predators eat prey, and populations and environments adjust over time. These processes need not be fast or constantly obvious to the player. The goal is a believable world with gradual consequences.

Designers need to tune probabilities and weights. Creative or admin tools should be able to override agent behavior for direction and testing. The first prototype exposes needs, utility scores, current action, and target; it supports weight changes and a persistent forced goal that can be cleared. Production permissions, temporary overrides, and richer debugging remain open.

### Multiplayer and persistence

Support small private servers and aspire to global scale within one seamless shared world, with everyone potentially able to gather in the same place. Many independent worlds or forced player separation do not satisfy this ambition. Internally distributing work by region may still be useful if it preserves seamless play and addresses large gatherings. Actual population targets and how to achieve them remain unresolved; this is not a first-version performance promise.

While the server runs, settlements and world history should continue evolving even when no players are logged in. Characters should disappear safely on logout rather than remain vulnerable sleeping avatars. Combat logout timing and abuse prevention have not been designed.

Offline changes to player property will be a server policy, with a protected default. The default should protect player buildings and stored goods while the surrounding world evolves. The exact definition of ownership, permissions, and allowed environmental effects remains open; selecting configurable policy does not require a general rules engine in the first version.

Progress during server downtime is a separate, unresolved long-term question. The prototype resumes its saved simulation time without replaying downtime. Continued simulation with zero connected players does not imply catching up through every hour that the server was stopped.

The deployment target is the user's ARM64 OCI Kubernetes cluster, with configuration in `~/dev/fleet-infra/apps/rubblekin`. The user approved public TCP 7878 through its existing ingress-nginx load balancer on 2026-10-04. The server uses one nonroot `Recreate` replica and a retained world PVC; scaling replicas does not distribute the world. `imagePullPolicy: Always` applies on container starts, while Flux tracks the `latest` digest and commits image changes to trigger rollouts. GitHub Actions publishes from `main` to `ghcr.io/maccam912/rubblekin`. Authentication is not part of this test deployment; anyone who can reach the server can join and edit, with developer/admin controls disabled.

### Graphics and supported computers

Target Linux, Windows, and macOS, including integrated graphics. The user's children's computers run Veloren on minimum settings and Minecraft acceptably, but become choppy with more demanding effects or higher resolution. Their actual hardware has not been inspected for this project.

The game should look good at modest settings. More expensive effects can improve presentation on stronger hardware, but must not become a prerequisite for the core experience. Exact hardware, resolution, frame rate, memory, and server population targets are still unknown.

On 2026-10-04, the user requested more depth and shadows without sacrificing the children's frame rate, then explicitly selected Balanced as the default with Low available. This supersedes the prototype's former Low default. Cheap terrain shading applies at every quality; real shadows stay limited in distance and resolution. Representative hardware measurements remain necessary before treating these presets as a performance guarantee.

### Development tools and architecture preferences

Rust with Bevy and wgpu is the user's preferred stack and is now used by the prototype client (Bevy 0.19.1). [Bevy already uses wgpu](https://bevy.org/news/bevy-webgpu/); these are complementary layers rather than two separate renderers to build.

LLMs are allowed for development. The user's initial mention of LLMs referred to developing the game, not a requirement for LLM-driven gameplay. Gameplay models or other decision models may be explored later. Do not add a model provider framework or API dependency now.

The user found Veloren's client/server messaging difficult to extend because of abstraction and indirection. Adding a simple feature should remain easy to trace. Good design should leave room to grow while following YAGNI and progressive disclosure of complexity.

Ask for feedback on consequential design choices throughout development. The user wants an active role in these decisions.

## Starting architecture and current implementation

Status: the user accepted one authoritative server, a client, and an explicit shared protocol. The implementation is deliberately small; exact transport and rendering choices remain replaceable prototype decisions.

| Area | Current responsibility |
| --- | --- |
| [Client](crates/client/src/main.rs) | Bevy rendering, input, third-person camera, local movement prediction, authoritative correction, and UI. |
| [Server](crates/server/src/lib.rs) | A 20 Hz authoritative loop, direct message handlers, validation, NPC simulation, and persistence. |
| [Core](crates/core/src/lib.rs) | Pure Rust seeded terrain, edited cells, character physics, and a small explicit protocol module. |
| [Launcher](crates/launcher/src/lib.rs) | Check GitHub, verify and install a complete client, then launch it with persistent game data. A small eframe window displays progress and explicit fallback; the updater also supports a headless CLI. |

The headless server uses the Rust standard library without Bevy. [Bevy can run headlessly](https://bevy.org/learn/quick-start/getting-started/plugins/), but this prototype does not yet need a server ECS. Add that dependency when actual simulation work justifies it.

The implemented block-edit path is:

```text
Client edit_blocks selects a position and material
  -> ClientMessage::Edit over newline-delimited JSON/TCP
  -> server handle_message / validate_edit
  -> World::set_block and atomic save
  -> ServerMessage::BlockChanged to connected players
  -> client receive_network updates World and rebuilds affected chunk meshes
```

Rejected edits receive `ServerMessage::Rejected` with a reason. Validation covers rate limits, editable bounds, reach, line of sight, and character intersections. Connections and buffers are bounded; malformed or backlogged clients disconnect. There is no generic message bus, generated registry, or automatic ECS replication.

Movement uses numbered inputs (introduced in protocol v2 and retained in v4): each rendered frame predicts and immediately sends a numbered `Input` with its duration. The server executes each command once with the same shared controller and acknowledges its sequence in `PlayerSnapshot`; the client starts from that body and replays only unacknowledged inputs through [prediction.rs](crates/client/src/prediction.rs). Snapshots and NPC/world simulation remain at 20 Hz. Individual commands are limited to 250 ms, and a server-owned wall-clock budget allows a bounded 500 ms burst without sustained faster-than-real-time movement. The client retains at most 512 commands/two seconds before disconnecting instead of accumulating unlimited history. After 500 ms without input, the server applies neutral gravity without continuing held walking or jump input. Actual terrain changes or long stalls can still require authoritative corrections. This is a narrow repair to the existing authoritative model, not a change to movement speed, step height, or transport.

Initial connections receive a seed and terrain-generation version, all saved terrain edits, player snapshots, NPC state, and simulation time. Stable session IDs identify players during a connection; accounts and persistent character identities are not implemented. On 2026-10-04 the user selected a guest join screen with server address, display name, and local play, rather than authenticated accounts. Protocol v4 retains the explicit player/observer session modes and guest permission model, and adds the terrain-generation version. Clients generate the same global plan in their join worker so generation does not block menu input. Rendering resources stay outside world data. TCP and full-world snapshots make this first implementation easy to inspect, but are not a commitment for future large populations.

Accepted edits are acknowledged only after a temporary save is synced and atomically replaces the previous JSON file. An OS lock on a sidecar file prevents multiple server writers. NPC state and time are checkpointed every five seconds and on graceful shutdown. SIGTERM and Ctrl+C perform a final save. Invalid saves fail startup without replacement; save failures stop the server visibly. Existing worlds retain their saved seed and terrain generator. Version-1 valley saves are read without changing their terrain; the next save writes version 2 with an explicit ValleyV1 generator. New geographic worlds record GeographyV1. Unsupported or missing generation metadata in version-2 saves fails visibly. Automatic conversion of existing valleys into islands is intentionally unavailable because it would move terrain beneath saved edits.

Prototype defensive bounds are 32 connections and 100,000 edited cells, with bounded inbound messages and outgoing buffers. These are limits, not tested population targets. All players currently have creative materials and shared edit rights. The server's `allow_admin` switch grants developer controls to every connected player and permits read-only observer connections; local auto-hosting enables it and dedicated hosting defaults to disabled. Public authentication, TLS, ownership claims, and configurable offline protection are not implemented. Moss does not alter buildings.

The Kubernetes template has one server, persistent storage, and a private service. Sharding, cross-server migration, microservices, and automatic scaling are deferred. A seamless global world could require substantial architectural changes; this server is not a promise that the goal will scale unchanged. Splitting distant regions across processes alone would not solve the confirmed requirement for potentially large gatherings in one place. Revisit that challenge through explicit population targets and measurements before promising scale.

## Simulation and performance baseline

Status: the first small implementation is working; wider simulation and low-end performance remain to be validated.

Moss is a forager with hunger, energy, gathered berries, and weighted forage/rest choices plus wandering. The inspector displays the selected action, scores/reason, needs, and target. The server reevaluates decisions twice a second while movement and world time advance at 20 Hz, including with zero connected players. Moss walks using the shared collision controller; a simple blocked-motion response can jump, but general pathfinding is absent. Three shared, renewable logical berry patches provide food. They are not yet plant entities with depletion, growth, or lifecycle simulation.

Developer controls can set needs, favor rest or restore equal weights, force forage/rest, and clear a forced goal. Overrides and weights persist in the save. NPC hunger is part of the simulation; the player has no hunger timer or inventory management. The approved creative palette avoids imposing an unchosen gathering/crafting loop.

The prototype separates:

- Decorative visual detail, such as meadow plants and berry shrubs.
- 50 cm editable terrain cells, including voxel trees.
- An upright character collision body and raycast-based editing.

Legacy valleys remain 160 × 160 m with their original scenery. New geographic worlds span 32.768 × 32.768 km; ocean, mountain ranges, valleys, rivers, and lakes belong to the actual world. Nearby terrain uses exposed-face 50 cm voxel meshes with baked corner shading. A bounded set of chunks follows the player or observer, while coarser terrain uses the same geography across the full world. Mesh generation runs in bounded background jobs, and edits invalidate stale detailed work. Water follows generated drainage and basin surfaces but remains static: it does not flow into excavations or affect player swimming/buoyancy. Distant terrain represents the generated surface; individual voxel edits and trees appear within detailed terrain range. Aggregated populations and travel vehicles remain unimplemented.

Graphics budgets are explicit in [graphics.rs](crates/client/src/graphics.rs):

| Preset | Current rendering choices |
| --- | --- |
| Low (`--low`) | No dynamic shadows or MSAA; inexpensive ground-shadow shapes under characters. |
| Balanced (`--balanced`, default) | One 1024 × 1024 sun-shadow cascade ending at 32m camera depth, `Hardware2x2` filtering, no MSAA. |
| High (`--high`) | Two 2048 × 2048 cascades with bounds at 18m and 90m, Gaussian filtering, 4× MSAA. |

F2 cycles Low → Balanced → High → Low without reconnecting. Ground-shadow shapes are visible only in Low, avoiding doubled shadows in the other presets. Distant scenery does not cast shadows, while the landscape's view distance remains unchanged. Bevy's final cascade ends at a hard boundary; no smooth distance fade is implemented. All presets use the same authoritative gameplay.

Terrain corner ambient occlusion is baked into mesh vertex colors with brightness factors `[1.0, 0.88, 0.76, 0.64]`; it adds no separate per-frame screen-space pass. Face diagonals follow corner shading, and edits refresh affected neighboring chunks, including diagonals. Global ambient brightness was reduced from 850 to 600 to strengthen directional light contrast. The art direction continues to emphasize terrain shape, color, vegetation, and distant atmosphere. The prototype uses a bundled readable font and an on-screen control reference.

Historical October 3 native macOS samples at 1440 × 900 showed about 119–125 fps on the former low preset and 93–120 fps on the former high preset on an Apple M5 Pro with 20 GPU cores and 64 GB memory. A second native client connected successfully; its captured state showed two explorers and two saved edits. Both original presets were visually inspected. Scene-construction logs showed about 0.16–0.23 seconds at that checkpoint.

The October 4 native playtest verified the full Balanced → High → Low → Balanced cycle without errors, visible nearby player/tree shadows, and ground-shadow shapes only in Low. Brief foreground HUD observations reached around 120 fps on the M5 Pro. Focus and capture interruptions make these unsuitable for a frame-time comparison. These and the older samples are observations on a strong machine, not controlled benchmarks or a measured before/after speed comparison. Active building, larger populations, representative integrated graphics, and other operating systems remain untested. The children's hardware has not been inspected or tested for this project.

Comparing 25 cm and 50 cm construction remains a useful experiment after feedback on this baseline. Halving cell width produces eight times as many cells in the same volume; actual memory/rendering cost depends on storage, compression, visible surfaces, and meshing. No final terrain-storage or fidelity decision should be inferred from the prototype alone.

## First playable version

Status: the user confirmed the balanced valley, building, multiplayer, and autonomous NPC scope on 2026-10-03 and then approved implementation with a forager and creative materials. The first implementation is present; user feedback and representative hardware validation remain outstanding.

| Completion check | Current evidence |
| --- | --- |
| Two clients receive accepted edits and explicit rejections. | Real TCP integration tests pass, including out-of-reach and character-intersection rejections. A second native client also joined and showed two explorers and the two saved edits. |
| Terrain survives restart; reconnecting clients receive current state. | Integration tests cover acknowledgment after saving, reconnection, restart, seed preservation, and a single save writer. A native restart also restored the two blocks placed in the UI playtest. |
| Digging and building work with the camera and collision. | Geometry/physics tests and the visible-ground regression pass. Native input placed wood and brick (three saved edits), then removed a brick (two saved edits). User feedback on comfort remains needed. |
| An NPC chooses actions independently; tuning and overrides are observable. | Unit/integration tests cover harvesting, eating, autonomous progress without players, weights, overrides, and disabled developer controls. Native UI rest override and restoration of autonomy were exercised. |
| The scene is measured on representative lower-end hardware. | Not satisfied. Native macOS evidence exists on an M5 Pro; lower-end hardware and Windows/Linux remain untested. |

Combat, dungeon content, quests, farming, a complete economy, kingdoms, and world-spanning ecology remain outside this first version. They remain part of the broader vision where confirmed above. The implemented creative building mode is not a completed survival, economy, or protected-property design.

## Open design questions

### Next feedback and choices

1. Does third-person movement, camera framing, and direct building feel comfortable in the running prototype? With camera interpolation implemented, do terrain steps feel smoother while controls remain responsive, and would smoothing the visible player help too?
2. Is the visual direction and 50 cm cell size a useful baseline, or should the next experiment compare finer cells or different terrain proportions?
3. Are Moss's motives and behavior understandable? What first player interaction should let someone help the forager without turning it into an obedient unit?
4. Which representative integrated-graphics computer and frame-rate/resolution target should guide the next performance pass?

The starting architecture, forager, and unrestricted prototype palette are decided. The larger-world geography pass is now implemented at the user-selected Veloren scale; its tuning and initial island boundary need feedback. Scenic travel/gliding goals remain confirmed, with their mechanics and priority open. The independent fast admin camera now has an approved read-only scope and an implemented prototype; its controls and speed range need playtesting. Do not ask whether these goals are wanted again as if still pending.

### Questions to address as they become relevant

- Building rights: open cooperation, owned plots, faction territory, NPC claims, and creative/admin privileges.
- The source of building materials and how storage and crafting avoid inventory chores.
- Food's role in trade, ecology, NPC needs, or optional player benefits without a player hunger timer.
- Authored quests, requests generated by real world needs, or a mixture; how to avoid repetitive errands.
- Combat feel, player versus player rules, death penalties, and recovery.
- How much NPC construction and destruction may alter player work, and how those decisions are communicated.
- Exact terrain cell size, first-person camera support, and creative tool scope.
- Whether the initial island shape and Veloren-scale geography feel right in play; erosion, biome, and drainage tuning; future terrain-generator migration policy. The current size, global plan with local detail, and versioned edit interpretation are implementation defaults for this pass.
- Transportation types and routes, travel times, passenger interaction, and glider handling and landing rules.
- Admin camera feel and speed tuning, future per-user permissions or editing capabilities beyond the approved read-only scope, and the feel/performance of terrain loading during rapid movement in a larger world.
- World progression during server downtime and recovery from interrupted saves.
- Representative client hardware and measurable performance targets; expected first-server player and NPC counts.

These are a backlog of decisions, not a request to settle everything before prototyping.

## Decision and challenge history

| Date | Topic | Status and reasoning |
| --- | --- | --- |
| 2026-10-03 | Greenfield project | Confirmed. Build a new game inspired by existing games, with freedom to use LLMs in development and revisit inherited design choices. |
| 2026-10-03 | Player agency | Confirmed. The player is embodied; inhabitants perform broader social and economic activities independently. |
| 2026-10-03 | Building fidelity | Partly decided. Direct terrain editing and 25–50 cm cells plus decoration fit the vision; exact resolution needs comparison. |
| 2026-10-03 | Autonomous behavior | Confirmed direction. Agents can refuse, leave, and pursue goals; designers need tunable weights and admin/creative overrides. Detailed rules remain open. |
| 2026-10-03 | LLM misunderstanding | Resolved. LLM use means development tooling initially. Gameplay models are optional future work. |
| 2026-10-03 | Complexity and chores | Confirmed. Apply progressive disclosure and YAGNI; avoid player hunger timers, inventory juggling, and recipe memorization. |
| 2026-10-03 | Client/server complexity | User accepted the simple authoritative architecture. Direct JSON/TCP message types and handlers are implemented and socket-tested; transport scalability remains unproven. |
| 2026-10-03 | Persistent world and absence | Confirmed direction. A running world evolves with no players online; characters disappear safely on logout. User selected server-configurable property protection with a protected default. Detailed ownership rules and server downtime behavior remain open. |
| 2026-10-03 | Global scale versus simplicity | Ambition confirmed: one seamless shared world, with everyone potentially in one place. A small authoritative server is implemented; large gatherings remain an architectural challenge to validate. Independent worlds or forced player separation do not satisfy the ambition; internal regional distribution is not ruled out. |
| 2026-10-03 | First playable scope | Confirmed. User selected the balanced valley, movement, building, persistence, two-player multiplayer, and inspectable autonomous NPC version. |
| 2026-10-03 | Graphics and accessibility | Confirmed targets. Integrated graphics, Linux, Windows, and macOS. Exact hardware and performance budgets require verification. |
| 2026-10-03 | Project continuity | Confirmed. Keep this design document current with goals, completed work, challenges, decisions, and rationale. `AGENTS.md` directs future conversations to it. |
| 2026-10-03 | First NPC and material loop | User approved a forager and unrestricted creative materials. Moss and the six-material palette implement that starting choice. |
| 2026-10-03 | Server ECS | Implementation choice: use a standard-library headless loop now. Bevy ECS remains available if future simulation complexity warrants it. |
| 2026-10-03 | First terrain resolution | Implementation baseline: 50 cm cells within the approved 25–50 cm range. A 25 cm comparison and final choice remain open. |
| 2026-10-03 | Edit visibility | Fixed a discovered validation defect: rays to a block center could reject a visible top face at a shallow angle. Validation now checks surface points, uses the shared eye height, and retains wall-occlusion tests. |
| 2026-10-03 | Persistence and shutdown | Atomic save-before-acknowledgment, exclusive writer locking, corruption errors, and SIGTERM/Ctrl+C final saves are implemented and tested. Server downtime is currently not replayed. |
| 2026-10-03 | Initial graphics evidence | Native macOS rendering and inspection UI were exercised. M5 Pro frame-rate observation does not satisfy the integrated-graphics target. |
| 2026-10-03 | Deployment limitation | Dockerfile and private Kubernetes template created; daemon unavailable prevented image validation. No cluster changes were made. |
| 2026-10-04 | World scale and generation | Confirmed direction: a much larger finite world with geography established up front for believable scale and coherent landforms. The later full-geography request below supersedes the proposal-only status of size, generation, and streamed detail. Island shape remains an initial choice awaiting feedback. |
| 2026-10-04 | Scenic transportation and gliding | Confirmed experience: long-distance rides that let players enjoy the view and jump off to explore with a glider, inspired by the children's enjoyment of Veloren airships. Airships motivate the design; other transportation is welcome. Detailed mechanics and implementation priority remain open. |
| 2026-10-04 | Independent admin camera | Confirmed: a free-flying admin camera usable without attachment to a player, with fast movement across the map. Clarifies that embodied-player guidance applies to ordinary gameplay. Existing creative flight does not fulfill this requirement; the subsequent read-only observer scope and implementation are recorded below. |
| 2026-10-04 | Shadows and default quality | User explicitly approved Balanced by default with Low available after requesting more visual depth without harming the children's frame rate. Supersedes the former Low default. Implemented stronger terrain shading, one short-range sun-shadow cascade in Balanced, and an optional High preset; exact budgets are implementation choices awaiting representative hardware measurements. |
| 2026-10-04 | Movement reconciliation | User reported backward pull on W release and cliffs even on localhost. Replaced latest-held-input sampling and position-only correction with exact command durations, sequence acknowledgments, and bounded client replay. Keeps the confirmed authoritative server and current controller behavior; protocol v2 supersedes v1 and requires matching rebuilt peers. |
| 2026-10-04 | Camera and player smoothing | User approved camera interpolation to soften abrupt view-height changes when climbing terrain steps, then requested implementation. Implemented camera-only follow smoothing with immediate look/zoom and final terrain obstruction checks; response speed needs subjective playtesting. Interpolating the visible player remains a possible experiment, with visual lag to assess before adoption. |
| 2026-10-04 | Test-server deployment | User approved a public `maccam912/rubblekin` repository, GHCR image publishing, an OCI Kubernetes test server exposed on TCP 7878, and an address/display-name join screen. The guest protocol remains unauthenticated, and remote admin controls remain disabled. One persistent save writer and Flux digest tracking provide safe restarts and latest-image rollouts. |
| 2026-10-04 | CI ordering and caching | User requested parallel checks that cancel on any failure, followed by client/server builds only after all checks pass, with compilation reuse where possible. One orchestrating workflow now gates reusable build workflows on all selected commits. Release-profile tests match the client build configuration to reuse cached dependencies; native client and Debian container caches remain separate. This supersedes independent push-triggered builds. |
| 2026-10-04 | Read-only admin observation | User approved a separate observer session with no avatar or editing on local/admin-enabled servers, using WASD, mouse look, vertical movement, and a speed boost. Implemented with the existing server-wide admin gate and protocol v3. Speed values, scroll adjustment, and R/Home reset are prototype defaults; per-user authorization and larger-world loading remain open. |
| 2026-10-04 | Full geography pass | User selected mountains, erosion, valleys, and travel distances mirroring Veloren. Implemented a 32.768 km region with global geography and bounded local detail. Island shape, erosion tuning, and rendering distances are initial implementation choices awaiting feedback. Existing valleys retain their generator; protocol v4 and save v2 carry generation identity. |
| 2026-10-04 | Default server address | User selected `rubblekin.oci.koski.co:7878` as the default connection address, superseding the join form's localhost default. Explicit `--connect` addresses and local hosting retain their existing behavior. |

## Work log

### 2026-10-03 — design discovery

- Inspected the project directory and confirmed it was empty; no applicable ancestor `AGENTS.md` was found.
- Discussed inspirations, player roles, terrain editing, agent independence, LLM use, persistence, accessibility, and engineering preferences.
- Checked official Bevy documentation on wgpu and headless operation, and Veloren documentation on terrain generation and decorative voxel models. These establish technical context, not performance guarantees for this game.
- Captured user decisions and clearly labeled architecture and simulation recommendations that still need feedback.
- Incorporated follow-up answers confirming the balanced first playable scope, one seamless world as the global-scale goal, and server-configurable offline property protection with a protected default. Replaced the earlier pending questions with the next unresolved choices.
- Created `DESIGN.md` and `AGENTS.md` for continuity between conversations. Reviewed the notes against the conversation for decision status and omissions.
- At the end of discovery, no code, benchmarks, or Kubernetes changes existed. The implementation work below supersedes that initial state.

### 2026-10-03 — first implementation

- Recorded the user's approval of the simple architecture, forager, and creative palette; built the Rust core/server/client workspace and pinned the client to Bevy 0.19.1.
- Added seeded finite terrain, persistent edited cells, editable trees, exposed-face meshing, collision/movement, third-person camera, direct building, creative flight, and an inspection HUD.
- Added the authoritative TCP server, explicit edit responses, bounded network work, save locking, atomic persistence, reconnect snapshots, and safe avatar removal on disconnect.
- Added Moss's independent forage/rest/wander behavior, visible berry patches, needs/weights controls, and clearable persistent overrides. Fixed blocked-motion detection to use actual displacement rather than desired velocity.
- Fixed shallow-angle digging validation and synchronized the client/server eye-height constant. Added regression coverage for visibility and retained obstruction checks.
- Verified 29 automated tests at this checkpoint: 13 core, 4 client, and 12 server. Workspace Clippy passes with warnings denied, and the formatting check passes. Real socket and dedicated-process signal tests required localhost networking outside the restricted execution sandbox.
- Launched the native client on macOS and exercised actual input for wood/brick placement, block removal, the NPC rest override, and restoration of autonomy. Connected a second native client and captured two explorers with the current edits. Visually inspected low/high graphics, added arrow-key camera controls and inexpensive low-preset ground shadows, and recorded the limited M5 Pro observations above. A native restart restored the two remaining placed blocks and NPC state; the final full test rerun passed all 29 tests. Saved screenshots show the current visual state, two-client session, and restored world.
- Added and exercised the macOS development launcher. Added the server Dockerfile, one-replica Kubernetes template, and a Linux/Windows/macOS CI matrix. CI has not run. Docker image validation is blocked by the unavailable daemon; no deployment has been made.
- Added README commands, controls, feature tracing, save behavior, verification instructions, and concrete prototype limitations. No gameplay LLM integration, global-server infrastructure, claim system, or survival economy was added.

### 2026-10-04 — world, travel, and admin design clarification

- Recorded the larger finite-world and up-front geography direction, keeping island shape and generation/storage choices explicitly open.
- Added scenic transportation and jump-off glider exploration, preserving the children's enjoyment of aerial sightseeing as the design rationale.
- Added the explicitly requested fast admin camera independent of any player avatar and clarified the ordinary-gameplay scope of the embodied-player principle.
- Reconciled current-state notes, open questions, and decision history with these additions. Reviewed the document for consistency and feature status; this was a documentation-only update, with no code changes or new runtime verification.

### 2026-10-04 — shadows and terrain shading

- Recorded the user's explicit choice of Balanced as the default with a Low fallback. Added three presets, matching startup flags, HUD labels, and live F2 switching.
- Bounded Balanced sun shadows to one 1024 × 1024 cascade at 32m with hardware filtering and no MSAA; High uses two 2048 × 2048 cascades to 90m with Gaussian filtering and 4× MSAA. Low keeps character ground-shadow shapes, which are hidden when real shadows are enabled. Distant scenery is excluded from shadow casting.
- Strengthened mesh-baked corner shading, adjusted face diagonals, preserved edit-driven rebuilding of affected neighboring chunks, and reduced ambient brightness. Added regressions for shading, winding, and chunk-edge/corner edits.
- Inspected Bevy 0.19.1 cascade resizing and filtering paths. The final cascade has a hard distance cutoff; this remains a known visual tradeoff of the bounded presets.
- Native macOS input verified the full Balanced → High → Low → Balanced cycle, nearby player/tree shadows, and no doubled ground shadow. Captured each preset with the game visible. Earlier empty occluded-window captures were excluded from the visual evidence. Screenshots and HUD frame-rate observations are appearance evidence, not controlled performance measurements. Windows, Linux, and the children's hardware remain untested.
- All 30 automated tests pass: 13 core, 5 client, and 12 server. Workspace Clippy with warnings denied and the formatting check pass.

### 2026-10-04 — larger UI text

- Increased the small HUD text by about 20–30% for comfortable reading: palette labels 10→13 px, section labels 11→14 px, controls/status/notices 13→16 px, and inspector text 15→18 px. Enlarged the title from 25→28 px.
- Widened the inspector from 292→340 px and material slots from 66→76 px, with slot height increased from 65→70 px to preserve spacing.
- The offline locked client build and workspace formatting check pass. Visually checked the running native macOS client at the default 1440×900 viewport: controls, inspector including developer hints, and all six palette labels fit without clipping or overlap. Saved [UI evidence](artifacts/ui-larger-text.png). Smaller windows and other platforms were not checked for this change.

### 2026-10-04 — movement reconciliation

- Reproduced two causes: the old 50 ms server loop lost simulated movement time to scheduling delays, and client/server frame subdivisions could disagree by a full 50 cm step on a controlled staircase. The previous correction deferred small errors until stopping, then pulled the client 40% toward the older server position.
- Added numbered per-frame movement commands with original durations and server acknowledgments. The server uses the identical shared controller, and the client replays newer commands after the latest snapshot instead of comparing positions from different points in input history. Input sends now flush immediately through the existing nonblocking socket.
- Retained authority with strict sequence/finite-duration checks, a 250 ms command limit, a 500 ms wall-clock burst allowance, bounded receive work and prediction history, and neutral gravity after input silence. Removed the server's second input normalization to keep both controller paths identical. No movement-speed, step-height, save-format, dependency, or transport changes.
- Added nine regressions: stop/release replay, uneven-frame staircase/cliff reconciliation with delayed states, genuine authoritative correction, prediction/acknowledgment bounds, real localhost client prediction, exact-once server execution, speed-budget rejection, invalid sequences/durations, and valid long-frame batching.
- All 39 tests pass, including real sockets and dedicated-process shutdown, with localhost access enabled. Workspace Clippy with warnings denied, formatting, and the offline locked workspace build pass on macOS. Subjective feel, other platforms, and representative integrated graphics remain unverified for this change.
- The rebuilt native macOS client completed a 12-second startup smoke test against its own localhost server and temporary save. Its eight-second screenshot showed the rendered world and connected HUD without a disconnect notice. This verifies startup/rendering, not the subjective feel of walking; existing user saves were not used by this smoke test.

### 2026-10-04 — test-server deployment and guest connection screen

- The user approved public `maccam912/rubblekin`, a guest server-address/display-name connection screen with local play, GHCR publishing, and public TCP 7878 on the existing OCI load balancer. Created the repository and configured Git LFS for seven curated documentation screenshots; generated artifacts/saves remain ignored and the small font stays in ordinary Git.
- Built the nonroot server-only ARM64 container and passed 29 Linux core/server tests. A real container smoke test connected two clients, checked admin was disabled, ran with a read-only root filesystem, verified SIGTERM exit 0 and persistence, and restarted from the same volume. Published public AMD64/ARM64 images with `latest` and full-commit tags. The first successful emulated build took 11 minutes, with ARM compilation dominating; replaced it with native `ubuntu-24.04`/`ubuntu-24.04-arm` jobs and a final manifest publication after both pass. Native build/publish run `37213598254` succeeded for source `dd1c918`, producing digest `sha256:76649a45b292ddca8c0ce321bc447fad217441000be87fef433a952113cc5ea0`. Anonymous registry pulls are verified. Linux and macOS CI for the same client source passed formatting, all 45 workspace tests, and Clippy; Windows CI is still running. Local macOS checks remain successful.
- Added the OCI Flux app to fleet-infra with one Recreate replica, `Always`, a retained namespace and world PVC, startup/readiness probes, five-minute latest-digest tracking, and an OCI-scoped TCP forwarding patch. Committed/pushed fleet revision `483a804`. The root Flux configuration is now healthy; the user explicitly approved deleting the previously disabled Nitter/Gloomreach/Curling workloads whose stale references blocked reconciliation. Their namespaces are gone. OCI provisioned a 50 GiB volume for the 1 GiB request. The server is running from the public GHCR image. Flux generated image-digest commits `1f74f54` and `dae5648` automatically; the second tracks the native-runner publication, which rolled out automatically and is Ready. The save retained seed 42 and simulation time across both replacements (over 558 seconds observed after the second rollout). A real two-client protocol check through a Kubernetes port-forward passed handshake, shared player state, ping, disabled admin controls, and logout removal. The native macOS client also joined this cluster server from the new form and rendered its world/HUD; it returned to the same form with a clear disconnect message when the next rollout closed its connection.
- Fixed public access after the user refreshed OCI authentication. The load-balancer listener and backend were correct; the shared VCN security list lacked TCP 7878. Added exactly one stateful public TCP 7878 ingress rule with an ETag guard, preserving all 13 existing ingress and six egress rules. Existing private VCN rules already covered the backend NodePort. A public two-client check at `147.224.165.110:7878` then passed protocol-v2 handshakes, distinct player IDs, shared state, ping, disabled admin controls, and logout removal. The native macOS client joined the same public endpoint through the form and rendered the world/HUD.
- Added a native guest join form, asynchronous connection/local hosting, display-name/address validation, retry messages, F10 disconnect, and reconnect cleanup. `--local` preserves direct local startup and `--connect` remains available. The wire protocol is unchanged. Added six regressions; 45 tests, formatting, Clippy, and native build pass on macOS. Native checks found and fixed two issues: macOS modifier flags needed to be read from the existing Winit backend, and the gameplay camera needed explicit UI selection. A native retest passed Cmd+A/paste, correct-address connection refusal, display-name editing, local play/HUD rendering, and F10 return with values preserved. Added the already-locked Winit version as a direct client dependency; no new framework or protocol change.

### 2026-10-04 — client releases and separate launcher

- User requested a GitHub release for each commit and a separately downloaded launcher that automatically updates the client on startup. Added native four-platform builds, complete draft publication, immutable full-SHA tags, checksums, and commit-ordered Latest promotion. Multi-commit pushes release each new main first-parent commit; incomplete builds cannot become releases. GitHub's historical workflow-permission restriction has an explicit tag/token recovery path.
- Added a Rust launcher with a small graphical window and headless CLI. HTTPS downloads are bounded and checksum-verified before extraction, installation, and atomic version selection. Fixed archive paths preserve Mac bundle signatures and the font license. Per-target installations avoid Intel/Apple Silicon collisions. Saves and screenshots use a separate stable game directory. Failed checks/downloads preserve the installed client; fallback requires an explicit choice. Cached executable bytes are checked again before launch.
- Packaging/publication and updater regression tests cover target/path validation, incomplete or mixed packages, out-of-order completion, interrupted drafts/installations, truncated or corrupted downloads, single-writer locking, cache integrity, literal client arguments, and save retention. All 57 ordinary Rust tests and 22 Python tests pass; workspace Clippy with warnings denied and formatting pass. A separate native package test passed using actual macOS debug binaries: ZIP through loopback HTTP, verified installation, strict codesign verification, and successful client startup. Native launcher UI checks passed first-run failure, Retry, Close, and explicit cached launch; no existing world was opened. The server-only ARM64 Docker build still passes all 29 Linux core/server tests with the new workspace manifest. Actionlint passes with only its outdated recognition of the documented `queue: max` field suppressed. The user authorized committing and pushing this implementation to `main`; the first remote release build and GitHub download remain to be verified. Windows/Linux/native Intel builds, release-profile packaging, and browser-downloaded Gatekeeper behavior await CI/native verification.

### 2026-10-04 — gated checks, builds, and shared caches

- Made `ci.yml` the sole branch-push/PR/manual entry point. A read-only planner selects the exact event commits and server input changes. Each selected commit runs ten parallel checks: formatting, release/planning tooling, and release-profile tests/Clippy on the four client targets. Both matrix levels cancel siblings on failure; client and server workflows require the complete checks matrix to succeed. Main runs retain all per-commit releases, while obsolete branch/PR runs cancel.
- Preserved the 64-commit client release limit, first-parent selection, historical manual rebuilds, and server path filtering. A manual historical rebuild checks the requested SHA and never publishes a server image. Tag reservation stays alongside checks to preserve the historical GitHub permission workaround without publishing releases before validation. A failed check blocks every build in its batch; a later package failure can still leave other complete checked commits releasable.
- Added Cargo caches to tests and Clippy. Tests use the same pinned runner, Rust version, explicit target, release profile, and Windows flags as client builds. Check/build cache suffixes preserve further build progress; restored earlier entries remain a best-effort optimization. Removed duplicate release tests from the client build stage, retaining package startup and launcher-install smoke tests. Kept architecture-scoped Docker layer caches and container-native tests; native client compilation is not portable into the Debian server build.
- Serialized only server publication and compared the existing image revision with main's first-parent history before moving `latest`. This prevents out-of-order builds from rolling deployment back and allows later documentation/client-only commits. Verified ten promotion scenarios with temporary Git histories and mocked Docker, including force pushes, initial publication, registry errors, and inconsistent labels. A read-only inspection of the existing GHCR image confirmed the expected revision metadata on both architectures.
- Verification: all 22 existing Python release tests and 16 new planner tests pass. Planner tests exercise real temporary Git histories, multi-commit and merge selection, path filtering, historical dispatch, missing/non-fast-forward history, limits, CLI outputs, and absence of tag/network mutations. Actionlint 1.7.12 passes with only its unsupported but GitHub-documented `concurrency.queue` key excluded; Python compilation and `git diff --check` pass. No Rust source changed. The user then authorized pushing the pipeline; actual nested cancellation, cache-hit rates, timing, and cross-platform execution await live GitHub verification.

### 2026-10-04 — idle-simulation CI test timing fix

- [The first gated run](https://github.com/maccam912/rubblekin/actions/runs/37217664796/job/111481407891) failed only the macOS ARM64 idle-world test: its 650 ms sleep did not guarantee more than 400 ms of simulation. The server intentionally avoids unbounded catch-up under load. Other unfinished checks were cancelled and client/server builds were skipped, confirming the requested failure gate.
- Replaced the fixed sleep with a bounded 15-second wait for the existing atomic autosave to show both world-time and NPC-position progress while no clients are connected. Reconnecting is deferred until that evidence exists. Kept the disabled-admin notice and unforced-NPC checks; removed the assumption that the autonomous NPC must still be foraging after the longer wait. Server behavior and tick timing are unchanged. The test normally takes about five seconds because it observes the autosave interval.
- Reproduced the original assertion failure by suspending only its test subprocess for one second, then verified the corrected test passes the identical scheduling pause. All 29 core/server tests pass with the CI release profile and explicit Apple Silicon target. Targeted Clippy with warnings denied, workspace formatting, and diff checks pass. The fix is ready for the replacement GitHub run; remote completion remains pending.

### 2026-10-04 — camera smoothing design note

- Recorded the user's request for interpolated camera follow, including the discontinuity caused by instant terrain step-height changes. Kept player visual interpolation as an optional experiment and its separation from collision as a recommendation.
- Updated current state, feedback questions, and decision history. Reviewed the documentation for consistent decision and implementation status; no code changes or runtime verification were part of this update.

### 2026-10-04 — independent read-only admin camera

- User approved a standalone read-only observer session: no avatar or editing, admitted on local/admin-enabled servers, with WASD, mouse look, vertical movement, and a speed boost. Added **Play as explorer / Observe as admin** to the join screen and `--observe` to direct startup. Selection persists through leaving or failed joins; switching back creates an ordinary player session.
- Protocol v3 gives the handshake an explicit session mode and identity. Observers receive the world snapshot, live terrain edits, players, and Moss without creating a player. The server gates admission with `allow_admin`, rejects observer movement/building/NPC mutation messages, and flushes clear denial/version notices before closing. Existing player validation, connection limits, persistence format, and public-server admin settings remain unchanged.
- Added a local free camera outside character collision and prediction. Prototype defaults: 12 m/s, scroll adjustment from 2–64 m/s, Shift 5× boost, Q/E vertical flight, and R/Home reset. Motion follows the view with normalized combined inputs; release or loss of focus stops it. The HUD hides building/NPC mutation controls while retaining live inspection, speed, position, and return instructions.
- Verified all 65 ordinary workspace tests on macOS (13 core, 21 client, 19 server, 12 launcher; the existing packaged-launcher test remains opt-in). Eight new tests cover camera motion/speed/reset, handshake/avatar invariants, actual ECS controls and socket writes, observer mutation rejection/live replication/lifecycle, denied admission, duplicate handshakes, and protocol mismatch. Existing join/rejoin and coalesced-network tests now cover both session modes. A final integrated rerun also included the concurrent third-person camera smoothing changes: all 71 tests (27 client) passed, including the observer system regression. Workspace Clippy with warnings denied, the locked offline workspace build, formatting, and diff checks pass.
- Native macOS verification used temporary saves: observer startup rendered zero explorers and no avatar, camera movement/look and speed adjustment worked, R reset/F10 return to the menu worked, mode selection survived leaving, ordinary play restored an avatar and palette, and a dedicated server with admin disabled displayed the expected denial in the join form. Saved [observer evidence](artifacts/observer-native.png). Native GPU access required running outside the execution sandbox. Home input was not reliably delivered by macOS automation, so R was added as an alternate reset and tested natively; automated ECS checks cover both reset keys. Windows/Linux, representative integrated graphics, larger-world streaming, and deployment of protocol v3 remain unverified. No existing user save or public server was modified.

### 2026-10-04 — third-person camera smoothing

- Added camera-owned follow state and a frame-rate-independent exponential lerp, settling 95% of a terrain step in about 170 ms. Mouse look and zoom stay immediate; the local avatar, collision controller, prediction, and observer motion are unchanged. New cameras and large corrections initialize directly at the target.
- Constrained the final smoothed camera candidate from the actual player eye, including its shoulder offset. This prevents lag from carrying the view through a wall and removes the old minimum-distance clamp that could place it beyond a nearby obstruction. Collision remains a single-ray approximation.
- Added six regressions for half-meter step smoothing and monotonic settling, equal elapsed-time response at 30/144 Hz, fresh-camera/large-correction initialization, immediate look/zoom, newly placed obstructions, and an anchor lagging behind a wall. Updated the observer system test's camera fixture with the required follow component.
- All 71 ordinary workspace tests pass on macOS, including observer controls and socket regressions; the packaged-launcher integration test remains opt-in. Workspace Clippy with warnings denied, formatting, and the locked offline native client build pass. A temporary-save native macOS startup rendered the world, avatar, and HUD without logged errors; its screenshot was inspected. Native UI automation returned late, so walking and subjective smoothing feel were not verified. Other platforms and representative lower-end hardware remain untested for this change.

### 2026-10-04 — default public server address

- Changed the join form's default address to the user-selected `rubblekin.oci.koski.co:7878`. Launcher startup uses the same client default. Updated README connection examples; explicit addresses and local hosting still follow their existing paths.
- The locked offline client compilation check, workspace formatting check, and diff check pass. This change did not verify DNS or a live connection through the hostname.

### 2026-10-04 — full geography at Veloren scale

- User requested mountains, erosion, valleys, and travel distances resembling Veloren. Checked Veloren's documented scale and bundled default map, then implemented an independent 32.768 km generator with a 513 × 513 global plan, warped mountain ranges, coastlines, and 48 erosion passes. Drainage accumulation, channel incision, sediment transport/deposition, slope weathering, and basin spillways shape rivers and lakes. Elevation, moisture, temperature, and rain shadows drive biomes; sampled seeds reach about 2.2 km above sea level. The initial island boundary is not a separately confirmed user choice.
- Added bounded nearby 50 cm terrain detail and asynchronous distant terrain shared by explorers and observers. Two detail jobs and one landscape job maintain at most 169 nearby chunks; sparse edits and a bounded column cache avoid materializing mountain interiors. Terrain edge interpolation and matching slope normals remove distant mesh cracks and obvious shading seams. Water triangles share terrain diagonals and keep dry vertices below ground to avoid floating sheets. Distant terrain currently omits voxel edits and vegetation until nearby detail loads.
- Introduced versioned generation in protocol v4 and save v2. Legacy version-1 worlds retain their exact valley generator, seed, edits, simulation time, and NPC state. New local worlds use `saves/geography.json`; `--save saves/valley.json` reopens an existing valley. Invalid generation metadata fails without replacing saves. Generation and initial session construction run in the join worker so the menu stays responsive. NPC home and berry patches use a stable spawn anchor.
- Expanded movement, collision, raycasts, and edit validation to the new bounds. Fixed contact tolerances that became smaller than floating-point precision at kilometer coordinates, causing false penetration recovery and wall climbing. Regressions cover wall contact, low ceilings, stepping, distant edits, terrain cuts, and restoring a geographic save despite changed server defaults.
- Verification: all 89 ordinary workspace tests pass, including real localhost networking; the packaged-launcher integration test remains opt-in. Geography audits across six seeds verify finite terrain, ocean borders, a dry gentle spawn, acyclic drainage, and downhill sampled channels. Renderer tests cover bounded assets across camera moves, stale edit jobs, deep cuts, edge alignment, and water placement. Workspace Clippy with warnings denied, formatting, the locked offline workspace build, and diff checks pass.
- Native macOS startup with a temporary new world rendered mountains, lake, editable ground, vegetation, avatar, Moss, and HUD; the final [ground view](artifacts/geography-ground.png) was inspected. A separate native observer startup logged successful terrain construction, but UI automation stalled, so manual long-distance flight and transition feel were not verified. Exported the actual seed-42 [geography map](artifacts/geography-map.png); its cream cross marks spawn. Windows/Linux and representative lower-end hardware remain untested for this pass. Water is static, drainage still follows eight directions on the 64 m plan, and there is no new travel vehicle or glider. This work has not been published or deployed.

## Technical references

These references informed the initial discussion. They are not project requirements or substitutes for checking the actual dependency versions during implementation.

- [Bevy rendering on wgpu](https://bevy.org/news/bevy-webgpu/)
- [Bevy plugins and headless applications](https://bevy.org/learn/quick-start/getting-started/plugins/)
- [Veloren world generation and world scale](https://book.veloren.net/players/world-generation.html)
- [Veloren decorative voxel models within landscape blocks](https://book.veloren.net/contributors/guides/adding-sprites/guide.html)
