# MCP Skills Server Contract (sk-cqm / sk-xtt)

## Goals
- Give repo-scoped agents a stdio MCP server they can point at the local checkout so they can list/search vendored skills without crawling the tree manually.
- Match the MCP lifecycle so Claude (and other MCP clients) can auto-discover the tools and expose them in the Skill Tool surface.
- Provide structured responses (JSON) plus human-readable fallbacks so both automated tooling and humans can inspect the results quickly.

## Lifecycle + Capabilities
- The server runs via `sk mcp-server [--root ./skills]` and speaks JSON-RPC 2.0 over stdio.
- On launch it waits for the MCP client to send `initialize`; the server replies with:
  - `protocolVersion`: `2025-03-26` (latest spec with stable lifecycle docs).citeturn0search0
  - `capabilities.tools.listChanged: false` (we rebuild the tool list on demand, but do not push notifications yet).citeturn0search3
  - `serverInfo`: `{ name: "sk-mcp", version: <crate version> }`.
  - `instructions`: short text telling the client to prefer `skills.list` for exhaustive metadata and `skills.search` for targeted lookups.
- After we respond, the client will send `notifications/initialized`; we acknowledge but do not emit a response, as required by MCP lifecycle rules.citeturn0search0

## Tool Surface
We expose two MCP tools so Claude can mirror the current workflow of manually hunting for skills:

### 1. `skills.list`
- **Purpose**: enumerate every `SKILL.md` under the skills root, return metadata + optional body text so the Skill Tool can show available skills.
- **Input schema**:
  ```json
  {
    "type": "object",
    "properties": {
      "query": {
        "type": "string",
        "description": "Optional substring filter applied to install name, meta.name, or description"
      },
      "includeBody": {
        "type": "boolean",
        "description": "Include the SKILL.md body (minus YAML front-matter) in the structured payload"
      }
    },
    "additionalProperties": false
  }
  ```
- **Output**: structured object `{ skills: SkillSummary[] }` where each entry contains:
  - `installName`: directory name under `./skills`
  - `meta.name` & `meta.description`
  - `skillPath`: relative path to the directory
  - `skillFile`: path to the SKILL.md file
  - `body` (optional) when `includeBody` is true
- We also emit a text blob summarizing how many skills matched so the host UI has something human-friendly.

### 2. `skills.search`
- **Purpose**: keyword search across install name, metadata, and SKILL.md body so Claude can ask “find a skill mentioning bd ready” etc.
- **Input schema**:
  ```json
  {
    "type": "object",
    "properties": {
      "query": {
        "type": "string",
        "description": "Keywords to match (all whitespace-separated tokens must be present)",
        "minLength": 1
      },
      "limit": {
        "type": "integer",
        "description": "Maximum number of hits",
        "minimum": 1,
        "maximum": 25,
        "default": 10
      }
    },
    "required": ["query"],
    "additionalProperties": false
  }
  ```
- **Output**: `{ query, limit, total, results: SearchHit[] }` where each hit includes the same metadata as `skills.list` plus:
  - `score`: number of matched tokens
  - `excerpt`: ~160-character window around the first match so the agent can judge relevance before loading the full skill doc.

## Data & Implementation Notes
- Skills are discovered by walking `<project-root>/<root>/` (default `./skills`). We treat every `SKILL.md` (any depth) as a skill entry. If parsing a front-matter fails we log to stderr and skip it instead of crashing the MCP session.
- We reuse the existing `skills::parse_skill_frontmatter_str` helper to keep YAML/kv parsing logic in one place.
- Search tokenization: lowercase the query, split on ASCII whitespace, and require each token to appear in the concatenated string of install name + meta + skill body. This keeps scoring deterministic and avoids “best effort” hallucinations.
- Because clients can re-use the same process during long editing sessions, `skills.list` and `skills.search` both rescan the filesystem per call so the output always reflects the latest SKILL.md state without needing a custom invalidate command.

## Testing Strategy
- Unit-test the pure helpers:
  - Scan fixture directories with nested SKILL.md files and assert we return the right `SkillSummary` structs.
  - Search scoring/excerpt logic to ensure multi-token queries behave as documented.
- Integration smoke test: run `sk mcp-server` inside `cargo test` with a synthetic stdin stream that sends `initialize`, `tools/list`, and `tools/call` frames, assert we emit valid JSON-RPC replies. (Stretch goal: add after core functionality works.)

## Open Items / Follow-ups
- Emit `notifications/tools/list_changed` when files change (requires file watcher) so we can set `listChanged: true` later.
- Add optional `resources/read` to stream full SKILL.md content for clients that prefer the resources primitive over tool calls.

## Incremental Refresh (sk-3z2)
- The server now watches the skills directory (recursive) via `notify` and emits `notifications/tools/list_changed` with a short reason string whenever a change occurs.
- Events are debounced (~500ms) to avoid spamming clients during bulk edits or git operations.
- Notifications are only sent after the MCP `initialize` handshake succeeds, preventing noise during startup.
