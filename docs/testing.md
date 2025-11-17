# Testing & Coverage

This repo enforces a lightweight coverage gate in CI and provides a simple local command to run coverage consistently.

- CI runs `cargo llvm-cov` on Ubuntu and fails the job if line coverage drops below 45%.
- Locally, run `scripts/coverage.sh` (uses the same tooling) and optionally raise the threshold via `THRESHOLD=NN`.
- Prefer property tests for tricky parsing/IO boundaries, and provide fakes for external effects so business logic stays deterministic and covered.

This mirrors the approach used in the adjacent `isura` repo, adapted for this workspace.

## Quickstart

- Run tests: `cargo test`
- Run coverage (default 40% gate): `./scripts/coverage.sh`
- Raise threshold locally (to experiment): `THRESHOLD=50 ./scripts/coverage.sh`

## CI Behavior

- Coverage runs only on `ubuntu-latest` to keep CI time reasonable across platforms.
- Threshold is currently 45% in CI; we ratchet this upward as we add tests.

## Examples

- Front‑matter parsing: see `tests/skills_frontmatter.rs` for validating `SKILL.md` YAML front‑matter via `skills::parse_frontmatter_file`.
- Repo skill discovery: see `tests/skills_list.rs` for creating a temporary git repo (with `git -C <dir> ...`) and asserting `skills::list_skills_in_repo` finds multiple skills.
- Path utilities and cache override: see `tests/paths_cache.rs` for using `tempfile` and `SK_CACHE_DIR` to drive `paths::cache_root` deterministically, and for `resolve_project_path` absolute/relative behavior.

## Integration Fixtures

End-to-end CLI tests share a reusable harness under `tests/support/mod.rs`:

- `CliFixture` bootstraps a temp git project, a cache override (`SK_CACHE_DIR`), and a throwaway config directory via the new `SK_CONFIG_DIR` override.
- `RemoteRepo` wraps local bare remotes plus worktrees so tests can push upgrades (`overwrite_file`) without reimplementing git plumbing.
- Helper utilities expose common assertions such as `parse_status_entries` for `sk doctor --status --json`.

Example:

```rust
#[path = "support/mod.rs"]
mod support;

use support::{CliFixture, parse_status_entries};

#[test]
fn lifecycle_flow() {
    let fx = CliFixture::new();
    fx.sk_success(&["init"]);
    let remote = fx.create_remote("repo", "skills/demo", "demo");
    fx.install_from_remote(&remote, "demo");

    let status = parse_status_entries(fx.run_json(&["doctor", "--status", "--json"]));
    assert_eq!(status[0].state, "clean");
}
```

This fixture keeps every test hermetic (no writes to the developer’s real `~/.config/sk` or cache) and exercises the exact CLI binaries that CI runs.

## Testing Guidelines

- Unit tests close to the logic, minimal filesystem/network. Prefer capability traits + small fakes.
- Integration tests for end‑to‑end CLI flows (see `tests/doctor.rs`).
- Keep tests fast; avoid sleeps and large fixtures. When needed, use `tempfile` and in-memory constructs.
- Name tests by behavior, not method names.
- New code should include tests; refactors should keep coverage steady or increasing.

## Tooling

- `cargo llvm-cov` requires the `llvm-tools-preview` component. The script installs it if missing.
- If running on macOS, ensure Xcode/Command Line Tools include `llvm-profdata` and `llvm-cov`, or use the Rust component-provided tools.
- `SK_CONFIG_DIR` mirrors `SK_CACHE_DIR`: point it at a temp directory to keep test config files away from your real workstation settings.

## Ratcheting Policy

- When coverage rises and stabilizes on `main`, bump the CI threshold by 5% increments in `.github/workflows/ci.yml`.
- Never reduce the threshold on `main` without a clear justification.
- Every PR already runs the linux formatter/clippy gate plus macOS and Windows build+test jobs; match that matrix locally if you’re debugging platform-specific failures.

See also: `skills/testing/SKILL.md` for broader testing patterns.
