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

The language comes from `TRAINSIM_LANG`, otherwise from the operating system,
otherwise English; both editors switch it at runtime under View → Language.
