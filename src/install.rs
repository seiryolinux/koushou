// src/install.rs

use std::fs::File;
use std::path::{Path, PathBuf};
use tar::Archive;
use zstd::stream::read::Decoder as ZstdDecoder;
use flate2::read::GzDecoder;
use tempfile::TempDir;
use walkdir::WalkDir;
use thiserror::Error;

use crate::package;
use crate::pkgdb;

#[derive(Error, Debug)]
pub enum InstallError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("Package parse error: {0}")]
    PackageParse(#[from] package::PackageParseError),
    #[error("Invalid package: missing 'files.tar.zst'")]
    MissingFilesTar,
    #[error("Failed to create temporary directory")]
    TempDir,
    #[error("Target root is not a directory: {0}")]
    InvalidRoot(PathBuf),
    #[error("Package database error: {0}")]
    PkgDb(#[from] pkgdb::PkgDbError),
}

pub fn install_package(kpkg_path: &Path, root: &Path) -> Result<(), InstallError> {
    if !root.is_dir() {
        return Err(InstallError::InvalidRoot(root.to_path_buf()));
    }

    let file = File::open(kpkg_path)?;
    let gz = GzDecoder::new(file);
    let mut archive = Archive::new(gz);

    let temp_dir = TempDir::new().map_err(|_| InstallError::TempDir)?;
    let temp_path = temp_dir.path();

    archive.unpack(temp_path)?;

    let kdl_path = temp_path.join("package.kdl");
    let kdl_content = std::fs::read_to_string(&kdl_path)
        .map_err(|_| InstallError::PackageParse(package::PackageParseError::MissingPackageNode))?;
    let pkg = package::Package::from_kdl(&kdl_content)?;

    let files_tar_path = temp_path.join("files.tar.zst");
    if !files_tar_path.exists() {
        return Err(InstallError::MissingFilesTar);
    }

    let staging_dir = temp_path.join("staging");
    std::fs::create_dir_all(&staging_dir)?;

    let files_file = File::open(&files_tar_path)?;
    let zstd_decoder = ZstdDecoder::new(files_file)?;
    let mut files_archive = Archive::new(zstd_decoder);
    files_archive.unpack(&staging_dir)?;

    let mut files = Vec::new();
    for entry in WalkDir::new(&staging_dir).into_iter().filter_map(|e| e.ok()) {
        if entry.file_type().is_file() {
            let rel_path = entry.path().strip_prefix(&staging_dir).unwrap();
            files.push(rel_path.to_string_lossy().to_string());
        }
    }

    for entry in std::fs::read_dir(&staging_dir)? {
        let entry = entry?;
        let src_path = entry.path();
        let file_name = entry.file_name();
        let dest_path = root.join(file_name);

        if dest_path.exists() {
            if dest_path.is_dir() {
                std::fs::remove_dir_all(&dest_path)?;
            } else {
                std::fs::remove_file(&dest_path)?;
            }
        }

        std::fs::rename(&src_path, &dest_path)?;
    }

    let installed_pkg = pkgdb::InstalledPackage {
        name: pkg.name.clone(),
        version: pkg.version.clone(),
        arch: pkg.arch.clone(),
        flavor: pkg.flavor.clone(),
        depends: pkg.depends.clone(),
        files,
    };

    let db_path = root.join("var/lib/koushou/db.json");
    std::fs::create_dir_all(db_path.parent().unwrap())?;
    let mut db = pkgdb::PackageDatabase::load_or_new(&db_path)?;
    db.add(installed_pkg);
    db.save(&db_path)?;

    println!(
        "✓ Installed {}-{} ({}) into {}",
        pkg.name,
        pkg.version,
        pkg.arch,
        root.display()
    );

    Ok(())
}
