# Open bugs

## B001 — hidden cursor remains hidden after a kick

Status: open. Reported by the user on 2026-10-10; not reproduced or fixed in this follow-up.

Reproduction reported:

1. Enter gameplay with the mouse captured and cursor hidden.
2. Get kicked or disconnected back to the login/join page.
3. The cursor remains hidden, preventing normal clicking on the login controls.

Expected: returning to the login/join page restores a visible cursor and releases mouse capture so its controls can be clicked.

Acceptance check for a future fix: reproduce an unexpected disconnect while captured, click the login controls afterward, and confirm mouse capture still works when joining again.
