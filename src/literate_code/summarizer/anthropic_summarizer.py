from __future__ import annotations

import os
import sys

import anthropic

from ..parsers.base import FileUnits, TranslationUnit

_SYSTEM = """You summarize source code files for a semantic search index.
Given a file path and its parsed symbols (functions, types, constants), write a 3-5 sentence summary of the file's purpose and key functionality.
- Explain what the file does, not how (purpose over mechanics)
- Preserve domain-specific keywords, type names, and API names
- Mention the most important functions/types by name
- Note key dependencies or patterns when relevant
Output ONLY the summary text, no formatting or markdown."""

# Billing errors that indicate credit exhaustion (worth retrying via subscription)
_BILLING_MARKERS = ("credit balance", "insufficient_quota", "billing")


def _extract_text(response: anthropic.types.Message) -> str:
    """Safely extract text from an Anthropic response."""
    if not response.content:
        raise ValueError(f"Empty response (stop_reason={response.stop_reason})")
    block = response.content[0]
    if block.type != "text":
        raise ValueError(f"Unexpected content type: {block.type}")
    return block.text


def _is_billing_error(exc: Exception) -> bool:
    """Check if an API error is a billing/credit issue."""
    msg = str(exc).lower()
    return any(marker in msg for marker in _BILLING_MARKERS)


def _create_client() -> anthropic.AsyncAnthropic:
    """Create an Anthropic async client, preferring auth_token (subscription) over api_key (credits)."""
    auth_token = os.environ.get("ANTHROPIC_AUTH_TOKEN")
    api_key = os.environ.get("ANTHROPIC_API_KEY")

    if auth_token:
        return anthropic.AsyncAnthropic(auth_token=auth_token)
    if api_key:
        return anthropic.AsyncAnthropic(api_key=api_key)
    return anthropic.AsyncAnthropic()


def _build_prompt(file_units: FileUnits) -> str:
    """Build prompt from parsed units — signatures, names, doc comments only (no source)."""
    parts = [f"File: {file_units.path}", ""]
    for unit in file_units.units:
        label = _unit_label(unit)
        parts.append(f"- {label}")
        if unit.signature:
            parts.append(f"  {unit.signature}")
        if unit.doc_comment:
            parts.append(f"  {unit.doc_comment}")
    return "\n".join(parts)


def _unit_label(unit: TranslationUnit) -> str:
    if unit.kind == "method" and unit.receiver:
        return f"method `{unit.name}` on {unit.receiver}"
    if unit.kind == "imports":
        return f"imports: {unit.source[:200]}"
    return f"{unit.kind} `{unit.name}`"


def _fallback_summary(file_units: FileUnits) -> str:
    """Generate a minimal summary when LLM fails."""
    kinds: dict[str, int] = {}
    for u in file_units.units:
        kinds[u.kind] = kinds.get(u.kind, 0) + 1
    parts = [f"{count} {kind}(s)" for kind, count in sorted(kinds.items())]
    return f"Contains {', '.join(parts)}."


class AnthropicSummarizer:
    def __init__(self, model: str = "claude-haiku-4-5-20251001") -> None:
        self._model = model
        self._client = _create_client()
        self._switched_to_subscription = False

    def _try_switch_to_subscription(self) -> bool:
        """If we hit a billing error on api_key, try switching to auth_token."""
        if self._switched_to_subscription:
            return False
        auth_token = os.environ.get("ANTHROPIC_AUTH_TOKEN")
        if not auth_token:
            return False
        print("  Switching to subscription auth (ANTHROPIC_AUTH_TOKEN)...", file=sys.stderr)
        self._client = anthropic.AsyncAnthropic(auth_token=auth_token)
        self._switched_to_subscription = True
        return True

    async def _call(self, **kwargs) -> anthropic.types.Message:
        """Make an API call with automatic billing-error fallback to subscription auth."""
        try:
            return await self._client.messages.create(**kwargs)
        except (anthropic.BadRequestError, anthropic.AuthenticationError) as e:
            if _is_billing_error(e) and self._try_switch_to_subscription():
                return await self._client.messages.create(**kwargs)
            raise

    async def summarize_file(self, file_units: FileUnits) -> str:
        """Summarize a file from its parsed translation units. One LLM call."""
        prompt = _build_prompt(file_units)
        response = await self._call(
            model=self._model,
            max_tokens=512,
            system=_SYSTEM,
            messages=[{"role": "user", "content": prompt}],
        )
        return _extract_text(response).strip()
