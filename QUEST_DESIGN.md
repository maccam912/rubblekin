# Rubblekin visual quests and activities

2026-10-08. **Design proposal.** The user requested varied goals, reasons to explore and travel, challenges and puzzles understandable without reading, and a mixture of chosen quests and incidental encounters. Those are confirmed requirements. The mechanics, rewards, difficulty and implementation order below are recommendations for feedback. The October 8 request authorized design; on October 9 the user explicitly asked to begin implementing this document, with POI additions assigned to another chat.

Build a small vocabulary of actions that players learn once and recognize in many situations. Show an unfinished situation, let the player change it, and make the result visible. A missing cart wheel, a picnic with empty places, and a dry garden communicate more than a paragraph asking for resources.

The proposed set has eleven activity families. Some provide purposeful work, some test observation or reasoning, and some make movement and companionship enjoyable. Larger adventures combine two or three families. The user's follow-up explicitly asks to incorporate the [POI design](POI_DESIGN.md): a place's shape, terrain relationship, history and occupants should determine which activity fits it. Most scenery should still be scenery: a beautiful place need not contain a job or reward marker.

## Implementation checkpoint — 2026-10-09

The first playable slice contains spilled supplies and shape stones in a small review area beside the existing starting village (also supported in legacy valleys). Three distinct non-tradable props can be taken and placed in matching trays. Each actual placement pays two coins once, through the existing ledger. A three-piece tree/fish/mountain puzzle starts with one matching piece; turning the others to their visible references completes it. Models generate the supporting picture icons. T / touch Use acts, Backspace / Return puts a carried supply back, and Y / Hint highlights the relevant piece or matching tray. Touch controls, visible carrying and picture progress are implemented. J or touch Show me now replays the focused take/carry/place or turn/match action through the same model-derived pictures; replay does not change shared progress or rewards. A server-rejected wrong fit also replays the carried prop’s matching action and highlights its real tray, without consuming the supply or paying a reward.

This is a prototype of shared finite scenes, using the document's persistent-outcome recommendation provisionally while feedback is pending. Completed contributions and puzzle states are saved, and disconnect/restart returns unfinished carried props to their original site without duplicating them. Edited support or access can pause interaction; returning a prop never restores terrain. No activity prop is traded cargo. The first placements use real controller-checked outward/return routes and are saved explicitly; this chat adds no POI geometry. Six compatible composed POIs now host these scenes: supplies on quarry floors, kiln courts and lower crossing bypasses, and shape stones at grove portals, stone spans and broken ribs. Explicit supported positions reference stable seed/grid identities. Existing saves gain them without resetting receipts; most scenery stays quiet.

Picture parcels now wrap the existing delivery contract in source-derived destination pictures, distinct town emblems repeated on parcel/market/station/map, visible back-carried parcels, a pinned destination picture and T/touch handover at the actual market. Only a confirmed paid handover produces the receiving parcel animation. Ten village emblems differ by shape; these first pictures need family review.

The complete eleven-family design is not implemented. Picture expeditions, keepsakes/journal, expressive helpers, further activity families and broader island distribution remain next work. Personal once-per-place postcards without coin rewards are recommended for expeditions, pending the user’s feedback. The initial art and shared-state behavior need family feedback, especially text-free and muted Low-graphics play. [DESIGN.md](DESIGN.md) records verification and remaining limits.

## Understanding the goal without reading

### Show objects and consequences

Every activity needs three independently readable things:

1. **The situation:** a traveller beside a tilted cart, looking between an empty axle and a loose wheel.
2. **The next action:** the wheel can be picked up; the axle shows its matching outline. When carrying the wheel, the nearby socket responds.
3. **The result:** the wheel fits, the cart stands upright, and the traveller tests it and waves.

The child should usually understand through the scene. A compact picture card reinforces it. For this example, it shows the actual wheel, the actual broken cart, and a small image of the repaired cart. Selecting the card replays a short demonstration with the same models. A parent can explain the convention once; later instances keep the convention while changing the situation.

Do not assume that a hand, arrow, checkmark or exclamation mark is self-explanatory. Introduce each through a concrete action and consistent animation. The success animation and changed world must communicate completion even if the checkmark means nothing yet.

| Meaning | World cue | Supporting picture cue |
| --- | --- | --- |
| This person wants help | Looks or gestures between the problem and a relevant object | Their portrait beside an image of the desired result |
| This object is usable | Distinct outline on approach; one consistent interaction prompt | A picture of that same object, not an unrelated inventory symbol |
| Something is missing | An empty tray, broken connection or visible gap | One empty silhouette per missing object; fill each separately |
| Take this somewhere | Parcel and destination share an emblem | Parcel beside a thumbnail of the receiving place |
| Follow something | Tracks, dropped belongings or repeated trail marks | The next recognizable landmark or clue type |
| Something worked | Object visibly changes; a mechanism moves or a person responds | Corresponding silhouette fills and settles into place |

Use at most three or four visible items in an early task. Show three actual objects instead of requiring the child to interpret “3/7.” Larger projects divide into small, visible stages. Show the current stage prominently; a longer sequence can be opened on demand.

