# Code Style & Language

All code, comments, and documentation must be in **English**.

- Variable names, function names, class names: English
- Comments and docstrings: English  
- README, docs, specifications: English
- Commit messages: English (standard in collaborative projects)

This applies consistently across the entire codebase, including but not limited to:
- Source code files
- Configuration files  
- Documentation files
- Commit messages

After code changes check if you need to update existing md files.

**Rationale:** Ensures consistency, improves maintainability, and aligns with standard software development practices.

# Localisation

**No user-visible text as a literal in the code.** Every string the player or the
editor user reads goes through `i18n`, and both languages ship with the change:

- Add the key to `crates/i18n/locales/en/main.ftl` **and**
  `crates/i18n/locales/de/main.ftl` in the same commit — `cargo test -p i18n`
  fails on a key that only one of them has.
- Read it back with `i18n::t!("key")`, or `t!("key", file = path)` for
  placeholders. Numbers are formatted in Rust (`format!("{v:.1}")`) and passed
  in as text, so the column layout of the HUD survives.
- Key naming: flat kebab-case, prefixed by area (`veh-`, `brk-`, `drv-`, `hud-`,
  `status-`, `action-`). A tooltip is the field's key plus `-hint`; `row()` in
  the vehicle editor picks it up by itself.
- The `.ftl` files are the Crowdin source (`crowdin.yml`); `de` is a translation
  like any other. Keep the section comments — Crowdin shows them as context.

What stays a literal: log output (`info!`, `warn!`), panic messages, test
assertions, and type designations that are names rather than prose (`KE-GPR`,
`PZB 90 V2.0`, `LOD0`).

The language comes from `TRAINSIM_LANG`, otherwise from the choice made under
View → Language (editors) or Settings → Language (simulator), otherwise from the
operating system, otherwise English. The editors switch it at runtime; the
vehicle and route editors remember the choice in their own `settings.rs`, the
simulator in the settings file of `crates/app/src/settings.rs` (Bevy's
`bevy::settings`). The signal editor does not — `i18n::set_language` sets an
in-memory value, so a menu that calls it and nothing else throws the choice
away at the next start.

# Multiplayer

The simulator runs single player and against a dedicated server out of the same binary
(`crates/app/src/net.rs`, plan ch. 20). **Every new feature has to work in both.** That is
not a porting step afterwards — it decides how the feature is built:

- **State that matters to other players belongs in `sim-core`,** where the fixed 200 Hz step
  makes it deterministic, and it is driven by values a client can send. Anything living only
  in an ECS component or a Bevy resource exists on one machine.
- **Replicate the setpoint, not the result.** A new control goes into `CabInputs` — that
  struct is what travels, so a lever added there is networked with no further work. Never
  replicate what the setpoint already implies (a position, a pressure, a lamp).
- **Never replicate a `Transform`.** Positions travel as `(edge, s, dir, v, a)` on the track
  graph; the pose is rebuilt from the spline locally.
- **Never correct by setting a value.** A difference to the server is worked off gently
  through the speed (`Train::nudge`, `client_correct`); setting a position is what rubber
  banding is.
- **Ask who owns it.** The server owns the interlocking, the AI drivers and the scenario; a
  client owns nothing but its own levers. A feature that writes to the world from the client
  needs a message to the server instead, and the server has to be able to say no.
- **Watch the frequency.** Anything sent per frame per train has to survive a hundred trains
  on a line hundreds of kilometres long. Send on the change, and let interest management
  drop what is far away.

If a feature genuinely cannot work over the network, say so in the commit and gate it — a
feature that silently only works in single player is a bug report waiting to happen.
