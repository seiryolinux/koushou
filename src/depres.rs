// src/depres.rs

use std::collections::{HashMap, HashSet};
use std::path::Path;
use thiserror::Error;
use serde::{Deserialize, Serialize};
use regex::Regex;
use rusqlite::{Connection, params};

#[derive(Error, Debug)]
pub enum DepresError {
    #[error("Package '{0}' not found in any repository")]
    PackageNotFound(String),
    #[error("No solution found: {0}")]
    NoSolution(String),
    #[error("Flavour mismatch: package requires '{required}', system is '{system}'")]
    FlavourMismatch { required: String, system: String },
    #[error("Circular dependency detected involving: {0}")]
    CircularDependency(String),
    #[error("Version constraint not satisfied: {0}")]
    VersionConstraint(String),
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("SQLite error: {0}")]
    Sqlite(#[from] rusqlite::Error),
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct PackageId {
    pub name: String,
    pub version: String,
    pub arch: String,
    pub flavour: String,
}

impl PackageId {
    pub fn key(&self) -> String {
        format!("{}-{}-{}", self.name, self.version, self.arch)
    }
}

#[derive(Debug, Clone)]
pub enum VersionPredicate {
    Any,
    Exact(String),
    GreaterOrEqual(String),
    LessThan(String),
}

impl VersionPredicate {
    pub fn matches(&self, candidate: &str) -> bool {
        match self {
            VersionPredicate::Any => true,
            VersionPredicate::Exact(v) => candidate == v,
            VersionPredicate::GreaterOrEqual(v) => candidate >= v.as_str(),
            VersionPredicate::LessThan(v) => candidate < v.as_str(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct Dependency {
    pub name: String,
    pub predicate: VersionPredicate,
}

#[derive(Debug, Clone)]
pub struct PackageMetadata {
    pub id: PackageId,
    pub url: String,
    pub sha256: String,
    pub depends: Vec<Dependency>,
}

#[derive(Debug)]
pub struct PackageUniverse {
    packages: HashMap<(String, String, String), Vec<PackageMetadata>>,
}

impl PackageUniverse {
    pub fn load_from_cache(root: &Path) -> Result<Self, DepresError> {
        let db_path = root.join("var/cache/koushou/repos/core.db");
        let conn = Connection::open(&db_path)?;

        let mut packages: HashMap<(String, String, String), Vec<PackageMetadata>> = HashMap::new();

        let mut stmt = conn.prepare(
            "SELECT name, version, arch, flavour, filename, sha256 FROM packages"
        )?;
        let pkg_iter = stmt.query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, String>(4)?,
                row.get::<_, String>(5)?,
            ))
        })?;

        for pkg in pkg_iter {
            let (name, version, arch, flavour, filename, sha256) = pkg?;
            let id = PackageId {
                name: name.clone(),
                version: version.clone(),
                arch: arch.clone(),
                flavour: flavour.clone(),
            };
            let url = format!(
                "https://seiryolinux.github.io/repo/{}/{}/{}/{}",
                flavour, "core", arch, filename
            );
            packages.entry((name, arch, flavour)).or_default().push(PackageMetadata {
                id,
                url,
                sha256,
                depends: Vec::new(),
            });
        }

        let mut dep_stmt = conn.prepare(
            "SELECT package_name, dep_name, dep_predicate FROM dependencies"
        )?;
        let dep_iter = dep_stmt.query_map([], |row| {
            Ok((
                row.get(0)?, // package_name
                row.get(1)?, // dep_name
                row.get(2)?, // dep_predicate (TEXT, may be NULL)
            ))
        })?;

        let mut dep_map: HashMap<String, Vec<(String, Option<String>)>> = HashMap::new();
        for dep in dep_iter {
            let (pkg_name, dep_name, predicate) = dep?;
            dep_map.entry(pkg_name).or_default().push((dep_name, predicate));
        }

        for pkg_list in packages.values_mut() {
            for pkg in pkg_list {
                if let Some(deps) = dep_map.get(&pkg.id.name) {
                    for (dep_name, predicate_str) in deps {
                        let predicate = match predicate_str.as_deref() {
                            Some(p) if p.starts_with(">=") => {
                                VersionPredicate::GreaterOrEqual(p[2..].to_string())
                            }
                            Some(p) if p.starts_with("<") => {
                                VersionPredicate::LessThan(p[1..].to_string())
                            }
                            Some(p) if p.starts_with("=") => {
                                VersionPredicate::Exact(p[1..].to_string())
                            }
                            Some(p) => VersionPredicate::Exact(p.to_string()),
                            None => VersionPredicate::Any,
                            _ => VersionPredicate::Any,
                        };
                        pkg.depends.push(Dependency {
                            name: dep_name.clone(),
                            predicate,
                        });
                    }
                }
            }
        }

        Ok(Self { packages })
    }

    pub fn resolve(
        &self,
        root_packages: &[String],
        system_flavour: &str,
        arch: &str,
    ) -> Result<ResolutionSolution, DepresError> {
        let mut selected: HashMap<String, PackageMetadata> = HashMap::new();
        let mut visited: HashSet<String> = HashSet::new();

        for pkg_name in root_packages {
            self.resolve_package(pkg_name, system_flavour, arch, &mut selected, &mut visited)?;
        }

        let mut packages = Vec::new();
        let mut download_urls = HashMap::new();
        let mut sha256_sums = HashMap::new();

        for meta in selected.values() {
            packages.push(meta.id.clone());
            download_urls.insert(meta.id.name.clone(), meta.url.clone());
            sha256_sums.insert(meta.id.name.clone(), meta.sha256.clone());
        }

        Ok(ResolutionSolution {
            packages,
            download_urls,
            sha256_sums,
        })
    }

    fn resolve_package(
        &self,
        name: &str,
        flavour: &str,
        arch: &str,
        selected: &mut HashMap<String, PackageMetadata>,
        visited: &mut HashSet<String>,
    ) -> Result<(), DepresError> {
        if visited.contains(name) {
            return Err(DepresError::CircularDependency(name.to_string()));
        }
        visited.insert(name.to_string());

        let key = (name.to_string(), arch.to_string(), flavour.to_string());
        let candidates = self.packages.get(&key)
            .ok_or_else(|| DepresError::PackageNotFound(name.to_string()))?;

        let best = candidates.iter()
            .max_by_key(|m| &m.id.version)
            .unwrap();

        if best.id.flavour != flavour {
            return Err(DepresError::FlavourMismatch {
                required: best.id.flavour.clone(),
                system: flavour.to_string(),
            });
        }

        selected.insert(name.to_string(), best.clone());

        for dep in &best.depends {
            if !selected.contains_key(&dep.name) {
                self.resolve_package(&dep.name, flavour, arch, selected, visited)?;
            }
        }

        visited.remove(name);
        Ok(())
    }
}

fn parse_dependency(s: &str) -> Option<(String, Option<String>)> {
    let re = Regex::new(r"^([a-zA-Z0-9._-]+)([<>=!]+)?(.*)$").ok()?;
    if let Some(caps) = re.captures(s) {
        let name = caps.get(1)?.as_str().to_string();
        let op = caps.get(2)?.as_str();
        let version = caps.get(3)?.as_str();
        if version.is_empty() {
            Some((name, None))
        } else {
            Some((name, Some(format!("{}{}", op, version))))
        }
    } else {
        Some((s.to_string(), None))
    }
}

#[derive(Debug)]
pub struct ResolutionSolution {
    pub packages: Vec<PackageId>,
    pub download_urls: HashMap<String, String>,
    pub sha256_sums: HashMap<String, String>,
}
