use std::collections::BTreeMap;
use std::fs;
use std::path::Path;
use std::sync::Arc;

use anyhow::{Context, Result};
use globset::{Glob, GlobSetBuilder};
use tokio::sync::Semaphore;
use walkdir::WalkDir;

use crate::config::Config;
use crate::meta;
use crate::parser::{self, FileUnits, TranslationUnit};
use crate::report::{self, print_report};
use crate::summarizer::{self, Summarizer};

const SUMMARIES_FILENAME: &str = "code-primer.json";
const MARKER_BEGIN: &str = "<!-- code-primer:begin -->";
const MARKER_END: &str = "<!-- code-primer:end -->";

const CLAUDE_SNIPPET_TEMPLATE: &str = r#"## Code Primer

File-level summaries are available in `{{output_path}}/code-primer.json`.
Read this file at the start of a conversation to understand the codebase structure before exploring code.

**When to refresh summaries** (run `/refresh-primer` or `code-primer status .` to check):
- After adding new source files or modules
- After significant restructuring or refactoring that changes a file's role
- After deleting source files

**After refreshing:** Re-read the updated `code-primer.json` and use the latest version — disregard any earlier copy in this conversation."#;

const REFRESH_COMMAND: &str = r#"Refresh the code-primer file summaries for this project. This re-summarizes only changed, new, or deleted files using content hashing.

Run the following command:

```bash
code-primer refresh $ARGUMENTS .
```

After the command completes:
1. Read the JSON report from stdout — it lists what changed
2. Read the updated `code-primer.json` from the output directory
3. Use the refreshed summaries for the rest of this conversation — disregard any earlier version loaded at session start"#;

// ── Init ───────────────────────────────────────────────────────────

pub fn cmd_init(project_dir: &Path, output: Option<&Path>) -> Result<()> {
    let cfg = Config::new(project_dir.to_path_buf(), output.map(|p| p.to_path_buf()))?;
    let mut actions = Vec::new();

    // 1. Create output directory
    if !cfg.output_dir.exists() {
        fs::create_dir_all(&cfg.output_dir)?;
        actions.push(format!("Created output directory: {}", cfg.output_dir.display()));
    } else {
        actions.push(format!("Output directory already exists: {}", cfg.output_dir.display()));
    }

    // 2. Install slash command
    let commands_dir = cfg.project_dir.join(".claude").join("commands");
    let command_file = commands_dir.join("refresh-primer.md");
    if !command_file.exists() {
        fs::create_dir_all(&commands_dir)?;
        fs::write(&command_file, REFRESH_COMMAND)?;
        actions.push(format!("Installed /refresh-primer command: {}", command_file.display()));
    } else {
        actions.push("Slash command /refresh-primer already installed (skipped)".to_string());
    }

    // 3. Inject CLAUDE.md snippet
    let claude_md = cfg.project_dir.join("CLAUDE.md");
    // Compute relative path from project to output dir for the CLAUDE.md snippet.
    // If the output dir is a sibling (default), this yields "../code-primer-<name>".
    // For custom --output paths, use the absolute path.
    let output_rel = pathdiff(&cfg.output_dir, &cfg.project_dir);
    let snippet = CLAUDE_SNIPPET_TEMPLATE.replace("{{output_path}}", &output_rel);
    let block = format!("{MARKER_BEGIN}\n{snippet}\n{MARKER_END}\n");

    if claude_md.exists() {
        let content = fs::read_to_string(&claude_md)?;
        if content.contains(MARKER_BEGIN) {
            actions.push("CLAUDE.md snippet already present (skipped)".to_string());
        } else {
            // Ensure a blank line separates existing content from the snippet
            let sep = if content.ends_with("\n\n") || content.is_empty() {
                ""
            } else if content.ends_with('\n') {
                "\n"
            } else {
                "\n\n"
            };
            let new_content = format!("{content}{sep}{block}");
            fs::write(&claude_md, new_content)?;
            actions.push("Appended code-primer snippet to CLAUDE.md".to_string());
        }
    } else {
        fs::write(&claude_md, &block)?;
        actions.push("Created CLAUDE.md with code-primer snippet".to_string());
    }

    print_report(&report::InitReport {
        command: "init",
        project_dir: cfg.project_dir.display().to_string(),
        output_dir: cfg.output_dir.display().to_string(),
        actions,
    });

    eprintln!();
    eprintln!("Next steps:");
    eprintln!("  1. Generate summaries:  code-primer generate {}", cfg.project_dir.display());
    eprintln!("  2. Preview first:       code-primer generate --dry-run {}", cfg.project_dir.display());
    eprintln!("  3. Later, refresh:      code-primer refresh {}", cfg.project_dir.display());
    eprintln!("  4. In Claude Code:      /refresh-primer");

    Ok(())
}

