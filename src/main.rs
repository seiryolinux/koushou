// src/main.rs

use std::path::PathBuf;
use thiserror::Error;
use clap::Parser;

mod package;
mod pkgdb;
mod install;
mod removal;
mod pkgutil;
mod list;
mod sync;

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    #[command(subcommand)]
    command: Command,
}

#[derive(clap::Subcommand, Debug)]
enum Command {
    Install(InstallArgs),
    Remove(RemoveArgs),
    List(ListArgs),
    Sync(SyncArgs),
    Genpkg(GenpkgArgs),
    Buildpkg(BuildpkgArgs),
}

#[derive(clap::Args, Debug)]
struct InstallArgs {
    #[arg(help = "Path to .kpkg file")]
    kpkg_path: PathBuf,
    #[arg(long, short = 'r', default_value = "/", help = "Target root directory")]
    root: PathBuf,
}

#[derive(clap::Args, Debug)]
struct RemoveArgs {
    #[arg(help = "Name of package to remove")]
    package_name: String,
    #[arg(long, short = 'r', default_value = "/", help = "Target root directory")]
    root: PathBuf,
}

#[derive(clap::Args, Debug)]
struct ListArgs {
    #[arg(long, short = 'r', default_value = "/", help = "Target root directory")]
    root: PathBuf,
}

#[derive(clap::Args, Debug)]
struct SyncArgs {
    #[arg(long, short = 'r', default_value = "/", help = "Target root directory")]
    root: PathBuf,
}

#[derive(clap::Args, Debug)]
struct GenpkgArgs {
    #[arg(help = "Name of the new package")]
    name: String,
}

#[derive(clap::Args, Debug)]
struct BuildpkgArgs {
    #[arg(help = "Path to package directory")]
    dir: PathBuf,
}

#[derive(Error, Debug)]
pub enum KspkgError {
    #[error("Install error: {0}")]
    Install(#[from] install::InstallError),
    #[error("Removal error: {0}")]
    Removal(#[from] removal::RemovalError),
    #[error("List error: {0}")]
    List(#[from] list::ListError),
    #[error("Sync error: {0}")]
    Sync(#[from] sync::SyncError),
    #[error("Package utility error: {0}")]
    PkgUtil(#[from] pkgutil::PkgUtilError),
}

#[tokio::main]
async fn main() -> Result<(), KspkgError> {
    let args = Args::parse();

    match args.command {
        Command::Install(install_args) => {
            install::install_package(&install_args.kpkg_path, &install_args.root)?;
        }
        Command::Remove(remove_args) => {
            removal::remove_package(&remove_args.root, &remove_args.package_name)?;
        }
        Command::List(list_args) => {
            list::list_packages(&list_args.root)?;
        }
        Command::Sync(sync_args) => {
            sync::sync_repos(&sync_args.root).await?;
        }
        Command::Genpkg(genpkg_args) => {
            pkgutil::generate(&genpkg_args.name)?;
        }
        Command::Buildpkg(buildpkg_args) => {
            pkgutil::build(&buildpkg_args.dir)?;
        }
    }

    Ok(())
}
