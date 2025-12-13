/// src/pkgdb.rs

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;
use thiserror::Error;
use std::fs;
use std::io;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstalledPackage {
    pub name: String,
    pub version: String,
    pub arch: String,
    pub flavour: String,
    pub depends: Vec<String>,
    pub files: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PackageDatabase {
    packages: HashMap<String, InstalledPackage>,
}

#[derive(Error, Debug)]
pub enum PkgDbError {
    #[error("IO error: {0}")]
    Io(#[from] io::Error),
    #[error("JSON serialization error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("Package not found: {0}")]
    PackageNotFound(String),
}

impl PackageDatabase {
    pub fn new() -> Self {
        Self {
            packages: HashMap::new(),
        }
    }

    pub fn load_or_new<P: AsRef<Path>>(path: P) -> Result<Self, PkgDbError> {
        let path = path.as_ref();
        if path.exists() {
            let content = fs::read_to_string(path)?;
            let db: PackageDatabase = serde_json::from_str(&content)?;
            Ok(db)
        } else {
            Ok(Self::new())
        }
    }

    pub fn save<P: AsRef<Path>>(&self, path: P) -> Result<(), PkgDbError> {
        let content = serde_json::to_string_pretty(self)?;
        fs::write(path, content)?;
        Ok(())
    }

    pub fn add(&mut self, pkg: InstalledPackage) {
        self.packages.insert(pkg.name.clone(), pkg);
    }

    pub fn get(&self, name: &str) -> Result<&InstalledPackage, PkgDbError> {
        self.packages
            .get(name)
            .ok_or_else(|| PkgDbError::PackageNotFound(name.to_string()))
    }

    pub fn remove(&mut self, name: &str) -> Result<InstalledPackage, PkgDbError> {
        self.packages
            .remove(name)
            .ok_or_else(|| PkgDbError::PackageNotFound(name.to_string()))
    }

    pub fn list(&self) -> impl Iterator<Item = &InstalledPackage> {
        self.packages.values()
    }

    pub fn contains(&self, name: &str) -> bool {
        self.packages.contains_key(name)
    }
}