// ── Generate / Refresh ─────────────────────────────────────────────

pub async fn cmd_generate(config: &Config) -> Result<()> {
    let command_name = if config.refresh { "refresh" } else { "generate" };

    // Validate auth early and run preflight check (unless dry-run)
    let summarizer = if !config.dry_run {
        let s = Summarizer::new(config.model.clone(), config.force_api)?;
        eprintln!("Backend: {}", s.backend_name());
        eprintln!("Verifying auth...");
        s.preflight_check().await?;
        eprintln!("Auth OK.");
        Some(Arc::new(s))
    } else {
        None
    };

    let files = discover_files(config)?;
    if files.is_empty() && !config.refresh {
        eprintln!("No matching source files found.");
        print_report(&report::GenerateReport {
            command: command_name,
            project_dir: config.project_dir.display().to_string(),
            output_file: config.output_dir.join(SUMMARIES_FILENAME).display().to_string(),
            files_discovered: 0,
            files_parsed: 0,
            files_summarized: 0,
            files_fallback: 0,
            files_skipped: 0,
            files_deleted: 0,
            total_summaries: 0,
            parse_errors: 0,
        });
        return Ok(());
    }

    // Parse all files
    let mut all_file_units = Vec::new();
    let mut total_units = 0usize;
    let mut parse_errors = 0usize;

    for rel_path in &files {
        let full_path = config.project_dir.join(rel_path);
        let source = match fs::read(&full_path) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("  ERROR {rel_path}: {e}");
                parse_errors += 1;
                continue;
            }
        };

        match parser::parse_file(&source, rel_path) {
            Ok(Some(fu)) => {
                if !fu.units.is_empty() {
                    total_units += fu.units.len();
                    all_file_units.push(fu);
                }
            }
            Ok(None) => {
                eprintln!("  SKIP {rel_path} (no parser for this extension)");
            }
            Err(e) => {
                eprintln!("  ERROR {rel_path}: {e}");
                parse_errors += 1;
            }
        }
    }

    eprintln!(
        "Found {} files with {total_units} translation units.",
        all_file_units.len()
    );
    if parse_errors > 0 {
        eprintln!("  ({parse_errors} files failed to parse)");
    }

    if config.dry_run {
        print_dry_run(config, &all_file_units);
        return Ok(());
    }

    let summarizer = summarizer.unwrap();
    let output_path = config.output_dir.join(SUMMARIES_FILENAME);

    // Load existing summaries
    let mut summaries: BTreeMap<String, String> = if output_path.exists() {
        let data = fs::read_to_string(&output_path).context("reading existing summaries")?;
        serde_json::from_str(&data).context("parsing existing summaries")?
    } else {
        BTreeMap::new()
    };

    // Determine which files to process
    let files_to_process: Vec<&FileUnits>;
    let mut deleted_count = 0usize;
    let mut skipped_count = 0usize;

    if config.refresh {
        let old_meta = meta::load_meta(&config.output_dir)?;
        let discovered: Vec<String> = all_file_units.iter().map(|fu| fu.path.clone()).collect();
        let plan = meta::diff_files(&discovered, &old_meta, &config.project_dir)?;

        eprintln!(
            "Refresh: {} changed/new, {} unchanged, {} deleted",
            plan.changed.len(),
            plan.unchanged.len(),
            plan.deleted.len()
        );

        deleted_count = plan.deleted.len();
        skipped_count = plan.unchanged.len();

        // Remove deleted files from summaries
        for deleted in &plan.deleted {
            summaries.remove(deleted);
        }

        // Only process changed/new files
        let changed_set: std::collections::HashSet<&str> =
            plan.changed.iter().map(|s| s.as_str()).collect();
        files_to_process = all_file_units
            .iter()
            .filter(|fu| changed_set.contains(fu.path.as_str()))
            .collect();
    } else if config.resume {
        files_to_process = all_file_units
            .iter()
            .filter(|fu| !summaries.contains_key(&fu.path))
            .collect();
        skipped_count = all_file_units.len() - files_to_process.len();
    } else {
        files_to_process = all_file_units.iter().collect();
    };

    let total = files_to_process.len();
    if total == 0 {
        if config.resume && skipped_count > 0 {
            eprintln!("All files already summarized.");
        } else if !config.refresh {
            eprintln!("No files to summarize.");
        }
    }

    // Summarize concurrently
    let sem = Arc::new(Semaphore::new(config.concurrency));
    let mut handles = Vec::new();

    for (i, fu) in files_to_process.iter().enumerate() {
        let path = fu.path.clone();
        let idx = i + 1;

        eprintln!("  [{idx}/{total}] {path} ({} units)", fu.units.len());

        let sem = sem.clone();
        let summarizer = summarizer.clone();
        let file_units_owned = owned_file_units(fu);

        handles.push(tokio::spawn(async move {
            let _permit = sem.acquire().await.unwrap();
            match summarizer.summarize_file(&file_units_owned).await {
                Ok(summary) => Some((path, summary, false)),
                Err(e) => {
                    eprintln!("  ERROR summarizing {path}: {e}");
                    let fallback = summarizer::fallback_summary(&file_units_owned);
                    Some((path, fallback, true))
                }
            }
        }));
    }

    let mut new_count = 0usize;
    let mut fallback_count = 0usize;
    for handle in handles {
        match handle.await {
            Ok(Some((path, summary, is_fallback))) => {
                summaries.insert(path, summary);
                if is_fallback {
                    fallback_count += 1;
                } else {
                    new_count += 1;
                }
            }
            Ok(None) => {}
            Err(e) => {
                eprintln!("  ERROR: summarization task failed: {e}");
            }
        }
    }

    // Write summaries atomically (write to temp, then rename)
    fs::create_dir_all(&config.output_dir)?;
    let json = serde_json::to_string_pretty(&summaries)? + "\n";
    let tmp_path = output_path.with_extension("json.tmp");
    fs::write(&tmp_path, &json).context("writing summaries")?;
    if let Err(e) = fs::rename(&tmp_path, &output_path) {
        // Clean up temp file on rename failure, fall back to direct write
        let _ = fs::remove_file(&tmp_path);
        fs::write(&output_path, &json).context("writing summaries (fallback)")?;
        eprintln!("  WARN: atomic rename failed ({e}), used direct write");
    }

    // Always write meta for future refresh
    let mut new_meta = meta::MetaMap::new();
    for fu in &all_file_units {
        let full_path = config.project_dir.join(&fu.path);
        match meta::compute_entry(&full_path) {
            Ok(entry) => {
                new_meta.insert(fu.path.clone(), entry);
            }
            Err(e) => {
                eprintln!("  WARN: could not compute meta for {}: {e}", fu.path);
            }
        }
    }
    meta::save_meta(&config.output_dir, &new_meta)?;

    let fallback_note = if fallback_count > 0 {
        format!(", {fallback_count} fallback")
    } else {
        String::new()
    };
    eprintln!(
        "\n{command_name}: {new_count} summarized{fallback_note}, {skipped_count} skipped, {deleted_count} deleted ({} total) → {}",
        summaries.len(),
        output_path.display()
    );

    print_report(&report::GenerateReport {
        command: command_name,
        project_dir: config.project_dir.display().to_string(),
        output_file: output_path.display().to_string(),
        files_discovered: files.len(),
        files_parsed: all_file_units.len(),
        files_summarized: new_count,
        files_fallback: fallback_count,
        files_skipped: skipped_count,
        files_deleted: deleted_count,
        total_summaries: summaries.len(),
        parse_errors,
    });

    if !config.refresh && new_count > 0 {
        eprintln!();
        eprintln!("Summaries written to: {}", output_path.display());
        eprintln!("Next: check status with  code-primer status {}", config.project_dir.display());
        eprintln!("      refresh later with code-primer refresh {}", config.project_dir.display());
    }

    Ok(())
}

