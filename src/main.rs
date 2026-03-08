mod config;
mod meta;
mod parser;
mod pipeline;
mod summarizer;

use std::path::PathBuf;
use std::process;

use clap::Parser;

#[derive(Parser)]
#[command(name = "code-primer", version, about = "Prime your AI coding assistant with file-level codebase understanding")]
struct Cli {
    /// Project directory to analyze
    project_dir: PathBuf,

    /// Output directory (default: literate-code-<project>/ next to project)
    #[arg(short, long)]
    output: Option<PathBuf>,

    /// Model name
    #[arg(long, default_value = "claude-haiku-4-5-20251001")]
    model: String,

    /// File glob patterns to include (repeatable)
    #[arg(long = "include")]
    include_patterns: Vec<String>,

    /// File glob patterns to exclude (repeatable)
    #[arg(long = "exclude")]
    exclude_patterns: Vec<String>,

    /// Parse and show units without calling LLM
    #[arg(long)]
    dry_run: bool,

    /// Skip files already in literate-summaries.json
    #[arg(long, conflicts_with = "refresh")]
    resume: bool,

    /// Re-summarize only changed/new files, prune deleted
    #[arg(long, conflicts_with = "resume")]
    refresh: bool,

    /// Parallel LLM requests
    #[arg(long, default_value_t = 4)]
    concurrency: usize,
}

#[tokio::main]
async fn main() {
    let cli = Cli::parse();

    let mut cfg = config::Config::new(cli.project_dir, cli.output);
    cfg.model = cli.model;
    cfg.dry_run = cli.dry_run;
    cfg.resume = cli.resume;
    cfg.refresh = cli.refresh;
    cfg.concurrency = cli.concurrency;

    if !cli.include_patterns.is_empty() {
        cfg.include_patterns = cli.include_patterns;
    }
    if !cli.exclude_patterns.is_empty() {
        cfg.exclude_patterns = cli.exclude_patterns;
    }

    if let Err(e) = pipeline::run(&cfg).await {
        eprintln!("ERROR: {e:#}");
        process::exit(1);
    }
}
