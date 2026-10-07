# Gameplay review and fix backlog — 2026-10-06

Status: **28 open findings; no fixes implemented by this review.** Priorities and remedies below are assistant recommendations for discussion, not newly approved gameplay or art decisions.

The recording makes the central weakness clear: the simulation has more substance than the presentation communicates. We can follow a worker, board a moving ship, cross a large landscape, and reach another inhabited village. But the ship looks like a flying construction platform, the settlements look assembled from the same small kit, and the scenery visibly changes representation around us. Too much of the interesting world exists in inspector text. I would improve this journey before adding more systems.

## Evidence and limits

- Source: `Screen Recording 2026-10-06 at 7.25.03 PM.mov`, supplied from the user's Desktop. Duration **5:03.312**, encoded size **1664 × 1188**, video only; there is no audio stream to assess.
- Reviewed **303 samples at one-second intervals** across the recording, plus a four-samples-per-second pass over **03:42–03:46** and full-size inspection of key frames. Timestamps below are approximate recording time, not the HUD's world clock. Sampling is sufficient for the visible findings, but does not establish fine animation quality, input latency, or frame pacing.
- Committed **15 screenshots**, cropped to the **1440 × 900 game viewport**, without changing the scene. The full recording and bulk extracted frames are not repository assets. Screenshots retain the original HUD as evidence.
- At **04:54**, the settings show **High**, **512 m near detail**, **2184 blocks / 1092 m medium trees**, and **90 m shadows**. High is also visible in earlier HUD frames. The distances are verified at the end; the recording shows no intervening settings adjustment. These are not the defaults.
- The route dialogue identifies **Pinemead → Pinevale**. The footage shows departure around **01:40**, then the player leaving the ship around **03:52** before its displayed arrival countdown finishes. This review does **not** claim a completed ordinary disembarkation by the player.
- Repository inspected at `f2a6994`; the recording does not identify its executable commit, world seed, actual atlas resolution, or device. Source pointers below identify places to investigate, not proof that this executable used that exact code.
- **Observed** means the symptom is visible. **Investigate** means the footage suggests a problem but does not establish cause or a reproducible failure. **Design** means a subjective weakness or an acknowledged missing feature. None of these entries establishes crashes, save loss, platform compatibility, or a controlled performance result.

## Recommended order

P1 means a high-impact problem with this basic village-to-airship journey, not a release-blocking severity claim. P2 means substantial presentation or usability work. P3 means polish after the journey reads well. All entries remain open.

| Pass | Items | Intended result |
| --- | --- | --- |
| Repair visible discontinuities | R01–R04, R09; profile R10 | Forests, terrain, buildings, camera, and timetable stay believable while moving. |
| Make the port usable and legible | R05–R08, R11 | Roads and boarding surfaces have deliberate relationships; people can pass; ships look like destinations. |
| Give villages and travel character | R12–R23, R28 | The second town and the ride have reasons to look around and explore. |
| Reduce UI friction | R24–R27 | The current activity gets attention before diagnostics and repeated instructions. |

## P1 — repair the core journey

### R01 — Far forests cease to read as forests

**Observed / rendering investigation · 01:45–03:40 · [02:12](artifacts/gameplay-review-2026-10-06/0212-forest-cutoff.jpg), [02:37](artifacts/gameplay-review-2026-10-06/0237-water.jpg).** Recognizable tree silhouettes cover the closer landscape, then give way to broad, almost bare green surfaces. Distant canopy coverage is not visually convincing. This directly undermines the reason to ride an airship. It does not prove that every distant tree texel is absent; biome differences, mip filtering, contrast, and the active build still need checking.

- **Recommendation:** compare the same forest patch in detailed geometry, simplified geometry, the generated atlas, and its rendered mip levels while flying toward it. Verify active build/settings/material binding first. Preserve the approved map-painted distant ground; don't assume a bigger tree radius or atlas is the fix.
- **Done when:** the same wooded region remains recognizably wooded across all three representations, without a bare band appearing between them, in native moving captures at ordinary settings and this recording's settings. Measure the memory cost on a target machine.