// ── Status ─────────────────────────────────────────────────────────

pub fn cmd_status(config: &Config) -> Result<()> {
    let output_path = config.output_dir.join(SUMMARIES_FILENAME);
    let initialized = output_path.exists();

    if !initialized {
        print_report(&report::StatusReport {
            command: "status",
            project_dir: config.project_dir.display().to_string(),
            output_dir: config.output_dir.display().to_string(),
            initialized: false,
            needs_refresh: true,
            files_changed: Vec::new(),
            files_new: Vec::new(),
            files_deleted: Vec::new(),
            files_unchanged: 0,
            total_tracked: 0,
        });
        return Ok(());
    }

    // Parse files to match what generate/refresh would actually process
    let parseable_paths = discover_parseable_paths(config)?;
    let old_meta = meta::load_meta(&config.output_dir)?;
    let plan = meta::diff_files(&parseable_paths, &old_meta, &config.project_dir)?;

    // Separate changed into truly changed vs new
    let mut changed = Vec::new();
    let mut new = Vec::new();
    for path in &plan.changed {
        if old_meta.contains_key(path) {
            changed.push(path.clone());
        } else {
            new.push(path.clone());
        }
    }

    let needs_refresh = !changed.is_empty() || !new.is_empty() || !plan.deleted.is_empty();

    print_report(&report::StatusReport {
        command: "status",
        project_dir: config.project_dir.display().to_string(),
        output_dir: config.output_dir.display().to_string(),
        initialized: true,
        needs_refresh,
        files_changed: changed,
        files_new: new,
        files_deleted: plan.deleted,
        files_unchanged: plan.unchanged.len(),
        total_tracked: old_meta.len(),
    });

    Ok(())
}

