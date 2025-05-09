use std::{
    collections::HashSet,
    ffi::OsStr,
    fs::{self, File},
    path::{Path, PathBuf},
};

use anyhow::{bail, Context};
use clap::{Parser, Subcommand};
use fs_extra::dir;
use rand::Rng;
use rand_distr::Distribution;
use xmltree::{Element, XMLNode};

/// ZuSi schlechtes Wetter
///
/// Cause general chaos.
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

/// TODO
#[derive(Debug, Parser)]
struct Modify {
    /// Path of the folder containing the timetable files. This folder should contain '.trn' and '.timetable.xml' files.
    directory: PathBuf,

    /// Multiply the acceleration/deceleration of all trains by this factor.
    ///
    /// This affects the `APBeschl` property of trains.
    #[arg(short = 'm', long)]
    multiplier: Option<f32>,

    /// Modify train acceleration/deceleration assuming this is the coeffient of friction.
    ///
    /// This affects the `APBeschl` property of trains.
    ///
    /// The new `APBeschl` of the train is A*min(μ/M, 1) where Α is the old `APBeschl` value, μ is the new coefficient of friction, M is the coefficient of friction needed for the train to achieve full acceleration (see arguments loc_needed and mu_needed).
    #[arg(short = 'f', long, default_value = "0.4")]
    friction: f32,
    /// Coefficient of friction needed for locomotives to achieve full acceleration/deceleration.
    ///
    /// See the help of the friction argument for details.
    #[arg(short = 'l', long, default_value = "0.4")]
    loc_needed: f32,
    /// Coefficient of friction needed for multiple units to achieve full acceleration/deceleration.
    ///
    /// See the help of the friction argument for details.
    #[arg(short = 't', long, default_value = "0.25")]
    mu_needed: f32,

    /// Delay type A: probability of delay. Passing this argument applies delay type A.
    ///
    /// Delay type A delays the entry of trains by A(exp(μr)-1) where A is the amplitude and r is a random real in the interval [0, 1).
    #[arg(visible_alias = "dp", long)]
    delay_probability: Option<f32>,
    /// Delay type A: amplitude of delay.
    #[arg(visible_alias = "da", long, default_value = "360")]
    delay_amplitude: f32,
    /// Delay type A: λ parameter of delay.
    #[arg(visible_alias = "dl", long, default_value = "3")]
    delay_lambda: f32,

    /// Delay type B: mean delay in minutes. Passing this argument applies delay type B.
    ///
    /// Delay type B delays the entry of trains according to a normal distribution.
    #[arg(visible_alias = "bm", long)]
    bell_mean: Option<f32>,
    /// Delay type B: stardard deviation of delay in minutes.
    #[arg(visible_alias = "bd", long, default_value = "5")]
    bell_deviation: f32,

    /// Delay type U: probability of delay. Passing this argument applies delay type U.
    ///
    /// Delay type U delays the entry of trains by a uniformly chosen amount between 0 and the maximum with some probability.
    #[arg(visible_alias = "up", long, requires = "uniform_maximum")]
    uniform_probability: Option<f32>,
    /// Delay type U: maximum delay in minutes.
    #[arg(visible_alias = "um", long, requires = "uniform_probability")]
    uniform_maximum: Option<f32>,

    /// Do not let the train enter early.
    #[arg(short, long, action)]
    deny_early: bool,

    /// Delay trains as if passengers took a constant factor times longer to board.
    #[arg(visible_alias = "dfac", long, default_value = "1")]
    departures_delay_factor: f32,
    /// Maximum delay of non-entry departures in minutes.
    #[arg(visible_alias = "dmd", long, default_value = "6")]
    departures_max_delay: f32,

    /// Do not create `_zsw` folder used for resetting.
    #[arg(short = 'n', long, action)]
    no_copy: bool,

    /// Double all trains.
    ///
    /// The second train will have 'B' appended to its train number.
    #[arg(short = 'D', long, action)]
    duplicate: bool,

    /// Don't duplicate trains in certain groups, a group is matched if the group name contains the text specified here.
    /// 
    /// More formally, a group is matched if the name specified here is a substring of the group name. A string s is a substring of a string t if s can be obtained from t by deletion of several (possibly, zero or all) characters from the beginning and several (possibly, zero or all) characters from the end.
    #[arg(visible_alias = "!Dg", long, num_args=0..)]
    dont_duplicate_group: Vec<String>,
}

