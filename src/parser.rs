use anyhow::{Context, Result};

#[derive(Debug)]
pub struct TranslationUnit {
    pub kind: &'static str,
    pub name: String,
    pub signature: String,
    pub source: String,
    pub line_start: usize,
    pub line_end: usize,
    pub doc_comment: String,
    pub receiver: String,
}

#[derive(Debug)]
pub struct FileUnits {
    pub path: String,
    pub units: Vec<TranslationUnit>,
}

pub fn parse_file(source: &[u8], relative_path: &str) -> Result<Option<FileUnits>> {
    let ext = relative_path
        .rsplit('.')
        .next()
        .unwrap_or("");
    match ext {
        "go" => parse_go(source, relative_path).map(Some),
        "rs" => parse_rust(source, relative_path).map(Some),
        _ => Ok(None),
    }
}

// ── Go parser ──────────────────────────────────────────────────────

const GO_GENERATED_MARKER: &[u8] = b"// Code generated";

const GO_UNIT_NODE_TYPES: &[&str] = &[
    "package_clause",
    "import_declaration",
    "const_declaration",
    "var_declaration",
    "type_declaration",
    "function_declaration",
    "method_declaration",
];

fn parse_go(source: &[u8], relative_path: &str) -> Result<FileUnits> {
    // Skip generated files
    let first_line = source.split(|&b| b == b'\n').next().unwrap_or(b"");
    if first_line.starts_with(GO_GENERATED_MARKER) {
        return Ok(FileUnits {
            path: relative_path.to_string(),
            units: Vec::new(),
        });
    }

    let mut parser = tree_sitter::Parser::new();
    parser
        .set_language(&tree_sitter_go::LANGUAGE.into())
        .context("setting Go language")?;

    let tree = parser
        .parse(source, None)
        .context("parsing Go source")?;
    let root = tree.root_node();
    let mut units = Vec::new();

    let children: Vec<_> = (0..root.child_count())
        .filter_map(|i| root.child(i))
        .collect();

    for (i, node) in children.iter().enumerate() {
        if node.kind() == "comment" {
            continue;
        }
        if !GO_UNIT_NODE_TYPES.contains(&node.kind()) {
            continue;
        }

        let doc_comment = collect_preceding_comments_go(&children, i, source);

        if let Some(unit) = go_node_to_unit(node, source, &doc_comment) {
            units.push(unit);
        }
    }

    Ok(FileUnits {
        path: relative_path.to_string(),
        units,
    })
}

fn collect_preceding_comments_go(
    children: &[tree_sitter::Node],
    unit_index: usize,
    source: &[u8],
) -> String {
    let mut comments = Vec::new();
    let mut j = unit_index;
    while j > 0 {
        j -= 1;
        if children[j].kind() == "comment" {
            let text = &source[children[j].start_byte()..children[j].end_byte()];
            comments.push(String::from_utf8_lossy(text).into_owned());
        } else {
            break;
        }
    }
    comments.reverse();
    comments.join("\n")
}

