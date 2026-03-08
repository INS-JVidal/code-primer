# code-primer

Prime your AI coding assistant with file-level codebase understanding — fewer tool calls, faster orientation.

code-primer parses source code with [tree-sitter](https://tree-sitter.github.io/), extracts function/type signatures, and sends them to an LLM to generate concise file-level summaries. Output is a single `code-primer.json` that AI assistants read at session start to understand your codebase without exploring files.

## Install

### Quick install (Linux/macOS)

```bash
curl -fsSL https://raw.githubusercontent.com/INS-JVidal/code-primer/main/install.sh | bash
```

### Windows

Download `code-primer-windows-x86_64.zip` from [GitHub Releases](https://github.com/INS-JVidal/code-primer/releases/latest), extract, and add to your PATH.

Or via Git Bash / MSYS2:

```bash
curl -fsSL https://raw.githubusercontent.com/INS-JVidal/code-primer/main/install.sh | bash
```

### From source

```bash
cargo install --git https://github.com/INS-JVidal/code-primer
```

### Pre-built binaries

Available for Linux (x86_64, aarch64), macOS (x86_64, aarch64), and Windows (x86_64) on [GitHub Releases](https://github.com/INS-JVidal/code-primer/releases/latest).

## Quick start

```bash
# 1. Initialize (creates output dir, CLAUDE.md snippet, /refresh-primer command)
code-primer init ./my-project

# 2. Preview what will be parsed (no LLM calls)
code-primer generate --dry-run ./my-project

# 3. Generate summaries
code-primer generate ./my-project

# 4. Check for stale summaries
code-primer status ./my-project

# 5. Re-summarize only changed files
code-primer refresh ./my-project
```

## How it works

```
Source files → tree-sitter parsing → function/type signatures → LLM → file summaries
```

For each source file, code-primer:
1. Parses with tree-sitter to extract functions, types, constants, and their signatures
2. Sends the signatures (not source code) to an LLM
3. Gets back a 3-5 sentence summary of what the file does
4. Writes all summaries to `code-primer.json`

A sidecar `code-primer.meta.json` tracks SHA-256 hashes so `refresh` only re-summarizes files that actually changed.

## Output format

```json
{
  "src/parser.rs": "Parses Go and Rust source files using tree-sitter to extract structured information about declarations...",
  "src/pipeline.rs": "Orchestration layer implementing all code-primer CLI subcommands..."
}
```

## Authentication

code-primer supports two backends:

| Backend | Auth | Flag |
|---------|------|------|
| **claude CLI** (default) | Your Max/Pro subscription — no API keys needed | _(automatic)_ |
| **Direct API** | `ANTHROPIC_API_KEY` env var | `--api` |

The `claude` CLI backend is preferred — it uses your existing subscription auth. The direct API backend is available as a fallback with `--api`.

## Commands

| Command | Description |
|---------|-------------|
| `init` | Set up output dir, CLAUDE.md snippet, `/refresh-primer` slash command |
| `generate` | Parse files and generate LLM summaries |
| `refresh` | Re-summarize only changed/new files, prune deleted |
| `status` | Check if summaries are stale (no LLM calls) |
| `verify` | Validate JSON integrity and check hashes |
| `clean` | Remove generated output files |
| `uninstall` | Remove all code-primer artifacts |

All commands output structured JSON to stdout (progress on stderr), making them composable with other tools.

```bash
# Machine-readable status check
code-primer status ./my-project | jq '.needs_refresh'
```

## Supported languages

- Go
- Rust

More languages can be added by implementing a tree-sitter parser in `src/parser.rs`.

## Claude Code integration

After `code-primer init`, your project gets:

- **CLAUDE.md snippet** — tells Claude to read `code-primer.json` at session start
- **`/refresh-primer` slash command** — one command to update stale summaries

## Man pages

```bash
cargo build --release
./install-man.sh
man code-primer
```

## License

MIT