/// Reset using the `_zsw` folder.
#[derive(Debug, Parser)]
struct Reset {
    directory: PathBuf,
}

fn is_wagon_locomotive(data_tag: &Element) -> anyhow::Result<bool> {
    let wagon_location = data_tag
        .attributes
        .get("Dateiname")
        .context("tag 'Datei' inside tag 'FahrzeugInfo' has no attribute 'Dateiname'")?;

    Ok(wagon_location.contains("lok"))
}

fn consist_has_locomotive(consist: &Element) -> anyhow::Result<bool> {
    for child in &consist.children {
        let XMLNode::Element(element) = child else {
            continue;
        };

        match element.name.as_str() {
            "Datei" => {
                if is_wagon_locomotive(element)? {
                    return Ok(true);
                }
            }
            "FahrzeugInfo" => {
                let data = element
                    .get_child("Datei")
                    .context("tag 'FahrzeugInfo' has no tag 'Datei'")?;

                if is_wagon_locomotive(data)? {
                    return Ok(true);
                }
            }
            "FahrzeugVarianten" => {
                if consist_has_locomotive(element)? {
                    return Ok(true);
                }
            }
            name => bail!("Unknown tag '{name}' inside tag 'FahrzeugVarianten' or 'FahrzeugInfo'"),
        }
    }

    Ok(false)
}

fn group_name(train: &Element) -> anyhow::Result<&String> {
    train
        .attributes
        .get("FahrplanGruppe")
        .context("`Zug` has no tag FahrplanGruppe")
}

/// `train` is XML tag `Zug`.
fn is_passenger(train: &Element) -> bool {
    let zugtyp = train.attributes.get("Zugtyp");

    let Some(train_type) = zugtyp else {
        return false;
    };

    *train_type == "1"
}

fn modify_multiplier(
    tree: &mut Element,
    loc_multiplier: f32,
    mu_multiplier: f32,
) -> anyhow::Result<()> {
    let train = tree.get_mut_child("Zug").context("no tag 'Zug'")?;

    let consist = train
        .get_child("FahrzeugVarianten")
        .context("no tag 'FahrzeugVarianten'")?;

    let has_locomotive = consist_has_locomotive(consist)
        .context("trying to determine whether consist has a locomotive")?;

    let apbeschl = train
        .attributes
        .get_mut("APBeschl")
        .context("no attribute 'APBeschl'")?;

    let acceleration: f32 = apbeschl
        .parse()
        .with_context(|| "unable to parse `APBeschl`")?;

    let multiplier = match has_locomotive {
        true => loc_multiplier,
        false => mu_multiplier,
    };

    *apbeschl = (multiplier * acceleration).to_string();

    Ok(())
}

