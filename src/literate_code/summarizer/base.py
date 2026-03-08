from __future__ import annotations

from typing import Protocol

from ..parsers.base import TranslationUnit


class Summarizer(Protocol):
    """Protocol for LLM-based summarizers."""

    async def summarize_units(
        self, units: list[TranslationUnit], file_path: str
    ) -> list[str]:
        """Generate descriptions for a batch of translation units.

        Returns a list of description strings in the same order as the input units.
        """
        ...

    async def summarize_file(
        self, file_path: str, unit_descriptions: list[tuple[str, str, str]]
    ) -> str:
        """Generate a file-level summary from unit descriptions.

        unit_descriptions: list of (kind, name, description) tuples.
        Returns a 1-2 sentence file summary.
        """
        ...
