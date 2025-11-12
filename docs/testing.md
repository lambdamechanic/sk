# Testing & Coverage

This repo enforces a lightweight coverage gate in CI and provides a simple local command to run coverage consistently.

- CI runs `cargo llvm-cov` on Ubuntu and fails the job if line coverage drops below 40%.
- Locally, run `scripts/coverage.sh` (uses the same tooling) and optionally raise the threshold via `THRESHOLD=NN`.
- Prefer property tests for tricky parsing/IO boundaries, and provide fakes for external effects so business logic stays deterministic and covered.

This mirrors the approach used in the adjacent `isura` repo, adapted for this workspace.

## Quickstart

- Run tests: `cargo test`
- Run coverage (40% gate): `./scripts/coverage.sh`
- Raise threshold locally (to experiment): `THRESHOLD=50 ./scripts/coverage.sh`

## CI Behavior

- Coverage runs only on `ubuntu-latest` to keep CI time reasonable across platforms.
- Threshold starts at 40% so the current codebase passes; we will ratchet this upward as we add tests.

## Testing Guidelines

- Unit tests close to the logic, minimal filesystem/network. Prefer capability traits + small fakes.
- Integration tests for end‑to‑end CLI flows (see `tests/doctor.rs`).
- Keep tests fast; avoid sleeps and large fixtures. When needed, use `tempfile` and in-memory constructs.
- Name tests by behavior, not method names.
- New code should include tests; refactors should keep coverage steady or increasing.

## Tooling

- `cargo llvm-cov` requires the `llvm-tools-preview` component. The script installs it if missing.
- If running on macOS, ensure Xcode/Command Line Tools include `llvm-profdata` and `llvm-cov`, or use the Rust component-provided tools.

## Ratcheting Policy

- When coverage rises and stabilizes on `main`, bump the CI threshold by 5% increments in `.github/workflows/ci.yml`.
- Never reduce the threshold on `main` without a clear justification.

See also: `skills/testing/SKILL.md` for broader testing patterns.
