from __future__ import annotations

import asyncio
import fnmatch
import json
import os
import sys

from .config import Config
from .parsers.base import FileUnits, TranslationUnit
from .parsers.base import Parser
from .parsers.go_parser import GoParser
from .parsers.rust_parser import RustParser
from .summarizer.anthropic_summarizer import AnthropicSummarizer


def _discover_files(config: Config) -> list[str]:
    """Walk project_dir and return relative paths matching include/exclude patterns."""
    include = config.include_patterns
    exclude = config.exclude_patterns
    project = config.project_dir

    if not include:
        known_exts = {".go", ".rs"}
        found_exts: set[str] = set()
        for dirpath, _, filenames in os.walk(project):
            for f in filenames:
                _, ext = os.path.splitext(f)
                if ext in known_exts:
                    found_exts.add(ext)
        if not found_exts:
            return []
        include = [f"**/*{ext}" for ext in sorted(found_exts)]

    result: list[str] = []
    for dirpath, _, filenames in os.walk(project):
        for f in filenames:
            full = os.path.join(dirpath, f)
            rel = os.path.relpath(full, project)

            if any(fnmatch.fnmatch(rel, pat) for pat in exclude):
                continue
            if any(fnmatch.fnmatch(rel, pat) for pat in include):
                result.append(rel)

    result.sort()
    return result


_PARSER_MAP: dict[str, type[Parser]] = {
    ".go": GoParser,
    ".rs": RustParser,
}

_PARSERS: dict[str, Parser] = {}


def _select_parser(path: str) -> Parser | None:
    """Select the appropriate parser based on file extension. Reuses instances."""
    _, ext = os.path.splitext(path)
    cls = _PARSER_MAP.get(ext)
    if cls is None:
        return None
    if ext not in _PARSERS:
        _PARSERS[ext] = cls()
    return _PARSERS[ext]


def _estimate_tokens(units: list[TranslationUnit]) -> int:
    """Rough token estimate: ~4 chars per token for source code."""
    total_chars = sum(len(u.signature) + len(u.doc_comment) + len(u.name) for u in units)
    return total_chars // 4


async def run_pipeline(config: Config) -> None:
    """Main pipeline: discover → parse → summarize → write JSON."""
    # Validate auth early (unless dry-run)
    if not config.dry_run and config.provider == "anthropic":
        has_key = os.environ.get("ANTHROPIC_API_KEY")
        has_token = os.environ.get("ANTHROPIC_AUTH_TOKEN")
        if not has_key and not has_token:
            print("ERROR: No Anthropic auth configured.", file=sys.stderr)
            print("  Set ANTHROPIC_AUTH_TOKEN (subscription) or ANTHROPIC_API_KEY (credits).", file=sys.stderr)
            print("  Or use --dry-run to preview without LLM calls.", file=sys.stderr)
            sys.exit(1)

    files = _discover_files(config)
    if not files:
        print("No matching source files found.")
        return

    # Parse all files
    all_file_units: list[FileUnits] = []
    total_units = 0
    parse_errors = 0

    for rel_path in files:
        parser = _select_parser(rel_path)
        if parser is None:
            print(f"  SKIP {rel_path} (no parser for this extension)")
            continue

        full_path = os.path.join(config.project_dir, rel_path)
        try:
            with open(full_path, "rb") as f:
                source = f.read()
            file_units = parser.parse(source, rel_path)
        except Exception as e:
            print(f"  ERROR {rel_path}: {e}", file=sys.stderr)
            parse_errors += 1
            continue

        if not file_units.units:
            continue

        all_file_units.append(file_units)
        total_units += len(file_units.units)

    print(f"Found {len(all_file_units)} files with {total_units} translation units.")
    if parse_errors:
        print(f"  ({parse_errors} files failed to parse)", file=sys.stderr)

    if config.dry_run:
        _print_dry_run(all_file_units)
        return

    # Load existing summaries for resume
    output_path = os.path.join(config.output_dir, "literate-summaries.json")
    summaries: dict[str, str] = {}
    if config.resume and os.path.exists(output_path):
        with open(output_path) as f:
            summaries = json.load(f)

    # Summarize
    summarizer = AnthropicSummarizer(model=config.model)
    semaphore = asyncio.Semaphore(config.concurrency)

    async def summarize_one(fu: FileUnits, index: int) -> tuple[str, str] | None:
        if config.resume and fu.path in summaries:
            print(f"  [{index}/{len(all_file_units)}] SKIP (exists) {fu.path}")
            return None
        print(f"  [{index}/{len(all_file_units)}] {fu.path} ({len(fu.units)} units)")
        try:
            async with semaphore:
                summary = await summarizer.summarize_file(fu)
            return (fu.path, summary)
        except Exception as e:
            print(f"  ERROR summarizing {fu.path}: {e}", file=sys.stderr)
            return None

    results = await asyncio.gather(
        *(summarize_one(fu, i) for i, fu in enumerate(all_file_units, 1))
    )

    new_count = 0
    for result in results:
        if result is not None:
            path, summary = result
            summaries[path] = summary
            new_count += 1

    # Write output
    os.makedirs(config.output_dir, exist_ok=True)
    with open(output_path, "w") as f:
        json.dump(summaries, f, indent=2, ensure_ascii=False)
        f.write("\n")

    print(f"\nDone. {new_count} new summaries ({len(summaries)} total) written to {output_path}")


def _print_dry_run(all_file_units: list[FileUnits]) -> None:
    """Print parsed units and estimated costs without calling LLM."""
    total_tokens = 0
    for fu in all_file_units:
        units_needing_llm = [u for u in fu.units if u.kind not in ("package", "imports")]
        tokens = _estimate_tokens(units_needing_llm)
        total_tokens += tokens
        print(f"\n  {fu.path} ({len(fu.units)} units, ~{tokens} input tokens)")
        for u in fu.units:
            sig = f"  sig: {u.signature}" if u.signature else ""
            print(f"    {u.kind:8s} {u.name:30s} L{u.line_start}-L{u.line_end}{sig}")

    # Cost estimate (Haiku: $0.80/MTok input, $4.00/MTok output)
    est_output = total_tokens // 5
    cost_input = total_tokens * 0.80 / 1_000_000
    cost_output = est_output * 4.00 / 1_000_000
    total_cost = cost_input + cost_output

    print(f"\n  Estimated: ~{total_tokens:,} input tokens, ~{est_output:,} output tokens")
    print(f"  Estimated cost (Haiku): ${total_cost:.4f}")
