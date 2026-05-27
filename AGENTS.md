## Hard requirements
- You must read **README.md**, **DESIGN.md**, **ARCHITECTURE.md**, **STYLE_GUIDE.md**, and **RUST_STYLE_GUIDE.md** before making changes.
- Do not perform drive-by refactors (renames, formatting sweeps, dependency upgrades) unless explicitly required.
- If requirements are underspecified: make the smallest reasonable assumption and document it in the PR/summary.
- Keep diffs small and readable. Avoid unrelated whitespace changes.
- Use atomic commits that typecheck and pass all checks.

### Refactor gating rule
If your task is blocked by a large refactor that you are not cleared to do:
- Do **not** do the refactor.
- Fail the task and message the Coordinator Agent:
  - specify the required refactor
  - request the Coordinator to create/assign a bead for it first

## Definition of Done (for any coding task)
- `cargo fmt --check` passes
- `cargo clippy` passes (no warnings)
- `cargo nextest run` passes within the global timeout (see `TESTING.md`)
- No policy violations in `STYLE_GUIDE.md` or `RUST_STYLE_GUIDE.md`