fn go_node_to_unit(
    node: &tree_sitter::Node,
    source: &[u8],
    doc_comment: &str,
) -> Option<TranslationUnit> {
    let text = String::from_utf8_lossy(&source[node.start_byte()..node.end_byte()]).into_owned();
    let line_start = node.start_position().row + 1;
    let line_end = node.end_position().row + 1;

    match node.kind() {
        "package_clause" => {
            let name = node_named_children(node)
                .into_iter()
                .find(|c| c.kind() == "package_identifier")
                .map(|c| node_text(&c, source))
                .unwrap_or_else(|| "unknown".to_string());
            Some(TranslationUnit {
                kind: "package",
                name,
                signature: String::new(),
                source: text,
                line_start,
                line_end,
                doc_comment: doc_comment.to_string(),
                receiver: String::new(),
            })
        }

        "import_declaration" => Some(TranslationUnit {
            kind: "imports",
            name: "(block)".to_string(),
            signature: String::new(),
            source: text,
            line_start,
            line_end,
            doc_comment: doc_comment.to_string(),
            receiver: String::new(),
        }),

        "const_declaration" | "var_declaration" => {
            let kind = if node.kind() == "const_declaration" {
                "const"
            } else {
                "var"
            };
            let name = extract_go_decl_name(node, source);
            Some(TranslationUnit {
                kind,
                name,
                signature: String::new(),
                source: text,
                line_start,
                line_end,
                doc_comment: doc_comment.to_string(),
                receiver: String::new(),
            })
        }

        "type_declaration" => {
            let name = extract_go_type_name(node, source);
            Some(TranslationUnit {
                kind: "type",
                name,
                signature: String::new(),
                source: text,
                line_start,
                line_end,
                doc_comment: doc_comment.to_string(),
                receiver: String::new(),
            })
        }

        "function_declaration" => {
            let mut name = node
                .child_by_field_name("name")
                .map(|n| node_text(&n, source))
                .unwrap_or_else(|| "unknown".to_string());
            if name == "init" {
                name = format!("init@L{line_start}");
            }
            let sig = extract_func_signature(node, source);
            Some(TranslationUnit {
                kind: "func",
                name,
                signature: sig,
                source: text,
                line_start,
                line_end,
                doc_comment: doc_comment.to_string(),
                receiver: String::new(),
            })
        }

        "method_declaration" => {
            let name = node
                .child_by_field_name("name")
                .map(|n| node_text(&n, source))
                .unwrap_or_else(|| "unknown".to_string());
            let receiver = node
                .child_by_field_name("receiver")
                .map(|n| node_text(&n, source))
                .unwrap_or_default();
            let sig = extract_func_signature(node, source);
            Some(TranslationUnit {
                kind: "method",
                name,
                signature: sig,
                source: text,
                line_start,
                line_end,
                doc_comment: doc_comment.to_string(),
                receiver,
            })
        }

        _ => None,
    }
}

fn extract_go_decl_name(node: &tree_sitter::Node, source: &[u8]) -> String {
    let spec_type = if node.kind() == "const_declaration" {
        "const_spec"
    } else {
        "var_spec"
    };

    let children = node_named_children(node);
    let specs: Vec<_> = children
        .iter()
        .filter(|c| c.kind() == spec_type)
        .collect();

    if let Some(first) = specs.first() {
        if let Some(name_node) = first.child_by_field_name("name") {
            let name = node_text(&name_node, source);
            if specs.len() > 1 {
                return format!("{name}...");
            }
            return name;
        }
    }
    "(block)".to_string()
}

fn extract_go_type_name(node: &tree_sitter::Node, source: &[u8]) -> String {
    node_named_children(node)
        .into_iter()
        .find(|c| c.kind() == "type_spec")
        .and_then(|c: tree_sitter::Node| c.child_by_field_name("name"))
        .map(|n| node_text(&n, source))
        .unwrap_or_else(|| "unknown".to_string())
}

fn extract_func_signature(node: &tree_sitter::Node, source: &[u8]) -> String {
    if let Some(body) = node.child_by_field_name("body") {
        let sig_bytes = &source[node.start_byte()..body.start_byte()];
        return String::from_utf8_lossy(sig_bytes).trim_end().to_string();
    }
    let text = &source[node.start_byte()..node.end_byte()];
    String::from_utf8_lossy(text)
        .lines()
        .next()
        .unwrap_or("")
        .to_string()
}

// ── Rust parser ────────────────────────────────────────────────────

const RUST_GENERATED_MARKERS: &[&[u8]] = &[b"// @generated", b"// DO NOT EDIT"];

const RUST_UNIT_NODE_TYPES: &[&str] = &[
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
];

