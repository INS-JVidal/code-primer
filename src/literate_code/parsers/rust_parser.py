from __future__ import annotations

import tree_sitter_rust as tsrust
from tree_sitter import Language, Parser

from .base import FileUnits, TranslationUnit

RUST_LANGUAGE = Language(tsrust.language())

# Top-level node types that become translation units
_UNIT_NODE_TYPES = frozenset({
    "use_declaration",
    "const_item",
    "static_item",
    "struct_item",
    "enum_item",
    "type_item",
    "trait_item",
    "function_item",
    "impl_item",
    "mod_item",
})

_GENERATED_MARKERS = (b"// @generated", b"// DO NOT EDIT")


class RustParser:
    def __init__(self) -> None:
        self._parser = Parser(RUST_LANGUAGE)

    def supported_extensions(self) -> list[str]:
        return [".rs"]

    def parse(self, source: bytes, relative_path: str) -> FileUnits:
        # Skip generated files
        first_line = source.split(b"\n", 1)[0]
        if any(first_line.startswith(m) for m in _GENERATED_MARKERS):
            return FileUnits(path=relative_path)

        tree = self._parser.parse(source)
        root = tree.root_node
        units: list[TranslationUnit] = []

        children = list(root.children)
        # Collect all use declarations into a single imports unit
        uses = _collect_uses(children, source)
        if uses:
            units.append(uses)

        i = 0
        while i < len(children):
            node = children[i]

            if node.type in ("line_comment", "block_comment", "attribute_item", "use_declaration"):
                i += 1
                continue

            if node.type not in _UNIT_NODE_TYPES:
                i += 1
                continue

            doc_comment = _collect_doc_comments(children, i, source)

            if node.type == "impl_item":
                # Expand impl blocks into individual method units
                units.extend(_parse_impl(node, source, doc_comment))
            elif node.type == "mod_item":
                # Only include mod declarations, not inline mod bodies
                if not _has_body(node):
                    unit = _simple_unit("mod", node, source, doc_comment)
                    if unit:
                        units.append(unit)
            else:
                unit = _node_to_unit(node, source, doc_comment)
                if unit:
                    units.append(unit)

            i += 1

        return FileUnits(path=relative_path, units=units)


def _collect_uses(children: list, source: bytes) -> TranslationUnit | None:
    """Collect all top-level use declarations into a single imports unit."""
    use_nodes = [c for c in children if c.type == "use_declaration"]
    if not use_nodes:
        return None
    first = use_nodes[0]
    last = use_nodes[-1]
    text = "\n".join(
        source[n.start_byte:n.end_byte].decode() for n in use_nodes
    )
    return TranslationUnit(
        kind="imports", name="(block)", signature="", source=text,
        line_start=first.start_point[0] + 1,
        line_end=last.end_point[0] + 1,
    )


def _collect_doc_comments(children: list, unit_index: int, source: bytes) -> str:
    """Walk backwards collecting /// doc comments and #[...] attributes."""
    comments: list[str] = []
    j = unit_index - 1
    while j >= 0:
        node = children[j]
        if node.type == "line_comment":
            text = source[node.start_byte:node.end_byte].decode()
            if text.startswith("///") or text.startswith("//!"):
                comments.append(text)
                j -= 1
                continue
        elif node.type == "attribute_item":
            # Skip #[derive(...)] etc — not useful as doc comments
            j -= 1
            continue
        break
    comments.reverse()
    return "\n".join(comments)


