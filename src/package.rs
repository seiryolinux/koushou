// src/package.rs

use kdl::KdlDocument;
use thiserror::Error;

#[derive(Debug, Clone)]
pub struct Package {
    pub name: String,
    pub version: String,
    pub arch: String,
    pub flavor: String,
    pub depends: Vec<String>,
    pub homepage: Option<String>,
    pub license: Option<String>,
}

#[derive(Error, Debug)]
pub enum PackageParseError {
    #[error("Failed to parse KDL: {0}")]
    Parse(#[from] kdl::KdlError),
    #[error("Missing 'package' node")]
    MissingPackageNode,
    #[error("Package name not provided as first argument")]
    MissingName,
    #[error("Missing required property: {0}")]
    MissingProperty(String),
    #[error("Expected string value for property: {0}")]
    InvalidPropertyValue(String),
}

fn kdl_value_to_string(value: &kdl::KdlValue) -> Result<String, PackageParseError> {
    match value {
        kdl::KdlValue::String(s) => Ok(s.clone()),
        _ => Err(PackageParseError::InvalidPropertyValue(
            "non-string value found".to_string(),
        )),
    }
}

impl Package {
    pub fn from_kdl(input: &str) -> Result<Self, PackageParseError> {
        let doc: KdlDocument = input.parse().map_err(PackageParseError::Parse)?;

        let pkg_node = doc.get("package").ok_or(PackageParseError::MissingPackageNode)?;

        let args: Vec<&kdl::KdlValue> = doc.iter_args("package").collect();
        if args.is_empty() {
            return Err(PackageParseError::MissingName);
        }
        let name = kdl_value_to_string(args[0])?;

        let version = kdl_value_to_string(
            pkg_node
                .get("version")
                .ok_or(PackageParseError::MissingProperty("version".to_string()))?,
        )?;
        let arch = kdl_value_to_string(
            pkg_node
                .get("arch")
                .ok_or(PackageParseError::MissingProperty("arch".to_string()))?,
        )?;
        let flavor = kdl_value_to_string(
            pkg_node
                .get("flavor")
                .ok_or(PackageParseError::MissingProperty("flavor".to_string()))?,
        )?;

        let mut depends = Vec::new();
        let mut homepage = None;
        let mut license = None;

        for child_doc in pkg_node.children() {
            let nodes = child_doc.nodes();
            if nodes.is_empty() {
                continue;
            }
            let first_node = &nodes[0];
            let child_name_id = first_node.name();
            let child_name = child_name_id.to_string();

            let child_args: Vec<&kdl::KdlValue> = child_doc.iter_args(&child_name).collect();

            if child_args.is_empty() {
                continue;
            }

            if let Ok(value) = kdl_value_to_string(child_args[0]) {
                match child_name.as_str() {
                    "depends" => depends.push(value),
                    "homepage" => homepage = Some(value),
                    "license" => license = Some(value),
                    _ => {}
                }
            }
        }

        Ok(Self {
            name,
            version,
            arch,
            flavor,
            depends,
            homepage,
            license,
        })
    }
}