### R02 — Terrain transitions expose the renderer's construction

**Observed · 02:52–03:45 and 04:10 · [03:44](artifacts/gameplay-review-2026-10-06/0344-village-partial.jpg), [04:10](artifacts/gameplay-review-2026-10-06/0410-airship-exterior.jpg).** Contour-like dark steps appear in local patches within otherwise smooth ground during flight. At ground level there are conspicuous triangular wedges along contour boundaries. Whether these are holes, mismatched surfaces, shading, or temporary mesh transitions is unverified; the visible result looks unfinished.

- **Recommendation:** inspect placeholder/detail joins and landscape replacement in motion, including the 512 m detail setting. Separate geometry continuity from the legitimate change between voxel and map-painted style.
- **Done when:** no isolated triangular wedges or conspicuously advancing patches of step edges reveal chunk replacement during the same approach; stable voxel terraces remain intentional and coherent.

### R03 — The destination village assembles in front of the passenger

**Observed · 03:42–03:46 · [before](artifacts/gameplay-review-2026-10-06/0343-village-before.jpg), [partial](artifacts/gameplay-review-2026-10-06/0344-village-partial.jpg), [after](artifacts/gameplay-review-2026-10-06/0345-village-after.jpg).** Roads are visible first; isolated roofs and then most of the settlement appear over a short approach interval. A whole destination should be recognizable before we are nearly over it. The denser four-frame-per-second check confirms a burst of appearance, not just one misleading still.

- **Recommendation:** inspect the building proxy, placeholder, and detailed-chunk handoff together. Ensure a recognizable settlement footprint and silhouette survives until its replacement is ready. Tune for an arriving passenger's view, not only walking speed.
- **Done when:** a fixed-camera approach shows a continuous village at distance and a controlled increase in detail, without missing houses between representations.

### R04 — The airship envelope can swallow the camera view

**Observed · 01:38, with further obstruction during deck camera rotation · [01:38](artifacts/gameplay-review-2026-10-06/0138-camera-obstruction.jpg).** A featureless gray envelope surface covers most of the upper view while the player is on deck. Even if collision is technically keeping the camera outside the mesh, this framing fails the sightseeing goal.

- **Recommendation:** reproduce a full camera orbit and zoom range while aboard, including departure, turns, and edges. Improve obstruction handling and the relationship between envelope clearance, camera anchor, and usable deck. Changing ship proportions is an art decision to preview first.
- **Done when:** ordinary looking and walking preserve a usable view of the avatar and surroundings; camera recovery from obstruction is stable. Check in motion, not just a staged unobstructed pose.

### R05 — Gangways look pasted directly over the roads

**Observed, also explicitly raised by the user · 01:16–01:22, 04:13–04:38 · [04:10](artifacts/gameplay-review-2026-10-06/0410-airship-exterior.jpg), [04:36](artifacts/gameplay-review-2026-10-06/0436-trader-cargo.jpg).** Narrow smooth planks follow the dirt road's alignment, leaving another brown surface underneath and an arbitrary wedge at the end. The walkway and ship deck can read as one continuous slab. This is confusing infrastructure even when physically walkable; the footage does not establish z-fighting.

- **Recommendation:** prefer a short, visibly distinct spur to a berth. Where a road-aligned berth is necessary, make its junction, support, pedestrian continuation, and deck boundary look intentional. Current landing code explicitly allows a road-aligned fallback, so this is partly layout policy, not automatically an accidental overlap.
- **Done when:** both filmed ports have legible road/approach/deck boundaries from ground level and above, with a usable through-route while a ship is present or absent. Preserve existing saves and journey identities when changing placement.

### R06 — Investigate the prolonged trader/player encounter on the approach

**Investigate · 04:16–04:38 · [04:21](artifacts/gameplay-review-2026-10-06/0421-walkway-encounter.jpg), [04:36](artifacts/gameplay-review-2026-10-06/0436-trader-cargo.jpg).** The purple-clad trader and player remain in very close contact on the narrow walkway for roughly fifteen seconds. The trader eventually leaves; the later inspector says it is walking off the airship to the road. The player is in its path, so this is not proof of autonomous NPC deadlock or a regression in the recent avoidance fix.

