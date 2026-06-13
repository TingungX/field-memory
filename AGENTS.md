# AGENTS.md — DSE-Memory

Project-local agent instructions. Complements (does not replace) any parent agent session configuration.

## Always applicable

1. **Communicate in Chinese.** Preserve code, commands, errors in original language.
2. **Prefer simple, direct, maintainable solutions.** Avoid over-engineering.
3. **Stay consistent with existing code style.** Don't refactor unrelated code.
4. **Never hardcode** secrets, tokens, or private URLs.
5. **Never commit non-Rust build artifacts** — `/target`, `*.sled/`, `Cargo.lock` (currently in `.gitignore` for the workspace root, but `Cargo.lock` is intentionally **committed** because the workspace ships binaries).

## Project structure

- **Workspace root**: `Cargo.toml`, two members under `crates/`
- **`crates/dse-core/`**: library crate, all engine logic in `src/*.rs` (one module per concern: `types`, `math`, `embed`, `physics`, `cycle`, `recall`, `init`, `paradigm`, `ecg`, `engine`, `persist`)
- **`crates/dse-cli/`**: binary crate, end-to-end demo driver
- **`docs/design.md`**: v2 architecture rationale (read this before proposing non-trivial changes)
- **`docs/superpowers/plans/`**: TDD plans with checkbox tracking; each plan file is the authoritative spec for its phase

## Engineering conventions

- **5 config params only** — `DseCoreParams` is a frozen surface, not a config dump. New params require a plan update.
- **One file, one concern** — each `src/*.rs` has exactly one responsibility. Don't bleed physics into `cycle.rs` or recall into `engine.rs`.
- **No business logic in `engine.rs`** — `DseEngine` is an orchestrator only. Algorithm changes belong in `physics.rs`, `cycle.rs`, etc.
- **`pub use` re-exports in `lib.rs`** are intentional public surface. Don't change them without updating downstream callers.

## Testing conventions

- **Inline `#[cfg(test)] mod tests`** in the same file as the implementation. No separate `tests/` files for unit tests.
- **`tests/integration.rs`** for end-to-end multi-module scenarios.
- **Test names**: `test_<function_or_behavior>_<expected_outcome>`. Use Chinese-language test data when the production code is Chinese-aware (e.g., `init_anchors` label extraction).
- **Plan tests that depend on the hash-based `DummyEmbedProvider`** are inherently flaky (see "Lessons learned" below). Two such tests are currently marked `#[ignore]`. Do not "fix" them by changing the math; either inject a real `EmbedProvider` or accept them as plan-level design issues.

## Output conventions

- First **state what was done**, then **explain why**.
- List modified files explicitly.
- For non-trivial changes, also list **plan-level impacts** (which `DseCoreParams` are touched, which public API changes, which persistence format version).

## Engineering preferences

- **No magic numbers** in production code — extract constants or add named fields to `DseCoreParams`.
- **Reuse `crate::math::*` helpers** rather than reimplementing vector ops in each module.
- **Snapshot before mutating** when you need to call a function that borrows immutably while iterating mutably. The `RelaxationCycle::run` pattern in `cycle.rs` is the reference implementation.

## Debug / fix conventions

- **Root cause first.** Don't suppress panics with `unwrap_or_default` or comment out code paths.
- **Plan vs. code drift.** If you discover the plan file (`docs/superpowers/plans/*.md`) has an internal contradiction, **fix the plan first**, then commit the plan fix in its own commit before implementing. This keeps `git log` honest about what the plan was vs. what the code is.
- **Mark unverifiable tests `#[ignore]` with a comment** explaining *why*. Don't delete them — they document plan intent.

## Lessons learned (do not repeat)

These were paid for in real debugging time during Phase 1. Read them before changing similar code.

1. **`lib.rs` re-exports must not reference symbols from sibling modules that don't yet exist** — the workspace scaffold (`Task 1`) writes re-exports like `pub use engine::DseEngine;`, but if those targets don't exist yet, `cargo check` fails. Either forward-declare or postpone re-exports to the task that introduces the target.
2. **`bincode::Error` in 1.x is `Box<bincode::ErrorKind>`, not `bincode::Error`.** If you write a `PersistError` with `#[from] bincode::Error`, the build fails. Map to a `Serialization(String)` variant manually.
3. **`sled::Error::Unsupported(...)` API does not take a string argument.** Use a custom `MissingData(&'static str)` variant instead.
4. **`use` lists in `cycle.rs` / `paradigm.rs` / `recall.rs` must include every type referenced**, including those only used in fn signatures or full-path call sites. The borrow checker + dead-code pass will catch some, but not all.
5. **Two `detect_paradigm_shifts` bugs caught at the plan level**: (a) let-bound `n` reused after `Vec::remove` causes index-out-of-bounds panic; (b) `break` only exits the inner loop, violating the "one shift per cycle" comment. Use `return` (not `break`) after `remove(j)`.
6. **`DummyEmbedProvider` is hash-based, not semantic.** Two semantically related Chinese strings ("Rust 编程语言" vs "Rust 的类型系统真强大") produce near-orthogonal vectors with high probability. Any test that asserts density growth from "more mentions of a topic" needs a real `EmbedProvider` to be deterministic.
7. **`orthogonal event + orthogonal anchor` cannot satisfy pull-style tests.** `cos_sim(orthogonal) = 0`, so `impact = 0`, so `effective_direction` adds zero contribution. Plan tests that assert "anchor pulls event toward it" with orthogonal directions are mathematically unsatisfiable.

## When in doubt

1. Read `docs/design.md` to understand the *why* before touching the *what*.
2. Read the most recent plan in `docs/superpowers/plans/` for task-level context.
3. Read at least 3 sibling files in `crates/dse-core/src/` to understand the local style.
4. If a change touches public API or persistence format, update the plan AND the design doc in the same commit (or in two coordinated commits).

