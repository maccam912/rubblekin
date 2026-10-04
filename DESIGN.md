# Rubblekin game design and project history

Last updated: 2026-10-04

This is the living design document and handoff for future conversations. It records the intended game, confirmed choices, proposed implementation, open questions, and work actually completed. Rubblekin is the working name taken from the project folder; the final name has not been decided.

## Current state

A first playable prototype is implemented in a three-crate Rust workspace. The native Bevy 0.19.1 client opens a scenic editable valley, supports third-person movement and a creative material palette, connects to an authoritative server, and displays one autonomous forager with inspection and developer controls. The user approved the simple starting architecture, a forager as the first NPC, and unrestricted materials for this prototype.

The user approved Balanced graphics by default with a Low fallback on 2026-10-04. The client now has stronger terrain corner shading in every preset, bounded nearby sun shadows in Balanced, and optional longer shadows with antialiasing in High. F2 cycles Low → Balanced → High → Low; startup flags select the same presets.

The HUD text was enlarged on 2026-10-04 at the user's request for more comfortable reading. Labels use 13–14 px, controls/status/notices use 16 px, and the inspector body uses 18 px, with wider inspector and material slots to accommodate them.

The broader design now includes a much larger finite world with geography generated up front, scenic long-distance transportation with airships and gliding as the motivating experience, and a fast admin camera usable without a player avatar. These directions were recorded on 2026-10-04; they are not implemented. An island is the suggested world shape, with exact dimensions and implementation choices still open.

The server persists terrain edits, NPC state, and simulation time. Real socket tests verify multiplayer replication, rejection, reconnection, and restart behavior. Native macOS playtests placed wood and brick, removed a placed block, exercised and cleared the NPC rest override, and connected a second native client. Restarting the native game restored the two remaining placed blocks and the forager's state. The October 4 join-screen verification passes 45 automated tests on macOS: 13 core, 16 client, and 16 server. Native macOS checks confirm clipboard entry, connection-error recovery, local play with the HUD, and returning to the join form with fields preserved. Workspace Clippy with warnings denied and formatting checks pass.

The October 4 movement fix replaces held-key sampling and pullback on stopping with numbered movement commands and acknowledged-input replay. Client and server now simulate identical input durations, addressing accumulated movement drift and inconsistent step climbing. Automated tests verify delayed acknowledgments, stopping, uneven frames, stairs/cliffs, and real localhost reconciliation. Subjective movement feel still needs the user's next playtest. Protocol v2 requires restarting both clients and servers with the rebuilt binaries; the save format is unchanged.

The client has only been run on macOS so far. Windows, Linux, and representative integrated graphics have not been validated. Git is initialized, and the user approved the public `maccam912/rubblekin` repository on 2026-10-04. A server-only ARM64 Docker image now builds and passes its Linux core/server tests and a two-client runtime/save/restart smoke test. GitHub Actions builds AMD64 and ARM64 images and publishes `latest` plus immutable commit tags to GHCR. The OCI cluster app is defined in `~/dev/fleet-infra/apps/rubblekin`; live publishing/deployment verification is recorded in the work log below.

[README.md](README.md) contains commands, controls, file pointers, and known limits. Current graphics evidence is in [shadows-balanced.png](artifacts/shadows-balanced.png), [shadows-low.png](artifacts/shadows-low.png), and [shadows-high.png](artifacts/shadows-high.png). October 3 evidence remains in [prototype.png](artifacts/prototype.png) with the former high preset, [two-client.png](artifacts/two-client.png), and [restarted-world.png](artifacts/restarted-world.png) with the former default low preset. The next design feedback should concern movement/camera feel, 50 cm building scale, the visual direction, and whether the forager's choices are legible. Representative lower-end hardware remains the next important performance check.

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

### Terrain and building

Players can place and remove blocks directly, including carving through a mountain. Terrain edits should be persistent parts of the world. Editable cells around 25–50 cm, combined with smaller decorative details, match the desired fidelity. The prototype uses 50 cm cells as an initial baseline; a 25 cm comparison has not been built, and the final cell size remains open.

