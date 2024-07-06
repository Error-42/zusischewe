use std::{
    ffi::OsStr,
    fs::{self, File},
    path::{Path, PathBuf},
};

use anyhow::{anyhow, bail, Context};
use clap::{Parser, Subcommand};
use fs_extra::dir;
use rand::Rng;
use rand_distr::Distribution;
use xmltree::{Element, XMLNode};

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

    #[arg(visible_alias = "dp", long)]
    delay_probability: Option<f32>,
    #[arg(visible_alias = "da", long, default_value = "360")]
    delay_amplitude: f32,
    #[arg(visible_alias = "dl", long, default_value = "3")]
    delay_lambda: f32,

    #[arg(visible_alias = "bm", long)]
    bell_mean: Option<f32>,
    #[arg(visible_alias = "bd", long, default_value = "5")]
    bell_deviation: f32,
    #[arg(short, long, action)]
    deny_early: bool,
    
    /// do not create `_zsw` folder used for resetting
    #[arg(short = 'n', long, action)]
    no_copy: bool,
}

/// Reset the acceleration of all trains.
#[derive(Debug, Parser)]
struct Reset {
    directory: PathBuf,
}

fn modify_multiplier(tree: &mut Element, multiplier: f32) -> anyhow::Result<()> {
    let apbeschl = tree
        .get_mut_child("Zug")
        .ok_or(anyhow!("no tag 'Zug'"))?
        .attributes
        .get_mut("APBeschl")
        .ok_or(anyhow!("no attribute 'APBeschl'"))?;

    let decel: f32 = apbeschl
        .parse()
        .with_context(|| "unable to parse `APBeschl`")?;

    *apbeschl = (multiplier * decel).to_string();

    Ok(())
}

fn delay(tree: &mut Element, seconds: u32) -> anyhow::Result<()> {
    for child in &mut tree.get_mut_child("Zug").context("no tag `Zug`")?.children {
        if let XMLNode::Element(e) = child {
            if e.name == "FahrplanEintrag" {
                let ankunft = e
                    .attributes
                    .get_mut("Ank")
                    .context("no starting time: no attribute `Ank` on firt `FahrplanEintrag`")?;

                let arrival: chrono::NaiveDateTime =
                    chrono::NaiveDateTime::parse_from_str(ankunft, "%Y-%m-%d %H:%M:%S")
                        .context(format!("parsing arrival time `{ankunft}`"))?;
                let delayed = arrival
                    .checked_add_signed(chrono::TimeDelta::seconds(seconds as i64))
                    .context("calculating new arrival time")?;
                *ankunft = delayed.format("%Y-%m-%d %H:%M:%S").to_string();

                return Ok(());
            }
        }
    }

    bail!("no `FahrplanEintrag` entry inside `Zug`")
}

fn read_file(path: &Path) -> anyhow::Result<Element> {
    let contents = fs::read_to_string(path)?;

    Ok(Element::parse(contents.as_bytes())?)
}

fn write_file(path: &Path, tree: Element) -> anyhow::Result<()> {
    tree.write(File::create(path)?)?;

    Ok(())
}

fn modify_file(
    path: &Path,
    modify: &Modify,
    rng: &mut rand::rngs::ThreadRng,
) -> anyhow::Result<()> {
    let mut tree = read_file(path)?;

    if let Some(multiplier) = modify.multiplier {
        modify_multiplier(&mut tree, multiplier).context("applying multiplier")?;
    }

    {
        let mut minutes: f32 = 0.0;

        if let Some(p) = modify.delay_probability {
            let val: f32 = rng.gen();

            if val < p {
                minutes += modify.delay_amplitude * ((modify.delay_lambda * rng.gen::<f32>()).exp() - 1.0);
            }
        }

        if let Some(bell_mean) = modify.bell_mean {
            minutes += rand_distr::Normal::new(bell_mean, modify.bell_deviation)
                .context("unable to generate normal distribution for random number sampling with given parameters")?
                .sample(rng);
        }

        if modify.deny_early {
            minutes = minutes.max(0.0);
        }

        let seconds = (minutes * 60.0) as u32;

        if seconds != 0 {
            delay(&mut tree, (minutes * 60.0) as u32).context("delaying entry")?;
        }
    }

    write_file(path, tree)?;

    Ok(())
}

fn copy_name(dir: &Path) -> Option<PathBuf> {
    let mut file_name = dir.file_name()?.to_os_string();
    file_name.push("_zsw");
    Some(dir.with_file_name(file_name))
}

fn modify(cmd: Modify) {
    if !cmd.no_copy {
        let to = copy_name(&cmd.directory).unwrap();

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

        let _ = modify_file(&path, &cmd, &mut rng).inspect_err(|err| {
            eprintln!("Failed file modification, path: {}", path.to_string_lossy());

            eprintln!("| reason: {}", err.root_cause());

            for context in err.chain().rev().skip(1) {
                eprintln!("| when: {context}");
            }
        });
    }
}

fn reset(cmd: Reset) {
    let zsw_dir = copy_name(&cmd.directory).unwrap();

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
