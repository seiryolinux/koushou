// src/resolve.rs

use std::fs;
use std::path::{Path, PathBuf};
use std::io::Write;
use thiserror::Error;
use indicatif::{ProgressBar, ProgressStyle};
use crate::depres;

#[derive(Error, Debug)]
pub enum ResolveError {
    #[error("Package '{0}' not found")]
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
    #[error("Dependency resolution error: {0}")]
    Depres(#[from] depres::DepresError),
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

pub async fn resolve_transaction(
    package_names: Vec<&str>,
    flavour: &str,
    arch: &str,
    root: &Path,
) -> Result<Vec<ResolvedPackage>, ResolveError> {
    let universe = depres::PackageUniverse::load_from_cache(root)?;
    let root_pkgs: Vec<String> = package_names.into_iter().map(|s| s.to_string()).collect();

    let solution = universe.resolve(&root_pkgs, flavour, arch)?;

    let mut resolved = Vec::new();
    for pkg in solution.packages {
        let filename = format!("{}-{}-{}.kpkg", pkg.name, pkg.version, pkg.arch);
        resolved.push(ResolvedPackage {
            name: pkg.name.clone(),
            version: pkg.version,
            arch: pkg.arch,
            filename,
            url: solution.download_urls[&pkg.name].clone(),
            sha256: solution.sha256_sums[&pkg.name].clone(),
            depends: Vec::new(), // not needed post-resolve
        });
    }

    Ok(resolved)
}

pub async fn download_package(url: &str, output_path: &Path) -> Result<(), ResolveError> {
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

pub fn compute_sha256(path: &Path) -> Result<String, std::io::Error> {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    let mut file = fs::File::open(path)?;
    std::io::copy(&mut file, &mut hasher)?;
    Ok(format!("{:x}", hasher.finalize()))
}
