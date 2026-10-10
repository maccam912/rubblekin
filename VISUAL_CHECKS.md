# Repeatable village and airship checks

Use this small manual route when changing terrain streaming, camera framing, menus or capture. It complements the logic tests; passing those tests alone does not establish visual continuity. The original continuity route below uses **unedited GeographyV3, seed 42**, a 1440 × 900 desktop window, Balanced, 48 m near detail, 128 m trees and 32 m shadows. Repeat the approach at **512 m near detail**. Record the actual window size, preset and distances if they differ.

## Isolated starting scene

From the repository root on macOS/Linux:

```sh
cargo build --locked -p rubblekin_core --example airships
cargo build --locked -p rubblekin_client
qa_repo="$PWD"
qa_run="$(mktemp -d "${TMPDIR:-/tmp}/rubblekin-visual.XXXXXX")"
mkdir -p "$qa_run/artifacts"
cd "$qa_run"
"$qa_repo/target/debug/rubblekin" --local --bind 127.0.0.1:17889 \
  --seed 42 --generation v3 --save "$qa_run/world.json" --balanced \
  --screenshot "$qa_run/artifacts/initial.png"
```

The separate working directory isolates `graphics.json`, saves and F12 captures. Use an unused loopback port. On Windows, create an equivalent temporary working directory and run `target\debug\rubblekin.exe` by its full path. The initial screenshot waits for a joined scene and then eight seconds of real time. If an automated run needs `--exit-after`, allow for startup plus this interval and verify the log says the PNG was saved. F12 saves any visible menu or game view immediately. Keep the window visible during native capture.

1. Inspect the starting village: ground, buildings, workers and fields should appear without an empty scene between loading and play. Open and close Inspect, Map and Pause. Try Zoom, Center on you and Whole world; check that movement and edits stay blocked while a panel is open.
2. In Pause, check keyboard distance selection with Tab, Left/Right, Home/End and Reset distances. Change 48 → 512 → 48 m near detail while looking at buildings. The old coverage should stay visible until the replacement is ready. Record any stall separately from missing geometry.
3. Capture a working resident and their inspector, leave the world normally, and reopen this same save. Verify the scene and resident activity resume. Repeat with a retained copy if investigating a failure; preserve the failing original.

## Pinevale → Willowbank journey

This fixed seed has a direct route between **Pinevale (5)** and **Willowbank (9)**. The read-only helper below prints village/port coordinates and routes once, then the scheduled fleet at each simulation time supplied on stdin:

```sh
printf '0\n180\n360\n' | "$qa_repo/target/debug/examples/airships" 42 v3
```

Use the existing local admin console to `teleport X Y Z` to Pinevale's `port_position`, then walk to the Willowbank berth and board physically. If another character occupies the exact point, wait or choose nearby clear space. Keep the pilot dialog open across departure and arrival: the live destination and countdown should agree with the aboard HUD. Pause and other panels do not stop the timetable.

During flight, capture the view ahead, behind and to both sides, with the camera near and far from the avatar. Walk around the deck, then stand near an edge facing Willowbank for the approach. Capture the village before entering the chosen near-detail square, while its placeholders are pending, near 128 m and at the landing. Follow one recognizable roof through the sequence; it must remain present until its detailed replacement appears. Repeat at 48 and 512 m detail. A few still frames cannot prove every transition: use a recording when assessing popping or camera recovery.

The helper uses authoritative `f64` simulation seconds, not wall-clock time or the HUD's rounded minutes. It does not connect to the server or read edits. Its `deck_check_position` is a sampled deck-foot point, not a guaranteed safe live teleport: ships advance between sampling and issuing a command. Normal boarding avoids this timing dependency.

## Evidence to keep

Record the commit, OS/GPU, generation/seed, exact graphics distances, window size, route/direction, simulation time when known, and whether this was a fresh start or reload. Keep the small set of useful screenshots and the relevant log next to those notes. Record actual observations and incomplete checks separately. Screenshots do not measure frame-time distributions, and this route does not establish Android lifecycle or other-device performance.

