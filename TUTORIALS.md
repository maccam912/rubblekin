# First-use tutorials

Implemented in [tutorials.rs](crates/client/src/tutorials.rs). These are small prompts during ordinary play. Opening a mechanic or performing its first action introduces only that mechanic; actual actions advance the lesson. There is no tutorial world, reward, required order, or blocking overlay. The shared world continues running.

| Lesson | First interaction | Try it | Completion |
| --- | --- | --- | --- |
| Your hotbar | Open Inventory | Choose a pictured block, assign a bottom slot, return to play | Close after assignment |
| Build & dig | Attempt an edit | Place a block and remove a block you choose | Both direct edit requests confirmed for this player |
| Build a box | Enter Building selection or Shift-click a corner | Choose two corners, review the outline, choose a hotbar block and Fill | This player's server-confirmed fill; ordinary Build & dig progress stays separate |
| Find your way | Open Map | Zoom or pan, then return; matching town pictures identify destinations | Return after changing the view |
| Cargo & coins | Open Cargo | Review cargo and offers, then close; unlimited building stock is separate | Close the panel |
| A little work | Start accepted work with T/touch Work at its pictured site, or in Cargo | Stay beside the site while its bar fills; moving away cancels | Confirmed coins or cargo reward |
| Picture parcel | Accept a parcel | Open Map to find its town picture, then deliver at the matching market sign | Return from Map after opening it; paid handover also completes it |
| Carry & match | Take a supply | Carry to the matching outline and T/touch Place; Return safely puts it back | Confirmed matching placement |
| Turn & match | Turn a stone | Find its matching dotted reference along the route; Show me demonstrates and Hint guides | This player's accepted turn solves the puzzle |
| Follow the water | Turn a garden channel | Connect its open ends and watch where water stops; Hint finds the break and Show me demonstrates a turn | This player’s accepted turn waters the garden |
| Whip travel | Open station travel | Choose a destination, board, and Launch when everyone is ready | Authoritative launch |
| Your canopy | Enter personal gliding | Look to steer, then try Brake or Dive; ground contact closes the canopy | Steering followed by brake or dive |
| Creative flight | Turn on Fly | Rise or descend, then turn Fly off | Change height and leave creative flight |
| Your bike | Spawn or restore a bike | Steer and pedal, then try Space/touch Brake; hills add speed | Active movement followed by braking |
| Your kayak | Spawn or restore a kayak | Steer and paddle, then try Brake; releasing controls drifts with the current | Active movement followed by braking |
| Your sailboat | Spawn or restore a sailboat | Steer using the wind ribbon, zigzag upwind, then try Brake; automatic trim handles the sail | Active movement followed by braking |

Touch effort controls name **Pedal hard** and **Paddle hard** and are held alongside the movement stick. Sailboats and seated carriage trips hide the unused effort button. Carriage **Leave** steps off while waiting; **Glide** opens the canopy after launch. Canopy controls remain Brake/Dive. A finger holding a button that becomes unavailable is consumed until it lifts, preventing accidental camera movement or later reactivation.

Each prompt contains a short explanation and one next action. Desktop and touch instructions name their actual controls. The existing inventory pictures, town emblems, parcel signs, supply silhouettes, puzzle references, work bar and model demonstrations carry the visual explanation. Show me temporarily takes the activity card's space while its animation plays. The parcel lesson finishes when the player returns from Map; the existing parcel picture then keeps the destination visible during the journey. Skip is always available.

Inventory, map, cargo and station prompts sit inside their panels. Supply, stone and flow-garden prompts sit inside their picture card on desktop; touch uses the wide world strip above the separate pictures. World prompts avoid the hotbar and touch buttons. The box lesson sits below its live selection hint; mounted lessons follow the actual vehicle kind. On touch, ride, carriage, canopy and creative-flight lessons use a wide strip below the header, leaving Cargo, the rider and ride status clear. Transient server messages and connection waits take priority there and in the touch activity strip; the same lesson step resumes afterward. Station guidance inside the travel panel remains visible. Clearing a selection restores its next corner step. Passive wind/current drift and controls used in menus cannot complete a vehicle lesson. The paid-work picture and progress bar move below the first-use world lesson while it is visible. A confirmed payment then shows its actual coin or cargo picture; resource pictures also identify the buy/sell exchange in Cargo. In the compact inventory, Search appears after the hotbar lesson is completed or skipped, leaving space for the block pictures and six slots.

The map's Activities view shows three nearby choices from the four implemented scene types—supplies, stones, cart repairs and flow gardens—with their object pictures and real shared progress. Unfinished scenes come first, with distance ordering within each group. Choosing a picture centers the map on its start. Towns restores the settlement list; this navigation does not accept an expedition or award a postcard.

Tap/click **Skip**, or press **F3**, to dismiss the visible lesson. **Menu → Teach me again** clears tutorial completion for the current character on this device; the next interaction reintroduces the appropriate lesson. The full Controls reference remains available from Menu/H. Player sessions start with that reference and the inspector closed; observers retain their previous reference.

Completed and skipped lessons save atomically in `tutorials.json` beside client preferences. Names share the same case-insensitive slot across worlds on this device. Other character slots are retained, concurrent writes merge completed lessons, and malformed files are left untouched. Unfinished steps are session-local and start again after reconnecting. Tutorial state sends no network requests and changes no server save or protocol.

Presentation uses the small-card recommendation provisionally; the blocking-overlay alternative remains open for user feedback. Native screenshots and checks are recorded in [tutorial-native-checks.md](artifacts/tutorial-native-checks.md). Family comprehension, physical Android touch behavior and other platform/GPU presentation still require playtesting.
