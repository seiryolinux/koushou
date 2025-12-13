// src/mkdb.rs

use std::fs;
use std::path::{Path, PathBuf};
use clap::Parser;
use sha2::Sha256;
use rusqlite::{Connection, params};
use regex::Regex;
use kdl::KdlDocument;
use sha2::Digest;

#[derive(Parser, Debug)]
#[command(author, version, about = "Generate Seiryo Linux repo database from .kpkg files", long_about = None)]
struct Args {
    #[arg(help = "Input directory containing .kpkg files")]
    input_dir: PathBuf,
    #[arg(short, long, default_value = "repo.db", help = "Output database name (e.g. core.db)")]
    output: String,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();
    let input_dir = args.input_dir;
    if !input_dir.is_dir() {
        eprintln!("Error: '{}' is not a directory", input_dir.display());
        std::process::exit(1);
    }

    let db_path = args.output;
    generate_db(&input_dir, &PathBuf::from(db_path.clone()))?;

    println!("✓ Generated {}", db_path);
    Ok(())
}

fn generate_db(input_dir: &Path, output_path: &Path) -> Result<(), Box<dyn std::error::Error>> {
    let mut conn = Connection::open(output_path)?;
    conn.execute(
        "CREATE TABLE IF NOT EXISTS packages (
            name TEXT NOT NULL,
            version TEXT NOT NULL,
            arch TEXT NOT NULL,
            flavour TEXT NOT NULL,
            filename TEXT NOT NULL,
            sha256 TEXT NOT NULL,
            PRIMARY KEY (name, version, arch, flavour)
        )",
        [],
    )?;
    conn.execute(
        "CREATE TABLE IF NOT EXISTS dependencies (
            package_name TEXT NOT NULL,
            dep_name TEXT NOT NULL,
            dep_predicate TEXT
        )",
        [],
    )?;
    conn.execute("CREATE INDEX IF NOT EXISTS idx_packages_name ON packages(name)", [])?;

    let tx = conn.transaction()?;
    {
        let mut pkg_stmt = tx.prepare("INSERT INTO packages VALUES (?, ?, ?, ?, ?, ?)")?;
        let mut dep_stmt = tx.prepare("INSERT INTO dependencies VALUES (?, ?, ?)")?;

        for entry in fs::read_dir(input_dir)? {
            let entry = entry?;
            let path = entry.path();
            if path.extension().map_or(false, |ext| ext == "kpkg") {
                if let Ok(pkg) = process_kpkg(&path) {
                    pkg_stmt.execute(params![
                        pkg.name,
                        pkg.version,
                        pkg.arch,
                        pkg.flavour,
                        pkg.filename,
                        pkg.sha256
                    ])?;

                    for dep in pkg.depends {
                        let (dep_name, predicate) = parse_dep_for_db(&dep);
                        dep_stmt.execute(params![pkg.name, dep_name, predicate])?;
                    }
                }
            }
        }
    }
    tx.commit()?;
    Ok(())
}

fn process_kpkg(path: &Path) -> Result<RepoPackage, Box<dyn std::error::Error>> {
    let filename = path.file_name().unwrap().to_str().unwrap().to_string();
    let tar_file = fs::File::open(path)?;
    let gz = flate2::read::GzDecoder::new(tar_file);
    let mut archive = tar::Archive::new(gz);

    let mut kdl_content = String::new();
    for file in archive.entries()? {
        let mut file = file?;
        if file.path()?.file_name() == Some(std::ffi::OsStr::new("package.kdl")) {
            use std::io::Read;
            file.read_to_string(&mut kdl_content)?;
            break;
        }
    }

    if kdl_content.is_empty() {
        return Err("package.kdl not found".into());
    }

    let doc: KdlDocument = kdl_content.parse()?;
    let pkg_node = doc.get("package").ok_or("missing 'package' node")?;

    let args: Vec<&kdl::KdlValue> = doc.iter_args("package").collect();
    if args.is_empty() {
        return Err("package name missing".into());
    }

    let name = match args[0] {
        kdl::KdlValue::String(s) => s.clone(),
        _ => return Err("package name must be string".into()),
    };

    let get_prop = |key: &str| -> Result<String, Box<dyn std::error::Error>> {
        pkg_node
            .get(key)
            .and_then(|v| {
                if let kdl::KdlValue::String(s) = v { Some(s.clone()) } else { None }
            })
            .ok_or_else(|| format!("missing property: {}", key).into())
    };

    let version = get_prop("version")?;
    let arch = get_prop("arch")?;
    let flavour = get_prop("flavour")?;

    let mut depends = Vec::new();
    for child_doc in pkg_node.children() {
        let nodes = child_doc.nodes();
        if nodes.is_empty() { continue; }
        let child_name = &nodes[0].name();
        if child_name.to_string() == "depends" {
            let child_args: Vec<&kdl::KdlValue> = child_doc.iter_args("depends").collect();
            if let Some(dep_val) = child_args.first() {
                if let kdl::KdlValue::String(s) = dep_val {
                    depends.push(s.clone());
                }
            }
        }
    }

    let mut hasher = Sha256::new();
    let pkg_bytes = fs::read(path)?;
    hasher.update(&pkg_bytes);
    let sha256 = format!("{:x}", hasher.finalize());

    Ok(RepoPackage {
        name,
        version,
        arch,
        flavour,
        filename,
        sha256,
        depends,
    })
}

fn parse_dep_for_db(s: &str) -> (String, Option<String>) {
    let re = Regex::new(r"^([a-zA-Z0-9._-]+)([<>=!]+)?(.*)$").unwrap();
    if let Some(caps) = re.captures(s) {
        let name = caps.get(1).unwrap().as_str().to_string();
        let op = caps.get(2).map(|m| m.as_str()).unwrap_or("");
        let version = caps.get(3).map(|m| m.as_str()).unwrap_or("");
        if version.is_empty() {
            (name, None)
        } else {
            (name, Some(format!("{}{}", op, version)))
        }
    } else {
        (s.to_string(), None)
    }
}

#[derive(Debug)]
struct RepoPackage {
    name: String,
    version: String,
    arch: String,
    flavour: String,
    filename: String,
    sha256: String,
    depends: Vec<String>,
}
