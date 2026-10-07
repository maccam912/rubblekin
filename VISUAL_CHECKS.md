# Repeatable village and airship checks

Use this small manual route when changing terrain streaming, camera framing, menus or capture. It complements the logic tests; passing those tests alone does not establish visual continuity. The route uses **unedited GeographyV3, seed 42**, a 1440 × 900 desktop window, Balanced, 48 m near detail, 128 m trees and 32 m shadows. Repeat the approach at **512 m near detail**. Record the actual window size, preset and distances if they differ.

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
  --seed 42 --save "$qa_run/world.json" --balanced \
  --screenshot "$qa_run/artifacts/initial.png"
```

The separate working directory isolates `graphics.json`, saves and F12 captures. Use an unused loopback port. On Windows, create an equivalent temporary working directory and run `target\debug\rubblekin.exe` by its full path. The initial screenshot waits for a joined scene and then eight seconds of real time. If an automated run needs `--exit-after`, allow for startup plus this interval and verify the log says the PNG was saved. F12 saves any visible menu or game view immediately. Keep the window visible during native capture.

1. Inspect the starting village: ground, buildings, workers and fields should appear without an empty scene between loading and play. Open and close Inspect, Map and Pause. Try Zoom, Center on you and Whole world; check that movement and edits stay blocked while a panel is open.
2. In Pause, check keyboard distance selection with Tab, Left/Right, Home/End and Reset distances. Change 48 → 512 → 48 m near detail while looking at buildings. The old coverage should stay visible until the replacement is ready. Record any stall separately from missing geometry.
3. Capture a working resident and their inspector, leave the world normally, and reopen this same save. Verify the scene and resident activity resume. Repeat with a retained copy if investigating a failure; preserve the failing original.

## Pinevale → Willowbank journey

This fixed seed has a direct route between **Pinevale (5)** and **Willowbank (9)**. The read-only helper below prints village/port coordinates and routes once, then the scheduled fleet at each simulation time supplied on stdin:

```sh
printf '0\n180\n360\n' | "$qa_repo/target/debug/examples/airships" 42
```

Use the existing local admin console to `teleport X Y Z` to Pinevale's `port_position`, then walk to the Willowbank berth and board physically. If another character occupies the exact point, wait or choose nearby clear space. Keep the pilot dialog open across departure and arrival: the live destination and countdown should agree with the aboard HUD. Pause and other panels do not stop the timetable.

During flight, capture the view ahead, behind and to both sides, with the camera near and far from the avatar. Walk around the deck, then stand near an edge facing Willowbank for the approach. Capture the village before entering the chosen near-detail square, while its placeholders are pending, near 128 m and at the landing. Follow one recognizable roof through the sequence; it must remain present until its detailed replacement appears. Repeat at 48 and 512 m detail. A few still frames cannot prove every transition: use a recording when assessing popping or camera recovery.

The helper uses authoritative `f64` simulation seconds, not wall-clock time or the HUD's rounded minutes. It does not connect to the server or read edits. Its `deck_check_position` is a sampled deck-foot point, not a guaranteed safe live teleport: ships advance between sampling and issuing a command. Normal boarding avoids this timing dependency.

## Evidence to keep

Record the commit, OS/GPU, generation/seed, exact graphics distances, window size, route/direction, simulation time when known, and whether this was a fresh start or reload. Keep the small set of useful screenshots and the relevant log next to those notes. Record actual observations and incomplete checks separately. Screenshots do not measure frame-time distributions, and this route does not establish Android lifecycle or other-device performance.