fn delay_entry(tree: &mut Element, seconds: u32) -> anyhow::Result<()> {
    for child in &mut tree.get_mut_child("Zug").context("no tag `Zug`")?.children {
        if let XMLNode::Element(e) = child {
            if e.name == "FahrplanEintrag" {
                let ankunft = e
                    .attributes
                    .get_mut("Ank")
                    .context("no starting time: no attribute `Ank` on first `FahrplanEintrag`")?;

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

fn delay_departures(
    tree: &mut Element,
    factor: f32,
    max_wait_time: chrono::TimeDelta,
) -> anyhow::Result<()> {
    let zug = tree.get_mut_child("Zug").context("no tag `Zug`")?;

    if !is_passenger(zug) {
        return Ok(());
    }

    for child in &mut zug.children {
        if let XMLNode::Element(e) = child {
            if e.name == "FahrplanEintrag" {
                // A demo implementation of modifying factor based on station.
                //
                // ```
                // let betriebstelle = e.attributes.get("Betrst");
                // let factor = match betriebstelle {
                //     Some(str) => match str.as_str() {
                //         "Köln Hbf" => 6.0,
                //         "Köln Messe/Deutz Hp" => 4.5,
                //         _ => factor,
                //     }
                //     _ => factor,
                // };
                // ```

                let Some(ankunft) = e.attributes.get("Ank") else {
                    continue;
                };
                let ankunft = ankunft.clone();

                let Some(abfahrt) = e.attributes.get_mut("Abf") else {
                    continue;
                };

                let arrival: chrono::NaiveDateTime =
                    chrono::NaiveDateTime::parse_from_str(&ankunft, "%Y-%m-%d %H:%M:%S")
                        .context(format!("parsing arrival time `{ankunft}`"))?;

                let departure: chrono::NaiveDateTime =
                    chrono::NaiveDateTime::parse_from_str(abfahrt, "%Y-%m-%d %H:%M:%S")
                        .context(format!("parsing departure time `{abfahrt}`"))?;

                let original_wait_time = departure - arrival;
                let delayed_wait_time = chrono::TimeDelta::seconds(
                    (original_wait_time.num_seconds() as f32 * factor) as i64,
                )
                .min(max_wait_time);

                let delayed_departure = departure
                    .checked_add_signed(delayed_wait_time)
                    .context("calculating new arrival time")?;

                *abfahrt = delayed_departure.format("%Y-%m-%d %H:%M:%S").to_string();
            }
        }
    }

    Ok(())
}

fn read_file(path: &Path) -> anyhow::Result<Element> {
    let contents = fs::read_to_string(path)?;

    Ok(Element::parse(contents.as_bytes())?)
}

fn write_file(path: &Path, tree: &Element) -> anyhow::Result<()> {
    tree.write(File::create(path)?)?;

    Ok(())
}

fn modify_file(
    path: &Path,
    modify: &Modify,
    rng: &mut rand::rngs::ThreadRng,
) -> anyhow::Result<()> {
    let mut tree = read_file(path)?;

    // multiplier
    {
        let mut loc_multiplier = (modify.friction / modify.loc_needed).min(1.0);
        let mut mu_multiplier = (modify.friction / modify.mu_needed).min(1.0);

        if let Some(multiplier) = modify.multiplier {
            loc_multiplier *= multiplier;
            mu_multiplier *= multiplier;
        }

        // This is only here to not try to perform an unneeded operation if no changes are needed. If friction >= *_needed, then *_multiplier = 1.0, so this check is enough.
        if loc_multiplier != 1.0 || mu_multiplier != 1.0 {
            modify_multiplier(&mut tree, loc_multiplier, mu_multiplier)
                .context("applying multiplier")?;
        }
    }

    // delay entry
    {
        let mut minutes: f32 = 0.0;

        if let Some(p) = modify.delay_probability {
            let val: f32 = rng.gen();

            if val < p {
                minutes +=
                    modify.delay_amplitude * ((modify.delay_lambda * rng.gen::<f32>()).exp() - 1.0);
            }
        }

        if let Some(bell_mean) = modify.bell_mean {
            minutes += rand_distr::Normal::new(bell_mean, modify.bell_deviation)
                .context("unable to generate normal distribution for random number sampling with given parameters")?
                .sample(rng);
        }

        if let Some(p) = modify.uniform_probability {
            if rng.gen::<f32>() < p {
                minutes += modify.uniform_maximum.expect("argument required by clap")
                    * rng.gen::<f32>();
            }
        }

        if modify.deny_early {
            minutes = minutes.max(0.0);
        }

        let seconds = (minutes * 60.0) as u32;

        if seconds != 0 {
            delay_entry(&mut tree, seconds).context("delaying entry")?;
        }
    }

    // delay_departure
    if modify.departures_delay_factor != 1.0 {
        delay_departures(
            &mut tree,
            modify.departures_delay_factor,
            chrono::TimeDelta::seconds((modify.departures_max_delay * 60.0) as i64),
        )
        .context("delaying departures")?;
    }

    write_file(path, &tree)?;

    Ok(())
}

fn duplicate_trains_in_fpn(path: &Path, duplicated: &HashSet<String>) -> anyhow::Result<()> {
    let mut tree = read_file(path)?;

    let fahrplan: &mut Element = tree
        .get_mut_child("Fahrplan")
        .context("no tag `Fahrplan`")?;

    let mut new_fahrplan: Element = fahrplan.clone();

    for child in &fahrplan.children {
        let XMLNode::Element(e) = child else {
            continue;
        };

        if e.name == "Zug" {
            let mut new_element = e.clone();

            let datei = new_element
                .get_mut_child("Datei")
                .context("no tag `Datei` inside `Zug`")?;

            let dateiname = datei
                .attributes
                .get_mut("Dateiname")
                .context("`Datei` inside `Zug` has no attribute `Dateiname`")?;

            let (_folder, nummer) = dateiname
                .strip_suffix(".trn")
                .with_context(|| format!("expected `Dateiname` inside `Zug` to point to `.trn` file, instead it points to {dateiname}"))?
                .rsplit_once(|ch: char| !ch.is_ascii_digit())
                .with_context(|| format!("expected `Dateiname` inside `Zug` to point to a `.trn` file with path consisting of at least one non-digit character, instead it points to {dateiname}"))?;

            if !duplicated.contains(nummer) {
                continue;
            }

            *dateiname = dateiname
                .strip_suffix(".trn")
                .with_context(|| format!("expected `Dateiname` inside `Zug` to point to `.trn` file, instead it points to {dateiname}"))?
                .to_owned()
                .chars()
                .chain("B.trn".chars())
                .collect();

            new_fahrplan.children.push(XMLNode::Element(new_element));
        }
    }

    *fahrplan = new_fahrplan;

    write_file(path, &tree)
}

/// Returns the name of the train if it was duplicated
fn duplicate_trn(path: &Path, modify: &Modify) -> anyhow::Result<Option<String>> {
    let new_path = {
        let mut file_name = path
            .file_stem()
            .context("path to train has no file name")?
            .to_os_string();
        file_name.push("B.trn");
        path.with_file_name(file_name)
    };

    let mut tree = read_file(path).context("reading old `.trn` file")?;

    let zug = tree.get_mut_child("Zug").context("no tag `Zug`")?;

    let group = group_name(zug)?;
    if modify
        .dont_duplicate_group
        .iter()
        .any(|g| group.contains(g))
    {
        return Ok(None);
    }

    let nummer = zug
        .attributes
        .get_mut("Nummer")
        .context("tag `Zug` has no attribute `Nummer`")?;
    let original_nummer = nummer.clone();
    nummer.push('B');

    write_file(&PathBuf::from(&new_path), &tree).context("writing new `.trn` file")?;

    Ok(Some(original_nummer))
}

fn dir_copy_name(dir: &Path) -> Option<PathBuf> {
    let mut file_name = dir.file_name()?.to_os_string();
    file_name.push("_zsw");
    Some(dir.with_file_name(file_name))
}

fn fahrplan_copy_name(fahrplan_file: &Path) -> Option<PathBuf> {
    let mut file_name = fahrplan_file.file_stem()?.to_os_string();
    file_name.push("_zsw.fpn");
    Some(fahrplan_file.with_file_name(file_name))
}

fn print_stack_trace(err: &anyhow::Error) {
    eprintln!("| reason: {}", err.root_cause());

    for context in err.chain().rev().skip(1) {
        eprintln!("| when: {context}");
    }
}

fn create_backup(
    cmd: &Modify,
    dir_copy: &Option<PathBuf>,
    fahrplan: &Path,
    fahrplan_copy: &Option<PathBuf>,
) -> anyhow::Result<()> {
    let dir_copy = dir_copy
        .as_ref()
        .context("failed calculation of name of `_zsw` folder")?;
    let fahrplan_copy = fahrplan_copy
        .as_ref()
        .context("failed calculation of name of `_zsw` fahrplan")?;

    let dir_copy_exists = dir_copy.exists();
    let fahrplan_copy_exists = fahrplan_copy.exists();

    if dir_copy_exists && !fahrplan_copy_exists {
        bail!("`_zsw` folder exists, but `_zsw` fahrplan file doesn't");
    }

    if !dir_copy_exists && fahrplan_copy_exists {
        bail!("`_zsw` folder doesn't exist, but `_zsw` fahrplan file does");
    }

    if dir_copy_exists {
        return Ok(());
    }

    // I don't know how to prevent time-of-check to time-of-use bugs here.
    // It's not worth the time preventing.
    dir::create(dir_copy.clone(), false).context("creating `_zsw` folder")?;
    dir::copy(
        cmd.directory.clone(),
        dir_copy,
        &dir::CopyOptions::new().content_only(true),
    )
    .context("copying contents to `_zsw` folder")?;

    fs::copy(fahrplan, fahrplan_copy).context("copying fahrplan file")?;

    Ok(())
}

fn duplicate_trains(cmd: &Modify, fahrplan: &Path) {
    let Ok(files) = fs::read_dir(&cmd.directory) else {
        eprintln!(
            "Unable to iterate over files in `{}`",
            cmd.directory.to_string_lossy()
        );
        return;
    };

    let duplicated: HashSet<_> = files
        .filter_map(|file| {
            let Ok(path) = file.map(|f| f.path()) else {
                eprintln!(
                    "Error with entry trying to iterate over elements of folder `{}`",
                    cmd.directory.to_string_lossy()
                );

                return None;
            };

            if path.extension() != Some(OsStr::new("trn")) {
                return None;
            }

            match duplicate_trn(&path, cmd) {
                Err(err) => {
                    eprintln!(
                        "Failed to create copied train of {}",
                        path.to_string_lossy()
                    );

                    print_stack_trace(&err);

                    None
                }
                Ok(nummer) => nummer,
            }
        })
        .collect();

    let _ = duplicate_trains_in_fpn(fahrplan, &duplicated).inspect_err(|err| {
        eprintln!("Failed to duplicate train entries inside `.fpn` file");

        print_stack_trace(err);
    });
}

fn modify(cmd: Modify) {
    let dir_copy = dir_copy_name(&cmd.directory);
    let fahrplan = cmd.directory.with_extension("fpn");
    let fahrplan_copy = fahrplan_copy_name(&fahrplan);

    if !cmd.no_copy {
        if let Err(err) = create_backup(&cmd, &dir_copy, &fahrplan, &fahrplan_copy) {
            eprintln!("Failed to create `_zsw` backup folder/fahrplan file, DO NOT REVERT USING `RESET` SUBCOMMAND, revert by deleting `_zsw` folder/fahrplan file");
            print_stack_trace(&err);
            return;
        };
    }

    if cmd.duplicate {
        duplicate_trains(&cmd, &fahrplan);
    }

    let mut rng = rand::thread_rng();

    {
        let Ok(files) = fs::read_dir(&cmd.directory) else {
            eprintln!(
                "Unable to iterate over files in `{}`",
                cmd.directory.to_string_lossy()
            );
            return;
        };

        for file in files {
            let Ok(path) = file.map(|f| f.path()) else {
                eprintln!(
                    "Error with entry trying to iterate over elements of folder `{}`",
                    cmd.directory.to_string_lossy()
                );
                return;
            };

            if path.extension() != Some(OsStr::new("trn")) {
                continue;
            }

            let _ = modify_file(&path, &cmd, &mut rng).inspect_err(|err| {
                eprintln!("Failed file modification, path: {}", path.to_string_lossy());

                print_stack_trace(err);
            });
        }
    }
}

fn reset(cmd: Reset) {
    let Some(zsw_dir) = dir_copy_name(&cmd.directory) else {
        eprintln!("Failed calculation of name of `_zsw` folder");
        return;
    };

    let fahrplan = cmd.directory.with_extension("fpn");
    let Some(fahrplan_copy) = fahrplan_copy_name(&fahrplan) else {
        eprintln!("Failed calculation of name of `_zsw` fahrplan");
        return;
    };

    if !zsw_dir.exists() {
        eprintln!("`_zsw` folder does not exist");
        return;
    }

    if !fahrplan_copy.exists() {
        eprintln!("`_zsw.fpn` backup Fahrplan file does not exist");
        return;
    }

    // I don't know how to prevent time-of-check to time-of-use bugs here.
    // It's not worth the time preventing.
    if let Err(err) = dir::create(cmd.directory.clone(), true) {
        eprintln!("Failed to create empty direction in place of directory containing `.trn` files");
        eprintln!("| reason: {err}");
        return;
    };

    if let Err(err) = dir::move_dir(
        zsw_dir,
        cmd.directory,
        &dir::CopyOptions::new().content_only(true),
    ) {
        eprintln!("Failed to copy contents back from `_zsw` folder");
        eprintln!("| reason: {err}");
        return;
    }

    if let Err(err) = fs::rename(fahrplan_copy, fahrplan) {
        eprintln!("Failed to copy back fahrplan file");
        eprintln!("| reason: {err}");
        return;
    }
}

fn main() {
    let cli = Cli::parse();

    match cli.command {
        Command::Modify(cmd) => modify(cmd),
        Command::Reset(cmd) => reset(cmd),
    }
}