def _node_to_unit(node, source: bytes, doc_comment: str) -> TranslationUnit | None:
    text = source[node.start_byte:node.end_byte].decode()
    line_start = node.start_point[0] + 1
    line_end = node.end_point[0] + 1
    name_node = node.child_by_field_name("name")
    name = name_node.text.decode() if name_node else "unknown"

    if node.type == "function_item":
        sig = _extract_rust_signature(node, source)
        return TranslationUnit(
            kind="func", name=name, signature=sig, source=text,
            line_start=line_start, line_end=line_end, doc_comment=doc_comment,
        )

    if node.type == "struct_item":
        return TranslationUnit(
            kind="type", name=name, signature="", source=text,
            line_start=line_start, line_end=line_end, doc_comment=doc_comment,
        )

    if node.type == "enum_item":
        return TranslationUnit(
            kind="type", name=name, signature="", source=text,
            line_start=line_start, line_end=line_end, doc_comment=doc_comment,
        )

    if node.type == "trait_item":
        return TranslationUnit(
            kind="trait", name=name, signature="", source=text,
            line_start=line_start, line_end=line_end, doc_comment=doc_comment,
        )

    if node.type == "type_item":
        return TranslationUnit(
            kind="type", name=name, signature="", source=text,
            line_start=line_start, line_end=line_end, doc_comment=doc_comment,
        )

    if node.type in ("const_item", "static_item"):
        kind = "const" if node.type == "const_item" else "var"
        return TranslationUnit(
            kind=kind, name=name, signature="", source=text,
            line_start=line_start, line_end=line_end, doc_comment=doc_comment,
        )

    return None


def _simple_unit(kind: str, node, source: bytes, doc_comment: str) -> TranslationUnit | None:
    name_node = node.child_by_field_name("name")
    name = name_node.text.decode() if name_node else "unknown"
    text = source[node.start_byte:node.end_byte].decode()
    return TranslationUnit(
        kind=kind, name=name, signature="", source=text,
        line_start=node.start_point[0] + 1,
        line_end=node.end_point[0] + 1,
        doc_comment=doc_comment,
    )


def _parse_impl(node, source: bytes, impl_doc: str) -> list[TranslationUnit]:
    """Extract methods from an impl block as individual translation units."""
    type_node = node.child_by_field_name("type")
    trait_node = node.child_by_field_name("trait")
    receiver = ""
    if type_node:
        receiver = type_node.text.decode()
        if trait_node:
            receiver = f"{trait_node.text.decode()} for {receiver}"

    body = node.child_by_field_name("body")
    if not body:
        return []

    units: list[TranslationUnit] = []
    children = list(body.named_children)

    for i, child in enumerate(children):
        if child.type == "function_item":
            doc = _collect_doc_comments_in_body(children, i, source)
            name_node = child.child_by_field_name("name")
            name = name_node.text.decode() if name_node else "unknown"
            sig = _extract_rust_signature(child, source)
            text = source[child.start_byte:child.end_byte].decode()
            units.append(TranslationUnit(
                kind="method", name=name, signature=sig, source=text,
                line_start=child.start_point[0] + 1,
                line_end=child.end_point[0] + 1,
                doc_comment=doc, receiver=receiver,
            ))

    return units


def _collect_doc_comments_in_body(children: list, index: int, source: bytes) -> str:
    """Collect /// doc comments preceding a method inside an impl body."""
    comments: list[str] = []
    j = index - 1
    while j >= 0:
        node = children[j]
        if node.type == "line_comment":
            text = source[node.start_byte:node.end_byte].decode()
            if text.startswith("///"):
                comments.append(text)
                j -= 1
                continue
        elif node.type == "attribute_item":
            j -= 1
            continue
        break
    comments.reverse()
    return "\n".join(comments)


def _extract_rust_signature(node, source: bytes) -> str:
    """Extract function/method signature (everything before the body block)."""
    body_node = node.child_by_field_name("body")
    if body_node:
        sig_bytes = source[node.start_byte:body_node.start_byte].rstrip()
        return sig_bytes.decode()
    return source[node.start_byte:node.end_byte].decode().split("\n")[0]


def _has_body(node) -> bool:
    """Check if a mod_item has an inline body (vs just `mod foo;`)."""
    body = node.child_by_field_name("body")
    return body is not None