## Sky and day/night

Use a disposable world and ordinary graphics distances. A new world starts in the morning; the sun reaches overhead after five simulation minutes, sets after fifteen, reaches midnight after twenty-five, and rises after thirty-five. Each complete cycle is forty minutes, with twenty above the horizon and twenty below. The clock continues in Pause/Map and resumes from the saved simulation time after restarting; clients joining the same server should see the same phase.

Look across the horizon and overhead: daylight should be paler below and deeper blue above, with slow drifting clouds and a visible sun. In Balanced/High, compare a nearby building's shadow in afternoon and near sunset; it should lengthen within the existing shadow range. Watch the sky/clouds/sunlight warm before sunset and stars gradually appear afterward. At night, walk a path, look at a shaded wall and find a resident: these should remain readable without increasing display brightness. Check Low still has the complete sky but retains contact shadows instead of dynamic shadows. Leave/rejoin and cycle presets to check that the sky survives without duplication. [Native staged evidence and its limits](artifacts/sky-native-checks.md) supplements this natural-time route.

For shadow stability, hold the camera still and watch a stationary building's shadow edge in Balanced and High. It should hold between small once-per-second direction updates; continuous edge crawling should stop. Compare consecutive rendered frames, avoiding shadows cast by moving residents or airships. The visible sun, clouds and sunset intensity/color must continue smoothly. [Adjacent-frame comparison](artifacts/sky-shadow-comparison.png) shows the native reproduction and fix; it does not establish stability during camera motion or on other hardware.

## V5 markets, local work, and world variety

Use a fresh path with `--generation v5 --name ContentTester` for this content route, in the isolated directory above. Keep `player-profiles.json` beside this client data; repeat with the same character name to check saved coins, cargo, contract, and location. Existing V3/V4 saves gain market/local-work actions while retaining their original terrain, buildings, and trees. A generation flag never upgrades an existing save. The Pinevale → Willowbank route above is explicitly V3; for V5, run `printf '0\n180\n360\n' | "$qa_repo/target/debug/examples/airships" 42 v5` and use its actual town/port names and route endpoints.

At seed 42 Pinemead, the market entrance is `(2823.75,205.5,-1920.25)`. Open B/Cargo nearby, accept a delivery, and verify the six sealed units reduce the origin stock. Walk/ride to the named destination, deliver for 12 coins, then buy/sell an ordinary good. Check insufficient coins, full cargo, and remote actions remain unavailable. Leave/reopen the same save and verify progress and position. Admin coordinate staging checks market transactions but does not establish a full physical delivery journey.

For paid local work, walk beside intact planting soil, open B/Cargo, and check the offer’s distance and availability. Complete six seconds of tending for two coins and verify the actual village crop cycle is planted or advanced. Ready crops must reject more tending. Visit the workshop’s stone workbench and complete maintenance for four coins; village stock should spend one Timber and one Stone while retaining eight units of each. Check Cancel, movement away, a blocked/removed work target, and leave/rejoin before completion: unfinished work must award nothing and must not resume. Completed rewards/effects should survive rejoining. Record stock changes with concurrent residents in mind.

For regional assets, visit Willowmead’s windmill entrance `(-4303.25,197.5,5728.25)`, Willowvale’s lookout `(-1355.75,226.0,-5679.75)`, and Fernwood’s timber houses around `(-12287.75,38.5,-3839.75)`. Verify clear approaches, interiors, stairs up/down, full silhouettes before detailed terrain arrives, and matching map/inspection labels. Windmill sails are stationary. These V4 buildings remain present in V5; they are separate from the new roadside sites below.

For V5 roadside content, seed 42 has 17 sites. Stage beside the trail ruin entrance `(3026.25,205.5,-153.25)` or waystone entrance `(2928.25,222.0,1448.75)`, then walk the short spur out and back. At the ruin, enter through both open arches and cross the roofless courtyard; inspect the stonework and compare its silhouette at near/medium detail. Check the main trail in both directions at each junction for new steps or blocked passage. The map should identify violet R/S markers and inspection should identify the actual structure; removing an inspected block must not leave stale building text. Teleport staging does not establish walking the whole inter-village route.

