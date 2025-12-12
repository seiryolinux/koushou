// src/mirror.rs

use kdl::KdlDocument;
use thiserror::Error;
use std::fs;
use std::str::FromStr;

#[derive(Error, Debug)]
pub enum MirrorError {
    #[error("Failed to read mirrorlist file: {0}")]
    Io(#[from] std::io::Error),
    #[error("Invalid KDL syntax in mirrorlist: {0}")]
    Parse(#[from] kdl::KdlError),
    #[error("Mirror '{name}' is missing required property: {field}")]
    MissingProperty { name: String, field: String },
    #[error("Mirror '{name}': invalid value for '{field}': {value}")]
    InvalidValue { name: String, field: String, value: String },
}

#[derive(Debug, Clone)]
pub struct Mirror {
    pub name: String,
    pub url: String,
    pub priority: i32,
    pub protocol: String,
    pub region: String,
    pub active: bool,
}

impl Mirror {
    /// Load and parse `/etc/koushou/mirrorlist.kdl`
    pub fn load_default() -> Result<Vec<Self>, MirrorError> {
        let path = "/etc/koushou/mirrorlist.kdl";
        let content = fs::read_to_string(path)?;
        Self::from_kdl(&content)
    }

    pub fn from_kdl(input: &str) -> Result<Vec<Self>, MirrorError> {
        let doc: KdlDocument = input.parse().map_err(MirrorError::Parse)?;

        let mut mirrors = Vec::new();

        for node in doc.nodes() {
            if node.name().as_ref() != "mirror" {
                continue;
            }

            let name = node
                .entries()
                .first()
                .and_then(|e| e.value())
                .and_then(|v| v.as_string())
                .ok_or_else(|| MirrorError::MissingProperty {
                    name: "unknown".to_string(),
                    field: "name".to_string(),
                })?
                .to_string();

            let get_str_prop = |prop_name: &str| -> Result<String, MirrorError> {
                node.get(prop_name)
                    .map(|v| v.clone().into_string())
                    .ok_or_else(|| MirrorError::MissingProperty {
                        name: name.clone(),
                        field: prop_name.to_string(),
                    })
            };

            let get_prop = |prop_name: &str, default: Option<&str>| -> Result<String, MirrorError> {
                match node.get(prop_name) {
                    Some(v) => Ok(v.clone().into_string()),
                    None => match default {
                        Some(d) => Ok(d.to_string()),
                        None => Err(MirrorError::MissingProperty {
                            name: name.clone(),
                            field: prop_name.to_string(),
                        }),
                    },
                }
            };

            let url = get_str_prop("url")?;

            let priority = match node.get("priority") {
                Some(v) => {
                    let s = v.clone().into_string();
                    s.parse::<i32>().map_err(|_| MirrorError::InvalidValue {
                        name: name.clone(),
                        field: "priority".to_string(),
                        value: s,
                    })?
                }
                None => 0,
            };

            let protocol = get_prop("protocol", Some("https"))?;
            let region = get_prop("region", Some("global"))?;

            let active = match node.get("active") {
                Some(v) => {
                    let s = v.clone().into_string();
                    if s == "true" {
                        true
                    } else if s == "false" {
                        false
                    } else {
                        return Err(MirrorError::InvalidValue {
                            name: name.clone(),
                            field: "active".to_string(),
                            value: s,
                        });
                    }
                }
                None => true,
            };

            mirrors.push(Mirror {
                name,
                url,
                priority,
                protocol,
                region,
                active,
            });
        }

        mirrors.sort_by(|a, b| b.priority.cmp(&a.priority));
        mirrors.retain(|m| m.active);

        Ok(mirrors)
    }

    pub fn repo_url(&self, flavour: &str, repo: &str, arch: &str) -> String {
        format!("{}/{}/{}/{}/{}.db.zst", self.url.trim_end_matches('/'), flavour, repo, arch, repo)
    }
}
