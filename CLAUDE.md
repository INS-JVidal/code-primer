# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

Literate Code generates file-level natural language summaries of source code by parsing functions/types via tree-sitter and sending their signatures to an LLM. Output is a single `literate-summaries.json` per project.

**Primary consumer**: fastmem4claude's indexer, which uses file summaries to enrich transcript embedding text with code semantics (improves both dense and sparse vector search).

## Output Format

Single `literate-summaries.json` in the output directory:
```json
{
  "crates/indexer/src/parser/chunker.rs": "Stateful JSONL line processor that groups Claude Code conversation exchanges by detecting user prompts, tool calls, and assistant responses...",
  "crates/indexer/src/store/qdrant.rs": "Qdrant vector database client that manages collection creation, point upserts, and hybrid search..."
}
```

## Architecture

```
src/literate_code/
  cli.py                     # Click CLI entry point
  config.py                  # Config dataclass
  pipeline.py                # discover → parse → summarize → write JSON
  parsers/
    base.py                  # TranslationUnit, FileUnits, Parser protocol
    go_parser.py             # tree-sitter Go parser
    rust_parser.py           # tree-sitter Rust parser
  summarizer/
    anthropic_summarizer.py  # Haiku via Anthropic API (async, one call per file)
```

## Build & Run

```bash
pip install -e ".[dev]"
literate-code --dry-run /path/to/project    # preview units + cost
literate-code /path/to/project              # generate literate-summaries.json
literate-code /path/to/project --resume     # skip already-summarized files
```

Requires `ANTHROPIC_AUTH_TOKEN` (subscription) or `ANTHROPIC_API_KEY` (credits).

## Key Design Decisions

- **One LLM call per file**: sends parsed signatures + names + doc comments (not source), gets back 3-5 sentence file summary
- **Auth priority**: ANTHROPIC_AUTH_TOKEN preferred over ANTHROPIC_API_KEY. Auto-fallback on billing errors.
- **Parsers**: Go and Rust via tree-sitter. Parser instances cached and reused.
- **Resume support**: loads existing `literate-summaries.json` and skips files already present

## fastmem4claude Integration

File summaries enrich fastmem's embedding text for tool calls that reference files (Read, Edit, Write):

```
Before:  "Edit: crates/indexer/src/parser/chunker.rs (old → new)"
After:   "Edit: crates/indexer/src/parser/chunker.rs — Stateful JSONL processor that groups conversation exchanges... (old → new)"
```

This improves both dense vectors (semantic meaning) and sparse vectors (domain keywords like "exchange", "conversation", "JSONL").

## Related

- fastmem4claude: `/home/jvidal/PROJECTS/fastmem4claude`