Colors reinforce identity but never carry it alone. A sunflower emblem differs from a fish by shape, outline and texture as well as color. Reuse each destination's emblem on its parcel, market, port sign and map marker. Similar nearby destinations must have distinguishable emblems and recognizable thumbnails.

### Make interaction forgiving

Use one contextual action for take, place, turn, work and talk; the target determines the action. Show the target and preview before acting. Snap carried activity props into generous compatible sockets. Offer tap-to-start for work and an optional hold setting; moving away cancels the current action without losing finished steps. Avoid rapid tapping, dragging tiny objects, precise camera alignment and simultaneous key combinations.

A carried activity prop is visible in the hands or on the back. Nearby placement automatically uses it, without opening an inventory. Small local puzzles allow one prop at a time and have a visible place to put it down. Parcel deliveries retain the existing cargo rules; these local props are not another trade inventory or a way to create tradable materials.

The default HUD has one focused goal card. A chosen longer journey can remain pinned while a nearby opportunity appears as a small temporary cue. Looking at an opportunity does not accept it or replace the pinned goal. The first deliberate action starts participation. Leaving hides its local cues; persistent progress remains available at the site. A compact picture journal stores accepted tasks and completed keepsakes without making a wall of objectives.

Help has three levels, available throughout rather than only after failure:

- Replay a short demonstration of the current action.
- Highlight the relevant area or next visible clue.
- Show a more direct route or the next correct interaction.

Hints never reduce rewards. Keep text available for readers and optional spoken explanations, but remove both during accessibility checks. Sound, particles, bloom, dynamic shadows and distinguishing red from green must never be required. An outlined water stream and moving wheel should remain readable on Low graphics.

### Offer difficulty through choices

Start with a visible target and one action. Later versions can add a hidden target with clues, a choice of routes, or a combination of two already familiar actions. Increase one demand at a time. More distance, more objects and less guidance all at once would mostly increase frustration.

Optional challenge variants can ask for fewer moves, a longer clue chain or an extra viewpoint. Timed runs, memory sequences and difficult jumping should be clearly chosen extras, with the ordinary reward available without them. No accepted quest expires while the family is away, and missing an airship just means catching another.

## The activity families

Durations below are local playtest targets, excluding long-distance travel. Actual journey offers should use route length and available transport; the island is too large for straight-line distance to be a useful promise of completion time.

| Family | How it starts | Main pleasure | Local target |
| --- | --- | --- | --- |
| Picture parcels | Deliberately choose a parcel | Travel with a purpose | 1–2 minutes of interaction plus journey |
| Spilled supplies | Notice scattered goods | Collect and match | 1–3 minutes |
| Roadside repairs | Notice trouble or choose a repair picture | Understand and fix a visible problem | 2–5 minutes |
| A place worth making | Choose a local project | Build something others use | 3–8 minutes |
| A trail of belongings | Notice an unusual object | Follow evidence and discover an owner | 3–6 minutes |
| Picture expeditions | Choose an intriguing destination picture | Recognize a place in the landscape | 3–10 minutes of local search plus journey |
| Shape stones | Discover an incomplete pattern | Compare and reason | 1–5 minutes |
| Flow gardens | Discover water stopping short | Experiment with visible cause and effect | 2–6 minutes |
| Lookout trails | Notice a marked route or choose its picture | Explore a satisfying physical route | 2–5 minutes |
| A travelling companion | Meet a willing traveller | Share a journey and help someone | 3–8 minutes on a validated local route |
| Wildlife moments | Notice an animal or choose its picture | Observe living behavior | Brief encounter; no guaranteed animal schedule |

### Picture parcels

**Scene and goal.** A market displays parcels beside pictures of their destinations. Taking one gives the player a visible parcel with the destination emblem. The same emblem appears on the destination port and receiving market. Completion means handing it over at that market; the parcel leaves the player's hands, appears at the receiver, and payment is shown.

**The interesting part.** Choose between a nearby walk and a longer journey using an airship. Show the actual route as a short row of town/port thumbnails, revealing the next transfer only when needed. The port sign and pilot picture show the next destination, including intervening stops. Ordinary physical boarding remains the interaction.

**Variations.** Food to a low-stock village, timber to a workshop village, a parcel for a settlement beyond a memorable arch. Initially use the existing market-to-market contract and its one-parcel limit. Building-specific recipients, fragile cargo and multi-stop deliveries are separate later changes.

**Rules.** Source stock and cargo capacity must permit acceptance. Do not generate an offer to a disconnected destination or promise an unbuilt transport route. Preserve the existing expiry-free contract and origin return option, with pictures explaining both. Never introduce spoilage or a speed requirement just to make a delivery difficult.

**Reward and scope.** Existing coins, a receiving gesture, and purpose for a scenic ride. The economic transaction already exists; emblem routing, pictures, physical parcel presentation and receiving animations are new work.

### Spilled supplies

**Scene and goal.** An upright cart has three empty shaped trays. Nearby lie a round basket, a long bundle and a square crate. Its owner picks up one demonstration item, leaving the player to recognize the pattern. Pick up an object and put it in the matching tray. All trays filled completes the activity.

**The interesting part.** Search around the site, distinguish shapes and choose an efficient collection route. A later variant distributes the items between two visible levels or among several carts with different emblems. Search difficulty comes from spatial arrangement, not almost-invisible objects.

