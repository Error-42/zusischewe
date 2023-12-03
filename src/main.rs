use std::{path::{Path, PathBuf}, fs::{self, File}, error::Error, ffi::OsStr};

use argh::FromArgs;
use fs_extra::dir;
use xmltree::Element;

/// ZuSi schlecht Wetter v2.0.0
/// 
/// Modify the acceleration of all trains.
#[derive(FromArgs, Debug)]
struct Command {
    #[argh(subcommand)]
    subcommand: Subcommand,
}

#[derive(FromArgs, Debug)]
#[argh(subcommand)]
enum Subcommand {
    Modify(Modify),
    Reset(Reset),
}

/// Modify the acceleration of all trains.
#[derive(FromArgs, Debug)]
#[argh(subcommand, name = "modify")]
struct Modify {
    #[argh(positional)]
    directory: PathBuf,
    #[argh(positional)]
    multiplier: f32,
    /// do not create `_zsw` folder used for resetting
    #[argh(switch, short = 'n')]
    no_copy: bool,
}

/// Reset the acceleration of all trains.
#[derive(FromArgs, Debug)]
#[argh(subcommand, name = "reset")]
struct Reset {
    #[argh(positional)]
    directory: PathBuf,
}

fn modify_file(path: &Path, multiplier: f32) -> Result<(), Box<dyn Error>> {
    let contents = fs::read_to_string(path)?;

    let mut tree = Element::parse(contents.as_bytes())?;
    let apbeschl = tree.get_mut_child("Zug").unwrap().attributes.get_mut("APBeschl").unwrap();

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

        modify_file(&path, cmd.multiplier).unwrap();
    }
}

fn reset(cmd: Reset) {
    let zsw_dir = copy_name(&cmd.directory);

    dir::create(cmd.directory.clone(), true).unwrap();
    dir::move_dir(zsw_dir, cmd.directory, &dir::CopyOptions::new().content_only(true)).unwrap();
}

fn main() {
    let cmd: Command = argh::from_env();

    match cmd.subcommand {
        Subcommand::Modify(cmd) => modify(cmd),
        Subcommand::Reset(cmd) => reset(cmd),
    }
}
