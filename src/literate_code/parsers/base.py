from __future__ import annotations

from dataclasses import dataclass, field
from typing import Protocol


@dataclass
class TranslationUnit:
    """A single translatable unit extracted from source code."""
    kind: str          # "package", "imports", "const", "var", "type", "func", "method"
    name: str          # e.g. "HookEventToDocument", "store", "Document"
    signature: str     # For func/method: the full signature line. Empty for others.
    source: str        # The raw source code of this unit
    line_start: int    # 1-based line number in original file
    line_end: int      # 1-based line number in original file
    doc_comment: str = ""   # Any preceding doc comment
    receiver: str = ""      # For methods: the receiver type. Empty for functions.


@dataclass
class FileUnits:
    """All translation units extracted from a single source file."""
    path: str                        # Relative path from project root
    units: list[TranslationUnit] = field(default_factory=list)


class Parser(Protocol):
    """Protocol for language-specific parsers."""

    def parse(self, source: bytes, relative_path: str) -> FileUnits:
        """Parse source bytes into translation units."""
        ...

    def supported_extensions(self) -> list[str]:
        """Return file extensions this parser handles (e.g. ['.go'])."""
        ...
