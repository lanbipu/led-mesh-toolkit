# AGENTS.md

<!-- DOCSMITH:KNOWLEDGE:BEGIN -->
## Knowledge Base (Managed by Docsmith)

- Knowledge entrypoint: `.Codex/knowledge/_INDEX.md`
- Config file: `.Codex/knowledge.json`

### Current Sources
- `help-disguise-one` (262 files) → `.Codex/knowledge/help-disguise-one/`
- `trimble-sx-docs` (33 files) → `.Codex/knowledge/trimble-sx-docs/`
- `ue57-docs` (411 files) → `.Codex/knowledge/ue57-docs/`

### Query Protocol
1. Read `.Codex/knowledge/_INDEX.md` to route to the relevant source.
2. Open `<source>/_INDEX.md` and shortlist target documents by `topic/summary/keywords`.
3. Read target file TL;DR first, then read full content when needed.
4. Before answering, prioritize evidence from `KnowledgeBase docs`; use external knowledge only when KB coverage is insufficient.
5. In every answer, include:
   - `Knowledge Sources`: exact KB document paths used.
   - `External Inputs`: non-KB knowledge used and why.
   - If no KB match: `No relevant KnowledgeBase docs found`.

### Refresh Command
```bash
.venv/bin/python -m cli --project-links --refresh-index .
```
<!-- DOCSMITH:KNOWLEDGE:END -->
