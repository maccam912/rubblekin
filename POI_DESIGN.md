# Rubblekin point of interest design proposal

2026-10-08. **Status: proposal for discussion; no gameplay implementation authorized.** The user wants substantial procedural variety in POI size, form and experience, and explicitly asked to include future dungeons and encounters. That confirms the design scope, not the particular mechanics, content, dimensions or priorities recommended below.

Generate recognizable places with different spatial ideas, then vary how those ideas fit the landscape. A player should remember “the quarry with the descending ledges” or “the crossing where I went beneath the broken bridge.” Materials, decoration and names reinforce that identity.

**Why the current sites repeat.** [BuildingPlot](crates/core/src/settlement.rs) contains a kind, origin and quarter-turn rotation. Each kind has one fixed set of dimensions and one fixed voxel shape in [village_assets.rs](crates/core/src/village_assets.rs) and [village_assets_exploration.rs](crates/core/src/village_assets_exploration.rs). At the current 0.5 m cell size, the arch is always 18 × 12 m in footprint, the camp 14 × 14 m, and the ruined tower 13 × 15 m. These are different assets, but every instance of a kind repeats its structure.

[Wilderness placement](crates/core/src/settlement_wilderness.rs) adds some biome/resource filtering, a jittered 360 m grid, separation and bounded retries. It places one building per discovery and falls back to small rocks or cairns on harder terrain. Its dry, gentle footprint and entrance checks solve accessibility, but restrict the situations sites can occupy. Placement variety therefore exceeds structural variety. Source review supports this diagnosis; the existing [native arch capture](artifacts/wilderness-arch-native.png) is a presentation reference, not a new playtest.

**Give discoveries different sizes and different jobs in a journey.** Keep the requested frequent wilderness interest. Change its prominence and complexity: a small discovery can be a few stones or a glimpse into a clearing; a destination should contain several spaces worth exploring.

| Proposed tier | Illustrative extent | Role in exploration |
| --- | --- | --- |
| Small discovery | 2–12 m | Notice a distinctive tree, a weathered wall, a shelter or evidence of a nearby place. Often worth a brief look; some support existing gathering or salvage. |
| Local site | 20–80 m | Spend a few minutes exploring a clearing, a ruin with several spaces, a crossing or a working yard. Include a recognizable approach and at least one discovery on arrival. |
| Major landmark | 100–300 m including its terrain setting | Recognize a destination from afar, choose how to approach it, and explore multiple connected spaces. Some can contain future dungeons. |

These are review ranges, not approved quotas or promises about duration. Large extent can mean widely separated piers, terraces or openings around existing terrain; it need not mean a solid 300 m structure. Change width, height, depth, number of spaces and footprint independently. A long low causeway should feel different from a compact tall watchpost. Keep human doors, stairs and walkways at usable dimensions instead of scaling an entire model.

Keep something noticeable roughly every one or two minutes on suitable walking routes, with clusters and quieter intervals. A substantial detour might appear every five to ten minutes on a curated review route. These are pacing hypotheses to walk and assess, not a rule that stamps another building whenever a timer expires. Wilderness coverage must remain part of review. Do not solve repetition by simply removing the scattered discoveries the user requested.

**Build a small vocabulary of strongly different places.** Each family needs several spatial arrangements. A family is a coherent subject that can support multiple experiences.

| Family | Visibly different arrangements | Possible future depth |
| --- | --- | --- |
| Stone landforms | A low portal into a sheltered grove; a natural span over a dry cut; broken ribs enclosing an open basin | A side grotto, an animal refuge, or a deeper passage where the terrain supports it |
| Old crossings | A shallow causeway; a bridge with a walkable route beneath it; hillside stairs ending at the remains of an older crossing | Rooms inside an abutment, travellers using a shelter, or occupants controlling one approach |
| Abandoned workings | A compact kiln beside clay; a quarry descending through ledges; workshops arranged along a long extraction face | Excavation galleries, a salvage crew, or a blocked haul route with a second way through |
| Forgotten homes and gardens | One collapsed farmhouse; several narrow terraces; a roofless court around a large tree | Cellars connected to former storage areas, temporary inhabitants, or a request tied to something actually present |
| Watchposts and markers | A lone ridge lookout; linked remnants visible across a valley; a watch courtyard split between different heights | A vertical ruin, occupied rooms, or a route through its foundations |
| Refuges and camps | A hollow trunk shelter; several small clearings; a camp tucked under an overhang | Social encounters, abandoned belongings or a nearby den; many remain peaceful surface places |

