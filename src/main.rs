use std::{
    error::Error,
    ffi::OsStr,
    fs::{self, File},
    path::{Path, PathBuf},
};

use clap::{Parser, Subcommand};
use fs_extra::dir;
use rand::Rng;
use xmltree::{Element, XMLNode};

use crate::date::Datetime;

mod date;

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
    #[arg(short = 'm', long)]
    multiplier: Option<f32>,
    #[arg(long)]
    delay_probability: Option<f32>,
    /// do not create `_zsw` folder used for resetting
    #[arg(short = 'n', long, action)]
    no_copy: bool,
}

/// Reset the acceleration of all trains.
#[derive(Debug, Parser)]
struct Reset {
    directory: PathBuf,
}

fn modify_multiplier(tree: &mut Element, multiplier: f32) -> Result<(), Box<dyn Error>> {
    let apbeschl = tree
        .get_mut_child("Zug")
        .ok_or("no tag 'Zug'")?
        .attributes
        .get_mut("APBeschl")
        .ok_or("no attribute 'APBeschl'")?;

    let decel: f32 = apbeschl.parse()?;

    *apbeschl = (multiplier * decel).to_string();

    Ok(())
}

fn delay(tree: &mut Element) -> Result<(), Box<dyn Error>> {
    for child in &mut tree.get_mut_child("Zug").ok_or("no tag `Zug`")?.children {
        if let XMLNode::Element(e) = child {
            if e.name == "FahrplanEintrag" {
                let ankunft = e
                    .attributes
                    .get_mut("Ank")
                    .ok_or("no starting time: no attribute `Ank` on firt `FahrplanEintrag`")?;

                let mut datetime: Datetime = ankunft.parse()?;
                datetime.inc_seconds(3600);
                *ankunft = datetime.to_string();

                return Ok(());
            }
        }
    }

    Err("no `FahrplanEintrag` entry inside `Zug`".into())
}

fn read_file(path: &Path) -> Result<Element, Box<dyn Error>> {
    let contents = fs::read_to_string(path)?;

    Ok(Element::parse(contents.as_bytes())?)
}

fn write_file(path: &Path, tree: Element) -> Result<(), Box<dyn Error>> {
    tree.write(File::create(path)?)?;

    Ok(())
}

fn modify_file(
    path: &Path,
    modify: &Modify,
    rng: &mut rand::rngs::ThreadRng,
) -> Result<(), Box<dyn Error>> {
    let mut tree = read_file(path)?;

    if let Some(multiplier) = modify.multiplier {
        modify_multiplier(&mut tree, multiplier)?;
    }

    if let Some(p) = modify.delay_probability {
        let val: f32 = rng.gen();

        if val < p {
            delay(&mut tree)?;
        }
    }

    write_file(path, tree)?;

    Ok(())
}

fn copy_name(dir: &Path) -> PathBuf {
    let mut to = dir.to_path_buf();
    to.as_mut_os_string().push("_zsw");
    to
}

fn modify(cmd: Modify) {
    if !cmd.no_copy {
        let to = copy_name(&cmd.directory);

        dir::create(to.clone(), false).unwrap();
        dir::copy(
            cmd.directory.clone(),
            to,
            &dir::CopyOptions::new().content_only(true),
        )
        .unwrap();
    }

    let mut rng = rand::thread_rng();

    for file in fs::read_dir(&cmd.directory).unwrap() {
        let path = file.unwrap().path();

        if path.extension() != Some(OsStr::new("trn")) {
            continue;
        }

        let _ = modify_file(&path, &cmd, &mut rng).inspect_err(|e| {
            eprintln!(
                "failed file modification, path: {}, reason: {:?}",
                path.to_string_lossy(),
                e
            )
        });
    }
}

fn reset(cmd: Reset) {
    let zsw_dir = copy_name(&cmd.directory);

    dir::create(cmd.directory.clone(), true).unwrap();
    dir::move_dir(
        zsw_dir,
        cmd.directory,
        &dir::CopyOptions::new().content_only(true),
    )
    .unwrap();
}

fn main() {
    let cli = Cli::parse();

    match cli.command {
        Command::Modify(cmd) => modify(cmd),
        Command::Reset(cmd) => reset(cmd),
    }
}
