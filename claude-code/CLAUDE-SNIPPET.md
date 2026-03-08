## Code Primer

File-level summaries are available in `code-primer-<project>/code-primer.json`.
Read this file at the start of a conversation to understand the codebase structure before exploring code.

**When to refresh summaries** (run `/refresh-primer` or `/refresh-primer --dry-run` to preview):
- After adding new source files or modules
- After significant restructuring or refactoring that changes a file's role
- After deleting source files

**After refreshing:** Re-read the updated `code-primer.json` and use the latest version — disregard any earlier copy in this conversation.
