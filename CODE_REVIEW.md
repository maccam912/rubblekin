# Codebase review — 2026-10-06

Reviewed revision: `0cf4a9455eed7e2191f32fe3360060d31d5846db`.

**Verdict: keep the architecture and refactor a few crowded parts.** This is a credible, deliberately direct prototype with unusually useful regression tests. It does not need a rewrite. It does need clearer ownership of terrain loading, modal input, and resident transitions before those areas acquire more features. Several visible shortcomings in the [gameplay review](GAMEPLAY_REVIEW.md) are art, layout, or feedback problems; moving Rust functions between files will not solve those.

This review judges the checked-in code and its behavior, irrespective of which model wrote it. Findings and priorities are assistant recommendations, not approved implementation or gameplay decisions. No implementation was changed by this review.

## Scope and evidence

Inspected the four-crate structure and representative paths through world generation, settlement planning, shared physics, movement prediction, TCP messaging, server simulation, persistence, terrain streaming, airships, menus/touch input, connection lifecycle, the desktop launcher, Android lifecycle/updater, and release orchestration. This is a source and architecture review with the existing tests run locally, not an exhaustive security audit or a line-by-line proof of every platform branch.

The Rust tree is roughly 38,000 lines including tests. File length overstates some problems: `server/villages.rs` is about 4,500 lines, but over 2,000 are tests; `client/terrain.rs` is about 3,000, with over 1,200 test lines. The concern is the number of responsibilities and cross-dependent states in the production code, not an arbitrary line limit.

Evidence labels below distinguish a **source-confirmed behavior**, a **refactor recommendation**, and a **performance hypothesis requiring measurement**. The supplied recording does not identify its executable revision, so a plausible source explanation is not proof of that recording's exact cause.

## What I would preserve

| Area | Assessment |
| --- | --- |
| Four crates: core, server, client, launcher | Good boundaries. Shared deterministic data/physics stay independent of Bevy; the server owns authority; the launcher stays separate from saves and gameplay. |
| Direct protocol and message handlers | A gameplay change remains traceable through an explicit message and handler. Bounded JSON/TCP is adequate for the current prototype. No evidence here justifies an event bus, generated replication layer, new transport, or server ECS rewrite. |
| Shared collision and prediction | The same controller handles ground and moving-platform movement. Numbered inputs, acknowledgments, bounded replay, and teleport epochs address real failure modes. Keep this shared authority rather than inventing a second client movement model. |
| Persistence | Temporary-file replacement, file/directory sync, exclusive save ownership, generator versions, validation, and fail-visible behavior protect real user work. These are requirements to preserve during optimization. |
| Shared local navigation | `server/navigation.rs` is a useful extraction: workers, traders, and Moss reuse bounded support-aware detours while retaining their own destinations and jobs. Disposable detours are deliberately excluded from saves. |
| Focused modules | Prediction, follow-camera smoothing, observer movement, graphics settings, inspection details, terrain albedo, and the airship mesh already demonstrate sensible local boundaries. Expand these patterns selectively. |
| Launcher and release tools | Fixed-path extraction, size limits, checksum verification, separate game data, recoverable installation, complete release checks, and ancestry-based promotion are substantive safeguards. Their complexity generally earns its place. |
| Tests | Physical boarding/alighting, blocked deliveries, needs interruption/resumption, corrupt saves, delayed snapshots, modal closing frames, and failed updates are behavior tests worth keeping. Passing them is meaningful even though they cannot establish visual quality. |

## Ranked findings

| ID | Priority | Type | Recommendation |
| --- | --- | --- | --- |
| C01 | High | Source-confirmed coverage defect | Keep building silhouettes in undetailed near chunks beyond the separate far-building cutoff. |
| C02 | High | Refactor + performance investigation | Give terrain preparation, job scheduling, and mesh installation explicit ownership and budgets. |
| C03 | Medium | Refactor | Establish one owner of modal input policy; narrow the client `Session` afterward. |
| C04 | Medium | Refactor before more NPC features | Make resident transitions and their associated state explicit. |
| C05 | Measure first | Performance investigation | Measure synchronous durable saves on the server tick before changing persistence. |
| C06 | Medium | Verification gap | Add a small reproducible visual/streaming test route alongside the strong logic tests. |
| C07 | Low / opportunistic | Cleanup | Extract a few cohesive modules and keep current documentation factual; avoid a repository-wide reorganization. |