// ── Verify ─────────────────────────────────────────────────────────

pub fn cmd_verify(config: &Config) -> Result<()> {
    let output_path = config.output_dir.join(SUMMARIES_FILENAME);

    // Load and validate summaries JSON
    let summaries: BTreeMap<String, String> = if output_path.exists() {
        let data = fs::read_to_string(&output_path).context("reading summaries")?;
        serde_json::from_str(&data).context("invalid JSON in code-primer.json")?
    } else {
        eprintln!("code-primer.json not found — run `code-primer generate` first");
        print_report(&report::VerifyReport {
            command: "verify",
            project_dir: config.project_dir.display().to_string(),
            output_dir: config.output_dir.display().to_string(),
            valid: false,
            summaries_count: 0,
            meta_count: 0,
            hash_mismatches: Vec::new(),
            missing_summaries: Vec::new(),
            orphan_summaries: Vec::new(),
        });
        return Ok(());
    };

    // Load meta
    let meta_map = meta::load_meta(&config.output_dir)?;

    // Cross-reference: files in meta but not in summaries
    let missing_summaries: Vec<String> = meta_map
        .keys()
        .filter(|k| !summaries.contains_key(*k))
        .cloned()
        .collect();

    // Files in summaries but not in meta
    let orphan_summaries: Vec<String> = summaries
        .keys()
        .filter(|k| !meta_map.contains_key(*k))
        .cloned()
        .collect();

    // Verify SHA-256 hashes
    let mut hash_mismatches = Vec::new();
    for (rel_path, entry) in &meta_map {
        let full_path = config.project_dir.join(rel_path);
        if !full_path.exists() {
            hash_mismatches.push(format!("{rel_path} (file missing)"));
            continue;
        }
        match meta::compute_entry(&full_path) {
            Ok(current) => {
                if current.sha256 != entry.sha256 {
                    hash_mismatches.push(rel_path.clone());
                }
            }
            Err(e) => {
                hash_mismatches.push(format!("{rel_path} (error: {e})"));
            }
        }
    }

    let valid = missing_summaries.is_empty()
        && orphan_summaries.is_empty()
        && hash_mismatches.is_empty();

    print_report(&report::VerifyReport {
        command: "verify",
        project_dir: config.project_dir.display().to_string(),
        output_dir: config.output_dir.display().to_string(),
        valid,
        summaries_count: summaries.len(),
        meta_count: meta_map.len(),
        hash_mismatches,
        missing_summaries,
        orphan_summaries,
    });

    Ok(())
}

