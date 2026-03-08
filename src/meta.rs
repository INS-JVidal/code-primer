use std::collections::BTreeMap;
use std::fs;
use std::path::Path;
use std::time::UNIX_EPOCH;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

#[derive(Debug, Serialize, Deserialize)]
pub struct MetaEntry {
    pub sha256: String,
    pub mtime: u64,
}

pub type MetaMap = BTreeMap<String, MetaEntry>;

#[derive(Debug)]
pub struct RefreshPlan {
    pub changed: Vec<String>,
    pub unchanged: Vec<String>,
    pub deleted: Vec<String>,
}

const META_FILENAME: &str = "code-primer.meta.json";

pub fn load_meta(output_dir: &Path) -> Result<MetaMap> {
    let path = output_dir.join(META_FILENAME);
    if !path.exists() {
        return Ok(MetaMap::new());
    }
    let data = fs::read_to_string(&path).context("reading meta.json")?;
    let meta: MetaMap = serde_json::from_str(&data).context("parsing meta.json")?;
    Ok(meta)
}

pub fn save_meta(output_dir: &Path, meta: &MetaMap) -> Result<()> {
    fs::create_dir_all(output_dir)?;
    let path = output_dir.join(META_FILENAME);
    let data = serde_json::to_string_pretty(meta)? + "\n";
    fs::write(&path, data).context("writing meta.json")?;
    Ok(())
}

pub fn compute_entry(full_path: &Path) -> Result<MetaEntry> {
    let content = fs::read(full_path).with_context(|| format!("reading {}", full_path.display()))?;
    let mut hasher = Sha256::new();
    hasher.update(&content);
    let hash = hex::encode(hasher.finalize());

    let metadata = fs::metadata(full_path)?;
    let mtime = metadata
        .modified()?
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    Ok(MetaEntry {
        sha256: hash,
        mtime,
    })
}

pub fn diff_files(
    discovered: &[String],
    old_meta: &MetaMap,
    project_dir: &Path,
) -> Result<RefreshPlan> {
    let mut changed = Vec::new();
    let mut unchanged = Vec::new();

    for rel_path in discovered {
        let full_path = project_dir.join(rel_path);
        match old_meta.get(rel_path) {
            None => {
                // New file
                changed.push(rel_path.clone());
            }
            Some(old_entry) => {
                // Check mtime first as fast-path
                let metadata = fs::metadata(&full_path)?;
                let mtime = metadata
                    .modified()?
                    .duration_since(UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs();

                if mtime == old_entry.mtime {
                    unchanged.push(rel_path.clone());
                } else {
                    // Mtime changed — verify with hash
                    let entry = compute_entry(&full_path)?;
                    if entry.sha256 == old_entry.sha256 {
                        unchanged.push(rel_path.clone());
                    } else {
                        changed.push(rel_path.clone());
                    }
                }
            }
        }
    }

    // Files in old meta but not discovered = deleted
    let discovered_set: std::collections::HashSet<&str> =
        discovered.iter().map(|s| s.as_str()).collect();
    let deleted: Vec<String> = old_meta
        .keys()
        .filter(|k| !discovered_set.contains(k.as_str()))
        .cloned()
        .collect();

    Ok(RefreshPlan {
        changed,
        unchanged,
        deleted,
    })
}
