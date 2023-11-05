use std::{path::{Path, PathBuf}, fs::{self, File}, error::Error, ffi::OsStr};

use argh::FromArgs;
use xmltree::Element;

/// ZuSi schlecht Wetter v1.0.0
/// 
/// Modify the acceleration of all trains.
#[derive(FromArgs, Debug)]
struct Command {
    #[argh(positional)]
    directory: PathBuf,
    #[argh(positional)]
    multiplier: f32,
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

fn main() {
    let cmd: Command = argh::from_env();

    for file in fs::read_dir(cmd.directory).unwrap() {
        let path = file.unwrap().path();

        if path.extension() != Some(OsStr::new("trn")) {
            continue;
        }

        modify_file(&path, cmd.multiplier).unwrap();
    }
}
