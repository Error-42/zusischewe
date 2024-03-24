use std::{path::{Path, PathBuf}, fs::{self, File}, error::Error, ffi::OsStr};

use clap::{Parser, Subcommand};
use fs_extra::dir;
use xmltree::Element;

/// ZuSi schlechtes Wetter
/// 
/// Modify the acceleration of all trains.
#[derive(Debug, Parser)]
#[clap(version)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    #[command(visible_alias = "m")]
    Modify(Modify),
    #[command(visible_alias = "r")]
    Reset(Reset),
}

/// Modify the acceleration of all trains.
#[derive(Debug, Parser)]
struct Modify {
    directory: PathBuf,
    multiplier: f32,
    /// do not create `_zsw` folder used for resetting
    #[arg(short = 'n', long, action)]
    no_copy: bool,
}

/// Reset the acceleration of all trains.
#[derive(Debug, Parser)]
struct Reset {
    directory: PathBuf,
}

fn modify_file(path: &Path, multiplier: f32) -> Result<(), Box<dyn Error>> {
    let contents = fs::read_to_string(path)?;

    let mut tree = Element::parse(contents.as_bytes())?;
    let apbeschl = tree
        .get_mut_child("Zug")
        .ok_or("no tag 'Zug'")?
        .attributes
        .get_mut("APBeschl")
        .ok_or("no attribute 'APBeschl'")?;

    let decel: f32 = apbeschl.parse()?;
    
    *apbeschl = (multiplier * decel).to_string();

    tree.write(File::create(path)?)?;

    Ok(())
}

fn copy_name(dir: &PathBuf) -> PathBuf {
    let mut to = dir.clone();
    to.as_mut_os_string().push("_zsw");
    to
}

fn modify(cmd: Modify) {
    if !cmd.no_copy {
        let to = copy_name(&cmd.directory);

        dir::create(to.clone(), false).unwrap();
        dir::copy(cmd.directory.clone(), to, &dir::CopyOptions::new().content_only(true)).unwrap();
    }

    for file in fs::read_dir(cmd.directory).unwrap() {
        let path = file.unwrap().path();

        if path.extension() != Some(OsStr::new("trn")) {
            continue;
        }

        let _ = modify_file(&path, cmd.multiplier)
            .inspect_err(|e| eprintln!("failed file modification, path: {}, reason: {:?}", path.to_string_lossy(), e));
    }
}

fn reset(cmd: Reset) {
    let zsw_dir = copy_name(&cmd.directory);

    dir::create(cmd.directory.clone(), true).unwrap();
    dir::move_dir(zsw_dir, cmd.directory, &dir::CopyOptions::new().content_only(true)).unwrap();
}

fn main() {
    let cli = Cli::parse();

    match cli.command {
        Command::Modify(cmd) => modify(cmd),
        Command::Reset(cmd) => reset(cmd),
    }
}