The user prefers Veloren's grand, believable terrain to Minecraft's terrain. The confirmed direction is a much larger finite world than the prototype valley, with geography generated across the world up front to support realistic scales and coherent features. An island inspired by Veloren is the suggested shape; its adoption, exact dimensions, boundaries, and generation algorithms remain open. The intent is to establish landforms together before exploration, so future areas follow the same world plan.

Implementation proposal, not yet chosen: generate the whole world's coastline, mountain ranges, river systems, and biomes first, then generate or load detailed voxel chunks as needed and render simplified distant terrain. This would preserve the global geography while limiting the detailed terrain loaded at once. Whether to pre-generate every voxel or only the world plan remains open; generation time, storage, terrain-edit persistence, and distant views need validation before selecting the approach.

The user approved a creative palette with unrestricted materials for the first prototype. Direct placement and removal, repeated edits, and creative flight are implemented. More capable tools similar to a built-in WorldEdit are desirable; their scope, availability, and priority remain undecided.

### Travel and exploration

Long-distance transportation should make a large world enjoyable to explore. The user's children enjoy riding Veloren's airships, seeing the landscape from high above, and jumping off with a glider to investigate interesting places. Scenic passenger travel and the freedom to leave a ride for local exploration are desired experiences in Rubblekin.

Airships are the motivating example, with other forms of transportation also welcome. The design should support looking around during a journey and gliding down to explore. Vehicle types, routes, boarding, control by players or NPCs, travel times, glider handling, and landing consequences remain open. These features are not implemented, and their implementation order has not been selected.

### Admin observation

Provide an admin mode with a free-flying camera that can operate independently of any player avatar. An administrator should be able to inspect the world without creating, possessing, or following a character, and move at high speeds to cover large distances quickly. This is separate from passenger travel, gliding, and the prototype's existing character-based creative flight.

The independent camera and fast movement are confirmed requirements. Exact controls and speed range, access permissions, and available inspection or editing actions remain open. Loading and displaying terrain at the camera's location, including during rapid travel and high-altitude viewing, is an implementation challenge to validate on the target hardware. No standalone admin camera is implemented yet.

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

Movement uses protocol v2: each rendered frame predicts and immediately sends a numbered `Input` with its duration. The server executes each command once with the same shared controller and acknowledges its sequence in `PlayerSnapshot`; the client starts from that body and replays only unacknowledged inputs through [prediction.rs](crates/client/src/prediction.rs). Snapshots and NPC/world simulation remain at 20 Hz. Individual commands are limited to 250 ms, and a server-owned wall-clock budget allows a bounded 500 ms burst without sustained faster-than-real-time movement. The client retains at most 512 commands/two seconds before disconnecting instead of accumulating unlimited history. After 500 ms without input, the server applies neutral gravity without continuing held walking or jump input. Actual terrain changes or long stalls can still require authoritative corrections. This is a narrow repair to the existing authoritative model, not a change to movement speed, step height, or transport.

Initial connections receive a seed, all saved terrain edits, player snapshots, NPC state, and simulation time. Stable session IDs identify players during a connection; accounts and persistent character identities are not implemented. On 2026-10-04 the user selected a guest join screen with server address, display name, and local play, rather than authenticated accounts. This keeps the existing protocol and permission model. Rendering resources stay outside world data. TCP and full-world snapshots make this first implementation easy to inspect, but are not a commitment for future large populations.

Accepted edits are acknowledged only after a temporary save is synced and atomically replaces the previous JSON file. An OS lock on a sidecar file prevents multiple server writers. NPC state and time are checkpointed every five seconds and on graceful shutdown. SIGTERM and Ctrl+C perform a final save. Invalid saves fail startup without replacement; save failures stop the server visibly. Existing worlds retain their saved seed. Save migration and terrain-generation version migration are not implemented.

Prototype defensive bounds are 32 connections and 100,000 edited cells, with bounded inbound messages and outgoing buffers. These are limits, not tested population targets. All players currently have creative materials and shared edit rights. The server's `allow_admin` switch grants developer controls to every connected player; local auto-hosting enables it and dedicated hosting defaults to disabled. Public authentication, TLS, ownership claims, and configurable offline protection are not implemented. Moss does not alter buildings.