// ── Clean ──────────────────────────────────────────────────────────

/// Known code-primer output files. Used by both clean and uninstall
/// to ensure they stay in sync.
const OUTPUT_FILES: &[&str] = &[
    "code-primer.json",
    "code-primer.json.tmp",
    "code-primer.meta.json",
];

/// Remove known code-primer files from the output directory.
/// Returns (removed_any, actions) describing what was done.
fn remove_output_files(output_dir: &Path) -> Result<(bool, Vec<String>)> {
    let mut actions = Vec::new();
    let mut removed = false;

    if !output_dir.exists() {
        return Ok((false, actions));
    }

    for filename in OUTPUT_FILES {
        let path = output_dir.join(filename);
        if path.exists() {
            fs::remove_file(&path)?;
            actions.push(format!("Removed {}", path.display()));
            removed = true;
        }
    }

    // Remove dir only if empty
    if fs::read_dir(output_dir)?.next().is_none() {
        fs::remove_dir(output_dir)?;
        actions.push(format!("Removed directory {}", output_dir.display()));
    } else if !removed {
        actions.push(format!(
            "No code-primer files found in {}",
            output_dir.display()
        ));
    }

    Ok((removed, actions))
}

pub fn cmd_clean(config: &Config) -> Result<()> {
    let (removed, actions) = if config.output_dir.exists() {
        remove_output_files(&config.output_dir)?
    } else {
        (false, vec!["Output directory does not exist (nothing to clean)".to_string()])
    };

    print_report(&report::CleanReport {
        command: "clean",
        output_dir: config.output_dir.display().to_string(),
        removed,
        actions,
    });

    Ok(())
}

// ── Uninstall ──────────────────────────────────────────────────────

