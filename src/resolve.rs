// src/resolve.rs

use std::fs;
use std::path::{Path, PathBuf};
use std::io::Write;
use thiserror::Error;
use serde::{Deserialize, Serialize};
use indicatif::{ProgressBar, ProgressStyle};

#[derive(Error, Debug)]
pub enum ResolveError {
    #[error("Package '{0}' not found in any repository")]
    NotFound(String),
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("TOML parse error: {0}")]
    Toml(#[from] toml::de::Error),
    #[error("HTTP error: {0}")]
    Http(#[from] reqwest::Error),
    #[error("SHA256 mismatch for {filename}: expected {expected}, got {actual}")]
    Sha256Mismatch {
        filename: String,
        expected: String,
        actual: String,
    },
}

#[derive(Debug, Serialize, Deserialize)]
struct RepoPackage {
    version: String,
    arch: String,
    filename: String,
    sha256: String,
    depends: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize)]
struct RepoDatabase {
    #[serde(flatten)]
    packages: std::collections::HashMap<String, RepoPackage>,
}

#[derive(Debug, Clone)]
pub struct ResolvedPackage {
    pub name: String,
    pub version: String,
    pub arch: String,
    pub filename: String,
    pub url: String,
    pub sha256: String,
    pub depends: Vec<String>,
}

pub async fn resolve_and_download(
    name: &str,
    flavour: &str,
    arch: &str,
    root: &Path,
    cache_dir: &Path,
) -> Result<PathBuf, ResolveError> {
    let resolved = resolve_package(name, flavour, arch, root)?;

    let output_path = cache_dir.join(&resolved.filename);
    if output_path.exists() {
        // TODO: verify existing file SHA256
    }

    println!("📥 Fetching {}...", resolved.filename);
    download_with_progress(&resolved.url, &output_path).await?;

    let actual_sha = compute_sha256(&output_path)?;
    if actual_sha != resolved.sha256 {
        return Err(ResolveError::Sha256Mismatch {
            filename: resolved.filename,
            expected: resolved.sha256,
            actual: actual_sha,
        });
    }

    Ok(output_path)
}

fn resolve_package(
    name: &str,
    flavour: &str,
    arch: &str,
    root: &Path,
) -> Result<ResolvedPackage, ResolveError> {
    for repo in ["core", "main", "extra"] {
        let db_path = root.join(format!("var/cache/koushou/repos/{}.db", repo));
        if !db_path.exists() {
            continue;
        }

        let content = fs::read_to_string(&db_path)?;
        let db: RepoDatabase = toml::from_str(&content)?;

        if let Some(pkg) = db.packages.get(name) {
            if pkg.arch == arch {
                let url = format!(
                    "https://seiryolinux.github.io/repo/{}/{}/{}/{}",
                    flavour, repo, arch, pkg.filename
                );
                return Ok(ResolvedPackage {
                    name: name.to_string(),
                    version: pkg.version.clone(),
                    arch: pkg.arch.clone(),
                    filename: pkg.filename.clone(),
                    url,
                    sha256: pkg.sha256.clone(),
                    depends: pkg.depends.clone(),
                });
            }
        }
    }

    Err(ResolveError::NotFound(name.to_string()))
}

async fn download_with_progress(url: &str, output_path: &Path) -> Result<(), ResolveError> {
    let response = reqwest::get(url).await?;
    let total_size = response.content_length().unwrap_or(0);

    let pb = ProgressBar::new(total_size);
    pb.set_style(
        ProgressStyle::default_bar()
            .template("{spinner:.green} [{elapsed_precise}] [{bar:40.cyan/blue}] {bytes}/{total_bytes} ({eta})")
            .unwrap()
            .progress_chars("=>-"),
    );

    let mut file = fs::File::create(output_path)?;
    let mut stream = response.bytes_stream();
    let mut downloaded: u64 = 0;

    use futures_util::StreamExt;
    while let Some(item) = stream.next().await {
        let chunk = item.map_err(|e| ResolveError::Http(e.into()))?;
        file.write_all(&chunk)?;
        downloaded += chunk.len() as u64;
        pb.set_position(downloaded);
    }

    pb.finish_with_message("Downloaded");
    Ok(())
}

fn compute_sha256(path: &Path) -> Result<String, std::io::Error> {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    let mut file = fs::File::open(path)?;
    std::io::copy(&mut file, &mut hasher)?;
    Ok(format!("{:x}", hasher.finalize()))
}