The Kubernetes template has one server, persistent storage, and a private service. Sharding, cross-server migration, microservices, and automatic scaling are deferred. A seamless global world could require substantial architectural changes; this server is not a promise that the goal will scale unchanged. Splitting distant regions across processes alone would not solve the confirmed requirement for potentially large gatherings in one place. Revisit that challenge through explicit population targets and measurements before promising scale.

## Simulation and performance baseline

Status: the first small implementation is working; wider simulation and low-end performance remain to be validated.

Moss is a forager with hunger, energy, gathered berries, and weighted forage/rest choices plus wandering. The inspector displays the selected action, scores/reason, needs, and target. The server reevaluates decisions twice a second while movement and world time advance at 20 Hz, including with zero connected players. Moss walks using the shared collision controller; a simple blocked-motion response can jump, but general pathfinding is absent. Three shared, renewable logical berry patches provide food. They are not yet plant entities with depletion, growth, or lifecycle simulation.

Developer controls can set needs, favor rest or restore equal weights, force forage/rest, and clear a forced goal. Overrides and weights persist in the save. NPC hunger is part of the simulation; the player has no hunger timer or inventory management. The approved creative palette avoids imposing an unchosen gathering/crafting loop.

The prototype separates:

- Decorative visual detail, such as meadow plants and berry shrubs.
- 50 cm editable terrain cells, including voxel trees.
- An upright character collision body and raycast-based editing.

The playable valley is 160 × 160 meters. More distant mountains are scenery outside the editable world. Water is visual and does not flow into excavations. Terrain uses exposed-face chunk meshes with material colors and vertex ambient occlusion. An edit rebuilds its chunk and affected neighbors. All playable chunks are currently constructed up front; streaming, distant terrain LOD, and aggregated populations are not implemented. This small-valley approach does not settle how the planned larger world will be generated or loaded. Aerial travel and the fast admin camera will need wider views and rapid access to terrain beyond this baseline.

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

1. Does third-person movement, camera framing, and direct building feel comfortable in the running prototype?
2. Is the visual direction and 50 cm cell size a useful baseline, or should the next experiment compare finer cells or different terrain proportions?
3. Are Moss's motives and behavior understandable? What first player interaction should let someone help the forager without turning it into an obedient unit?
4. Which representative integrated-graphics computer and frame-rate/resolution target should guide the next performance pass?

The starting architecture, forager, and unrestricted prototype palette are decided. The larger-world direction, scenic travel and gliding goals, and independent fast admin camera are also confirmed; their implementation details and priority remain open. Do not ask whether these goals are wanted again as if still pending.

### Questions to address as they become relevant

