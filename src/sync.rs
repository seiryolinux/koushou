// src/sync.rs

use std::fs;
use std::path::{Path, PathBuf};
use zstd::stream::read::Decoder as ZstdDecoder;
use std::io::Read;
use thiserror::Error;
use serde::{Deserialize, Serialize};

#[derive(Error, Debug)]
pub enum SyncError {
    #[error("HTTP error: {0}")]
    Http(#[from] reqwest::Error),
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("TOML parse error: {0}")]
    Toml(#[from] toml::de::Error),
    #[error("Failed to read flavour from {{root}}/etc/koushou/flavour")]
    MissingFlavour,
    #[error("Unsupported architecture: {0}")]
    UnsupportedArch(String),
    #[error("Other: {0}")]
    Other(String),
}

#[derive(Debug, Serialize, Deserialize)]
pub struct RepoPackage {
    pub version: String,
    pub arch: String,
    pub filename: String,
    pub sha256: String,
    pub depends: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct RepoDatabase {
    #[serde(flatten)]
    pub packages: std::collections::HashMap<String, RepoPackage>,
}

fn detect_arch() -> Result<String, SyncError> {
    match std::env::consts::ARCH {
        "x86_64" => Ok("x86_64".to_string()),
        "aarch64" => Ok("aarch64".to_string()),
        arch => Err(SyncError::UnsupportedArch(arch.to_string())),
    }
}

fn read_flavour(root: &Path) -> Result<String, SyncError> {
    let flavour_path = root.join("etc/koushou/flavour");
    if !flavour_path.exists() {
        return Err(SyncError::MissingFlavour);
    }
    let content = fs::read_to_string(flavour_path)?;
    Ok(content.trim().to_string())
}

pub async fn sync_repos(root: &Path) -> Result<(), SyncError> {
    println!("📡 Syncing repositories...");

    let flavour = read_flavour(root)?;
    let arch = detect_arch()?;
    let cache_dir = root.join("var/cache/koushou/repos");
    fs::create_dir_all(&cache_dir)?;

    let repo_base = "https://seiryolinux.github.io/repo";

    sync_repo(repo_base, &flavour, "core", &arch, &cache_dir).await?;
    sync_repo(repo_base, &flavour, "main", &arch, &cache_dir).await?;

    println!("✓ Repos synced successfully.");
    Ok(())
}

async fn sync_repo(
    repo_base: &str,
    flavour: &str,
    repo_name: &str,
    arch: &str,
    cache_dir: &Path,
) -> Result<(), SyncError> {
    let url = format!(
        "{}/{}/{}/{}/{}.db.zst",
        repo_base, flavour, repo_name, arch, repo_name
    );
    let cache_path = cache_dir.join(format!("{}.db.zst", repo_name));

    println!("  → Fetching {}", url);

    let response = reqwest::get(&url).await?;
    if !response.status().is_success() {
        eprintln!("    ⚠️ Repo {} not found ({}). Skipping.", repo_name, response.status());
        return Ok(());
    }

    let bytes = response.bytes().await?;
    fs::write(&cache_path, &bytes)?;

    let mut decoder = ZstdDecoder::new(&bytes[..])?;
    let mut db_content = String::new();
    decoder.read_to_string(&mut db_content)?;

    let _db: RepoDatabase = toml::from_str(&db_content)
        .map_err(|e| SyncError::Other(format!("Invalid repo DB {}: {}", repo_name, e)))?;

    fs::write(cache_dir.join(format!("{}.db", repo_name)), db_content)?;

    println!("    ✓ {} synced", repo_name);
    Ok(())
}
