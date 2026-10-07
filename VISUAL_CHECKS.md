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

Use a fresh isolated save with `--generation v6` and matching protocol13 peers. Seed42 retains all17 V5 sites and adds seven; existing saved V5 geometry does not upgrade. The quarry front is `(3733.75,236,1655.75)`, entrance `(3733.75,236,1669.75)`; the shelter front is `(1762.75,206,1136.25)`, entrance `(1762.75,206.5,1150.25)`. These fronts face roughly +Z. Walk the actual spur, enter the aisle/platform, and ascend/descend quarry terraces. Compare real edited cells, proxies, P/Q map markers, nearest-site text and the thirteen-asset catalog. Inspect sparse ground flora in its matching biome and edit support/headroom: decorative details should disappear.

At ripe crops, ensure village Food is above its reserve plus12 and leave room for the offered Food. Nearby B/Work should appear after at most a one-second query with other panels closed. Start harvest, remain at the captured intact soil for six seconds, and verify Food cargo and a single shared crop reset. A farmer or another player can win the crop first; losing must give nothing. Close Cargo promptly to see the local explorer face the site and carry an empty basket; tending uses a hoe, and workshop/quarry work uses a hammer. Check the tool stays attached to the working hand and disappears on completion, Cancel, movement, flight or leaving. Unconfirmed offers and other explorers must not show a work tool. At a market, sell the Food and check the exact coin quote. Leave/reopen with the same character and private local profile file to verify completed effects. A staged ripe world establishes UI and transactions, not natural crop availability or every shared race.

The native840×400 Low route harvested12 Food, physically walked to the market, sold five for15 coins and restored15 coins/seven Food on normal reopen. Captures: [harvest](artifacts/harvest-touch.png), [quarry](artifacts/quarry-yard.png), [shelter](artifacts/trail-shelter.png), [map](artifacts/v6-world-map.png). This desktop touch preview does not establish physical Android behavior.


## Finite quarry work and material pages

Use matching protocol13 peers. At seed42 V6 quarry17, `(3736.75,236,1672.75)` is clear, grounded and within reach of the visible pile block `(7473,473,3347)`. Its supporting Dirt cell `(7473,471,3347)` must remain untouched. B/Work offers Collect stone for1 Stone cargo after six seconds. Confirm that exact block is removed, no coins arrive yet, and a market sale pays the shown quote. Pinevale’s market entrance is `(2183.75,269.5,3071.75)`; coordinate staging checks access/transactions, not the physical journey. Rebuilding an extracted block must not pay again. Close/reopen the same Save5 world and verify cargo, terrain and consumed history. Old saves1–4 migrate; unknown/corrupt current consumed records fail visibly without replacement.

In840×400 touch and1440×900 desktop, use C or Blocks to cycle the two pages, select Clay with slot4 on page two, and place a real Clay cell. Check the hidden sixth slot cannot select beyond the five extra blocks. Open the map and use C to center: the selected block page must remain unchanged. Close/open panels while tapping materials; no unintended build or material change may pass through. Compare live V6 grain/leafy/root fields at several growth stages, edit covering cells and soil, and verify no plant mesh or aimed crop remains above a blocked site. Workshop tools and grain ties should remain on their actual intact fixtures and disappear when covered or removed. These remain decorative details and generic Food, without additional inventory types.


## Work tools and crop rendering

At the clear V6 Pinemead leafy-field stance `(2805.25,204.5,-1883.25)`, close Cargo promptly after Start to see the tool. Use a valid ripe disposable world with enough Food and free cargo capacity when checking harvest; do not alter a user save. Compare the basket during harvest, hoe during tending and hammer at the quarry above. Verify the local tool follows the hand, switches between jobs, and disappears when the timer completes. The [native basket view](artifacts/work-tools-touch.png) also shows mature leafy crops; the same field becomes bare after harvest.

At Oakvale’s root-field stance `(-7815.75,306,1574.75)`, face roughly west and look down to compare low orange roots and green tops. Actual soil is cell `(-15636,611,3149)`. These coordinates stage a local inspection; they do not establish a complete travel route. Compare positive growth with bare stage zero, and check empty crop geometry produces no GPU allocator warning on load or crop changes.
