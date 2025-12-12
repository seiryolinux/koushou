// src/removal.rs

use std::path::Path;
use thiserror::Error;
use crate::pkgdb::{PackageDatabase, PkgDbError};

#[derive(Error, Debug)]
pub enum RemovalError {
    #[error("Package not installed: {0}")]
    NotInstalled(String),
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("Package database error: {0}")]
    PkgDb(#[from] PkgDbError),
}

pub fn remove_package(root: &Path, package_name: &str) -> Result<(), RemovalError> {
    let db_path = root.join("var/lib/koushou/db.json");
    if !db_path.exists() {
        return Err(RemovalError::NotInstalled(package_name.to_string()));
    }

    let mut db = PackageDatabase::load_or_new(&db_path)?;
    let installed_pkg = db.remove(package_name)
        .map_err(|_| RemovalError::NotInstalled(package_name.to_string()))?;

    // DELETE ONLY FILES (directories are left behind — safe!)
    for file_path in installed_pkg.files.iter().rev() {
        let abs_path = root.join(file_path);
        if abs_path.exists() {
            std::fs::remove_file(&abs_path)?;
        }
    }

    // Save updated DB
    db.save(&db_path)?;
    println!("✓ Removed {} from {}", package_name, root.display());
    Ok(())
}
