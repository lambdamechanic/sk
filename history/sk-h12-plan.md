# sk-h12 Simplify `sk sync-back` defaults

## Context
- Current `sk sync-back` requires explicit repo, branch, and skill path flags.
- README quickstart implies defaults that aren't yet implemented.
- bd acceptance criteria emphasize auto-deriving repo/branch/path, plus graceful warnings for missing `rsync` or `gh`.

## Initial Plan
1. Audit existing sync-back CLI flags + config (`src/sync/mod.rs`, `src/config.rs`) to understand current behavior.
2. Design data flow for inferred defaults:
   - Repo: read `default_repo` from config, error if missing.
   - Skill path: derived from install name (probably from lockfile entry) unless `--path` override provided.
   - Branch: auto `sk/sync/<install>/<timestamp>` (UTC, safe characters) unless `--branch` override.
3. Update CLI argument parsing + builder logic to populate defaults and surface helpful errors/warnings.
4. Layer warnings + fallback implementations when `rsync`/`gh` binaries are missing:
   - `rsync` missing → warn and use `cp -R` fallback.
   - `gh` missing → proceed with push but instruct user to open PR manually.
5. Align README/doc examples and tests with new defaults + fallback flows.

## Questions / TODOs
- Where is the canonical place to read config + skill metadata? (likely `state::config` & `skills::` modules.)
- How are branch names currently chosen? confirm before overwriting behavior.
- Need to confirm existing tests for sync-back to extend coverage.

Will refine once code inspection is complete.
