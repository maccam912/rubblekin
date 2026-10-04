# Working on Rubblekin

Read `DESIGN.md` before proposing or implementing changes. It is the project handoff for new conversations and records the vision, decision status, work completed, and unresolved challenges.

## Collaborating with the user

- Keep the user involved in consequential gameplay, visual, and architectural decisions. Present a concrete recommendation and its tradeoffs, then ask for feedback before building around an unresolved choice.
- Continue independent, reversible work while a design question is pending. Routine implementation details do not require repeated permission.
- Treat explicit user decisions as confirmed. Keep assistant proposals and experiments labeled as such; silence is not acceptance.
- Favor progressive disclosure of complexity for both players and developers. Start with understandable behavior and add complexity when there is an actual need.
- Apply YAGNI. Do not build speculative frameworks, future model integrations, or distributed infrastructure merely because they might become useful.

## Engineering direction

- Simple features should have short, traceable implementation paths, especially across the client and server. Avoid unnecessary abstraction and conversion layers.
- Use clear ownership, explicit data, and small boundaries. Simplicity does not excuse skipping validation, persistence correctness, or useful diagnostics.
- The preferred stack is Rust with Bevy and its wgpu renderer, with a headless multiplayer server deployable as a container on the user's Kubernetes cluster. Exact libraries and architecture remain subject to the decisions in `DESIGN.md`.
- Integrated graphics on Linux, Windows, and macOS are core targets. Higher graphics settings must be optional. Do not claim hardware compatibility or performance without testing.
- LLMs are allowed as development tools. Gameplay LLMs and decision models are possible later features, not initial requirements.

## Maintaining the handoff

Update `DESIGN.md` as meaningful work happens:

- Keep the current state and next steps accurate.
- Record confirmed decisions with their rationale and date.
- Record challenges, alternatives considered, and the resolution or remaining question.
- Add concise work log entries with actual changes and verification results, including limitations.
- When a decision changes, update the current guidance and retain a short explanation of what superseded it. Do not leave contradictory instructions active.

Keep this record useful and concise. Do not copy entire conversations or describe planned features as implemented. Split detailed technical notes into linked files only when they earn their place.