Existing arches, towers, camps, stones and kilns become ingredients for these families. Keep effective existing small assets as minor details within or between larger sites. Repetition is useful for recognizing construction traditions, provided the whole place is not identical.

Three illustrative places make the intended difference concrete:

- **Root Court:** approach along the outside of a ruined wall. A breach reveals a tree filling the former courtyard. Walk around its roots and find a second exit through the old garden. A future cellar branches from the outer rooms; appreciating the court does not require entering it.
- **Split Crossing:** see two surviving bridge ends above a dry gully. A broad descending route passes beneath the missing span and climbs the opposite bank. From below, the fallen masonry explains the break. Future occupied rooms sit in one support, away from the ordinary bypass.
- **Quarry Steps:** see a cut in the hillside rather than a freestanding building. Descend through broad working terraces, with an upper route overlooking the lower ones. A future excavation continues from a believable working face and eventually returns to another daylight opening.

These names are illustrative descriptions. Naming should follow the generated place and observable features. A name or inspection paragraph cannot compensate for a visually ordinary layout.

**Make variation coherent by choosing a reason before choosing details.** The generation sequence should be short and inspectable:

1. **Find a useful terrain situation.** Sample ridges, shoulders, dry gullies, clearings, banks and resource edges. Check actual usable ground. Select major sites first so smaller details do not occupy their setting. Preserve the approved island and large landforms; initially fit sites to them with bounded local grading or carving.
2. **Choose the site's central idea.** For example: “an old work yard stepping down toward a clay deposit” or “a crossing whose broken span exposes rooms below.” This is a small authored recipe and explicit parameters, not generated prose or an LLM request.
3. **Lay out connected spaces.** Choose a family-specific arrangement, its dimensions, elevations, entrances, branches and sightlines. A court, a chain of terraces and a crossing require different arrangements. Establish an ordinary walking route and a way out before decorating.
4. **Choose a short, compatible history.** Purpose, one major change, and optionally a later reuse. A collapse puts rubble beneath the missing structure and changes access. A camp occupies a sheltered surviving room. Overgrowth favors openings and soil. These are generated starting conditions, not a simulation of centuries.
5. **Adapt structure and materials to the site.** Foundations meet local ground at several heights. Walls follow terraces; openings frame actual views. Doors and steps retain usable dimensions. Preserve natural trees where the composition calls for them. Use local material traditions without making every region contain only one kind of discovery.
6. **Check it against nearby places.** Prefer a different dominant silhouette, movement pattern or terrain relationship when neighbors already share those traits. Make a bounded alternative attempt; if none works, choose another appropriate family or a modest discovery. Do not force an implausible structure to fill a quota.
7. **Add secondary detail.** Props, cracks, smaller plants, surface variation and names come last. They should support the visible purpose and history.

Each substantial variant should differ in at least two of silhouette, route through it, terrain relationship and use. Color, rotation, a moved chest and a random damage percentage do not satisfy this design test. This is an authoring rule of thumb; a numeric score cannot prove that a place is interesting.

Damage needs a spatial cause and a bounded effect. Randomly deleting blocks tends to produce noise, broken paths and the same “ruined” texture everywhere. Prefer a fallen corner, a missing span, a partly buried lower room or a surviving wall used by later occupants. Check support, headroom and access after the change. The intact portions should still make the original place understandable.

Regional repetition can tell a larger story. A quarry, downstream masonry remnants and a lookout built from the same stone might belong to one local tradition. Two or three associated places are enough to try this. Each must remain interesting by itself; the connection is something players can notice, not a compulsory clue chain. Live shortages, inhabitants and wildlife must come from actual world state when the design refers to them; static scenery should not claim an event is currently happening.

**Use curation to avoid a second kind of sameness.** Pure random combinations make many outputs; they do not guarantee many experiences. Author the useful arrangements, causal changes and unusual focal features. Procedural generation fits, composes and varies those ideas.

For ordinary sites, limit repetition of the same arrangement, size tier and terrain setting within a neighborhood. Also review the sequence along real travel routes: two places on opposite sides of a hill can be nearby on a map and feel separate, while repeated silhouettes across an open valley can be obvious together. Start with simple spatial checks and review walks rather than a general novelty engine.