**Variations.** Harvest baskets beside a field, tools around a workshop, picnic items near a camp, cargo beside a port. First variants keep all objects in one clear area; later ones put one behind a clue-bearing obstacle or around a corner.

**Feedback and recovery.** An incompatible tray keeps the object in the player's hands and briefly shows the required shape. Compatible objects snap into place and stay. After the last, the owner secures the load or starts using the prepared space. Dropped props stay near the activity; inaccessible ones can be recovered to a visible rack with a replayable return animation.

**Reward and scope.** A small wage for useful work and the finished scene. Adds reusable activity props and matching sockets. These props are decorative job objects, never duplicated Food or Timber cargo. A picnic variant can consume real Food through a separate explicit contribution slot, without inventing berry-specific commodities.

### Roadside repairs

**Scene and goal.** A cart leans on a bare axle. A wheel rests nearby; a broken support has a plank-shaped gap. The traveller attempts to move it, notices the problem and gestures. No dialogue or acceptance panel is needed before helping.

**The interesting part.** Diagnose the missing part, find it and restore the connection. A wheel might be visible down a broad gully with a walkable return; a plank might sit in the nearby work rack. Bringing the wheel back changes the cart's stance immediately. Fitting both parts and completing one short hammer action finishes the repair.

**Variations.** A broken bench, campsite sign, workshop handle or small side footbridge. Vary the problem's topology: one missing part, a loose part that first needs turning, or supplies located across a short alternate route. Changing only required material quantities is not a new puzzle.

**Rules.** The introductory version supplies its own non-tradable parts. A later paid material job can require actual Timber or Stone, but must identify available sources and show each unit consumed. Depleted salvage cannot be its only supplier. A bridge repair opens an optional shortcut; it must not block the village's ordinary route or strand players.

**Reward and scope.** The object visibly works, its owner responds, and a modest wage is paid. Initially let the traveller demonstrate the repaired wheel in place. Moving carts and general vehicle physics are unnecessary. Repair sockets and bounded site state are new; resource work and durable payment provide existing foundations.

### A place worth making

**Scene and goal.** At a camp or village, a picture shows a finished bench under a shelter. Beside it is a small incomplete structure with a few ghosted, clearly bounded pieces. Contributing the pictured materials and placing those pieces makes the real structure appear stage by stage.

**The interesting part.** Choose which useful place to improve, gather locally or bring materials from another village, and choose between two equally complete arrangements. A bench can face the lake or the mountains. Cosmetic choice is offered after the required structure is understandable.

**Variations.** A picnic nook, a small observation platform, a garden border, a shelter roof or a roadside sign. Start with one compact prop assembly, not settlement-wide growth. Each needs a visible use: a resident sits, travellers shelter, or the new platform provides a view.

**Rules.** Mark the project's cells explicitly and never replace player-built cells to force a plan. The first version uses a designated site footprint. Material contribution uses real cargo; unrestricted creative blocks do not silently become cargo. If creative building already satisfies a geometry-only task, accept the structure. Do not pay for repeatedly destroying and rebuilding it.

**Reward and scope.** A persistent shared improvement, a small contribution reward, and local social recognition. Broad property rights, automated town expansion and permanent NPC reassignment remain separate decisions. Promised NPC use should be a bounded visit, with a picture or completed structure still recognizing success if the visitor is delayed.

### A trail of belongings

**Scene and goal.** A distinctive scarf is caught on a low branch. A matching scrap and a tipped basket are visible further on. Investigating the scarf shows a short picture of an owner with a missing belonging, then leaves the next physical clue to follow. At the end, a traveller's portrait/accessory matches the object.

**The interesting part.** Read a small environmental story: something rolled downhill, a traveller squeezed through a wall breach, or a bundle snagged while crossing a hollow log. The clues describe the actual route. The player returns the belonging to the nearby owner or places it at an unmistakable matching collection point.

**Variations.** A dropped tool, a kite caught safely in a low tree, a hat at a camp, a survey bag near a ruin. Two or three clues teach the family; later versions branch at a junction where one trail has the wrong emblem or footprint shape. Avoid a generic glowing line for the default experience.

**Rules.** Every turn has a visible clue from the preceding search area. Search areas are bounded and accessible with ordinary movement. Use static clues and a fixed waiting visitor first; do not depend on persistent historical footprints from the entire NPC simulation. The ending exists before the first clue is exposed.

**Reward and scope.** An expressive reunion and a small keepsake or wage when appropriate. The newly discovered camp or overlook is part of the reward. Reuse pickup, matching, portraits and site placement; clue-chain generation is new.

### Picture expeditions

**Scene and goal.** A board or traveller offers a picture of an actual distinctive place: an arch framing a split peak, a ruined tower with a tree through it, or a kiln below a particular cliff. The player chooses the picture they want to investigate. On reaching the broad viewing area, the picture lines up approximately and becomes a completed postcard.

**The interesting part.** Compare terrain and silhouettes, choose a route and recognize the place. An introductory picture includes the whole destination and an obvious nearby trail feature. Later pictures show a characteristic detail plus two landmarks visible from the route. A map hint circles a search region rather than immediately exposing an exact dot; stronger guidance remains available.

