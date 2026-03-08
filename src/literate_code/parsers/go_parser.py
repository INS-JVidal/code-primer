from __future__ import annotations

import tree_sitter_go as tsgo
from tree_sitter import Language, Parser

from .base import FileUnits, TranslationUnit

GO_LANGUAGE = Language(tsgo.language())

# Node types that represent translation units at the top level
_UNIT_NODE_TYPES = frozenset({
    "package_clause",
    "import_declaration",
    "const_declaration",
    "var_declaration",
    "type_declaration",
    "function_declaration",
    "method_declaration",
})

# Header comment that marks generated files
_GENERATED_MARKER = "// Code generated"


class GoParser:
    def __init__(self) -> None:
        self._parser = Parser(GO_LANGUAGE)

    def supported_extensions(self) -> list[str]:
        return [".go"]

    def parse(self, source: bytes, relative_path: str) -> FileUnits:
        # Skip generated files
        first_line = source.split(b"\n", 1)[0].decode(errors="replace")
        if first_line.startswith(_GENERATED_MARKER):
            return FileUnits(path=relative_path)

        tree = self._parser.parse(source)
        root = tree.root_node
        units: list[TranslationUnit] = []

        children = list(root.children)
        i = 0
        while i < len(children):
            node = children[i]

            if node.type == "comment":
                # Accumulate consecutive comment nodes as doc_comment for the next unit
                i += 1
                continue

            if node.type not in _UNIT_NODE_TYPES:
                i += 1
                continue

            # Collect preceding comments
            doc_comment = _collect_preceding_comments(children, i, source)

            unit = _node_to_unit(node, source, doc_comment)
            if unit is not None:
                units.append(unit)
            i += 1

        return FileUnits(path=relative_path, units=units)


def _collect_preceding_comments(
    children: list, unit_index: int, source: bytes
) -> str:
    """Walk backwards from unit_index to collect adjacent comment nodes."""
    comments: list[str] = []
    j = unit_index - 1
    while j >= 0 and children[j].type == "comment":
        comments.append(source[children[j].start_byte : children[j].end_byte].decode())
        j -= 1
    comments.reverse()
    return "\n".join(comments)


def _node_to_unit(
    node, source: bytes, doc_comment: str
) -> TranslationUnit | None:
    text = source[node.start_byte : node.end_byte].decode()
    line_start = node.start_point[0] + 1  # 1-based
    line_end = node.end_point[0] + 1

    if node.type == "package_clause":
        # package_clause has no field names; find the package_identifier child
        name = "unknown"
        for child in node.named_children:
            if child.type == "package_identifier":
                name = child.text.decode()
                break
        return TranslationUnit(
            kind="package", name=name, signature="", source=text,
            line_start=line_start, line_end=line_end, doc_comment=doc_comment,
        )

    if node.type == "import_declaration":
        return TranslationUnit(
            kind="imports", name="(block)", signature="", source=text,
            line_start=line_start, line_end=line_end, doc_comment=doc_comment,
        )

    if node.type in ("const_declaration", "var_declaration"):
        kind = "const" if node.type == "const_declaration" else "var"
        name = _extract_declaration_name(node, source)
        return TranslationUnit(
            kind=kind, name=name, signature="", source=text,
            line_start=line_start, line_end=line_end, doc_comment=doc_comment,
        )

    if node.type == "type_declaration":
        name = _extract_type_name(node, source)
        return TranslationUnit(
            kind="type", name=name, signature="", source=text,
            line_start=line_start, line_end=line_end, doc_comment=doc_comment,
        )

    if node.type == "function_declaration":
        name_node = node.child_by_field_name("name")
        name = name_node.text.decode() if name_node else "unknown"
        # Disambiguate multiple init() functions
        if name == "init":
            name = f"init@L{line_start}"
        sig = _extract_func_signature(node, source)
        return TranslationUnit(
            kind="func", name=name, signature=sig, source=text,
            line_start=line_start, line_end=line_end, doc_comment=doc_comment,
        )

    if node.type == "method_declaration":
        name_node = node.child_by_field_name("name")
        name = name_node.text.decode() if name_node else "unknown"
        receiver = _extract_receiver(node, source)
        sig = _extract_func_signature(node, source)
        return TranslationUnit(
            kind="method", name=name, signature=sig, source=text,
            line_start=line_start, line_end=line_end, doc_comment=doc_comment,
            receiver=receiver,
        )

    return None


def _extract_declaration_name(node, source: bytes) -> str:
    """Extract name from const/var declaration. For grouped blocks, use first identifier or '(block)'."""
    # Single declaration: const X = ... or var X = ...
    for child in node.children:
        if child.type == "const_spec" or child.type == "var_spec":
            name_node = child.child_by_field_name("name")
            if name_node:
                # Check if there are multiple specs (grouped block)
                specs = [c for c in node.children if c.type in ("const_spec", "var_spec")]
                if len(specs) > 1:
                    return f"{name_node.text.decode()}..."
                return name_node.text.decode()
    return "(block)"


def _extract_type_name(node, source: bytes) -> str:
    """Extract the type name from a type_declaration."""
    for child in node.children:
        if child.type == "type_spec":
            name_node = child.child_by_field_name("name")
            if name_node:
                return name_node.text.decode()
    return "unknown"


def _extract_func_signature(node, source: bytes) -> str:
    """Extract the function/method signature (everything before the body)."""
    body_node = node.child_by_field_name("body")
    if body_node:
        sig_bytes = source[node.start_byte : body_node.start_byte].rstrip()
        return sig_bytes.decode()
    # No body (e.g., external function) — use full text
    return source[node.start_byte : node.end_byte].decode().split("\n")[0]


def _extract_receiver(node, source: bytes) -> str:
    """Extract the receiver type from a method declaration."""
    receiver_node = node.child_by_field_name("receiver")
    if receiver_node:
        return receiver_node.text.decode()
    return ""
