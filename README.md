# sk — repo-scoped Claude Skills bridge for any agent

`sk` keeps Claude Skills vendored inside your Git repository so Codex, bespoke LLM runners, and CI can all reuse the same helpers. Install it once, and every skill install, upgrade, and publish happens under version control in `./skills`.

## Why sk?
- Vendored helpers travel with your git history, so reviewers and automation see the exact bits you edited.
- Works for any agent runtime—if it can run `sk`, it can share your Claude Skills.
- Publishing edits is just another PR via `sk sync-back`, so nothing drifts out of band.

## Quickstart: install → cache Anthropic → publish your own
`sk` is published on crates.io. Start inside the repo that already hosts your agent or automations.

### 0. Install `sk` (one time)
```bash
cargo install sk
```

### 1. Initialize inside your repo
```bash
cd /path/to/your/git/repo
sk init
```
This creates `./skills` plus `skills.lock.json`. Stay with that default root unless you explicitly change it later via `sk config set default_root <dir>`.

### 2. Cache the Anthropic catalog once
```bash
sk repo add @anthropics/skills --alias anthropic
sk repo list
```
Example `sk repo list` output:
```
ALIAS       REPO                                BRANCH
anthropic   github.com/anthropics/skills        main
```
Caching writes to `skills.repos.json` so teammates inherit the catalog.

### 3. Install a few canonical skills
```bash
sk install @anthropics/skills template-skill --alias template
sk install @anthropics/skills frontend-design
sk install @anthropics/skills artifacts-builder
sk list
```

### 4. Create a repo for skills you author
```bash
gh repo create your-gh-username/claude-skills --private --clone
sk config set default_repo @your-gh-username/claude-skills
```
`default_repo` is where `sk sync-back` pushes brand-new skills the first time you publish them.

### 5. Scaffold a new skill
```bash
sk template create retro-template "Retro two-column recap template"
```
`sk template` copies the canonical template into `./skills/<name>`, rewrites metadata, and leaves prompts/tests ready to edit. Run `sk doctor retro-template` while you iterate.

### 6. Inspect and publish changes
```bash
sk doctor
sk sync-back frontend-design -m "Revise guidance tone"
```
`sk sync-back` mirrors `skills/<name>` into a clean cached clone, pushes to the repo noted in `skills.lock.json` (or `default_repo` for new installs), and opens a PR via `gh`. Missing `rsync` or `gh` only triggers warnings—the push still completes.

### 7. Stay up to date
```bash
sk upgrade frontend-design
```
Use `sk upgrade --all` when you want every installed skill to follow its upstream tip.

## Need the gory details?
Implementation notes, machine-readable catalog output, cache layouts, building from source, and the full command cheat sheet now live in `GORYDETAILS.md`.