- **Recommendation:** reproduce a stationary player blocking this approach, then step aside; repeat with opposing NPCs and while the ship departs. Check available passing space, support-aware detours, and the alighting target. Widening the approach may be better than adding navigation complexity.
- **Done when:** a reachable detour is used without oscillation or body overlap; if the route is genuinely blocked, the NPC waits and resumes promptly after clearance. Retain its cargo and physical destination arrival.

### R07 — The airship looks like a flying floor sample

**Design · 01:17–03:52 and 04:09–04:16 · [deck](artifacts/gameplay-review-2026-10-06/0212-forest-cutoff.jpg), [exterior](artifacts/gameplay-review-2026-10-06/0410-airship-exterior.jpg).** The silhouette is a pale voxel balloon, thin straight suspension lines, and a nearly blank rectangular brown deck. There is little bow/stern identity, visible means of steering, structure, material detail, or evidence that people use it. The large dark envelope shadow makes the empty deck even more dominant. It fulfills transport but has little charm.

- **Recommendation:** preview one modest art pass: shaped hull edge, readable helm, stronger rigging attachments, plank structure, a little cargo or seating, and a recognizable accent. Partial rails could frame the edge, but must preserve the confirmed ability to walk/jump/fall off. Avoid cluttering the walking area or turning this into a ship customization system.
- **Done when:** the ship is recognizable from below, beside it, and aboard; the pilot has a believable working position and passengers have distinct places to stand and look. Approve the visual prototype before rolling it across the fleet.

### R08 — The port does not explain itself in the world

**Design / usability · 01:04–01:27 and 04:09–04:16 · [village](artifacts/gameplay-review-2026-10-06/0014-village.jpg), [port](artifacts/gameplay-review-2026-10-06/0410-airship-exterior.jpg).** Bare posts/banners and unmarked approaches don't clearly identify the port, destination, boarding edge, or waiting place. In the second town, multiple similar ships make this more important. Pilot dialogue and the existing proximity HUD help, but require already finding the correct place/person.

- **Recommendation:** add a small readable port sign and berth/destination cue, reusing the real timetable. Differentiate stationary infrastructure from the departing ship. Keep conversation optional and physical boarding direct.
- **Done when:** an unfamiliar player can find the port, identify the next destination, and choose where to wait/board without opening debug inspection or guessing from identical slabs.

## P2 — make the world readable and worth visiting

### R09 — Pilot dialogue contains conflicting departure times

**Observed · 01:27–01:31 · [01:31](artifacts/gameplay-review-2026-10-06/0131-pilot-times.jpg).** The spoken paragraph still says “We leave in 13 seconds” while the live line below says “departs in 9 seconds.” The paragraph's arrival estimate also remains the originally supplied value. Two countdowns with different freshness make the answer less trustworthy.

- **Recommendation:** keep the spoken text timeless and present one live timetable, or update every time-dependent phrase together. Give the pilot a human-facing name instead of `Pilot 1-2` as part of the later art/character pass.
- **Done when:** leaving the dialogue open through departure never presents contradictory times or a stale boarding instruction; closing it still returns directly to movement.

### R10 — Profile the arrival burst rather than trusting the usual FPS

**Investigate · 03:43–03:45 · [03:44 HUD](artifacts/gameplay-review-2026-10-06/0344-village-partial.jpg).** The sampled HUD falls to **54 fps** during the village appearance, then shows **110 fps** at 03:45; many other samples are around 120. That is a useful lead, not a benchmark or proof of GPU overload. Recording, scheduling, and the unusually large detail radius are confounders.

- **Recommendation:** capture frame times and terrain job/install timing on a repeatable arrival, at default distances and 512 m/1092 m. Check whether mesh installation creates bursts before increasing visual budgets.
- **Done when:** the cause and distribution of stalls are measured, any fix is compared on the same route, and target hardware is checked separately. Do not close this from a single high FPS screenshot.