**Variations.** A walking expedition, a view first glimpsed from an airship with a clue at the next port, or two perspectives on the same formation. Only use an airship sighting when the actual flight affords a readable view; otherwise show the destination picture at the port. Gliding is not required.

**Rules.** Derive the picture from the generated site and verified viewing pose, not a generic illustration of a vaguely similar arch. The target is a broad reachable zone plus visibility of the intended feature; pixel-perfect matching is never required. Local progress should fill the picture through a brief steady look, without a separate camera inventory.

**Reward and scope.** A keepsake showing a place the player deliberately set out to find. Collections are optional personal memories, with no required island-wide checklist or reward popup at every POI. Site thumbnails, viewing checks and the album are new. Distinct landmark silhouettes and adequate distant rendering depend on the separate [POI proposal](POI_DESIGN.md).

### Shape stones

**Scene and goal.** Three stone faces show a tree, a fish and a mountain. A nearby relief displays the same three shapes in a clear spatial arrangement. Turn each stone until the arrangement matches. A matching stone settles with a short motion; completing the arrangement turns a small sculpture or opens a view into a side chamber.

**The interesting part.** Compare shapes and spatial relationships. In the first version the example sits directly above the stones and one is already correct. Later, the reference appears on the opposite side of a court or must be understood from a particular broad viewpoint. Another variant places shaped objects into an arrangement instead of rotating faces.

**Variations.** Stones around a tree, carvings in a kiln courtyard, wind vanes at a lookout or display pieces in a workshop. The reference can describe order, orientation or correspondence; use only one new rule per instance. A branch pattern might ask the player to match which two symbols share a connection.

**Rules.** Start with at most three pieces and three orientations. Allow any sequence of inputs. Wrong settings are harmless and immediately reversible. Generate a solved arrangement first, then scramble it and verify a nontrivial solution. Accept every arrangement satisfying the visible rule, including symmetric alternatives.

**Reward and scope.** A visible mechanism, optional chamber, view or small keepsake. Reuse turn/place actions; add explicit puzzle state and rule checks. An editable wall or door may be bypassed by creative building. Reaching the chamber is allowed; the puzzle's separate completion requires satisfying its pictured rule once.

### Flow gardens

**Scene and goal.** A small spring trough feeds channels toward a dry flower bed. Water visibly stops at a rotated channel. Turn the pieces so the flow reaches the bed; its flowers rise, a little paddle wheel spins, or a fountain starts.

**The interesting part.** Watch a local cause and effect, make a hypothesis and change one connection. Begin with two elbows on a short single route. Later versions introduce one branch and two visible destinations. Completing one destination remains visible while the other is solved.

**Variations.** A courtyard garden, a clay-working rinse trough or a playful water wheel. The source, endpoint and obstacles change; the rule stays “connect the open ends.” A later dry variant can connect a visible mechanical belt or light guide, but should be introduced through its own demonstration.

**Rules.** Use a small authored channel network with snapped rotations and a known solution. Render water only along connected channel pieces. This is a self-contained prop mechanism, not world flooding, terrain fluid simulation or a change to the island's static rivers. Channels stay at a controlled height and a safe dry route reaches every control.

**Feedback and recovery.** Flow advances to the current break after every turn. Disconnected ends visibly drip into the trough. All states are reversible; nothing floods or kills the garden. Optional hints indicate the next open end. No sunlight, night wait, precise timing or expensive lighting is required.

**Reward and scope.** A restored garden or running mechanism. The connection puzzle and flow presentation are new work; the existing hydrology does not implement them. Start with one hand-built working garden before distributing variants.

### Lookout trails

**Scene and goal.** A broad route of low platforms, hollow logs or quarry terraces leads toward a visible flag at an overlook. Matching pennants identify intermediate checkpoints. Passing one raises it; the destination displays a little panorama of the place reached.

**The interesting part.** Traverse an appealing route, notice a useful branch and discover the view. Introductory trails require walking and ordinary stepping. An optional side route adds broad jumps; a family-friendly bypass still reaches the main destination.

**Variations.** Across a dry gully beneath an old bridge, through a hollow giant, around quarry ledges or up a ruined watchpost. The layout changes the movement and view, rather than repeating identical floating platforms in different materials.

**Rules.** Validate actual body clearance, support, landing space and camera room. Falls should lead to a nearby return path on the ordinary route. For an explicitly chosen challenge, offer return to the last cleared checkpoint at the same site; this does not introduce unrestricted world teleportation. Never erase earlier flags after a missed jump.

**Reward and scope.** The view, a postcard and an optional extra pennant for a harder route. A personal best timer can be added later without gating ordinary completion. Creative bridges and flight are legitimate ways to explore; this initial design has no competitive leaderboard or anti-shortcut rules. Movement exists; checkpoints, route rewards and validated arrangements are new.

### A travelling companion

**Scene and goal.** A traveller repeatedly compares a picture of a nearby camp with the surrounding landscape. Their gesture and a two-person walking picture invite company. Interacting makes them signal agreement and follow; the camp's emblem stays visible on their bag and the goal card.

**The interesting part.** Choose the route, notice useful scenery together, and arrive as a pair. The traveller occasionally points toward an actual visible landmark. They wait if the player explores and resume when approached. A later version offers two camps and lets the player choose the destination.

