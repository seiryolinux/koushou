// src/pkgutil.rs

use std::fs::{self, symlink_metadata};
use std::path::Path;
use std::os::unix::fs::PermissionsExt;
use tar::{Builder, Header, EntryType};
use zstd::stream::write::Encoder as ZstdEncoder;
use flate2::write::GzEncoder;
use flate2::Compression;
use walkdir::WalkDir;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum PkgUtilError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("Package metadata error: {0}")]
    Package(#[from] crate::package::PackageParseError),
    #[error("Package '{0}' already exists")]
    AlreadyExists(String),
    #[error("Missing package.kdl in: {0}")]
    MissingMetadata(String),
    #[error("Missing 'files' directory in: {0}")]
    MissingFilesDir(String),
}

pub fn generate(name: &str) -> Result<(), PkgUtilError> {
    let dir = Path::new(name);
    if dir.exists() {
        return Err(PkgUtilError::AlreadyExists(name.to_string()));
    }

    fs::create_dir_all(dir.join("files/usr/bin"))?;

    let kdl = format!(
        r#"package "{name}" version="0.1" arch="x86_64" flavor="glibc-systemd" {{
  depends "glibc"
  license "MIT"
}}"#
    );
    fs::write(dir.join("package.kdl"), kdl)?;

    let script = format!("#!/bin/sh\necho \"Hello from {name}\"\n");
    let script_path = dir.join("files/usr/bin").join(name);
    fs::write(&script_path, script)?;
    fs::set_permissions(
        script_path,
        std::os::unix::fs::PermissionsExt::from_mode(0o755),
    )?;

    println!("✨ Created package template: {}", name);
    Ok(())
}

pub fn build(pkg_dir: &Path) -> Result<(), PkgUtilError> {
    let kdl_path = pkg_dir.join("package.kdl");
    if !kdl_path.exists() {
        return Err(PkgUtilError::MissingMetadata(pkg_dir.display().to_string()));
    }
    let kdl_content = fs::read_to_string(&kdl_path)?;
    let pkg = crate::package::Package::from_kdl(&kdl_content)?;

    let files_dir = pkg_dir.join("files");
    if !files_dir.exists() {
        return Err(PkgUtilError::MissingFilesDir(pkg_dir.display().to_string()));
    }

    // Build files.tar.zst
    let files_tar_path = pkg_dir.join("files.tar.zst");
    let files_tar_file = fs::File::create(&files_tar_path)?;
    let zstd_encoder = ZstdEncoder::new(files_tar_file, 3)?;
    let mut files_tar = Builder::new(zstd_encoder);

    for entry in WalkDir::new(&files_dir).into_iter().filter_map(|e| e.ok()) {
        let rel_path = entry.path().strip_prefix(&files_dir).unwrap();
        if rel_path.as_os_str().is_empty() {
            continue;
        }

        let metadata = symlink_metadata(entry.path())?;
        let mut header = Header::new_gnu();
        header.set_path(rel_path)?;

        if metadata.is_file() {
            header.set_size(metadata.len());
            header.set_mode(metadata.permissions().mode());
            header.set_cksum();
            let file = fs::File::open(entry.path())?;
            files_tar.append(&header, file)?;
        } else if metadata.is_dir() {
            header.set_size(0);
            header.set_mode(0o755);
            header.set_entry_type(EntryType::Directory);
            header.set_cksum();
            files_tar.append(&header, std::io::empty())?;
        } else if metadata.file_type().is_symlink() {
            let target = fs::read_link(entry.path())?;
            header.set_size(0);
            header.set_mode(0o777);
            header.set_entry_type(EntryType::Symlink);
            header.set_link_name(target.to_str().ok_or_else(|| {
                std::io::Error::new(std::io::ErrorKind::InvalidData, "Non-UTF8 symlink target")
            })?)?;
            header.set_cksum();
            files_tar.append(&header, std::io::empty())?;
        }
    }

    files_tar.finish()?;
    let zstd_encoder = files_tar.into_inner()?;
    zstd_encoder.finish()?; // ← critical for valid zstd

    // Build .kpkg = .tar.gz
    let output_name = format!("{}-{}-{}.kpkg", pkg.name, pkg.version, pkg.arch);
    let output_path = pkg_dir.join(&output_name);
    let output_file = fs::File::create(&output_path)?;
    let gz_encoder = GzEncoder::new(output_file, Compression::default());
    let mut pkg_tar = Builder::new(gz_encoder);

    pkg_tar.append_path_with_name(&kdl_path, "package.kdl")?;
    pkg_tar.append_path_with_name(&files_tar_path, "files.tar.zst")?;

    pkg_tar.finish()?;

    fs::remove_file(&files_tar_path)?;

    println!("📦 Built: {}", output_name);
    Ok(())
}
