# Bugs

## B001 — hidden cursor remains hidden after a kick

Status: fixed locally on 2026-10-10. The join page enforces a visible, ungrabbed cursor every frame, and gameplay controls stop recapturing it after connection failure.

Reproduction reported:

1. Enter gameplay with the mouse captured and cursor hidden.
2. Get kicked or disconnected back to the login/join page.
3. The cursor remains hidden, preventing normal clicking on the login controls.

Expected: returning to the login/join page restores a visible cursor and releases mouse capture so its controls can be clicked.

Verification: the client regression starts with a hidden/locked cursor, simulates failure, verifies visibility and release, and checks that a delayed stale capture is repaired. Native macOS acceptance stopped the disposable TCP server during captured gameplay, returned to the login page, clicked the session choices and Join, and successfully rejoined. Other platforms and physical Android remain unverified.

## B002 — kayak repeatedly snags in rivers

Status: reproduced collision causes fixed locally on 2026-10-10; the user’s exact route remains unconfirmed.

The kayak stops against apparent small walls along a river. Dismounting, walking forward and spawning a new kayak lets the player continue until the next snag. The user confirmed the public server on 2026-10-10; the nearby town/river and travel direction remain unconfirmed. Expected: a navigable river should support continuous paddling without repeated dismounts, while real banks, edited obstructions and other people still block the hull.

Repair: the kayak now has a shallower draft; hull collision uses the same pitch as its visible model; and buoyancy supports the bow/stern across a steepening reach. Three real-terrain fixtures on seed2689504302 previously stopped without forward progress and now clear their bed steps. A separate native-observed bank deadlock now recovers by sculling toward input when the bow cannot turn, as confirmed by the user. Edited river walls, dry banks and other people still block the full hull. Protocol34 requires matching rebuilt clients and servers; Save12 and terrain generation are unchanged.

Verification: the latest full workspace run passed652 checks with one existing ignored launcher smoke, including delayed kayak snapshot replay, a slightly offset launch heading, bank-turn recovery, braking and collision preservation. Workspace all-target Clippy with warnings denied, native build, formatting and whitespace pass. The isolated macOS/Apple M5 Pro, Low, 840×400 touch preview mounted the kayak, inspected its pose/waterline and advanced its interactive lesson. After restoring the separately wedged pose, full upstream touch input turned the bow into the channel and advanced the saved position over1m while remaining mounted. This is not verification of every river or the user’s exact route; physical Android and public deployment remain unverified.

The bounded probe (`cargo run --locked -p rubblekin_core --example kayak_probe -- 2689504302`) prints stopped poses and their blockers. Its straight-heading paths can meet legitimate dry banks at bends; a reported stop is an investigation point, not automatically a defect.
