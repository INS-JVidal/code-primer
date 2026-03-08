use serde::Serialize;

// ── Init ───────────────────────────────────────────────────────────

#[derive(Serialize)]
pub struct InitReport {
    pub command: &'static str,
    pub project_dir: String,
    pub output_dir: String,
    pub actions: Vec<String>,
}

// ── Generate / Refresh ─────────────────────────────────────────────

#[derive(Serialize)]
pub struct GenerateReport {
    pub command: &'static str,
    pub project_dir: String,
    pub output_file: String,
    pub files_discovered: usize,
    pub files_parsed: usize,
    pub files_summarized: usize,
    pub files_fallback: usize,
    pub files_skipped: usize,
    pub files_deleted: usize,
    pub total_summaries: usize,
    pub parse_errors: usize,
}

// ── Dry Run ────────────────────────────────────────────────────────

#[derive(Serialize)]
pub struct DryRunReport {
    pub command: &'static str,
    pub project_dir: String,
    pub files: Vec<DryRunFile>,
    pub total_input_tokens: usize,
    pub estimated_output_tokens: usize,
    pub estimated_cost_usd: f64,
}

#[derive(Serialize)]
pub struct DryRunFile {
    pub path: String,
    pub units: usize,
    pub input_tokens: usize,
}

// ── Status ─────────────────────────────────────────────────────────

#[derive(Serialize)]
pub struct StatusReport {
    pub command: &'static str,
    pub project_dir: String,
    pub output_dir: String,
    pub initialized: bool,
    pub needs_refresh: bool,
    pub files_changed: Vec<String>,
    pub files_new: Vec<String>,
    pub files_deleted: Vec<String>,
    pub files_unchanged: usize,
    pub total_tracked: usize,
}

// ── Verify ─────────────────────────────────────────────────────────

#[derive(Serialize)]
pub struct VerifyReport {
    pub command: &'static str,
    pub project_dir: String,
    pub output_dir: String,
    pub valid: bool,
    pub summaries_count: usize,
    pub meta_count: usize,
    pub hash_mismatches: Vec<String>,
    pub missing_summaries: Vec<String>,
    pub orphan_summaries: Vec<String>,
}

// ── Clean ──────────────────────────────────────────────────────────

#[derive(Serialize)]
pub struct CleanReport {
    pub command: &'static str,
    pub output_dir: String,
    pub removed: bool,
    pub actions: Vec<String>,
}

// ── Uninstall ──────────────────────────────────────────────────────

#[derive(Serialize)]
pub struct UninstallReport {
    pub command: &'static str,
    pub project_dir: String,
    pub actions: Vec<String>,
}

// ── Helpers ────────────────────────────────────────────────────────

pub fn print_report(report: &impl Serialize) {
    println!("{}", serde_json::to_string_pretty(report).unwrap());
}
