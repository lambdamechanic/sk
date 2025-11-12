Title: sk: file:// install support + `sk precommit` warning (bd sk-083, sk-oaf)

Summary
- Add support for installing skills from local git remotes via `file://` URLs.
- Introduce `sk precommit` to warn/fail when lockfile contains local sources (file://, localhost, or host==local). Bypassable with `--allow-local`.

Context
- Requested to enable local development workflows while avoiding leaking local refs in commits/PRs.
- Tracks bd tasks: sk-083 (file URLs), sk-oaf (precommit warning).

Changes
- src/git.rs: parse_repo_input accepts `file://` (host=local, owner from parent dir, repo from basename w/o .git).
- src/precommit.rs: new pre-commit scanner for skills.lock.json; warns/fails unless `--allow-local`.
- src/cli.rs, src/main.rs: wire `precommit` subcommand.
- tests/install_file_url.rs: integration test for file:// install.
- tests/precommit.rs: integration tests covering fail/pass cases.

How to test
- `cargo test -q` should be green.
- Manual:
  - `sk precommit` in a repo containing local file:// sources in skills.lock.json => fails with clear messaging.
  - `sk precommit --allow-local` => success (warning still printed). 

Notes
- This keeps repo-root clean and aligns with skills/starting-the-task.
- .beads/issues.jsonl included in the commit to sync bd state.