Reserve a few unusual compositions for at most one instance of that composition per island: a very long broken viaduct, a quarry enclosing a surviving stone pillar, or a lookout reached through its own fallen tower. Fit each to suitable terrain, and omit it on unsuitable seeds. They should not share one universal “big dungeon with extra rooms” generator. This requires authored content, and expanding the vocabulary remains ongoing work. No finite generator can promise endless novelty.

**Future dungeons should inherit the exterior's identity.** The place above ground should explain why its interior has those spaces. A quarry has work faces, haul passages and stores; a watchpost has stairs, lookout rooms and defensible approaches; a former dwelling has living and storage spaces. The interior and exterior are generated together so a stair, window, cellar door or daylight opening actually leads where it appears to lead.

Start with several deliberately different arrangements, then vary their geometry and optional branches:

| Dungeon arrangement | Main experience | A suitable setting |
| --- | --- | --- |
| Descent with a return loop | See lower spaces early, work out how to reach them, and emerge by another route | A terraced quarry or hillside excavation |
| Court with several approaches | Understand a central space from different positions and choose an entrance | A ruined watch court or clustered former homes |
| Broken main route with a bypass | Recognize the original route, discover an alternate connection, and open or find an easier return | A collapsed crossing or work complex |
| Two intersecting levels | Learn how overhead and lower passages connect through visible openings | A compact vertical ruin or foundations beneath a bridge |

These are spatial arrangements, not room sequences with different skins. Establish the public approach, optional danger boundary, useful branches, encounter spaces and return paths before populating rooms. An optional side chamber should change what the player learns, can reach or chooses to do. Repeating the same fight-and-chest room down every branch would undermine the variety goal.

Possible purposes include retrieving something requested by a nearby resident, reaching a new exit or overlook, inspecting a strange natural chamber, helping a stranded group, or confronting occupants. These are candidate activities, not approved quest or combat mechanics. The outcome should fit the location; every dungeon need not end with a boss and a reward chest.

Design required routes for ordinary embodied movement. Keep optional difficult jumps separate. Creative building and excavation can provide alternate approaches under the current sandbox rules. Do not make these sites depend on indestructible puzzle doors; material limits, ownership and progression locks require their own decisions. Generated connectivity is a starting-world guarantee, not a promise that players cannot later obstruct or dismantle a place.

Water remains a substantial limitation: the current game has static water and no swimming/buoyancy. Early designs can use existing water as scenery with dry routes. Flooding puzzles, underwater passages and changes to water levels are separate future work. Underground and overhanging spaces also need representation beyond the existing heightmap; the render plan must account for their entrances and silhouettes.

**Encounters need a purpose and a physical situation.** Avoid a universal table that picks an enemy group when the player reaches a marker. A future encounter should specify who is there, why this site suits them, what they are doing, what the player can observe before entering, and what can change through interaction.

| Possible encounter | Why this place matters | Meaningful variation |
| --- | --- | --- |
| Travellers sheltering | A dry room, overhang or protected clearing | They may be resting, preparing to leave, or seeking help with an accessible obstruction; they need not offer a task |
| A salvage crew | Real accessible supplies and room to work | Players can observe, trade or help once those interactions exist; the crew's presence changes access and activity |
| Wildlife using a refuge | Suitable habitat and a physically reachable resting space | Presence follows actual populations and movement; the same place can be empty or occupied at different times |
| Hostile occupants | A useful choke point, defensible room or hidden base | Approaches, visibility, retreat and any noncombat option come from the layout; combat rules remain a separate design |

Peaceful and inhabited places should remain common. Entering dangerous depth should be a readable choice: visible occupation, damaged barriers, tracks or other appropriate signs before commitment, plus a retreat route. Some uncertainty is desirable, but making every interesting silhouette predict a fight would narrow exploration again.

Different visits can reveal different occupants or depleted resources while the site's recognizable geography persists. Existing shared salvage and forage already provide limited persistent change. Future encounter outcomes should be shared and saved; do not secretly replace cleared occupants or reconstruct player edits whenever someone approaches. Reoccupation, if later wanted, needs explicit rules and observable causes.

The site should remain worth visiting after its finite supplies have gone. Views, shortcuts, unusual spaces and visible history carry that lasting value. Existing markets can continue to value salvage without turning all exploration into a repeated collection job.

**Keep the future implementation direct.** Retain the four-crate architecture. The core should resolve a compact, deterministic site plan from the seed and terrain: a stable identity, bounded extent, arrangement, constituent spaces/structures, terrain changes, access points and any designated resources. Use a few explicit family generators first. A general grammar language, plugin system or runtime model service is unnecessary for this proposal.