Compare an aspen near `(2511.75,205.0,-1913.25)`, cedar near `(3522.75,362.5,-6556.75)`, and spreading canopy tree near `(-6198.25,182.0,3952.25)`. These are tree-base coordinates; stage on nearby clear ground. Check each crown in detailed voxels and simplified form, with consistent crown colors in the distant atlas. Confirm nearby landmarks and their approaches are clear of tree crowns. These source-derived coordinates are staging aids, not completed native acceptance.

Regenerate the reviewed source-geometry catalogs independently of a native run:

```sh
cargo run --locked -p rubblekin_core --example village_asset_catalog -- /tmp/village-assets.svg
cargo run --locked -p rubblekin_core --example village_asset_catalog -- /tmp/tree-silhouettes.svg --trees
```

Compare the current [thirteen building types](artifacts/village-assets.svg), including the V6 shelter and quarry, and [six tree silhouettes](artifacts/tree-silhouettes.svg). Catalog renders verify the asset source, not lighting, terrain streaming, or movement in the running client.

Repeat Cargo in an 840×400 `--touch --low` desktop preview. Its title, wallet, Close, quantity and Refresh controls should stay fixed while work, delivery and trade content scrolls. Swipe starting over Start work: a drag must scroll without starting work; a short tap must still start it. Verify Cancel, Buy/Sell and Delivery are reachable, keyboard selection scrolls the focused control into view, and the scroll hint stays visible. This layout preview does not establish physical Android behavior.


## V6 shelter, quarry and harvest route

Use a fresh isolated save with `--generation v6` and matching protocol18 peers. Seed42 retains all17 V5 sites and adds seven; existing saved V5 geometry does not upgrade. The quarry front is `(3733.75,236,1655.75)`, entrance `(3733.75,236,1669.75)`; the shelter front is `(1762.75,206,1136.25)`, entrance `(1762.75,206.5,1150.25)`. These fronts face roughly +Z. Walk the actual spur, enter the aisle/platform, and ascend/descend quarry terraces. Compare real edited cells, proxies, P/Q map markers, nearest-site text and the thirteen-asset catalog. Inspect sparse ground flora in its matching biome and edit support/headroom: decorative details should disappear.

At ripe crops, ensure village Food is above its reserve plus12 and leave room for the offered Food. Nearby B/Work should appear after at most a one-second query with other panels closed. Start harvest, remain at the captured intact soil for six seconds, and verify Food cargo and a single shared crop reset. A farmer or another player can win the crop first; losing must give nothing. Close Cargo promptly to see the local explorer face the site and carry an empty basket; tending uses a hoe, and workshop/quarry work uses a hammer. Check the tool stays attached to the working hand and disappears on completion, Cancel, movement, flight or leaving. Unconfirmed offers and other explorers must not show a work tool. At a market, sell the Food and check the exact coin quote. Leave/reopen with the same character and private local profile file to verify completed effects. A staged ripe world establishes UI and transactions, not natural crop availability or every shared race.

The native840×400 Low route harvested12 Food, physically walked to the market, sold five for15 coins and restored15 coins/seven Food on normal reopen. Captures: [harvest](artifacts/harvest-touch.png), [quarry](artifacts/quarry-yard.png), [shelter](artifacts/trail-shelter.png), [map](artifacts/v6-world-map.png). This desktop touch preview does not establish physical Android behavior.


## Finite quarry work and material pages

Use matching protocol18 peers. At seed42 V6 quarry17, `(3736.75,236,1672.75)` is clear, grounded and within reach of the visible pile block `(7473,473,3347)`. Its supporting Dirt cell `(7473,471,3347)` must remain untouched. B/Work offers Collect stone for1 Stone cargo after six seconds. Confirm that exact block is removed, no coins arrive yet, and a market sale pays the shown quote. Pinevale’s market entrance is `(2183.75,269.5,3071.75)`; coordinate staging checks access/transactions, not the physical journey. Rebuilding an extracted block must not pay again. Close/reopen the same Save5 world and verify cargo, terrain and consumed history. Old saves1–4 migrate; unknown/corrupt current consumed records fail visibly without replacement.

