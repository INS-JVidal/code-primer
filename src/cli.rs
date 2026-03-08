use std::path::PathBuf;

use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(
    name = "code-primer",
    version,
    about = "Prime your AI coding assistant with file-level codebase understanding",
    before_long_help = "\
  ______          __        ____       _                    \n\
 / ____/___  ____/ /__     / __ \\_____(_)___ ___  ___  _____\n\
/ /   / __ \\/ __  / _ \\   / /_/ / ___/ / __ `__ \\/ _ \\/ ___/\n\
/ /___/ /_/ / /_/ /  __/  / ____/ /  / / / / / / /  __/ /    \n\
\\____/\\____/\\__,_/\\___/  /_/   /_/  /_/_/ /_/ /_/\\___/_/     ",
    long_about = "\
code-primer generates file-level natural language summaries of source code \
by parsing functions, types, and constants via tree-sitter and sending their \
signatures to an LLM. Output is a single code-primer.json per project.\n\n\
The goal: prime AI coding assistants with codebase understanding so they need \
fewer exploratory tool calls.\n\n\
Typical workflow:\n  \
1. code-primer init <project>       # set up output dir, CLAUDE.md, slash command\n  \
2. code-primer generate <project>   # create summaries (calls LLM)\n  \
3. code-primer status <project>     # check for stale summaries\n  \
4. code-primer refresh <project>    # re-summarize only changed files\n\n\
Requires claude CLI (Max/Pro subscription) or ANTHROPIC_API_KEY."
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Commands,
}

/// Shared arguments for commands that discover and process source files.
#[derive(clap::Args)]
pub struct SourceArgs {
    /// Project directory to analyze (default: current directory)
    #[arg(default_value = ".")]
    pub project_dir: PathBuf,

    /// Output directory (default: code-primer-<project>/ next to project)
    #[arg(short, long)]
    pub output: Option<PathBuf>,

    /// Model name
    #[arg(long, default_value = "claude-haiku-4-5-20251001")]
    pub model: String,

    /// File glob patterns to include (repeatable)
    #[arg(long = "include")]
    pub include_patterns: Vec<String>,

    /// File glob patterns to exclude (repeatable)
    #[arg(long = "exclude")]
    pub exclude_patterns: Vec<String>,

    /// Parse and show units without calling LLM
    #[arg(long)]
    pub dry_run: bool,

    /// Parallel LLM requests
    #[arg(long, default_value_t = 4)]
    pub concurrency: usize,

    /// Use direct API (ANTHROPIC_API_KEY) instead of claude CLI
    #[arg(long)]
    pub api: bool,
}

#[derive(Subcommand)]
pub enum Commands {
    /// Initialize code-primer for a project (output dir, CLAUDE.md snippet, slash command)
    Init {
        /// Project directory (default: current directory)
        #[arg(default_value = ".")]
        project_dir: PathBuf,

        /// Output directory (default: code-primer-<project>/ next to project)
        #[arg(short, long)]
        output: Option<PathBuf>,
    },

    /// Generate file-level summaries for a project
    Generate {
        #[command(flatten)]
        args: SourceArgs,

        /// Skip files already summarized
        #[arg(long)]
        resume: bool,
    },

    /// Re-summarize only changed/new files, prune deleted
    Refresh {
        #[command(flatten)]
        args: SourceArgs,
    },

    /// Check if project needs a refresh (detect file changes, no LLM calls)
    Status {
        /// Project directory (default: current directory)
        #[arg(default_value = ".")]
        project_dir: PathBuf,

        /// Output directory
        #[arg(short, long)]
        output: Option<PathBuf>,

        /// File glob patterns to include (repeatable)
        #[arg(long = "include")]
        include_patterns: Vec<String>,

        /// File glob patterns to exclude (repeatable)
        #[arg(long = "exclude")]
        exclude_patterns: Vec<String>,
    },

    /// Verify integrity of generated summaries (validate JSON, check hashes)
    Verify {
        /// Project directory (default: current directory)
        #[arg(default_value = ".")]
        project_dir: PathBuf,

        /// Output directory
        #[arg(short, long)]
        output: Option<PathBuf>,
    },

    /// Remove generated output directory
    Clean {
        /// Project directory (default: current directory)
        #[arg(default_value = ".")]
        project_dir: PathBuf,

        /// Output directory
        #[arg(short, long)]
        output: Option<PathBuf>,
    },

    /// Remove all code-primer artifacts (clean + CLAUDE.md snippet + slash command)
    Uninstall {
        /// Project directory (default: current directory)
        #[arg(default_value = ".")]
        project_dir: PathBuf,

        /// Output directory
        #[arg(short, long)]
        output: Option<PathBuf>,
    },
}
