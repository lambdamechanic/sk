Title: sk: sync-back — branch/worktree; rsync/copy; commit/push; PR hint (bd sk-6cd)

- Implement `sk sync-back <installed-name>`:
  - Create a branch in the cache at the locked commit via `git worktree add -b <branch> <worktree> <locked-commit>`.
  - Mirror the local edited skill dir into the cached repo at `skillPath` using `rsync -a --delete` (fallback to copy).
  - Commit with timestamped default message or `--message`, and `git push -u origin <branch>`.
  - Print a ready-to-run `gh pr create --fill --head <branch>` hint.
- Fails clearly when installed dir is missing or locked commit is absent (suggests `sk doctor --apply` / `sk update`).
- No changes to lockfile semantics; cache-only behavior elsewhere unchanged.

bd issue: sk-6cd — Branch/worktree in cache; rsync; commit/push; PR hint (under EPIC: Sync-back).

Manual validation:
- Edited a skill under `skills/<name>`, ran `sk sync-back <name>` without flags to generate a timestamped branch; observed push success message.
- Verified fallback copy path works when `rsync` is unavailable.
