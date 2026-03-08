use std::collections::BTreeMap;
use std::fs;
use std::sync::Arc;

use anyhow::{Context, Result};
use globset::{Glob, GlobSetBuilder};
use tokio::sync::Semaphore;
use walkdir::WalkDir;

use crate::config::Config;
use crate::meta;
use crate::parser::{self, FileUnits, TranslationUnit};
use crate::summarizer::{self, Summarizer};

const SUMMARIES_FILENAME: &str = "code-primer.json";

pub async fn run(config: &Config) -> Result<()> {
    // Validate auth early (unless dry-run)
    let summarizer = if !config.dry_run {
        Some(Arc::new(Summarizer::new(config.model.clone())?))
    } else {
        None
    };

    let files = discover_files(config)?;
    if files.is_empty() {
        println!("No matching source files found.");
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

    println!("Found {} files with {total_units} translation units.", all_file_units.len());
    if parse_errors > 0 {
        eprintln!("  ({parse_errors} files failed to parse)");
    }

    if config.dry_run {
        print_dry_run(&all_file_units);
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
    let mut refresh_plan = None;

    if config.refresh {
        let old_meta = meta::load_meta(&config.output_dir)?;
        let discovered: Vec<String> = all_file_units.iter().map(|fu| fu.path.clone()).collect();
        let plan = meta::diff_files(&discovered, &old_meta, &config.project_dir)?;

        println!(
            "Refresh: {} changed/new, {} unchanged, {} deleted",
            plan.changed.len(),
            plan.unchanged.len(),
            plan.deleted.len()
        );

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
        refresh_plan = Some(plan);
    } else if config.resume {
        files_to_process = all_file_units
            .iter()
            .filter(|fu| !summaries.contains_key(&fu.path))
            .collect();
    } else {
        files_to_process = all_file_units.iter().collect();
    };

    let total = files_to_process.len();
    if total == 0 && !config.refresh {
        println!("All files already summarized.");
    }

    // Summarize concurrently
    let sem = Arc::new(Semaphore::new(config.concurrency));
    let mut handles = Vec::new();

    for (i, fu) in files_to_process.iter().enumerate() {
        let path = fu.path.clone();
        let idx = i + 1;

        if config.resume && summaries.contains_key(&path) {
            println!("  [{idx}/{total}] SKIP (exists) {path}");
            continue;
        }

        println!("  [{idx}/{total}] {path} ({} units)", fu.units.len());

        let sem = sem.clone();
        let summarizer = summarizer.clone();
        // Build prompt data before spawning
        let file_units_owned = owned_file_units(fu);

        handles.push(tokio::spawn(async move {
            let _permit = sem.acquire().await.unwrap();
            match summarizer.summarize_file(&file_units_owned).await {
                Ok(summary) => Some((path, summary)),
                Err(e) => {
                    eprintln!("  ERROR summarizing {path}: {e}");
                    let fallback = summarizer::fallback_summary(&file_units_owned);
                    Some((path, fallback))
                }
            }
        }));
    }

    let mut new_count = 0usize;
    for handle in handles {
        if let Ok(Some((path, summary))) = handle.await {
            summaries.insert(path, summary);
            new_count += 1;
        }
    }

    // Write summaries
    fs::create_dir_all(&config.output_dir)?;
    let json = serde_json::to_string_pretty(&summaries)? + "\n";
    fs::write(&output_path, &json).context("writing summaries")?;

    // Always write meta for future --refresh
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

    if let Some(plan) = refresh_plan {
        println!(
            "\nRefreshed: {new_count} updated, {} deleted ({} total) → {output_path}",
            plan.deleted.len(),
            summaries.len(),
            output_path = output_path.display()
        );
    } else {
        println!(
            "\nDone. {new_count} new summaries ({} total) → {}",
            summaries.len(),
            output_path.display()
        );
    }

    Ok(())
}

fn discover_files(config: &Config) -> Result<Vec<String>> {
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

fn print_dry_run(all_file_units: &[FileUnits]) {
    let mut total_tokens = 0usize;

    for fu in all_file_units {
        let units_for_llm: Vec<&TranslationUnit> = fu
            .units
            .iter()
            .filter(|u| u.kind != "package" && u.kind != "imports")
            .collect();
        let tokens = estimate_tokens(&units_for_llm);
        total_tokens += tokens;

        println!(
            "\n  {} ({} units, ~{tokens} input tokens)",
            fu.path,
            fu.units.len()
        );
        for u in &fu.units {
            let sig = if u.signature.is_empty() {
                String::new()
            } else {
                format!("  sig: {}", u.signature)
            };
            println!(
                "    {:8} {:30} L{}-L{}{}",
                u.kind, u.name, u.line_start, u.line_end, sig
            );
        }
    }

    let est_output = total_tokens / 5;
    let cost_input = total_tokens as f64 * 0.80 / 1_000_000.0;
    let cost_output = est_output as f64 * 4.00 / 1_000_000.0;
    let total_cost = cost_input + cost_output;

    println!("\n  Estimated: ~{total_tokens} input tokens, ~{est_output} output tokens");
    println!("  Estimated cost (Haiku): ${total_cost:.4}");
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
