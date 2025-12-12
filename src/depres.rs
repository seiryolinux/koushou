// src/depres.rs

use std::collections::{HashMap, HashSet};
use std::path::Path;
use thiserror::Error;
use serde::{Deserialize, Serialize};
use regex::Regex;

#[derive(Error, Debug)]
pub enum DepresError {
    #[error("Package '{0}' not found in any repository")]
    PackageNotFound(String),
    #[error("No solution found: {0}")]
    NoSolution(String),
    #[error("Flavor mismatch: package requires '{required}', system is '{system}'")]
    FlavorMismatch { required: String, system: String },
    #[error("Circular dependency detected involving: {0}")]
    CircularDependency(String),
    #[error("Version constraint not satisfied: {0}")]
    VersionConstraint(String),
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("TOML parse error: {0}")]
    Toml(#[from] toml::de::Error),
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct PackageId {
    pub name: String,
    pub version: String,
    pub arch: String,
    pub flavor: String,
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
        let mut packages: HashMap<(String, String, String), Vec<PackageMetadata>> = HashMap::new();

        for repo in ["core", "main", "extra"] {
            let db_path = root.join(format!("var/cache/koushou/repos/{}.db", repo));
            if !db_path.exists() {
                continue;
            }

            let content = std::fs::read_to_string(&db_path)?;
            let db: RepoDatabase = toml::from_str(&content)?;

            for (name, pkg) in db.packages {
                let id = PackageId {
                    name: name.clone(),
                    version: pkg.version.clone(),
                    arch: pkg.arch.clone(),
                    flavor: pkg.flavor.clone(),
                };

                let key = (name, pkg.arch.clone(), pkg.flavor.clone());
                let url = format!(
                    "https://seiryolinux.github.io/repo/{}/{}/{}/{}",
                    pkg.flavor, repo, pkg.arch, pkg.filename
                );

                let mut depends = Vec::new();
                for dep_str in &pkg.depends {
                    // Parse "glibc>=2.38" or "bash"
                    if let Some((dep_name, constraint)) = parse_dependency(dep_str) {
                        depends.push(Dependency {
                            name: dep_name,
                            predicate: constraint,
                        });
                    }
                }

                packages.entry(key).or_default().push(PackageMetadata {
                    id,
                    url,
                    sha256: pkg.sha256,
                    depends,
                });
            }
        }

        Ok(Self { packages })
    }

    pub fn resolve(
        &self,
        root_packages: &[String],
        system_flavor: &str,
        arch: &str,
    ) -> Result<ResolutionSolution, DepresError> {
        let mut selected: HashMap<String, PackageMetadata> = HashMap::new();
        let mut visited: HashSet<String> = HashSet::new();

        for pkg_name in root_packages {
            self.resolve_package(pkg_name, system_flavor, arch, &mut selected, &mut visited)?;
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
        flavor: &str,
        arch: &str,
        selected: &mut HashMap<String, PackageMetadata>,
        visited: &mut HashSet<String>,
    ) -> Result<(), DepresError> {
        if visited.contains(name) {
            return Err(DepresError::CircularDependency(name.to_string()));
        }
        visited.insert(name.to_string());

        let key = (name.to_string(), arch.to_string(), flavor.to_string());
        let candidates = self.packages.get(&key)
            .ok_or_else(|| DepresError::PackageNotFound(name.to_string()))?;

        let best = candidates.iter()
            .max_by_key(|m| &m.id.version)
            .unwrap();

        if best.id.flavor != flavor {
            return Err(DepresError::FlavorMismatch {
                required: best.id.flavor.clone(),
                system: flavor.to_string(),
            });
        }

        selected.insert(name.to_string(), best.clone());

        for dep in &best.depends {
            if !selected.contains_key(&dep.name) {
                self.resolve_package(&dep.name, flavor, arch, selected, visited)?;
            }
            // TODO: Validate version constraint
        }

        visited.remove(name);
        Ok(())
    }
}

fn parse_dependency(s: &str) -> Option<(String, VersionPredicate)> {
    let re = Regex::new(r"^([a-zA-Z0-9._-]+)([<>=!]+)?(.*)$").ok()?;
    if let Some(caps) = re.captures(s) {
        let name = caps.get(1)?.as_str().to_string();
        let op = caps.get(2)?.as_str();
        let version = caps.get(3)?.as_str();

        let predicate = match op {
            ">=" => VersionPredicate::GreaterOrEqual(version.to_string()),
            "<" => VersionPredicate::LessThan(version.to_string()),
            "=" | "==" => VersionPredicate::Exact(version.to_string()),
            "" => VersionPredicate::Any,
            _ => return None,
        };

        Some((name, predicate))
    } else {
        Some((s.to_string(), VersionPredicate::Any))
    }
}

#[derive(Debug, Serialize, Deserialize)]
struct RepoPackage {
    version: String,
    arch: String,
    flavor: String,
    filename: String,
    sha256: String,
    depends: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize)]
struct RepoDatabase {
    #[serde(flatten)]
    packages: HashMap<String, RepoPackage>,
}

/// Final solution
#[derive(Debug)]
pub struct ResolutionSolution {
    pub packages: Vec<PackageId>,
    pub download_urls: HashMap<String, String>,
    pub sha256_sums: HashMap<String, String>,
}