**Variations.** Walk a visitor from a port to a local meeting place, help a tired surveyor return to camp, or accompany someone along a short safe trail. Start within one local area. Long airship escorts, animal herding and transporting NPC cargo introduce more failure modes and come later.

**Rules.** Generate only when the traveller can physically follow a checked route. Keep escort willingness as an explicit temporary state; ordinary inhabitants do not become commandable units. Never require chasing an NPC faster than the player. Replan when blocked, then visibly wait and offer a route hint if no route works. Leaving or logging out suspends the request without punishment. The traveller's bounded resting/return behavior and saved meeting point must remain visible on resumption.

**Reward and scope.** A greeting at arrival and a small gift or payment. This is a later family: existing navigation and saved journeys help, but following a player, interruption behavior and recovery need real implementation and multiplayer testing. Prefer it after stationary interactions are reliable.

### Wildlife moments

**Scene and goal.** A real rabbit emerges near a meadow. An optional picture card shows the rabbit in a resting or grazing pose with an eye symbol previously demonstrated. From a generous observation distance, keep it in view briefly; the picture fills when the actual behavior is observed.

**The interesting part.** Notice where wildlife lives, slow down and watch it doing something. A willing player can collect a few chosen animal moments, or simply see a one-time completion after deliberately focusing on an animal. Walking past never starts a hunting checklist.

**Variations.** Rabbits grazing, two animals resting near each other, or animals passing a known crossing. Habitat, viewpoint and observed behavior matter more than quantities. Do not make a child wait indefinitely for a rare birth or hunt.

**Rules.** Offers inspect real current presence and plausible reachable viewpoints. Wildlife remains autonomous, can flee, migrate or die, and is never secretly spawned to fulfill the card. A sighting cannot be reserved as though it were a static puzzle prop. If an accepted subject disappears, retain completed observations and show that the subject is absent; offer another known live sighting or let the player shelve the card. When none exists, no replacement location is invented.

**Reward and scope.** An optional animal picture and a reason to notice ecology. Begin with ordinary visible poses, without player feeding, taming, capture or rescue. These would alter existing behavior and resource rules. Recognition checks, framing UI and humane presentation need playtesting; the existing wildlife simulation is only the foundation.

## Building activities into the POI design

The place and its activity should explain each other. A ruined work yard can contain a stopped rinse trough because water was useful there. A broken crossing exposes a lower route and the remains of an old store room. A root-filled court has a garden reached around the roots. These relationships let a nonreading player reason about what might be around the corner.

Do not place the same puzzle pedestal in the center of every generated structure. Reuse its rule, controls and feedback while changing how the site's actual spaces participate. A local activity should give the player a reason to notice the feature that makes the place distinctive.

### Give each site scale an appropriate role

| POI tier from the companion proposal | Activity role | Example |
| --- | --- | --- |
| Small discovery, roughly 2–12 m | Usually scenery; sometimes one action or a clue toward somewhere else | A dropped survey bag beside a marker shows a picture of the actual lookout visible beyond it |
| Local site, roughly 20–80 m | One complete activity with a visible end and perhaps a short optional branch | A broken cart above a dry gully, with its wheel accessible below |
| Major landmark, roughly 100–300 m including terrain | A destination with two or three connected activities and spaces with no assigned task | Quarry terraces leading to a work yard, an optional gallery and a second daylight exit |

The sizes remain illustrative POI proposals. Do not enlarge the number of required objects just because a site is bigger. Larger sites should change the journey and understanding of the place. A local-site quest can be complete before the player chooses to explore its deeper spaces.

### Match the six POI families to useful activity arrangements

| POI family | A fitting activity | How spatial variants change the experience |
| --- | --- | --- |
| Stone landforms | Picture expedition, clue trail, wildlife observation | A low portal frames the target; a span lets the player search above and below; a basin reveals clues gradually around its rim |
| Old crossings | Roadside repair, lookout trail, lost belonging | A causeway supports a broad easy approach; a broken bridge has an underneath route; hillside stair remnants lead to an alternate crossing |
| Abandoned workings | Flow garden, shaped tool return, material contribution | A compact kiln teaches matching nearby; quarry ledges separate target from approach; a long extraction face offers a choice of return paths |
| Forgotten homes and gardens | Garden restoration, small building project, shape stones | A cottage offers one visible problem; terraces show progress at successive levels; a root court makes clues readable from different entrances |
| Watchposts and markers | Landmark recognition, pattern comparison, lookout trail | A lone tower is a destination; paired remnants provide a clue and a visible target; a split-height court lets the player inspect a pattern from above |
| Refuges and camps | Spilled supplies, reunion, companion arrival | A trunk creates a through-route; separate clearings make a short search; an overhang frames a meeting place visible from the approach |

These are compatibility preferences, not guaranteed contents. Several instances of the same family should have no activity, and a recognizable arrangement can sometimes support a different compatible situation. Avoid “every kiln has pipes” becoming as predictable as repeated building models.

### Make the proposed places playable

**Root Court: bring the garden back.** From the wall breach, see a dry planting bed on the far side of the big tree. An old trough disappears behind a root; following it reveals a dislodged channel. Walk around the tree to reach the two controls and restore the local flow. The watered bed becomes the completion signal. A newly moving paddle reveals a carved pattern pointing toward an optional cellar, whose entrance was already visible from the outer rooms. The roots, alternate entrances and garden exit all matter to the activity; the cellar is a future extension, not part of the first promise.