fn parse_rust(source: &[u8], relative_path: &str) -> Result<FileUnits> {
    let first_line = source.split(|&b| b == b'\n').next().unwrap_or(b"");
    if RUST_GENERATED_MARKERS
        .iter()
        .any(|m| first_line.starts_with(m))
    {
        return Ok(FileUnits {
            path: relative_path.to_string(),
            units: Vec::new(),
        });
    }

    let mut parser = tree_sitter::Parser::new();
    parser
        .set_language(&tree_sitter_rust::LANGUAGE.into())
        .context("setting Rust language")?;

    let tree = parser
        .parse(source, None)
        .context("parsing Rust source")?;
    let root = tree.root_node();
    let mut units = Vec::new();

    let children: Vec<_> = (0..root.child_count())
        .filter_map(|i| root.child(i))
        .collect();

    // Collect all use declarations into a single imports unit
    if let Some(imports_unit) = collect_rust_uses(&children, source) {
        units.push(imports_unit);
    }

    for (i, node) in children.iter().enumerate() {
        let kind = node.kind();
        if kind == "line_comment"
            || kind == "block_comment"
            || kind == "attribute_item"
            || kind == "use_declaration"
        {
            continue;
        }
        if !RUST_UNIT_NODE_TYPES.contains(&kind) {
            continue;
        }

        let doc_comment = collect_rust_doc_comments(&children, i, source);

        match kind {
            "impl_item" => {
                units.extend(parse_rust_impl(node, source, &doc_comment));
            }
            "mod_item" => {
                if node.child_by_field_name("body").is_none() {
                    if let Some(unit) = rust_simple_unit("mod", node, source, &doc_comment) {
                        units.push(unit);
                    }
                }
            }
            _ => {
                if let Some(unit) = rust_node_to_unit(node, source, &doc_comment) {
                    units.push(unit);
                }
            }
        }
    }

    Ok(FileUnits {
        path: relative_path.to_string(),
        units,
    })
}

fn collect_rust_uses(children: &[tree_sitter::Node], source: &[u8]) -> Option<TranslationUnit> {
    let use_nodes: Vec<_> = children
        .iter()
        .filter(|c| c.kind() == "use_declaration")
        .collect();

    if use_nodes.is_empty() {
        return None;
    }

    let first = use_nodes.first().unwrap();
    let last = use_nodes.last().unwrap();
    let text: Vec<String> = use_nodes
        .iter()
        .map(|n| String::from_utf8_lossy(&source[n.start_byte()..n.end_byte()]).into_owned())
        .collect();

    Some(TranslationUnit {
        kind: "imports",
        name: "(block)".to_string(),
        signature: String::new(),
        source: text.join("\n"),
        line_start: first.start_position().row + 1,
        line_end: last.end_position().row + 1,
        doc_comment: String::new(),
        receiver: String::new(),
    })
}

fn collect_rust_doc_comments(
    children: &[tree_sitter::Node],
    unit_index: usize,
    source: &[u8],
) -> String {
    let mut comments = Vec::new();
    let mut j = unit_index;
    while j > 0 {
        j -= 1;
        let node = &children[j];
        match node.kind() {
            "line_comment" => {
                let text = String::from_utf8_lossy(&source[node.start_byte()..node.end_byte()]);
                if text.starts_with("///") || text.starts_with("//!") {
                    comments.push(text.into_owned());
                } else {
                    break;
                }
            }
            "attribute_item" => continue,
            _ => break,
        }
    }
    comments.reverse();
    comments.join("\n")
}

fn rust_node_to_unit(
    node: &tree_sitter::Node,
    source: &[u8],
    doc_comment: &str,
) -> Option<TranslationUnit> {
    let text = String::from_utf8_lossy(&source[node.start_byte()..node.end_byte()]).into_owned();
    let line_start = node.start_position().row + 1;
    let line_end = node.end_position().row + 1;
    let name = node
        .child_by_field_name("name")
        .map(|n| node_text(&n, source))
        .unwrap_or_else(|| "unknown".to_string());

    match node.kind() {
        "function_item" => {
            let sig = extract_rust_signature(node, source);
            Some(TranslationUnit {
                kind: "func",
                name,
                signature: sig,
                source: text,
                line_start,
                line_end,
                doc_comment: doc_comment.to_string(),
                receiver: String::new(),
            })
        }
        "struct_item" | "enum_item" | "type_item" => Some(TranslationUnit {
            kind: "type",
            name,
            signature: String::new(),
            source: text,
            line_start,
            line_end,
            doc_comment: doc_comment.to_string(),
            receiver: String::new(),
        }),
        "trait_item" => Some(TranslationUnit {
            kind: "trait",
            name,
            signature: String::new(),
            source: text,
            line_start,
            line_end,
            doc_comment: doc_comment.to_string(),
            receiver: String::new(),
        }),
        "const_item" | "static_item" => {
            let kind = if node.kind() == "const_item" {
                "const"
            } else {
                "var"
            };
            Some(TranslationUnit {
                kind,
                name,
                signature: String::new(),
                source: text,
                line_start,
                line_end,
                doc_comment: doc_comment.to_string(),
                receiver: String::new(),
            })
        }
        _ => None,
    }
}

