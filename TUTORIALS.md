# First-use tutorials

Implemented in [tutorials.rs](crates/client/src/tutorials.rs). These are small prompts during ordinary play. Opening a mechanic or performing its first action introduces only that mechanic; actual actions advance the lesson. There is no tutorial world, reward, required order, or blocking overlay. The shared world continues running.

| Lesson | First interaction | Try it | Completion |
| --- | --- | --- | --- |
| Your hotbar | Open Inventory | Choose a pictured block, assign a bottom slot, return to play | Close after assignment |
| Build & dig | Attempt an edit | Place a block and remove a block you choose | Both edits confirmed for this player |
| Find your way | Open Map | Zoom or pan, then return; matching town pictures identify destinations | Return after changing the view |
| Cargo & coins | Open Cargo | Review cargo and offers, then close; unlimited building stock is separate | Close the panel |
| A little work | Start accepted work | Stay beside the site while its bar fills; moving away cancels | Confirmed coins or cargo reward |
| Picture parcel | Accept a parcel | Open Map to find its town picture, then deliver at the matching market sign | Return from Map after opening it; paid handover also completes it |
| Carry & match | Take a supply | Carry to the matching outline and Use; Return safely puts it back | Confirmed matching placement |
| Turn & match | Turn a stone | Match the picture above each stone; Show me demonstrates and Hint guides | This player's accepted turn solves the puzzle |
| Whip travel | Open station travel | Choose a destination, board, and Launch when everyone is ready | Authoritative launch |
| Your canopy | Enter personal gliding | Look to steer, then try Brake or Dive; ground contact closes the canopy | Steering followed by brake or dive |
| Creative flight | Turn on Fly | Rise or descend, then turn Fly off | Change height and leave creative flight |

Each prompt contains a short explanation and one next action. Desktop and touch instructions name their actual controls. The existing inventory pictures, town emblems, parcel signs, supply silhouettes, puzzle references, work bar and model demonstrations carry the visual explanation. Show me temporarily takes the activity card's space while its animation plays. The parcel lesson finishes when the player returns from Map; the existing parcel picture then keeps the destination visible during the journey. Skip is always available.

Inventory, map, cargo and station prompts sit inside their panels. Supply and stone prompts use the existing picture card. World prompts avoid the hotbar and touch buttons. In the compact inventory, Search appears after the hotbar lesson is completed or skipped, leaving space for the block pictures and six slots.

Tap/click **Skip**, or press **F3**, to dismiss the visible lesson. **Menu → Teach me again** clears tutorial completion for the current character on this device; the next interaction reintroduces the appropriate lesson. The full Controls reference remains available from Menu/H. Player sessions start with that reference and the inspector closed; observers retain their previous reference.

Completed and skipped lessons save atomically in `tutorials.json` beside client preferences. Names share the same case-insensitive slot across worlds on this device. Other character slots are retained, concurrent writes merge completed lessons, and malformed files are left untouched. Unfinished steps are session-local and start again after reconnecting. Tutorial state sends no network requests and changes no server save or protocol.

Presentation uses the small-card recommendation provisionally; the blocking-overlay alternative remains open for user feedback. Native screenshots and checks are recorded in [tutorial-native-checks.md](artifacts/tutorial-native-checks.md). Family comprehension, physical Android touch behavior and other platform/GPU presentation still require playtesting.