### R11 — Paths alternate between raised ribbons and deep trenches

**Observed / terrain design · 00:20–01:16 and 03:58–04:05 · [raised path](artifacts/gameplay-review-2026-10-06/0045-farm.jpg), [trench](artifacts/gameplay-review-2026-10-06/0400-path-trench.jpg).** Dirt routes sit on sharp elevated strips in one village and run through abrupt cuts beside houses in the other. Some field approaches look like dead ends against a ledge. Passing a controller test does not make this pleasant or believable village construction.

- **Recommendation:** improve local grading and path-to-door/field transitions; use deliberate short steps or retaining edges where needed. Preserve the voxel style instead of trying to smooth the entire world.
- **Done when:** a walk from the center to each door, field, and berth feels intentionally graded, without surprising pits, unnecessary ledges, or narrow bottlenecks. Generator changes need a version/migration plan before implementation.

### R12 — Village centers are oversized brown intersections

**Design · 00:10–00:24 and 01:03–01:15 · [00:14](artifacts/gameplay-review-2026-10-06/0014-village.jpg).** Wide bare road polygons meet in a large empty hub. Buildings seem connected by spokes rather than shaping streets and usable public space. The ground occupies more attention than the village activity.

- **Recommendation:** tighten minor paths and define a modest central gathering/market space with a clear edge and one useful focal point. Add a few purposeful props, not random clutter.
- **Done when:** the square, through-road, frontages, and footpaths have visibly different roles, while existing movement and work routes remain usable.

### R13 — The second village feels like the first kit rearranged

**Design · 00:10–01:16 versus 03:45–04:53 · [first town](artifacts/gameplay-review-2026-10-06/0014-village.jpg), [arrival](artifacts/gameplay-review-2026-10-06/0345-village-after.jpg).** Repeated small red-roofed houses, central stalls, brown spokes, and rectangular fields provide little sense of arriving somewhere distinct. Long-distance travel needs a destination with a recognizable identity.

- **Recommendation:** start with one landmark and one resource-related visual trait per village, plus modest roof/frontage/layout variation. Keep the shared asset kit; a large procedural architecture framework is unnecessary.
- **Done when:** players can distinguish these two villages in unlabeled ground and aerial screenshots, with differences that follow their geography or work.

### R14 — Building purpose is difficult to read from its exterior

**Design / usability · 00:12–00:24, 03:55–04:09, 04:43–04:53.** Similar buildings and sparse stall contents don't clearly communicate home, storage, workshop, or exchange. A simulation can know their roles while a player sees interchangeable boxes.

- **Recommendation:** use a small shared vocabulary of signs and actual role props: stored goods, a visible work surface, or distinctive frontage. Put cues at the approach, not only in inspector text.
- **Done when:** players can find the market and storage and tell a workplace from a home without opening the inspector. Do not label a market as interactable until that action exists.

### R15 — Fields look like pegboards on dirt slabs

**Design · 00:28–00:58 and 04:00–04:04 · [00:45](artifacts/gameplay-review-2026-10-06/0045-farm.jpg).** Identical upright green pegs in a perfect rectangular grid communicate planting locations more strongly than crops. Bare flat soil and a raised edge make the field resemble a placed prop. Growth, readiness, and work results are difficult to read at walking distance.

- **Recommendation:** improve silhouettes for the existing growth stages, row/soil definition, and a small amount of deterministic variation. Keep visuals tied to authoritative growth and intact planting soil; no new farming simulation is needed for this pass.
- **Done when:** a player can distinguish newly planted, growing, and ready crops without opening a percentage readout, and can see a worker's successful action change the relevant presentation.

### R16 — Trade cargo is invisible except in the inspector

**Observed / presentation gap · 04:36–04:49 · [04:36](artifacts/gameplay-review-2026-10-06/0436-trader-cargo.jpg).** The inspector says the trader carries **6 Stone**, but the character gives no clear stone/cargo cue. A generic dark pack/body shape is not enough to communicate the transport economy.