fn rust_simple_unit(
    kind: &'static str,
    node: &tree_sitter::Node,
    source: &[u8],
    doc_comment: &str,
) -> Option<TranslationUnit> {
    let name = node
        .child_by_field_name("name")
        .map(|n| node_text(&n, source))
        .unwrap_or_else(|| "unknown".to_string());
    let text = String::from_utf8_lossy(&source[node.start_byte()..node.end_byte()]).into_owned();
    Some(TranslationUnit {
        kind,
        name,
        signature: String::new(),
        source: text,
        line_start: node.start_position().row + 1,
        line_end: node.end_position().row + 1,
        doc_comment: doc_comment.to_string(),
        receiver: String::new(),
    })
}

fn parse_rust_impl(
    node: &tree_sitter::Node,
    source: &[u8],
    _impl_doc: &str,
) -> Vec<TranslationUnit> {
    let type_node = node.child_by_field_name("type");
    let trait_node = node.child_by_field_name("trait");

    let receiver = match (type_node, trait_node) {
        (Some(t), Some(tr)) => format!("{} for {}", node_text(&tr, source), node_text(&t, source)),
        (Some(t), None) => node_text(&t, source),
        _ => String::new(),
    };

    let body = match node.child_by_field_name("body") {
        Some(b) => b,
        None => return Vec::new(),
    };

    let children: Vec<_> = (0..body.named_child_count())
        .filter_map(|i| body.named_child(i))
        .collect();

    let mut units = Vec::new();
    for (i, child) in children.iter().enumerate() {
        if child.kind() == "function_item" {
            let doc = collect_rust_doc_comments_in_body(&children, i, source);
            let name = child
                .child_by_field_name("name")
                .map(|n| node_text(&n, source))
                .unwrap_or_else(|| "unknown".to_string());
            let sig = extract_rust_signature(child, source);
            let text =
                String::from_utf8_lossy(&source[child.start_byte()..child.end_byte()]).into_owned();
            units.push(TranslationUnit {
                kind: "method",
                name,
                signature: sig,
                source: text,
                line_start: child.start_position().row + 1,
                line_end: child.end_position().row + 1,
                doc_comment: doc,
                receiver: receiver.clone(),
            });
        }
    }
    units
}

fn collect_rust_doc_comments_in_body(
    children: &[tree_sitter::Node],
    index: usize,
    source: &[u8],
) -> String {
    let mut comments = Vec::new();
    let mut j = index;
    while j > 0 {
        j -= 1;
        let node = &children[j];
        match node.kind() {
            "line_comment" => {
                let text = String::from_utf8_lossy(&source[node.start_byte()..node.end_byte()]);
                if text.starts_with("///") {
                    comments.push(text.into_owned());
                } else {
                    break;
                }
            }
            "attribute_item" => continue,
            _ => break,
        }
    }
    comments.reverse();
    comments.join("\n")
}

fn extract_rust_signature(node: &tree_sitter::Node, source: &[u8]) -> String {
    if let Some(body) = node.child_by_field_name("body") {
        let sig_bytes = &source[node.start_byte()..body.start_byte()];
        return String::from_utf8_lossy(sig_bytes).trim_end().to_string();
    }
    String::from_utf8_lossy(&source[node.start_byte()..node.end_byte()])
        .lines()
        .next()
        .unwrap_or("")
        .to_string()
}

// ── Helpers ────────────────────────────────────────────────────────

fn node_text(node: &tree_sitter::Node, source: &[u8]) -> String {
    String::from_utf8_lossy(&source[node.start_byte()..node.end_byte()]).into_owned()
}

fn node_named_children<'a>(node: &tree_sitter::Node<'a>) -> Vec<tree_sitter::Node<'a>> {
    let mut cursor = node.walk();
    node.named_children(&mut cursor).collect()
}
