# Changelog

## 0.1.0

### Breaking
- The canonical managed skills root changed from `./skills` to `./.agents/skills`.
- Existing repos should be migrated with `sk migrate-root`; legacy `./skills` fallback is temporary compatibility, not the long-term layout.

### Added
- `sk migrate-root` moves legacy repo-local installs into `./.agents/skills` and can repair native Codex/Claude exposure links during the same step.