- Building rights: open cooperation, owned plots, faction territory, NPC claims, and creative/admin privileges.
- The source of building materials and how storage and crafting avoid inventory chores.
- Food's role in trade, ecology, NPC needs, or optional player benefits without a player hunger timer.
- Authored quests, requests generated by real world needs, or a mixture; how to avoid repetitive errands.
- Combat feel, player versus player rules, death penalties, and recovery.
- How much NPC construction and destruction may alter player work, and how those decisions are communicated.
- Exact terrain cell size, first-person camera support, and creative tool scope.
- Whether to use an island, exact world dimensions and boundaries; generation algorithms and whether to pre-generate all voxel detail or a global world plan; how terrain edits interact with future world-generation updates.
- Transportation types and routes, travel times, passenger interaction, and glider handling and landing rules.
- Admin camera controls, speed range, permissions, inspection/editing capabilities, and terrain loading during rapid movement.
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
| 2026-10-04 | World scale and generation | Confirmed direction: a much larger finite world with geography established up front for believable scale and coherent landforms. This narrows the earlier fully open world-generation question. An island remains a suggested shape; exact size, algorithms, and how much voxel detail to pre-generate are undecided. Global planning with streamed detail and distant terrain LOD is an implementation proposal. |
| 2026-10-04 | Scenic transportation and gliding | Confirmed experience: long-distance rides that let players enjoy the view and jump off to explore with a glider, inspired by the children's enjoyment of Veloren airships. Airships motivate the design; other transportation is welcome. Detailed mechanics and implementation priority remain open. |
| 2026-10-04 | Independent admin camera | Confirmed: a free-flying admin camera usable without attachment to a player, with fast movement across the map. Clarifies that embodied-player guidance applies to ordinary gameplay. Existing creative flight does not fulfill this requirement; controls, permissions, and implementation remain open. |
| 2026-10-04 | Shadows and default quality | User explicitly approved Balanced by default with Low available after requesting more visual depth without harming the children's frame rate. Supersedes the former Low default. Implemented stronger terrain shading, one short-range sun-shadow cascade in Balanced, and an optional High preset; exact budgets are implementation choices awaiting representative hardware measurements. |
| 2026-10-04 | Movement reconciliation | User reported backward pull on W release and cliffs even on localhost. Replaced latest-held-input sampling and position-only correction with exact command durations, sequence acknowledgments, and bounded client replay. Keeps the confirmed authoritative server and current controller behavior; protocol v2 supersedes v1 and requires matching rebuilt peers. |
| 2026-10-04 | Test-server deployment | User approved a public `maccam912/rubblekin` repository, GHCR image publishing, an OCI Kubernetes test server exposed on TCP 7878, and an address/display-name join screen. The guest protocol remains unauthenticated, and remote admin controls remain disabled. One persistent save writer and Flux digest tracking provide safe restarts and latest-image rollouts. |

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
- Built the nonroot server-only ARM64 container and passed 29 Linux core/server tests. A real container smoke test connected two clients, checked admin was disabled, ran with a read-only root filesystem, verified SIGTERM exit 0 and persistence, and restarted from the same volume. Published public AMD64/ARM64 images with `latest` and full-commit tags. The first successful emulated build took 11 minutes, with ARM compilation dominating; replaced it with native `ubuntu-24.04`/`ubuntu-24.04-arm` jobs and a final manifest publication after both pass. Native build/publish run `37213598254` succeeded for source `dd1c918`, producing digest `sha256:76649a45b292ddca8c0ce321bc447fad217441000be87fef433a952113cc5ea0`. Anonymous registry pulls are verified. Linux CI for the same client source passed formatting, all 45 workspace tests, and Clippy; macOS/Windows CI is still running. Local macOS checks remain successful.
- Added the OCI Flux app to fleet-infra with one Recreate replica, `Always`, a retained namespace and world PVC, startup/readiness probes, five-minute latest-digest tracking, and an OCI-scoped TCP forwarding patch. Committed/pushed fleet revision `483a804`. The root Flux configuration is now healthy; the user explicitly approved deleting the previously disabled Nitter/Gloomreach/Curling workloads whose stale references blocked reconciliation. Their namespaces are gone. OCI provisioned a 50 GiB volume for the 1 GiB request. The server is running from the public GHCR image. Flux generated image-digest commits `1f74f54` and `dae5648` automatically; the second tracks the native-runner publication, which rolled out automatically and is Ready. The save retained seed 42 and simulation time across both replacements (over 558 seconds observed after the second rollout). A real two-client protocol check through a Kubernetes port-forward passed handshake, shared player state, ping, disabled admin controls, and logout removal. The native macOS client also joined this cluster server from the new form and rendered its world/HUD; it returned to the same form with a clear disconnect message when the next rollout closed its connection. Public TCP 7878 is still blocked; OCI CLI reauthentication is pending before checking its network rules.
- Added a native guest join form, asynchronous connection/local hosting, display-name/address validation, retry messages, F10 disconnect, and reconnect cleanup. `--local` preserves direct local startup and `--connect` remains available. The wire protocol is unchanged. Added six regressions; 45 tests, formatting, Clippy, and native build pass on macOS. Native checks found and fixed two issues: macOS modifier flags needed to be read from the existing Winit backend, and the gameplay camera needed explicit UI selection. A native retest passed Cmd+A/paste, correct-address connection refusal, display-name editing, local play/HUD rendering, and F10 return with values preserved. Added the already-locked Winit version as a direct client dependency; no new framework or protocol change.

## Technical references

These references informed the initial discussion. They are not project requirements or substitutes for checking the actual dependency versions during implementation.

- [Bevy rendering on wgpu](https://bevy.org/news/bevy-webgpu/)
- [Bevy plugins and headless applications](https://bevy.org/learn/quick-start/getting-started/plugins/)
- [Veloren world generation and world scale](https://book.veloren.net/players/world-generation.html)
- [Veloren decorative voxel models within landscape blocks](https://book.veloren.net/contributors/guides/adding-sprites/guide.html)
