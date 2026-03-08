use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};

pub struct Config {
    pub project_dir: PathBuf,
    pub output_dir: PathBuf,
    pub model: String,
    pub include_patterns: Vec<String>,
    pub exclude_patterns: Vec<String>,
    pub dry_run: bool,
    pub resume: bool,
    pub refresh: bool,
    pub concurrency: usize,
    pub force_api: bool,
}

const DEFAULT_EXCLUDES: &[&str] = &[
    "*_test.go",
    "vendor/*",
    "*.pb.go",
    "*_generated.*",
    "target/**",
    "*/target/*",
];

impl Config {
    pub fn new(project_dir: PathBuf, output_dir: Option<PathBuf>) -> Result<Self> {
        if !project_dir.exists() {
            bail!(
                "Project directory does not exist: {}",
                project_dir.display()
            );
        }
        if !project_dir.is_dir() {
            bail!(
                "Project path is not a directory: {}",
                project_dir.display()
            );
        }
        let project_dir = project_dir
            .canonicalize()
            .with_context(|| format!("resolving project directory: {}", project_dir.display()))?;

        let output = output_dir.unwrap_or_else(|| {
            let project_name = project_dir
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("project");
            project_dir
                .parent()
                .unwrap_or(Path::new("."))
                .join(format!("code-primer-{project_name}"))
        });

        Ok(Self {
            project_dir,
            output_dir: output,
            model: "claude-haiku-4-5-20251001".to_string(),
            include_patterns: Vec::new(),
            exclude_patterns: DEFAULT_EXCLUDES.iter().map(|s| s.to_string()).collect(),
            dry_run: false,
            resume: false,
            refresh: false,
            concurrency: 4,
            force_api: false,
        })
    }
}