Resolve the plan once and let voxel generation, collision, simplified rendering, map information, inspection and work targets read that same result. A natural formation or multi-level compound needs more than one rectangular building plot with one front entrance. Broad bounds should support indexing, but empty space inside them must not erase unrelated terrain, vegetation, routes or neighboring structures. Carving and placed blocks need explicit occupied regions. Client and server generation must agree before players can join.

Persistent edits and consumed resources remain authoritative overlays. Identity and random choices should not depend on the order chunks load, a mutable random stream, or a site's current position in a vector. Regeneration must preserve the saved interpretation of a consumed supply or fail visibly under an explicit generation/version policy. The current pre-release geometry waiver does not remove persistence validation.

Budget the three scales differently. Most small discoveries stay cheap and nearby; major landmarks can justify longer-range simplified silhouettes. The current [terrain renderer](crates/client/src/terrain.rs) uses a 128 m building-proxy cutoff outside detailed coverage. That would undermine seeing a large destination from an airship or distant slope. A future pass needs sparse, bounded landmark proxies preserving the distinctive opening, profile or broken span through the transition to detailed terrain. A painted footprint alone cannot represent a tall arch or a cave opening.

Generate site data and geometry through bounded work; use the existing spatial indexing and streaming direction. Static discoveries need no simulation tick. Give future inhabitants a separate population and navigation budget rather than attaching active agents to thousands of points. Low graphics must retain the shape, access cues and important openings. Representative hardware measurements are still required; this design establishes no frame-rate or memory result.

**Prove the content before expanding the generator.** If implementation is later requested, use this sequence:

1. Choose three families: stone landforms, abandoned workings and old crossings. Author three substantially different arrangements per family. Produce four terrain-adapted examples of each arrangement: 36 review examples, with normal human scale visible.
2. Compare silhouettes, overhead routes and ordinary Low-graphics views. Remove labels and reduce color in one review sheet: recognizable distinctions should survive. Replace weak arrangements rather than adding more decorative parameters.
3. Compose a walkable review area containing small discoveries, several local sites and one larger focal place. Include a road walk, a wilderness walk and a view from an airship. Judge density, natural placement and repeated sequences together.
4. Review native movement, camera clearance, entrances, return routes and distance transitions. Validate reproducible client/server geometry, terrain clearance, designated resources, save/reload and multiple players where applicable. These checks establish correctness; they do not establish fun.
5. After the surface places work, design two small dungeons with different arrangements and one peaceful/social encounter. Add hostile encounters only with a separately agreed combat direction. Review these before distributing variants across the island.

The principal playtest is recall: after a twenty-minute walk, can a player describe several places by what they saw and how they explored them, and say which one they want to revisit? Also ask whether they were surprised by the next place, whether the destination was visible soon enough to motivate the detour, and whether another example of the same family changed their approach. Counts of generated kinds and successful placements remain useful diagnostics, not evidence of interesting variety.

**Connect these places to visual activities.** The user's subsequent October 8 request asks for nonreading-friendly quests and incidental activities, and explicitly asks to incorporate this POI design. [QUEST_DESIGN.md](QUEST_DESIGN.md#building-activities-into-the-poi-design) maps its eleven proposed activity families onto these six site families and three size tiers. It develops Root Court as a garden-restoration situation, Split Crossing as a retrieval route beneath the broken span, and Quarry Steps as a tool-return activity using the terraces. Each keeps its distinctive geometry and an independent reason to visit after completion.

Choose a compatible activity while laying out a site's spaces, before final clues and props: its invitation needs an approach sightline, its objects need supported positions, and its required route needs an exit. Reuse the shared site plan for geometry, pictures and interaction checks. Small discoveries are usually scenery or occasional clues; local sites can offer one complete activity; larger destinations can combine two or three familiar actions with quiet spaces and optional depth. Not every site is active, and a familiar building family should not guarantee the same puzzle. The combined proposal retains editable terrain, bounded generation, shared persistent outcomes and separate approval for combat or deeper systems. Its details remain design recommendations.

The main tradeoff is more deliberate content design and review in exchange for places with stronger identities. The recommended commitment is a hybrid: a limited set of authored spatial ideas, procedural adaptation to real geography, coherent starting histories, and future encounters rooted in those places. The tier sizes, new families, rare compositions, dungeon arrangements and encounter behavior remain proposals awaiting feedback.
