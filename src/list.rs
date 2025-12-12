// src/list.rs

use std::path::Path;
use crate::pkgdb;

#[derive(Debug)]
pub struct ListError(String);

impl std::fmt::Display for ListError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::error::Error for ListError {}

pub fn list_packages(root: &Path) -> Result<(), ListError> {
    let db_path = root.join("var/lib/koushou/db.json");
    if !db_path.exists() {
        println!("No packages installed.");
        return Ok(());
    }

    let db = pkgdb::PackageDatabase::load_or_new(&db_path)
        .map_err(|e| ListError(format!("Failed to load package database: {}", e)))?;

    let mut found = false;
    for pkg in db.list() {
        found = true;
        println!("{}-{} ({})", pkg.name, pkg.version, pkg.arch);
    }

    if !found {
        println!("No packages installed.");
    }

    Ok(())
}