pub fn cmd_uninstall(config: &Config) -> Result<()> {
    let mut actions = Vec::new();

    // 1. Clean output directory
    if config.output_dir.exists() {
        let (_, clean_actions) = remove_output_files(&config.output_dir)?;
        actions.extend(clean_actions);
    }

    // 2. Remove slash command
    let command_file = config.project_dir.join(".claude").join("commands").join("refresh-primer.md");
    if command_file.exists() {
        fs::remove_file(&command_file)?;
        actions.push(format!("Removed slash command: {}", command_file.display()));

        // Clean up empty commands dir and .claude dir
        let commands_dir = command_file.parent().unwrap();
        if fs::read_dir(commands_dir)?.next().is_none() {
            fs::remove_dir(commands_dir)?;
            actions.push(format!("Removed empty directory: {}", commands_dir.display()));

            let claude_dir = commands_dir.parent().unwrap();
            if fs::read_dir(claude_dir)?.next().is_none() {
                fs::remove_dir(claude_dir)?;
                actions.push(format!("Removed empty directory: {}", claude_dir.display()));
            }
        }
    }

    // 3. Remove CLAUDE.md snippet
    let claude_md = config.project_dir.join("CLAUDE.md");
    if claude_md.exists() {
        let content = fs::read_to_string(&claude_md)?;
        if let (Some(begin_idx), Some(end_idx_start)) =
            (content.find(MARKER_BEGIN), content.find(MARKER_END))
        {
            let end_idx = end_idx_start + MARKER_END.len();
            if begin_idx >= end_idx {
                eprintln!("WARN: code-primer markers in CLAUDE.md are malformed (skipped)");
            } else {
                // Also remove surrounding newlines
                let mut start = begin_idx;
                if start > 0 && content.as_bytes()[start - 1] == b'\n' {
                    start -= 1;
                }
                let mut end = end_idx;
                if end < content.len() && content.as_bytes()[end] == b'\n' {
                    end += 1;
                }

                let new_content = format!("{}{}", &content[..start], &content[end..]);

                if new_content.trim().is_empty() {
                    fs::remove_file(&claude_md)?;
                    actions.push("Removed CLAUDE.md (was only code-primer snippet)".to_string());
                } else {
                    fs::write(&claude_md, new_content)?;
                    actions.push("Removed code-primer snippet from CLAUDE.md".to_string());
                }
            }
        }
    }

    print_report(&report::UninstallReport {
        command: "uninstall",
        project_dir: config.project_dir.display().to_string(),
        actions,
    });

    Ok(())
}

// ── File discovery ─────────────────────────────────────────────────

pub fn discover_files(config: &Config) -> Result<Vec<String>> {
    let project = &config.project_dir;

    // Build exclude glob set
    let mut exclude_builder = GlobSetBuilder::new();
    for pat in &config.exclude_patterns {
        exclude_builder.add(Glob::new(pat).context("invalid exclude pattern")?);
    }
    let exclude_set = exclude_builder.build().context("building exclude patterns")?;

    // Determine include patterns
    let include_patterns = if config.include_patterns.is_empty() {
        // Auto-detect: walk once to find known extensions
        let known_exts = ["go", "rs"];
        let mut found = std::collections::BTreeSet::new();
        for entry in WalkDir::new(project).into_iter().filter_map(|e| e.ok()) {
            if entry.file_type().is_file() {
                if let Some(ext) = entry.path().extension().and_then(|e| e.to_str()) {
                    if known_exts.contains(&ext) {
                        found.insert(format!("**/*.{ext}"));
                    }
                }
            }
        }
        if found.is_empty() {
            return Ok(Vec::new());
        }
        found.into_iter().collect()
    } else {
        config.include_patterns.clone()
    };

    let mut include_builder = GlobSetBuilder::new();
    for pat in &include_patterns {
        include_builder.add(Glob::new(pat).context("invalid include pattern")?);
    }
    let include_set = include_builder.build().context("building include patterns")?;

    let mut result = Vec::new();
    for entry in WalkDir::new(project).into_iter().filter_map(|e| e.ok()) {
        if !entry.file_type().is_file() {
            continue;
        }
        let rel = entry
            .path()
            .strip_prefix(project)
            .unwrap_or(entry.path());
        let rel_str = rel.to_string_lossy();

        if exclude_set.is_match(rel) {
            continue;
        }
        if include_set.is_match(rel) {
            result.push(rel_str.into_owned());
        }
    }

    result.sort();
    Ok(result)
}