- **Recommendation:** add a simple visible carried load or goods cue driven by existing cargo state, with a recognizable handoff at storage. Avoid item-by-item inventory animation.
- **Done when:** a player can follow a trader from berth to store and understand that goods arrived without watching a data panel; empty and loaded states differ.

### R17 — Characters read as mannequins more than inhabitants

**Design · 00:15–00:58 and 01:22–03:52 · [farmer](artifacts/gameplay-review-2026-10-06/0045-farm.jpg), [passenger](artifacts/gameplay-review-2026-10-06/0212-forest-cutoff.jpg).** Similar rigid bodies, minimal faces, and simple color swaps carry little personality. During the ride, people contribute almost no visible activity. Walking and farming poses do change in the samples; this is not a claim that animation is absent.

- **Recommendation:** make existing jobs and idle states read better with restrained poses, tools, orientation, and a few character details. Favor a pilot at a helm and a passenger looking outward over extra dialogue systems.
- **Done when:** roles and current activity are understandable at ordinary camera distance. Judge walking/idle transitions in a full-rate capture before choosing animation changes.

### R18 — The village loop offers spectatorship before participation

**Design / known scope gap · 00:10–01:16 and 03:54–04:53.** The footage's meaningful actions are moving, inspecting inhabitants, and traveling; nothing visible invites a small useful act in the local economy. Creative building is available, but the lived-in-village promise needs a player-facing point of contact. The recording alone does not establish every available action; DESIGN.md explicitly records player farming/trade as unfinished.

- **Recommendation:** after presentation repairs, choose one small voluntary interaction. A first coin-based market exchange is consistent with the user's leaning toward trade, but its priority, prices, funds, and cargo rules still need feedback. Do not add hunger chores or quest machinery to create artificial purpose.
- **Done when:** a player can discover, perform, and see the consequence of one useful village interaction. Selecting that interaction remains a design decision, not authorization from this review.

### R19 — Rivers reveal coarse angular segments from the air

**Observed / visual design · 02:20–03:35 · [02:37](artifacts/gameplay-review-2026-10-06/0237-water.jpg).** Long straight reaches, abrupt elbows, and blocky joins make several watercourses read like blue polylines laid over the ground. The problem is especially exposed by the smooth surrounding distant terrain.

- **Recommendation:** compare the distant river mask, water geometry, banks, and detailed river at the same locations. Improve visual continuity at bends while retaining the approved drainage network and island shape.
- **Done when:** those reaches look like channels through the land throughout the approach, with no sudden width/shape substitution between map and local terrain. Determine whether the repair is rendering-only before proposing generator changes.

### R20 — Water lacks enough surface and shoreline information

**Design · 01:43–03:12 · [02:12](artifacts/gameplay-review-2026-10-06/0212-forest-cutoff.jpg), [02:37](artifacts/gameplay-review-2026-10-06/0237-water.jpg).** Broad cyan areas have little depth, flow, or shore differentiation; large tonal patches on the lake can read as rendering regions instead of water. The landscape loses scale where water occupies most of the view.

- **Recommendation:** test inexpensive shore/depth coloration and restrained surface variation, then verify any visible patch boundaries. Preserve a readable Low preset; this does not require reflections, fluid simulation, or swimming mechanics.
- **Done when:** lake versus bank and shallow versus deep areas read clearly without visible patch seams, and the result remains comfortable in motion on target hardware.

### R21 — Terrain color and atmosphere flatten the grand landscape

**Design · 01:43–03:40 · [02:12](artifacts/gameplay-review-2026-10-06/0212-forest-cutoff.jpg).** Large bright yellow-green surfaces, smooth mountains, and a nearly uniform blue sky produce a model-table feeling. Near and far forms have too little visual separation, and close voxel terraces contrast sharply with softly shaded distant slopes.

- **Recommendation:** preview modest palette/relief and distance-atmosphere tuning before adding expensive effects. Keep the approved geographic forms and map-painted style. Distinguish intentional biome color from missing forest coverage in R01.
- **Done when:** foreground, middle distance, and mountain background separate naturally without hiding scenery or sacrificing trail readability. Obtain feedback on side-by-side native views.

### R22 — Simplified trees look like a different asset family