In840×400 touch and1440×900 desktop, use Inventory or I to browse the creative catalog. Choose Purple wool and assign it to slot4, choose Copper and assign it to slot5, then close and place both in the world. Use categories and Next/Previous; on desktop search for copper, including typing I/C inside search without closing the panel. Reopen the client and verify the hotbar choices and actual placed blocks survive. Map C must still center the map. Open/close Inventory while pressing movement/build keys: neither opening nor closing may leak an edit or movement. Compare new wood grain, masonry joints, woven wool, colored tile grout and metal panel details on actual placed cubes. These checks now use Protocol22 peers; the preceding two-page instructions are superseded.

Compare live V6 grain/leafy/root fields at several growth stages, edit covering cells and soil, and verify no plant mesh or aimed crop remains above a blocked site. Workshop tools and grain ties should remain on their actual intact fixtures and disappear when covered or removed. These remain decorative details and generic Food, separate from the unlimited creative library.


## Work tools and crop rendering

At the clear V6 Pinemead leafy-field stance `(2805.25,204.5,-1883.25)`, close Cargo promptly after Start to see the tool. Use a valid ripe disposable world with enough Food and free cargo capacity when checking harvest; do not alter a user save. Compare the basket during harvest, hoe during tending and hammer at the quarry above. Verify the local tool follows the hand, switches between jobs, and disappears when the timer completes. The [native basket view](artifacts/work-tools-touch.png) also shows mature leafy crops; the same field becomes bare after harvest.

At Oakvale’s root-field stance `(-7815.75,306,1574.75)`, face roughly west and look down to compare low orange roots and green tops. Actual soil is cell `(-15636,611,3149)`. These coordinates stage a local inspection; they do not establish a complete travel route. Compare positive growth with bare stage zero, and check empty crop geometry produces no GPU allocator warning on load or crop changes.


## Frequent V6 discoveries and side berths

Use a fresh isolated seed42 V6 world and matching protocol18 peers. Generate actual coordinates and pacing with `cargo run --locked -p rubblekin_core --example exploration -- 42 /tmp/exploration.json`; append `v5` for the earlier sites. The new source catalog uses `cargo run --locked -p rubblekin_core --example village_asset_catalog -- /tmp/exploration-assets.svg --exploration`.

Stage at the arch entrance `(2929.75,206.5,-635.75)` facing west, standing stones `(2908.75,204.5,-1727.75)` facing east, tower `(3085.75,209.5,-1382.25)` facing east, camp `(3086.25,207.5,-982.25)` facing east, fallen giant `(3086.25,209.5,432.25)` facing east, and kiln `(1591.25,206.5,928.25)` facing east. Walk each actual approach and interior out/back, including the tower steps. Inspect geometry, aimed names, edits and proxy transitions; natural ground should blend at the footprint. Native command staging requires Enter to finish before closing the console. Coordinate staging does not establish a full inter-village journey.

Open the world map: frequent discoveries should appear as small violet dots on the island view, then reveal A/S/F/T/O/K symbols when zoomed in. Check nearest-site text and visible route connections. Inspect an airship berth from its parent trail and above: the whole pier/turning deck must stand beside the through-trail, with clear ramp and tree crowns. Walk past the junction in both directions, then board normally. Automated checks cover every new approach across three seeds and berth clearance across five; native reviews and performance claims remain limited to the actual observations in DESIGN.md.


For the smaller V6 sites, stage at cairns `(2188.75,266,2693.75)` facing north, trail bench `(7826.75,410,1945.75)` facing south, cart `(10839.75,88,3278.75)` facing east, survey post `(2950.25,224.5,1661.25)` facing north, dead snag `(-387.75,269.5,-3123.75)` facing west and split boulder `(2307.25,267.5,2946.25)` facing east. Check the clear central aisles, cart wheels/shafts, bench canopy, bare branches and split-rock passage. Zoomed map symbols add C/B/W/V/D/H and the legend wraps to a second line on desktop. The compact map keeps its shorter legend. Render just these six with the catalog's `--small` flag. Add `v6 --diagnose` after the exploration report path to print remaining long-gap terrain profiles.


