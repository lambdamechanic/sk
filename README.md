# sk — repo-scoped Claude Skills bridge for any agent

`sk` keeps Claude Skills vendored inside your Git repository so Codex, bespoke LLM runners, and CI can all reuse the same helpers. 

## Why sk?
- Vendored helpers travel with your git history, so reviewers and automation see the exact bits you edited.
- Works for any agent runtime—if it can run `sk`, it can share your Claude Skills.
- Publishing edits is just another PR via `sk sync-back`, so nothing drifts out of band.

## Quickstart: install → cache Anthropic → publish your own

### 0. Install `sk`
```bash
cargo install sk
```

### 1. Initialize inside your repo
```bash
cd /path/to/your/git/repo
sk init
```

### 2. Add the Anthropic catalog
```bash
sk repo add @anthropics/skills --alias anthropic
sk repo list
```
Example `sk repo list` output:
```
ALIAS       REPO                                SKILLS  INSTALLED
anthropic   github.com/anthropics/skills        120     3
```

### 3. Install a few skills
```bash
sk install @anthropics/skills template-skill --alias template
sk install @anthropics/skills frontend-design
sk install @anthropics/skills artifacts-builder
sk list
```

### 4. Create a repo for skills you author
```bash
gh repo create your-gh-username/skills --private --clone
sk config set default_repo @your-gh-username/skills
```

### 5. Scaffold a new skill
```bash
sk template create retro-template "Retro two-column recap template"
```

### 6. Inspect and publish changes
```bash
sk doctor
sk sync-back frontend-design -m "Revise guidance tone"
```

### 7. Stay up to date
```bash
sk upgrade frontend-design
```
Use `sk upgrade --all` when you want every installed skill to follow its upstream tip.

## Need the gory details?
Implementation notes, machine-readable catalog output, cache layouts, building from source, and the full command cheat sheet now live in `GORYDETAILS.md`.
