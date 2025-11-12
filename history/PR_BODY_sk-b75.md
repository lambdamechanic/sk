Title: Doctor: prune unreferenced cache + duplicate lock checks (bd sk-b75)

Summary
- Implements initial `sk doctor` repairs:
  - Detect duplicate `installName` entries.
  - Detect and optionally prune unreferenced cache clones under `~/.cache/sk/repos`.
- Adds user‑facing output and safe cleanup of empty parent folders after prune.

Context
- bd issue: sk-b75 — Detect & optionally repair; prune cache (under EPIC: Doctor).
- Scope per history/MISSION_SK.md: analyze cache state, prune unreferenced, and support `--apply` to repair.

Testing
- `cargo test` passes on this branch.
- Manual: create stray cache dirs under the cache root and run `sk doctor --apply`.

Next
- Implement orphan lock entry removal + normalization.
- Add tests for duplicate detection and prune behavior.
