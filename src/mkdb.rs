// src/ksmkdb.rs

use std::fs;
use std::path::{Path, PathBuf};
use std::collections::HashMap;
use clap::Parser;
use sha2::Sha256;
use sha2::Digest;
use serde::{Serialize, Deserialize};
use std::io::Write;

#[derive(Parser, Debug)]
#[command(author, version, about = "Generate Seiryo Linux repo database from .kpkg files", long_about = None)]
struct Args {
    #[arg(help = "Input directory containing .kpkg files")]
    input_dir: PathBuf,

    #[arg(short, long, default_value = "repo.db", help = "Output database name (e.g. core.db)")]
    output: String,
}

#[derive(Serialize, Deserialize, Debug)]
struct RepoPackage {
    version: String,
    arch: String,
    flavour: String,
    filename: String,
    sha256: String,
    depends: Vec<String>,
}

#[derive(Serialize, Deserialize, Debug)]
struct RepoDatabase {
    #[serde(flatten)]
    packages: HashMap<String, RepoPackage>,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();

    let input_dir = args.input_dir;
    if !input_dir.is_dir() {
        eprintln!("Error: '{}' is not a directory", input_dir.display());
        std::process::exit(1);
    }

    let mut db = RepoDatabase {
        packages: HashMap::new(),
    };

    for entry in fs::read_dir(&input_dir)? {
        let entry = entry?;
        let path = entry.path();

        if path.extension().map_or(false, |ext| ext == "kpkg") {
            if let Err(e) = process_kpkg(&path, &mut db) {
                eprintln!("Warning: skipping {}: {}", path.display(), e);
            }
        }
    }

    let toml_output = toml::to_string_pretty(&db)?;
    let zst_output_path = format!("{}.zst", args.output);

    // Write uncompressed TOML (for readability/debugging)
    fs::write(&args.output, &toml_output)?;

    // Compress with zstd
    let zst_file = fs::File::create(&zst_output_path)?;
    let mut zstd_encoder = zstd::stream::write::Encoder::new(zst_file, 3)?;
    zstd_encoder.write(toml_output.as_bytes())?;
    zstd_encoder.finish()?;

    println!("✓ Generated {} and {}", args.output, zst_output_path);
    Ok(())
}

fn process_kpkg(path: &Path, db: &mut RepoDatabase) -> Result<(), Box<dyn std::error::Error>> {
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

    // Parse KDL using official kdl crate
    let doc: kdl::KdlDocument = kdl_content.parse()?;
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

    // Compute SHA256
    let mut hasher = Sha256::new();
    let pkg_bytes = fs::read(path)?;
    hasher.update(&pkg_bytes);
    let sha256 = format!("{:x}", hasher.finalize());

    db.packages.insert(
        name,
        RepoPackage {
            version,
            arch,
            filename,
            sha256,
            depends,
        },
    );

    Ok(())
}
