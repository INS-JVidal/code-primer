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
  main.rs          # Clap CLI entry point
  config.rs        # Config struct, defaults, output dir resolution
  pipeline.rs      # discover → parse → summarize → write JSON
  parser.rs        # TranslationUnit, FileUnits, Go + Rust tree-sitter parsers
  summarizer.rs    # Anthropic API client (async, one call per file)
  meta.rs          # SHA-256/mtime tracking for --refresh
```

## Build & Run

```bash
cargo install --path .
code-primer --dry-run /path/to/project    # preview units + cost
code-primer /path/to/project              # generate code-primer.json
code-primer /path/to/project --resume     # skip already-summarized files
code-primer /path/to/project --refresh    # re-summarize only changed/new, prune deleted
```

Requires `ANTHROPIC_AUTH_TOKEN` (subscription) or `ANTHROPIC_API_KEY` (credits).

## Key Design Decisions

- **One LLM call per file**: sends parsed signatures + names + doc comments (not source), gets back 3-5 sentence file summary
- **Auth priority**: ANTHROPIC_AUTH_TOKEN preferred over ANTHROPIC_API_KEY. Auto-fallback on billing errors.
- **Parsers**: Go and Rust via tree-sitter
- **Resume vs Refresh**: `--resume` skips files with existing summaries; `--refresh` uses SHA-256 hashes to detect actual changes and only re-summarizes modified files
- **Meta sidecar**: `code-primer.meta.json` stores per-file SHA-256 + mtime. Mtime is used as fast-path to skip hash computation; SHA-256 is authoritative

## Claude Code Integration

Copy `claude-code/commands/refresh-primer.md` to your project's `.claude/commands/` (or `~/.claude/commands/` for global). Then use `/refresh-primer` inside Claude Code to update stale summaries.

Add the snippet from `claude-code/CLAUDE-SNIPPET.md` to your target project's CLAUDE.md so Claude reads summaries at session start.

## Related

- fastmem4claude: `/home/jvidal/PROJECTS/fastmem4claude`