For a raised-trail viewing deck, stage at `(4368.75,345.5,1031.75)` facing north. Walk the plank spur and clear central aisle out/back; inspect its bench, rails and hillside piles. In creative flight, `(4369.25,341,1027.25)` stages the open underside. The original ground must remain beneath the platform. Zoomed maps add E. From `(2849.25,220,-1850)` facing north and down, inspect the Pinemead side landing approach and through-trail separately; the ship may be away.


## Peaceful wildlife

Use matching protocol18 peers and an isolated Save6 geographic world. The read-only `wildlife` admin command reports current animals and coordinates; positions move, so use the live result rather than fixed staging coordinates. Observe rabbits from beyond10m and wolves beyond16m, then approach: both should move away. Inspect with Tab while aiming at the actual small body; a wall must occlude it. Activity, hunger and habitat forage/counts should remain live after selection. Check real hops, ground/ceiling contact and wolf leg animation. On Low, shadows should stay on actual ground during hops and follow edited raised floors; larger gaps hide them. Balanced/High should show sun shadows without duplicate contact disks. Wildlife must not remove blocks or village crops.

Disconnect all clients while leaving the server running, reconnect and compare needs/forage; then close/reopen the isolated Save6 world and verify individuals/timers remain. The server's one-hour ecology test checks births, hunts, deaths, migration arrivals and population accounting; it does not establish long-term balance or other-hardware performance. Inspect habitat berry bushes, clover and herbs: their density follows the shared saved forage quantity, with stable positions returning on regrowth. Covering their headroom or removing Grass support must remove the actual clump and inspection target. Ordinary decorative terrain grass remains independent.

## Shared wild foraging

Use matching protocol18 client/server and an isolated Save6 geographic world. Near habitat clover, berry bushes or herbs, B / Cargo & work should offer Gather food for1 Food after six seconds. Complete it and check the ordinary cargo panel/HUD; no coins arrive until a market sale. Move away, cover the clump or let grazing remove it during work: gathering should stop without cargo or extra depletion. Full cargo including sealed deliveries prevents starting. Try two players at a habitat with only five forage points: only one can finish. Close/reopen after success and verify food/depletion together. Injected save failures have TCP coverage: no completion or depleted wildlife update is sent, and restart keeps the previous save. Density represents habitat supply, not individually inventoried bushes.

For longer ecology observations, run `cargo run --release -p rubblekin_server --example ecology -- 42 12 0.05`. The third argument is the step in seconds;0.05 matches the server,0.25 is a faster coarse probe. These are headless no-client simulations, without save/socket or production-load measurements. Initial twelve-hour runs exposed predator extinction and prey decline; balancing remains open.


## Dense off-path V6 discoveries

Use matching Protocol21 builds and an isolated GeographyV6 world. Run `cargo run --locked -p rubblekin_core --example exploration -- 42 /tmp/exploration.json`; add `--all-sites` after the report arguments to print every exploration coordinate. Pathless entries have null `trail_anchor` and `approach`. The report measures nearest-neighbor spacing and straight-line discovery coverage on sampled gentle, dry land; it does not measure walk times or guarantee mountain access. Seeds42/7/99 coverage is preserved in [wilderness-discovery-checks.json](artifacts/wilderness-discovery-checks.json).

On seed42, stage six metres north of the pathless arch entrance `(2469.75,205.5,-1857.25)`, facing south, or the camp entrance `(3650.75,211.5,-1740.25)`, facing south. Walk from natural terrain into each site and back. Inspect actual stonework or tent blocks: the location should say “In the wilderness, away from trails.” Open M at whole-world scale to see violet markers throughout the island; zoom near a site for its existing symbol. No spur should appear in the terrain or painted map. Compare a distant silhouette with detailed voxels and check that trees and terrain leave the entrance open. Wreck/kiln supplies keep their existing finite salvage behavior. Native coordinate staging checks local presentation and access, not the full journey from a village.

