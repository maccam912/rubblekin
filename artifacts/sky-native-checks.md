# Sky rendering checks — 2026-10-08

Native macOS 26.6.2, Apple M5 Pro, Metal, 1440 × 900. Disposable local GeographyV3/seed42 worlds, observer view, default 48 m detail/128 m trees and preset shadow distances. Existing worlds, player data and production servers were not changed.

- [Day](sky-day.png): Balanced, visible sun, pale horizon/deeper blue above, moving cloud pattern, and sun shadows on buildings/ground. Live F2 switching through High, Low and Balanced preserved the sky; High retained real shadows and Low removed them.
- [Sunset](sky-sunset.png): Balanced, looking west over the sea. Orange sun approaches the horizon, with orange/pink lower sky, violet upper sky and warm clouds. A separate village view showed warm lighting and bounded dynamic shadows; the direction/length relationship is covered by the tests below.
- [Night](sky-night.png): Low, same village direction as the day view. Stars and blue-gray clouds are visible while paths, grass, roofs, buildings, mountains and residents remain legible.

The images stage only sky presentation and camera pose with temporary capture code. Their sky offsets were -120/865/1500 seconds plus continuously advancing local world time; the saved simulation and inhabitants were not fast-forwarded. Temporary controls were removed before final tests/build. No accelerated mode or time override is part of the client. The final ordinary build rendered correctly and returned to the same sky/lighting after an actual leave/rejoin.

Day/night equality, repeatability at old world times, sun/light alignment through overhead, lengthening sunset shadows, continuous color/light transitions, the night illumination floor, shared presentation time, fog matching and asset reuse across rejoin are covered by four client tests. These captures sample phases; they are not a recorded full 40-minute playthrough or a performance benchmark. Windows, Linux, physical Android and representative lower-end hardware remain unverified.


## Shadow stability follow-up

The user reported flickering shadow edges and suggested comparing adjacent frames. Captured 32 consecutive native frames before the fix, 32 after in Balanced, and 32 after in High. All runs use the hardware, isolated seed42 V3 world, 1440 × 900 viewport and default distances above. No sky-time or camera override was used for this follow-up. Logged camera transforms were identical throughout each sequence: position `(2816.25, 210.02, -1913.75)`, rotation `(-0.058452554, 0.22270489, 0.01337835, 0.97304)`. The worlds continued simulating, so moving NPC/airship shadows were excluded from the sampled wall region.

[Adjacent-frame comparison](sky-shadow-comparison.png) shows the same stationary wall crop `(x30..510, y320..450)` before/after. Top pair: application frames 1576/1577, world time about 17.43s. Middle pair: frames 1189/1190, world time about 21.78s. The bottom row multiplies absolute RGB differences by 10; labels count pixels whose maximum RGB-channel difference exceeds 10/255. These are comparisons *within* each sequence, not pixel matching between independently started worlds.

- Before: the sampled wall changed in every one of 31 adjacent pairs, averaging 1,568 pixels above the threshold; the illustrated pair changes 4,175 pixels. The camera did not move, but the light rotated each frame and shadow edges crawled along the wall.
- Balanced after: 30/31 pairs have zero pixels above the threshold. One pair crosses the world-clock second boundary and changes 961 pixels as the light takes its intended small step.
- High after: 30/31 pairs have zero pixels above the threshold; the second-boundary pair changes 7 pixels.

The fix quantizes only directional-light orientation to one-second (0.15-degree) orbital steps derived from the existing shared clock. Bevy already stabilizes cascade translation with texel snapping; continuous light rotation was still moving the projection grid. The visible sun, sky, fog, stars, color and intensity ramps remain continuous. No larger shadow maps, extra cascades or temporal filtering were added. A fifth sky regression covers held directions, small step/error bounds, smooth sky/intensity and old-clock repeatability. Temporary per-frame capture instrumentation was removed; the final ordinary build also rendered the High scene and saved its automatic screenshot successfully.

This verifies stationary-camera edge stability in the sampled native morning scenes, not arbitrary camera motion, a full cycle, other devices or performance. The existing longer-shadow/day/night tests still cover orbital behavior. Source captures and frame/time logs were retained in the disposable QA directory for this session.

Validation after removing capture instrumentation: all 243 client tests plus binary/doc tests pass; the native build, sky-file formatting and whitespace checks pass. Strict all-target Clippy reports 11 `manual_is_multiple_of` warnings in the concurrent block-texture work. The same command passes with that single lint allowed on the command line; the sky source adds no lint allowances. Other hardware and publication remain unverified.