/// Discover files and parse them, returning only paths that produce non-empty
/// translation units. This matches the set of files that generate/refresh
/// would actually process and track in meta.
fn discover_parseable_paths(config: &Config) -> Result<Vec<String>> {
    let files = discover_files(config)?;
    let mut paths = Vec::new();
    for rel_path in &files {
        let full_path = config.project_dir.join(rel_path);
        let source = match fs::read(&full_path) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("  WARN: cannot read {rel_path}: {e}");
                continue;
            }
        };
        match parser::parse_file(&source, rel_path) {
            Ok(Some(fu)) if !fu.units.is_empty() => {
                paths.push(rel_path.clone());
            }
            _ => {}
        }
    }
    Ok(paths)
}

/// Compute a relative path from `base` to `target`. Both must be absolute
/// (canonicalized). Falls back to the absolute target path if no common
/// prefix exists.
fn pathdiff(target: &Path, base: &Path) -> String {
    let target_components: Vec<_> = target.components().collect();
    let base_components: Vec<_> = base.components().collect();

    let common = target_components
        .iter()
        .zip(base_components.iter())
        .take_while(|(a, b)| a == b)
        .count();

    if common == 0 {
        return target.display().to_string();
    }

    let ups = base_components.len() - common;
    let mut result = std::path::PathBuf::new();
    for _ in 0..ups {
        result.push("..");
    }
    for component in &target_components[common..] {
        result.push(component);
    }
    result.display().to_string()
}

// ── Dry run ────────────────────────────────────────────────────────

fn print_dry_run(config: &Config, all_file_units: &[FileUnits]) {
    let mut report_files = Vec::new();
    let mut total_tokens = 0usize;

    for fu in all_file_units {
        let units_for_llm: Vec<&TranslationUnit> = fu
            .units
            .iter()
            .filter(|u| u.kind != "package" && u.kind != "imports")
            .collect();
        let tokens = estimate_tokens(&units_for_llm);
        total_tokens += tokens;

        eprintln!(
            "  {} ({} units, ~{tokens} input tokens)",
            fu.path,
            fu.units.len()
        );
        for u in &fu.units {
            let sig = if u.signature.is_empty() {
                String::new()
            } else {
                format!("  sig: {}", u.signature)
            };
            eprintln!(
                "    {:8} {:30} L{}-L{}{}",
                u.kind, u.name, u.line_start, u.line_end, sig
            );
        }

        report_files.push(report::DryRunFile {
            path: fu.path.clone(),
            units: fu.units.len(),
            input_tokens: tokens,
        });
    }

    let est_output = total_tokens / 5;
    let cost_input = total_tokens as f64 * 0.80 / 1_000_000.0;
    let cost_output = est_output as f64 * 4.00 / 1_000_000.0;
    let total_cost = cost_input + cost_output;

    eprintln!("\n  Estimated: ~{total_tokens} input tokens, ~{est_output} output tokens");
    eprintln!("  Estimated cost (Haiku): ${total_cost:.4}");

    print_report(&report::DryRunReport {
        command: "dry-run",
        project_dir: config.project_dir.display().to_string(),
        files: report_files,
        total_input_tokens: total_tokens,
        estimated_output_tokens: est_output,
        estimated_cost_usd: total_cost,
    });
}

fn estimate_tokens(units: &[&TranslationUnit]) -> usize {
    let total_chars: usize = units
        .iter()
        .map(|u| u.signature.len() + u.doc_comment.len() + u.name.len())
        .sum();
    total_chars / 4
}

/// Create an owned copy of FileUnits for spawning into a tokio task.
fn owned_file_units(fu: &FileUnits) -> FileUnits {
    FileUnits {
        path: fu.path.clone(),
        units: fu
            .units
            .iter()
            .map(|u| TranslationUnit {
                kind: u.kind,
                name: u.name.clone(),
                signature: u.signature.clone(),
                source: u.source.clone(),
                line_start: u.line_start,
                line_end: u.line_end,
                doc_comment: u.doc_comment.clone(),
                receiver: u.receiver.clone(),
            })
            .collect(),
    }
}