**Design / LOD continuity · 02:12–03:45 and 04:10 · [aerial trees](artifacts/gameplay-review-2026-10-06/0212-forest-cutoff.jpg), [near trees](artifacts/gameplay-review-2026-10-06/0410-airship-exterior.jpg).** Distant silhouettes resemble thin sticks with tiny layered caps; nearby crowns are much rounder voxel masses. Repetition and the shape change reinforce the feeling that scenery is switching systems.

- **Recommendation:** compare proxy crown/trunk proportions with each generated tree kind, preserving overall mass and color with very little geometry. Address R01 first so increasing proxy detail does not mask the far-coverage problem.
- **Done when:** a tree keeps its recognizable crown size and type as detail changes, and forests remain coherent from an airship without needing near meshes everywhere.

### R23 — The flight has little progression beyond a countdown

**Design · about 01:40–03:52.** Most of this roughly two-minute ride offers a static deck, a mostly still companion, and scenery sliding by. Walking around is valuable, but there are few visible departure/cruise/arrival cues or named points of interest. Making the ship prettier will help; travel also needs to communicate where we are going and what is worth looking at.

- **Recommendation:** start with a clear destination/progress cue, a visible arrival phase, and a few discoverable landmarks tied to the real route. Reassess route duration/altitude only after scenery continuity is repaired. Keep relaxed optional sightseeing; don't require minigames, chatter, or passenger chores.
- **Done when:** an unfamiliar rider can tell the journey phase and point to something interesting they passed. Route timing changes need playtest feedback, not an assumption that every long ride is bad. Audio cannot be judged from this recording.

### R24 — The HUD keeps advertising building while the player travels

**Observed / usability · 00:10–03:52 · [farm HUD](artifacts/gameplay-review-2026-10-06/0045-farm.jpg), [flight HUD](artifacts/gameplay-review-2026-10-06/0212-forest-cutoff.jpg).** A title/subtitle, edit hint, statistics, material palette, controls panel, inspector, and transport hint can compete for the screen. During flight, the prominent instruction remains “Move closer to reach a block · aim down to build nearby,” even though the player is riding. The transport destination is tucked beside the palette.

- **Recommendation:** give context-sensitive travel/interact information priority. Let established controls and diagnostics collapse after onboarding, keeping them easy to reopen; preserve direct creative building access.
- **Done when:** a first visit teaches essential controls, while an ordinary ride foregrounds destination and scenery. No persistent out-of-reach build prompt unless the player is trying to build.

### R25 — Inspector presentation still looks like a development tool

**Design / usability · 00:17–00:19, 00:44–00:57, 04:36–04:49 · [farmer](artifacts/gameplay-review-2026-10-06/0045-farm.jpg), [trader](artifacts/gameplay-review-2026-10-06/0436-trader-cargo.jpg).** The large wire box, raw 0–100 need values, generic “Walking to work,” and route explanation are useful debugging evidence but weak character communication. “Hunger 100 / 100” has no plain-language severity explanation. Important cargo and destination details compete with implementation-oriented status.

- **Recommendation:** lead with name, role, concrete current intent, and relevant cargo; explain needs in words, with exact values available as secondary detail. Use a less intrusive selection treatment. Preserve locked aimed selection and live updates.
- **Done when:** a player can explain what the selected person is doing and whether they need help from a quick glance, while developers can still access the underlying numbers/reason.

## P3 — focused polish

### R26 — Joining spends several seconds without useful progress

**Observed / usability · 00:01–00:10 · [00:05](artifacts/gameplay-review-2026-10-06/0005-joining.jpg).** The small join panel remains largely unchanged while connecting, then jumps into the village. This sample takes roughly nine seconds; it does not reveal whether networking, world generation, atlas generation, or mesh preparation owns the wait.

- **Recommendation:** expose honest coarse loading stages and a clear pending state, with retry/cancel where supported. Avoid a fabricated percentage or redesigning the connection architecture just for a progress label.
- **Done when:** a player can distinguish connection work, world preparation, and an actual failure; time the stages separately before optimizing them.