**Split Crossing: retrieve the survey bag.** A surveyor at one surviving bridge end points to a distinctive bag visible on the lower abutment. Broad steps lead into the dry gully; scraps on old masonry explain the route beneath the missing span. Returning the bag completes the request. A separate material project can repair a short stair or side platform into a more convenient return, without magically rebuilding the entire large bridge. In another instance, the same lower route becomes an unguided lookout trail with no NPC. The missing span remains a memorable feature after the activity.

**Quarry Steps: put the yard back to work.** From the top, see a tool rack below with three unmistakable empty silhouettes. A broad terrace route passes two loose tools and a third beside an old rinse trough. Return them to the rack; a waiting worker uses one, completing the job. The trough offers an independent connection puzzle. A future excavation can continue from that working face and return through a second daylight opening. Retrieving the tools uses the descent, views between levels and return route; it is not three objects scattered randomly on a flat pad.

A regional chain can connect existing places without making a storyline mandatory: a mason at a crossing shows a rubbing that matches a nearby quarry's mark, and that quarry frames a watchpost built from the same stone. Each site remains worthwhile independently. The player can follow the visual relationship, take a paid delivery along it, or ignore it.

### Generate the spaces and opportunities together

The POI generator should first establish the site's terrain fit, connected spaces and visible history. Within that plan, choose an optional compatible activity before final props and access are fixed. A missing part must have somewhere plausible to land; a picture clue needs a real sightline; a channel puzzle needs a dry approach to each control. If a needed arrangement cannot fit, choose another activity or leave the site quiet.

For the first family generators, return a few explicit named positions alongside geometry: an approach where the invitation is visible, interaction points, supported prop positions, a completion/viewing area and a return route. Only include positions a chosen activity actually uses. This is a small part of the shared site plan, not a new universal attachment framework. Stable references let pictures, clues, world geometry and server checks describe the same place.

Add an activity after the site's structure is known but before validating its final playable state. Validate the combined site, not geometry and quest in isolation. Review transitions from the distant silhouette to the entrance cue to the actual next action. On Low graphics, a player must still see the landmark that their picture promises and the opening they are meant to enter.

Future dungeons reuse familiar actions inside the four spatial arrangements in POI_DESIGN.md: a descent and return loop, a court with several approaches, a broken route with a bypass, or intersecting levels. A short gallery can combine a visible target, a shape arrangement and an alternate way out. It need not add combat, a boss, locked progression or a new rule for every room. Any later hostile depth keeps its visible optional boundary and retreat route; combat design remains unresolved.

Do not count a decorative recolor as either a new POI or a new quest. Review whether the next variant changes what the player sees first, where they move, which clue they use, or what their action accomplishes. Keep the existing scenic and persistent-world direction: the repaired place, view and route should still be worth visiting when its finite reward is gone.

## Combining activities into little adventures

Use short authored recipes with procedural substitutions. A recipe defines the causal relationship, visible clues, compatible site types and completion conditions. It does not write a new story or arbitrarily string together errands.

### The cart below the arch

While walking, the player notices a traveller with a tilted cart. The empty axle matches a wheel visible below a stone arch. A scarf scrap at a broad descending path leads to it. Carrying the wheel back repairs the cart; the traveller waves and unrolls a picture of a camp further along the road.

This combines **repair → clue following → carry and fit**. The offered camp picture is an optional next adventure, not an extra requirement after the cart is fixed. Change the terrain relationship, missing part and accessible retrieval route. Keep the object, gap and outcome causally connected.

### The garden in Root Court

A chosen picture expedition leads to the Root Court layout from the POI proposal, with its unusually large tree and garden exit. Arriving completes the expedition. Inside, a dry garden and a stopped stream invite a flow puzzle. Restoring it turns a paddle wheel that reveals a carved pattern for nearby shape stones. Solving those reveals a bench nook overlooking the valley.

This combines **recognize a place → connect → match**. Each has its own satisfying end. The final nook can also be reached by creative excavation. For an early version, use an open surface courtyard; underground chambers wait for the geometry work described in POI_DESIGN.md.

### A parcel with a detour

The player intentionally chooses a parcel for the sunflower-emblem town and rides an airship. At the destination port, spilled picnic supplies invite a two-minute diversion. Finishing that local activity returns attention to the still-pinned parcel picture. Delivery completes at the market. A nearby lookout picture suggests another independent outing.

This combines **travel → optional help → delivery**, and demonstrates how incidental activities fit inside a chosen goal. Ignoring the picnic changes neither the delivery nor its reward.

Early adventures should contain one to three required stages and at most one optional branch. Finish and reward the promise before offering a continuation. A chain should change what the player notices, decides or does; five instances of collecting a different object remain one repetitive activity.

## Procedural rules that produce playable variety

### Fit an activity to a real situation

Use three sources:

- **Real requests:** existing village export/demand, surplus crops, or an actually unfinished local project. Recheck the need before offering it. After acceptance, honor the agreed completion even if background supply changes.
- **Prepared encounters:** a spilled load or repair scene placed as an explicit event at a compatible site. These are designed situations, not a claim that the economy accidentally broke a cart. Save their state and leave resolved scenes resolved.
- **Persistent discoveries:** a pattern, route or garden tied to a site's structure. It exists before a visitor approaches and works without a quest giver.

Do not attach every activity to a universal NPC “need” score. A puzzle garden does not need an economic justification; a request to transport real goods should use the real ledger.

For a new site, fit its spaces and activity together as described above. For an existing site or renewed real request, choose in this order: eligible place or request; compatible activity family; available actors/props/resources; solved result; route and clue arrangement within the available space; starting state; entry cues. Validate before exposing the cue. With bounded failed placement attempts, skip the event rather than leave a broken promise.

### Vary the experience, then the dressing

| Dimension | Useful variation | Constraint |
| --- | --- | --- |
| Entry | Board picture, person gesture, misplaced object, visible mechanism | Use a cue the player has learned; do not open a modal automatically |
| Terrain relationship | Across a gully, around a court, up a broad ledge, beyond a log | Real movement and return route pass clearance checks |
| Information | Target visible, target partly obscured, two successive clues | Early versions never hide both the objective and the rule |
| Action | Carry, turn, place, connect, observe, traverse | Combine at most two unfamiliar demands |
| Choice | Walk or ride, two routes, optional viewpoint, arrangement choice | Both advertised options actually work |
| Outcome | Repaired object, used place, reunion, flow, view, keepsake | End visibly differs from start |
| Appearance | Local material, emblem, character accessory, seasonless vegetation | Shape and object identity remain readable |

Keep a short recent-family list per player and per locality. Avoid offering the same main interaction several times in succession. This is a small weighting rule, not a player-personality model. Cooperative players can still choose a familiar task together.

Use region-specific reasons to travel: clay workings suggest channel puzzles, quarry terraces support traversal, old watchposts support picture and pattern challenges, and ports connect distant commissions. These are preferences, not exclusive biome locks. A destination picture should communicate what makes that specific trip appealing before the player commits.

### Validate what the pictures promise

Every generated activity must have:

- A supported start, required interactions within reach, adequate headroom, and a walkable exit under ordinary movement.
- A concrete success condition that matches the picture: actual arrival, specific occupied sockets, a satisfied pattern, connected endpoints, or an observed behavior.
- An available copy of each required activity prop. Material requests use actual stock/reservations or multiple feasible suppliers; a stale depleted wreck is not a promise of timber.
- A reachable completion point. If a receiver may leave, provide a visibly designated collection point or explicitly retained meeting behavior.
- A known solution for puzzles and a route for clue chains. Build from a solved state and scramble; do not rely on random pieces happening to be solvable.
- Clues visible from intended approach areas at Low graphics, including distant landmark representation where needed.

A prepared site is not proof against later player edits. Recheck changed supports, missing controls and access. If a required prop becomes unusable, offer visible recovery of that activity prop without restoring player terrain. If the site itself is destroyed, pause or cancel with pictorial explanation and refund its recorded paid contributions. Never silently reset the world or redirect a carried parcel.

## Starting, leaving and sharing

An incidental event can attract attention with movement, a gesture, an unusual object or optional sound. Attraction remains local; no global quest toast, compulsory camera turn or countdown. A short follow-up gesture is enough if ignored. Some sites remain empty, some contain people simply living there, and some offer activity.

Some invitations should actually unfold during ordinary play, rather than always being tableaux waiting for inspection:

- **A wheel rolls across the trail.** A visible traveller stops at a prepared wayside site, a loose wheel detaches and rolls a short distance across supported ground, and the cart tilts onto a support. Its final resting place becomes the repair task. The player can catch up later; the wheel never vanishes because they failed to react quickly. This is a bounded prop animation along a checked path, not general cart physics.
- **Unloading becomes a small mishap.** At an actual airship arrival, a visible helper sets down a load and two or three supplies tumble into safe positions beside the port. The helper catches one, then looks toward the rest. Returning them starts the spilled-supplies activity. If ignored, the helper eventually gathers them; once the player participates, preserve their completed contributions and show the helper dealing with any remainder. Do not take over an ordinary trader's real cargo to fake this event.
- **An animal crosses the journey.** An actual rabbit moving between forage patches can pass a traveller's route. Following at a distance may reveal a real habitat and invite observation when the player deliberately focuses on it. Its route is not rewritten to lead the player to a quest, and no promised rescue or reward depends on it remaining nearby.

Prepared mishaps have an explicit small state sequence: waiting for their physical trigger, visible action, available help, participation, and resolution. Place the actors and safe prop destinations before starting the action. Use a real arrival or ordinary proximity to an already placed site, never camera direction to teleport actors into view. A distant dormant event can stay cheap; after activation its shared outcome persists. If nobody helps, its bounded resolution leaves an ordinary scene. A new mishap requires a later compatible event, not a loop every time the player passes.

Use one clearly prominent opportunity in a small area. Leave quiet stretches between event clusters, and avoid spawning a new mishap simply because a player returned. Determine event placement/state independently of the camera. Repeatable work follows renewed supplies or a visible new request; a resolved wreck does not instantly break again.

