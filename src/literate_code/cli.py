from __future__ import annotations

import asyncio
import sys

import click

from .config import Config
from .pipeline import run_pipeline


@click.command()
@click.argument("project_dir", type=click.Path(exists=True, file_okay=False))
@click.option("-o", "--output", "output_dir", default="", help="Output directory (default: literate-code-<project>/)")
@click.option("--provider", default="anthropic", type=click.Choice(["anthropic", "ollama"]), help="LLM provider")
@click.option("--model", default="claude-haiku-4-5-20251001", help="Model name")
@click.option("--include", "include_patterns", multiple=True, help="File glob patterns to include (repeatable)")
@click.option("--exclude", "exclude_patterns", multiple=True, help="File glob patterns to exclude (repeatable)")
@click.option("--sql-include", "sql_include_patterns", multiple=True, help="SQL file patterns (repeatable)")
@click.option("--dry-run", is_flag=True, help="Parse and show units without calling LLM")
@click.option("--resume", is_flag=True, help="Skip files already in literate-summaries.json")
@click.option("--concurrency", default=4, type=int, help="Parallel LLM requests")
def main(
    project_dir: str,
    output_dir: str,
    provider: str,
    model: str,
    include_patterns: tuple[str, ...],
    exclude_patterns: tuple[str, ...],
    sql_include_patterns: tuple[str, ...],
    dry_run: bool,
    resume: bool,
    concurrency: int,
) -> None:
    """Generate literate-code descriptions for a source code project."""
    # Only override Config defaults when CLI args are explicitly provided
    kwargs: dict = dict(
        project_dir=project_dir,
        output_dir=output_dir,
        provider=provider,
        model=model,
        dry_run=dry_run,
        resume=resume,
        concurrency=concurrency,
    )
    if include_patterns:
        kwargs["include_patterns"] = list(include_patterns)
    if exclude_patterns:
        kwargs["exclude_patterns"] = list(exclude_patterns)
    if sql_include_patterns:
        kwargs["sql_include_patterns"] = list(sql_include_patterns)

    config = Config(**kwargs)

    try:
        asyncio.run(run_pipeline(config))
    except KeyboardInterrupt:
        print("\nInterrupted.", file=sys.stderr)
        sys.exit(130)
