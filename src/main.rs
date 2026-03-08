mod cli;
mod config;
mod meta;
mod parser;
mod pipeline;
mod report;
mod summarizer;

use std::process;

use clap::Parser;

use cli::{Cli, Commands, SourceArgs};

impl SourceArgs {
    fn into_config(self) -> anyhow::Result<config::Config> {
        let mut cfg = config::Config::new(self.project_dir, self.output)?;
        cfg.model = self.model;
        cfg.dry_run = self.dry_run;
        cfg.concurrency = self.concurrency;
        cfg.force_api = self.api;
        if !self.include_patterns.is_empty() {
            cfg.include_patterns = self.include_patterns;
        }
        if !self.exclude_patterns.is_empty() {
            cfg.exclude_patterns = self.exclude_patterns;
        }
        Ok(cfg)
    }
}

#[tokio::main]
async fn main() {
    if let Err(e) = run().await {
        eprintln!("ERROR: {e:#}");
        process::exit(1);
    }
}

async fn run() -> anyhow::Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Init { project_dir, output } => {
            pipeline::cmd_init(&project_dir, output.as_deref())
        }

        Commands::Generate { args, resume } => {
            let mut cfg = args.into_config()?;
            cfg.resume = resume;
            cfg.refresh = false;
            pipeline::cmd_generate(&cfg).await
        }

        Commands::Refresh { args } => {
            let mut cfg = args.into_config()?;
            cfg.refresh = true;
            pipeline::cmd_generate(&cfg).await
        }

        Commands::Status {
            project_dir,
            output,
            include_patterns,
            exclude_patterns,
        } => {
            let mut cfg = config::Config::new(project_dir, output)?;
            if !include_patterns.is_empty() {
                cfg.include_patterns = include_patterns;
            }
            if !exclude_patterns.is_empty() {
                cfg.exclude_patterns = exclude_patterns;
            }
            pipeline::cmd_status(&cfg)
        }

        Commands::Verify { project_dir, output } => {
            let cfg = config::Config::new(project_dir, output)?;
            pipeline::cmd_verify(&cfg)
        }

        Commands::Clean { project_dir, output } => {
            let cfg = config::Config::new(project_dir, output)?;
            pipeline::cmd_clean(&cfg)
        }

        Commands::Uninstall { project_dir, output } => {
            let cfg = config::Config::new(project_dir, output)?;
            pipeline::cmd_uninstall(&cfg)
        }
    }
}
