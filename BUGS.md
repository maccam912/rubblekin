# Bugs

## B001 — hidden cursor remains hidden after a kick

Status: fixed locally on 2026-10-10. The join page enforces a visible, ungrabbed cursor every frame, and gameplay controls stop recapturing it after connection failure.

Reproduction reported:

1. Enter gameplay with the mouse captured and cursor hidden.
2. Get kicked or disconnected back to the login/join page.
3. The cursor remains hidden, preventing normal clicking on the login controls.

Expected: returning to the login/join page restores a visible cursor and releases mouse capture so its controls can be clicked.

Verification: the client regression starts with a hidden/locked cursor, simulates failure, verifies visibility and release, and checks that a delayed stale capture is repaired. Native macOS acceptance stopped the disposable TCP server during captured gameplay, returned to the login page, clicked the session choices and Join, and successfully rejoined. Other platforms and physical Android remain unverified.
