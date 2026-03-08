# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

Code Primer generates file-level natural language summaries of source code by parsing functions/types via tree-sitter and sending their signatures to an LLM. Output is a single `code-primer.json` per project. The goal: prime AI coding assistants with codebase understanding so they need fewer exploratory tool calls.

**Primary consumer**: fastmem4claude's indexer, which uses file summaries to enrich transcript embedding text with code semantics (improves both dense and sparse vector search).

## Output Format

Single `code-primer.json` in the output directory (placed next to the project as `code-primer-<project>/`):
```json
{
  "src/parser/chunker.rs": "Stateful JSONL line processor that groups conversation exchanges...",
  "src/store/qdrant.rs": "Qdrant vector database client that manages collection creation..."
}
```

A sidecar `code-primer.meta.json` tracks file SHA-256 hashes and mtimes for `--refresh`.

## Architecture

```
src/
  main.rs          # Tokio async entry point, dispatches subcommands
  cli.rs           # Clap CLI definitions (shared with build.rs for man pages)
  config.rs        # Config struct, defaults, output dir resolution
  pipeline.rs      # All subcommand implementations (init, generate, refresh, status, verify, clean, uninstall)
  parser.rs        # TranslationUnit, FileUnits, Go + Rust tree-sitter parsers
  summarizer.rs    # Dual backend: claude CLI (subscription) + direct API
  meta.rs          # SHA-256/mtime tracking for refresh
  report.rs        # JSON report structs for all subcommands
build.rs           # Man page generation via clap_mangen
```

## Build & Run

```bash
cargo install --path .
```

Default backend: `claude` CLI (uses your Max/Pro subscription, no API keys needed).
Fallback: `--api` flag with `ANTHROPIC_API_KEY` env var for direct API access.

## CLI Lifecycle

```bash
code-primer init /path/to/project              # scaffold output dir, CLAUDE.md snippet, slash command
code-primer generate /path/to/project           # parse + summarize → code-primer.json
code-primer generate --dry-run /path/to/project  # preview units + estimated cost
code-primer generate --resume /path/to/project   # skip already-summarized files
code-primer refresh /path/to/project             # re-summarize only changed/new, prune deleted
code-primer status /path/to/project              # detect changes, report what needs refresh (no LLM)
code-primer verify /path/to/project              # validate JSON integrity, check hashes
code-primer clean /path/to/project               # remove generated output directory
code-primer uninstall /path/to/project           # clean + remove CLAUDE.md snippet + slash command
```

Every subcommand outputs a JSON report to stdout (progress on stderr) so it can be consumed by Claude or other tools.

## Key Design Decisions

- **One LLM call per file**: sends parsed signatures + names + doc comments (not source), gets back 3-5 sentence file summary
- **Auth**: Prefers `claude` CLI (subscription auth, strips ANTHROPIC_API_KEY from subprocess). Falls back to direct API with `--api` flag.
- **Parsers**: Go and Rust via tree-sitter
- **Resume vs Refresh**: `generate --resume` skips files with existing summaries; `refresh` uses SHA-256 hashes to detect actual changes and only re-summarizes modified files
- **Meta sidecar**: `code-primer.meta.json` stores per-file SHA-256 + mtime. Mtime is used as fast-path to skip hash computation; SHA-256 is authoritative
- **Structured output**: Every subcommand emits a JSON report to stdout; progress/debug goes to stderr. This makes the tool composable with Claude and scripts

## Claude Code Integration

Run `code-primer init /path/to/project` to automatically:
1. Create the output directory
2. Install the `/refresh-primer` slash command in `.claude/commands/`
3. Append the CLAUDE.md snippet so Claude reads summaries at session start

To reverse: `code-primer uninstall /path/to/project`

The template files in `claude-code/` are reference copies — the `init` command embeds them directly.

## Related

- fastmem4claude: `/home/jvidal/PROJECTS/fastmem4claude`

<!-- code-primer:begin -->
## Code Primer

File-level summaries are available in `../code-primer-code-primer/code-primer.json`.
Read this file at the start of a conversation to understand the codebase structure before exploring code.

**When to refresh summaries** (run `/refresh-primer` or `code-primer status .` to check):
- After adding new source files or modules
- After significant restructuring or refactoring that changes a file's role
- After deleting source files

**After refreshing:** Re-read the updated `code-primer.json` and use the latest version — disregard any earlier copy in this conversation.
<!-- code-primer:end -->
