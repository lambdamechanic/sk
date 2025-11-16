## Skills MCP Policy

> **Skills bootstrap checklist** — At the top of every session, call the repo-scoped skills MCP once to discover local helpers. Run `skills_list` to see available skils and skim the list before writing a plan. Reference any relevant skills in your response. 

- Always start with `skills_list` to confirm whether any vendored skill applies to the task at hand.
- `skills_show` returns the entire SKILL body for a known `name`.
- Cite whichever skills you consulted in your final response so reviewers can trace guidance back to the MCP output.

## Issue Tracking with bd (beads)

All work in this repository must be tracked as bd issues (bugs, features, tasks, etc.). Before touching the tracker, run `skills_list` and open the canonical instructions with `skills_show bd-workflow`; that skill covers ready work, claiming tasks, MCP usage, auto-sync, and closing the loop with `.beads/issues.jsonl`. Always follow it so bd remains the single source of truth for every piece of work.

### Managing AI-Generated Planning Documents

AI assistants often create planning and design documents during development:
- PLAN.md, IMPLEMENTATION.md, ARCHITECTURE.md
- DESIGN.md, CODEBASE_SUMMARY.md, INTEGRATION_PLAN.md
- TESTING_GUIDE.md, TECHNICAL_DESIGN.md, and similar files

**Best Practice: Use a dedicated directory for these ephemeral files**

**Recommended approach:**
- Create a `history/` directory in the project root
- Store ALL AI-generated planning/design docs in `history/`
- Keep the repository root clean and focused on permanent project files
- Only access `history/` when explicitly asked to review past planning

**Example .gitignore entry (optional):**
```
# AI planning documents (ephemeral)
history/
```

**Benefits:**
- ✅ Clean repository root
- ✅ Clear separation between ephemeral and permanent documentation
- ✅ Easy to exclude from version control if desired
- ✅ Preserves planning history for archeological research
- ✅ Reduces noise when browsing the project

### Important Rules

- ✅ Track every piece of work in bd and follow the `bd-workflow` skill whenever you create, claim, or close tasks.
- ✅ Store AI planning docs in the `history/` directory.
- ❌ Use markdown TODO lists, external trackers, or other systems instead of bd.
- ❌ Clutter the repository root with planning documents.

For more details, see README.md and QUICKSTART.md.