### R27 — Graphics distances expose inconsistent units and awkward tuning

**Observed / usability · 04:54 · [settings](artifacts/gameplay-review-2026-10-06/0454-settings.jpg).** Near detail is **512 m**, trees are **2184 blocks (1092 m)**, and shadows are **90 m**. The same kind of distance is presented differently, with only small plus/minus controls for large ranges. The panel explains general cost, but not what a novice should choose.

- **Recommendation:** use meters consistently as the primary unit, with blocks secondary if useful. Keep Low/Balanced/High easy to find, and preview a few useful distance steps or direct adjustment plus reset. Preserve the user's requested experimental maxima.
- **Done when:** ordinary settings and a return to defaults take few actions, while users can still reach an exact experimental value without dozens of clicks. Do not change the selected distances silently.

### R28 — Leaving the ship and landing has little visible physical punctuation

**Design · 03:52–03:55.** The player drops from the approaching ship and is quickly walking beside the destination road. The sequence supplies little visible sense of impact or arrival. This is not a fall-damage bug: the prototype deliberately has no fall damage, and the footage does not establish a movement correction.

- **Recommendation:** inspect this segment at full speed, then consider a restrained landing pose/contact cue and destination arrival acknowledgment. Keep the freedom to jump off; damage, forced exits, and gliding are separate gameplay choices.
- **Done when:** leaving and landing are legible and comfortable in motion, without forced camera shake or an unapproved survival penalty.

## Source entry points for follow-up

These were checked for relevance only. A source change should reproduce its chosen finding before claiming a diagnosis.

| Area | Start here |
| --- | --- |
| Far trees, appearance bursts, terrain joins | [terrain.rs](crates/client/src/terrain.rs): `stream_terrain`, `move_local_square`, `add_village_proxies`, `add_landscape_trees`; [terrain_albedo.rs](crates/client/src/terrain_albedo.rs): `paint_trees`, `paint_tree`, `append_mips`; [terrain.wgsl](crates/client/src/terrain.wgsl) |
| Berths and road overlaps | [airship_landings.rs](crates/core/src/airship_landings.rs): `landings`, `fit_ramp`, `exit_bridge`; its explicit zero-offset road fallback deserves visual review |
| Ship shape, camera, timetable UI | [airship_mesh.rs](crates/client/src/airship_mesh.rs), [airships.rs](crates/client/src/airships.rs), [follow_camera.rs](crates/client/src/follow_camera.rs) |
| Trader/player passage | [navigation.rs](crates/server/src/navigation.rs), [server airships.rs](crates/server/src/airships.rs), [server villages.rs](crates/server/src/villages.rs) |
| Village presentation and role cues | [village_assets.rs](crates/core/src/village_assets.rs), [settlement.rs](crates/core/src/settlement.rs), [client lib.rs](crates/client/src/lib.rs) |
| HUD, inspector, loading, settings | [ui.rs](crates/client/src/ui.rs), [inspection_details.rs](crates/client/src/inspection_details.rs), [join.rs](crates/client/src/join.rs), [pause.rs](crates/client/src/pause.rs), [graphics.rs](crates/client/src/graphics.rs) |

## Guardrails for implementing this backlog

Preserve the confirmed island/hydrology, map-painted medium/far ground, physical boarding and free movement/falling off ships, cargo-bearing physical NPC travel, and integrated-graphics targets. Visual improvements do not authorize relocating existing saved terrain/buildings/berths. Preview consequential art/layout/gameplay changes with the user before committing to them.

The farmer visibly resumes field activity after the player stops obstructing it. The later trader eventually leaves the walkway. The recording ends after **Leave world** and closing the app; its final black frames are not evidence of a crash. Existing ETA and role displays are present, even where this review criticizes their placement or wording. The world map, editing/persistence, multiplayer, Android, sound, and lower graphics presets were not exercised here.

Close an item only with evidence of its stated result: native before/after views for visual work, moving captures for transitions/camera/animation, and physical-route tests for navigation. A successful compilation or atlas unit test alone does not close a gameplay-appearance finding.