World changes are shared. Several players can contribute to a repair or puzzle; completed steps never revert because someone else joined. The active panel shows what is already finished. An activity prop has one server-authoritative holder, and disconnect recovery returns it to the saved site or holder consistently without duplication.

For a first implementation, separate shared and personal outcomes explicitly:

- **Shared finite repair or material project:** a bounded total wage attached to actual contributions, paid once per completed contribution. Joining late does not mint another full wage. Participants also see the shared celebration.
- **Shared puzzle:** its mechanism and solved state are persistent. Helpers present before completion can receive a personal keepsake once. Later arrivals see the resolved result; the first version does not reset it on approach. Any later practice mode should be voluntary, without repeat coin rewards or restoring player-edited terrain.
- **Personal journey or observation:** each player can complete their own postcard once, while seeing the same world and animals. Ordinary market deliveries retain their individual cargo and transactions.

This favors a durable shared world over guaranteeing every visitor an untouched puzzle. More private puzzle practice, parties and reward sharing can be designed later if family play reveals a need. No leaderboards, streaks, mandatory daily work or penalties for leaving are needed.

## Implementation boundaries

The current code provides practical anchors:

- [economy.rs](crates/core/src/economy.rs) defines five traded resources, cargo, one delivery contract, and the existing work kinds. Food is one resource despite different plant appearances.
- [player_economy.rs](crates/server/src/player_economy.rs), [local_work.rs](crates/server/src/local_work.rs) and [resource_work.rs](crates/server/src/resource_work.rs) handle real transactions, work access, finite supplies and validation. New visuals must not fabricate alternative commodity ledgers.
- [market.rs](crates/client/src/market.rs), [work_tools.rs](crates/client/src/work_tools.rs), [world_map.rs](crates/client/src/world_map.rs) and the inspection UI provide presentation entry points. They do not already implement the proposed picture cards or quest system.
- [settlement.rs](crates/core/src/settlement.rs) and the exploration asset generators provide real places. The [POI proposal](POI_DESIGN.md) describes future spatial variation; it is not implemented content.

Keep the path direct: a small enum of activity families, an explicit instance record, family-specific validation and a client presentation record. A record needs a stable ID, recipe version, site/actor/prop references, current step state, participants/contributions, and reward/completion receipts. Pictures and world props must derive from those same references. Introduce only the fields needed by the first families; no quest scripting language, arbitrary condition graph, general event bus or runtime LLM.

Validate actions on the server against the actual world, actor, distance, visibility, ownership and current step. Save completed effects, consumed materials and reward receipts atomically before acknowledgment. Duplicate requests, reconnects and server restarts must not repeat payouts. Save unfinished multi-step progress; brief action animation can restart without undoing earlier steps. Suppress canceled input on disconnect or UI transitions as existing gameplay does.

Most sites are static and need no simulation tick. Only nearby presentation and active actors require updates; use bounded site lookup and existing worker/generation budgets. Thousands of POIs must not imply thousands of waiting NPCs or per-frame puzzle evaluations. Client/server deterministic generation, protocol changes and save validation should follow existing conventions when implementation is authorized.

## Recommended first implementation and acceptance

Build four families first: **picture parcels, spilled supplies, picture expeditions and shape stones**. Together they cover deliberate and incidental entry, travel, physical manipulation, search, and a true puzzle. Reuse the carry/place pieces for roadside repairs next. Flow gardens add another strong reasoning activity; travelling companions should follow only after route recovery is reliable.

Use the same small review area as the POI proposal, with stone landforms, an old crossing, abandoned workings and two connected villages, rather than building a separate quest showcase or distributing events island-wide. Make three materially different instances of each first activity family, including one without a giver and one that can be solved by two people. Twelve review instances are an authoring target, not proof of twelve distinct experiences. The POI proposal's broader 36-example geometry review and this interaction review have different purposes; neither requires all example sites to be active quests.

The initial new content is a common picture-card/demonstration treatment, destination emblems and thumbnails, a few carryable props and generous sockets, a three-piece stone puzzle, and several expressive gestures. These are real art/UI tasks; procedural combinations do not eliminate that work. Reuse models and animations deliberately, and avoid adding a new icon language for every recipe.

For the family playtest, explain the first example once. In a different arrangement later, remove text and ask the child to play without further instruction. Watch whether they:

1. Notice the invitation and can choose to ignore it.
2. Point to or demonstrate what they think will happen.
3. Find and perform the next action without opening a text menu.
4. Recover after trying the wrong object, missing a turn, or stepping away.
5. Recognize completion through the changed scene.
6. Resume the original parcel journey after an incidental detour.

Also test muted audio, Low graphics, touch input, grayscale cues, a full cargo hold, an edited approach, a second player taking a prop, and leaving/rejoining midway. Logic checks should establish solvability, no duplicate rewards and persistence; native playtests must establish legibility, movement and fun. If the second example still needs the same verbal explanation, improve its cue before increasing content volume.

The main choices before implementation are whether persistent shared puzzle outcomes suit family play, how much optional pictorial guidance to show by default, and whether keepsakes are enough progression or the family wants additional cosmetic rewards. My recommendation is persistent shared outcomes, generous optional help, and useful work plus a small keepsake album. This keeps discovery relaxed while making progress tangible.