## Whip travel (supersedes the airship route above)

Use an isolated seed-42 V3 or V6 save and a freshly rebuilt Protocol30 client/server. Walk from the central street along an outgoing road to its side-branch whip station. Check that the tower/winch and waiting carriage stand outside the town buildings/crops with a clear approach. At the station, open G/Travel, verify reachable towns and active explorers with distances, and check green live explorer markers on M/Map. Board alone, invite a second local client to join the same carriage, and launch from either occupied seat. Check the wind-up, rising whip bend, continuous carriage/camera motion, fixed seat separation and gentle arrival near the destination's central storehouse, separate from its outskirts whip. Repeat with Jump partway through a flight: camera yaw/pitch steering without movement keys, dive acceleration, retained speed on returning to the horizon, climbing until stall, automatic nose-down recovery, Jump brake, Sprint dive, visible pitch/bank and canopy closing on actual ground contact.

Repeat the station menu at 840×400 with --touch. Swipe the list without boarding, then tap a destination, Launch and Jump. Test a friend moving outside range or disconnecting before launch, four seats and a fifth attempted rider, a distant forged boarding request, observer rejection and reconnect/restart while airborne. Empty carriages should linger near viewers and disappear after viewers leave; occupied ones must remain. Confirm no airships, pilot controls or NPC airship travel appear. Render checks and controlled clock advancement do not prove subjective full-route comfort or physical Android performance.


## First composed surface POIs

Use matching current Protocol26 builds and an isolated seed42 GeographyV6 save. Regenerate the source/route review sheets with `cargo run --locked -p rubblekin_core --example poi_catalog -- artifacts/poi-compositions`. The JSON contains actual placed coordinates and 36 separate terrain-adapted review examples; the sheets show solids, sampled ground, an approximate human scale reference and overhead walking routes. The unlabelled sheet also removes color. These are geometry-review tools, separate from native rendering or a walking-time/fun measurement.

Seed42 places32 compositions among2177 existing roadside/wilderness discoveries. Stage at split crossing `(7211.75,161.5,-4073.25)` facing north and follow the descending bypass beneath both bridge ends to the opposite natural approach. At quarry steps `(2727.75,204.5,-2747.75)`, face west and descend/return along the haul ramp, comparing the three ledges and surviving pillar. Grove portal `(4325.75,283.5,2959.75)` faces east. Test both route directions without jumping and compare views from beside and above each place. Verify map A/Q/X markers, nearest-site names and inspection of actual generated surfaces. Compare a silhouette around128m and toward900m, then the detailed replacement; openings and the missing span should remain.

Sites fit gentle dry ground in this first batch; broader slope/resource relationships, major landmarks and dungeons remain future content. Catalog distinctions and automated access checks do not establish memorable exploration, a complete twenty-minute route or representative hardware performance. Native sampled evidence is in the October9 POI work log in DESIGN.md.


## POI activity routes — 2026-10-09

Use matching Protocol26 peers and a fresh seed42 GeographyV6 save. Geographic worlds should have no spawn review trays/stones. Composed sites independently host10 supported encounters in this seed; inspect the explicit plans in the activity state/save to stage a site entrance, then walk normally inside it.

At Quarry Steps, look for the rack below the entrance and supplies at different parts of the haul route. Take a supply, carry it down and place it in its own silhouette. Check the filled picture and exactly two saved coins, then reopen and verify the contribution remains. Edited support must pause a damaged scene without generating a replacement reward; Return must still recover a carried supply.

At a grove portal/span/rib site, follow the route to the separated reference boards. Match one/two/three raised dots to the turning controls. The default card must show current faces, including wrong ones; Hint and Show me remain optional. Check the wider side positions from both the entrance and following camera, turn a piece and verify its matched progress border. Repeat on compact touch with Use and optional guidance. [Native evidence and limits](artifacts/poi-activity-spatial-checks.md).