### C01 — Building placeholders have a coverage gap

Follow-up 2026-10-06: source defect repaired; automated pending-job/approach/replacement coverage passes. The broader filmed native approach remains an acceptance follow-up.

**Source-confirmed behavior.** [`move_local_square`](crates/client/src/terrain.rs#L326) creates a placeholder for each newly needed near chunk. Its call to [`add_village_proxies`](crates/client/src/terrain.rs#L709) clips buildings to that chunk, but the helper first rejects every building more than `LOD_BUILDING_DISTANCE` (128 m) from the current horizontal chunk center. The same helper also builds the far silhouettes.

At the recorded 512 m near-detail setting, a building can therefore enter the local square well outside 128 m and get a placeholder containing no building geometry. The distant landscape excludes that entire local square. Existing placeholders are skipped when the square advances, so merely getting closer does not regenerate their omitted buildings. A detailed mesh eventually supplies them, subject to the two-job detail queue. The painted ground may remain, but the building silhouette does not.

This is a concrete mismatch with the helper's stated intent to retain silhouettes while detail loads. It is also a plausible contributor to [R03's destination appearing in a burst](GAMEPLAY_REVIEW.md#r03--the-destination-village-assembles-in-front-of-the-passenger); the recorded binary and precise timing remain unverified.

**Smallest useful fix:** separate the far-building visibility policy from placeholder coverage. An entering near chunk should contain its clipped building proxy until its detailed replacement is ready. Do not simply extend all far buildings to the maximum tree range or remove the established ground cutout.

**Acceptance:** add a regression with a generated building inside the near square and beyond 128 m, holding its detailed job pending. Confirm that the placeholder contains the building, persists during approach, and is replaced without missing or doubled geometry. Then replay an airship approach at both default and 512 m detail settings. This can be fixed independently of C02.

### C02 — Terrain loading mixes expensive preparation with presentation

**Source-confirmed work placement; runtime cost unmeasured in this review.** [`setup_terrain`](crates/client/src/terrain.rs#L83) generates the distant atlas, complete initial landscape, and local placeholders synchronously from the Bevy setup system. Moving detailed meshing to workers did not move all terrain preparation off the update thread.

During play, [`stream_terrain`](crates/client/src/terrain.rs#L196) starts a replacement landscape when the chunk center or settings change. [`landscape_geometry`](crates/client/src/terrain.rs#L625) combines terrain tiles, water, tree proxies, and building proxies in that replacement. Installing it also calls `move_local_square`, which synchronously creates every new placeholder and its mesh assets. Each update scans the local chunks and sorts the outstanding detail candidates; only two detail tasks run concurrently.

An 8 m chunk with a 512 m radius means up to 16,641 local chunks; the 1000 m cap means up to 63,001, before clipping at world edges. Those counts are consequences of the current square footprint, not measured draw-call counts or proof of a particular frame time. A cheap-looking slider can change the amount of work enormously. World task snapshots share geography/settlement data through `Arc`; claiming that every job copies the entire generated island would be incorrect.

**Refactor I would make:** keep pure geometry generation separate from the scene's task/asset lifecycle. Prepare the initial atlas and geometry on workers with a visible loading state. Represent desired coverage, work in flight, and installed coverage explicitly enough that geometry preparation and installation can each be bounded. Preserve the current valid old landscape until the replacement cutout and its necessary placeholders are ready together; a naive per-frame installation cap could introduce holes.

Start by measuring atlas time, placeholder preparation, landscape build time, mesh installation time, pending detail count, and stale/cancelled work along one fixed route. If the trace warrants it, retain a dirty/priority queue instead of sorting unchanged candidates each frame, and separate reusable terrain from the tree/building work that really changed. Do not begin with an engine-wide job framework or a terrain engine rewrite.

**Acceptance:** no unresponsive join/settings interval caused by bulk preparation; bounded update-thread work during an airship approach; continuous terrain/building/tree coverage; edits invalidate obsolete results; leaving or rejoining cannot install a job from the old world. Report frame-time distributions and queue depth at default, 512 m, and maximum settings on named hardware. Define the actual frame budget from the chosen target computer, not this source review.

### C03 — Modal input policy is spread across unrelated panels

**Refactor recommendation.** [`Session`](crates/client/src/lib.rs#L71) contains local body/prediction, replicated players/residents/villages, ship timing, camera state, selection, inspection, cursor state, connection labels, and HUD diagnostics. Many systems need mutable access to this broad resource. [`controls`](crates/client/src/lib.rs#L669) is also responsible for modal gating, capture, movement, shortcuts, and presentation-related values.

Pause, world map, pilot conversation, admin console, touch input, controls, and block editing know about overlapping subsets of each other's state. The repeated `open`, `input_blocked`, and `just_closed` checks are the real smell: adding one panel requires remembering several places where it might leak a click, key, or movement input. Existing integration tests correctly demonstrate why the closing frame matters.

**First step:** give the client one small owner for the active gameplay overlay and the resulting input policy, including the closing-frame block and cursor behavior. Keep each panel's contents local. Account explicitly for focus loss, observer permissions, touch/Android Back, and transitions such as pause-to-map; one generic boolean would lose necessary behavior.

**Second step:** split `Session` along actual ownership boundaries, such as local player/prediction, replicated world view, and view/UI state. Keep closely related fields together. Name the major system phases—network, input, simulation/presentation, UI—and preserve their required dependencies. The broad `.chain()` in [`run`](crates/client/src/lib.rs#L392) is explicit and currently understandable; removing it wholesale would risk ordering regressions. Only relax independent ordering after the data ownership is narrower.

**Acceptance:** adding a panel does not require editing every existing panel; the existing open/close-frame, focus, touch, observer, and teleport tests still pass; neutral physics and networking continue under menus. Re-run native mouse capture and Android keyboard checks when implementation changes.

### C04 — Resident state is becoming difficult to change safely

**Refactor recommendation.** [`Resident`, `Phase`, and `Transit`](crates/server/src/villages.rs#L60) represent jobs, needs interruptions, field progress, transport, cargo, and movement through several overlapping fields. `TransitStage` is paired with optional ride, reservation, and deck position. `Resume` preserves some of the interrupted job state. `ResidentSnapshot` is both embedded in saved resident state and updated as the public presentation of that resident; position also exists in `Body`, while passenger information also exists in transit state.

[`tick_internal`](crates/server/src/villages.rs#L732) coordinates crop checks, needs, transport, movement, work, stores, and snapshot updates. [`advance_transit`](crates/server/src/villages.rs#L1121) performs another large set of stage transitions. Extensive validation checks combinations of stage, identifiers, optional fields, and saved progress. Those validations are valuable; their size signals that valid state is difficult to express, not that checks should be deleted.

**Refactor I would make:** retain `VillageLife` as the authoritative owner, but extract cohesive farming, needs/resumption, and transport operations. Begin with behavior-preserving moves, then centralize transitions that currently assign phase, waypoint, elapsed time, ride, and resume fields in several places. Where it prevents a demonstrated illegal combination, put the required data on a state variant—for example, a riding state with its actual ride and deck position. Avoid encoding every conceivable future activity.

Treat snapshots as an explicit projection of resident state where feasible, instead of scattering duplicate position/passenger synchronization. Changing the serialized representation needs a deliberate compatibility path: keep existing version-3 saves readable, retain cargo and interrupted work, and rebuild only the already-disposable navigation data. Module extraction alone should not change the save format.

**Acceptance:** existing long-running village, every-port boarding, crowded landing, cargo conservation, blocked store, interrupted farm, and checkpoint round-trip tests continue to pass. Add a focused test only for each new invariant or discovered gap. No teleporting deliveries, reset jobs, lost cargo, or speculative behavior-tree/GOAP framework.

### C05 — Durable edit saving can consume the simulation tick

**Source-confirmed synchronous I/O; performance impact needs measurement.** [`handle_message`](crates/server/src/lib.rs#L810) saves the whole simulation before broadcasting every accepted edit. [`Simulation::save`](crates/server/src/persistence.rs#L144) serializes all saved edits and NPC/village state, syncs the file, replaces it, and on Unix syncs the directory. The server permits up to 16 edit attempts in one tick. Successful attempts can each trigger that work. Five-second checkpoints use the same synchronous path.

The durability guarantee is good. Its placement means slow storage or a large save can delay input handling, NPC movement, snapshots, and the world clock on the same 20 Hz loop. The review did not benchmark disk latency and does not establish that this caused a filmed stall.

**Recommendation:** first measure save size, serialization/sync time, and server tick overruns with a small and a near-limit edited world. If editing causes material stalls, collect accepted mutations for a tick and perform one durable commit before sending their acknowledgments. Preserve message order, edit validation against prior mutations, fail-visible behavior, and what a new connection can observe. A worker or journal should follow only if simpler batching is insufficient and its recovery semantics are specified.

**Acceptance:** restart tests prove every acknowledged edit survives; a failed commit never emits success; batching retains request ordering and permissions; measured tick behavior improves under the same workload. Do not trade durability for a faster-looking acknowledgment.

### C06 — Logic coverage is stronger than presentation coverage

**Verification gap, demonstrated by the gameplay review.** Mesh topology, proxy dimensions, input gating, camera collision, and route completion have useful tests. Those tests can pass while forests disappear perceptually into the atlas, a village pops into view, an envelope fills the camera, or a usable port looks like a plank pasted onto a road. More assertions about the same mesh helper would not establish that these are fixed.

**Small addition I would make:** a documented, repeatable capture route using one named seed/generation, settings, camera poses, and an airship journey. Capture approach/departure and a complete on-deck orbit, plus a save/load checkpoint with working residents. Record commit, platform, settings, and timings with the evidence. Use the existing screenshot/capture facilities where possible; start with manual review of a small fixed set, not a GPU-fragile pixel-diff service.

Add narrow automated invariants for gaps such as C01, keep the current physical route/needs tests, and require native visual inspection when changing LOD or materials. Platform-specific lifecycle, GPU loss, and performance still require their target devices; a macOS workspace test run cannot close them.

### C07 — Smaller cleanup is worthwhile when touching the area

**Lower priority recommendations.** These improve navigation and consistency but should not delay the recorded visual fixes.

- Extract avatar mesh/animation/presentation from the client root module, which currently also handles startup, networking, controls, editing, and capture. Keep the client root as a readable assembly point.
- Separate settlement site scoring, village layout, and route construction when the layout changes warrant it. Preserve generator behavior and saved-world compatibility; reorganizing deterministic generation must not silently reshape existing GeographyV3 worlds.
- Introduce a modest shared UI font/style resource during the UI polish pass. Several panel setup functions independently load the same bundled font and repeat styling. This is a consistency/ownership improvement, not a proven major performance fix or a reason for a general widget framework.
- Profile leaving a local world if it feels stuck: [`leave_world`](crates/client/src/join.rs#L1049) drops the local server handle, whose destructor joins the saving server thread. If this is visible, make the shutdown progress visible while retaining the durable completion requirement.
- Keep current docs compact and avoid stale protocol claims. `README.md` still said v8 while `protocol.rs` declares v9; this review corrects that reference. The handoff should link detailed reviews rather than duplicate their entire backlog or treat historical playtests as proof of the current build.

Do not split every large file just for symmetry, replace every `String` error with a framework, or wrap every function argument in a new context object. Inline tests and explicit call parameters are often helping. Keep full snapshots and simple bounded population scans until measurements and population requirements justify interest management or spatial indexing.

## Suggested sequence

1. Fix C01 and reproduce the filmed approach/camera/port problems. These can proceed without a general refactor.
2. Measure C02 and C05 on that same route/workload. Make the small terrain ownership changes needed to repair loading and transitions.
3. Consolidate input policy as menus are polished; extract resident transitions before adding more needs, jobs, or transport behavior.
4. Do C07 cleanup within those focused changes. Preserve the current architecture and existing tests throughout.

I would allocate the next pass primarily to the visible journey defects and the specific terrain path supporting them. A broad cleanup release would spend effort without necessarily making the game better to play.

## Verification performed

- `cargo test --workspace --locked --offline`: passed on this macOS host, including localhost multiplayer tests. The existing launcher test requiring a real native packaged client remained ignored.
- `cargo clippy --workspace --all-targets --locked --offline -- -D warnings`: passed.
- `python3 -m unittest discover -s scripts/release -q`: 38 passed, one native Linux ELF-tool test skipped on macOS.
- `python3 -m unittest discover -s scripts/ci -q`: 16 passed.
- Review-document links and whitespace checked before commit.

No new runtime, generator, protocol, save-format, or visual changes were implemented. No fresh graphics benchmark, Android build/device run, Windows/Linux runtime test, live-server change, or release publication was performed. Source inspection of those paths is not target-platform validation.
