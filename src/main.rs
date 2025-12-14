use std::{
    collections::HashSet,
    ffi::{OsStr, OsString},
    fs::{self, File},
    path::{Path, PathBuf},
};

use anyhow::{bail, Context};
use clap::{Parser, Subcommand};
use fs_extra::dir;
use rand::Rng;
use rand_distr::Distribution;
use xmltree::{Element, XMLNode};

/// zsw
///
/// Cause general chaos.
///
/// The main features of the program are:
///
/// - delay the entry of trains into the simulation (e.g. --uniform-probability, --uniform_maximum)
/// - make trains accelerate (and decelerate) slower (e.g. --fricition)
/// - duplicate trains for more traffic (--duplicated-trains)
///
/// The code doesn't really understand Zusi file structure and is more similar to a glorified complicated find-replace. As such it doesn't use the personal data directory (as it should), but instead creates a backup of the old data and modifies the data in place. (Maybe make a backup of your game before running this program? As you can tell, it's not well programmed, so you probably shouldn't trust it.)
///
/// ## Example usage
///
/// First run the following command inside `...\_ZusiData\Timetables\Deutschland\VDE8`:
///
/// ```cmd
/// zsw modify Erfurt-Theuern_2025_10-14Uhr_Fiktiver-D-Takt --friction 0.2 --uniform-probability 1 --uniform-maximum 15 --duplicate --dont-duplicate-group G16 --dont-duplicate-group G32 --dont-duplicate-group SV --dont-duplicate-group F45
/// ```
///
/// This creates a `Erfurt-Theuern_2025_10-14Uhr_Fiktiver-D-Takt_zsw` folder and `Erfurt-Theuern_2025_10-14Uhr_Fiktiver-D-Takt_zsw.fpn` file. This is the backup of the old data.
///
/// The file `Erfurt-Theuern_2025_10-14Uhr_Fiktiver-D-Takt.fpn` and folder `Erfurt-Theuern_2025_10-14Uhr_Fiktiver-D-Takt` were modified such that
///
/// - the trains accelerate and decelerate as the fricition coefficient were 0.2
/// - the entry of trains is randomly delayed by up to 15 minutes
/// - for every train, a second copy is created for more traffic; however, the trains in groups G16, G32, SV and F45 aren't duplicated to avoid deadlocks
///
/// Start Zusi and hopefully have a deadlock free ride.
///
/// To reset all modifications, finally now run
///
/// ```cmd
/// zsw reset Erfurt-Theuern_2025_10-14Uhr_Fiktiver-D-Takt
/// ```
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

/// Modify the timetables of trains.
///
/// By default, this creates a backup folder and `.fpn` file, so it can be reverted. This can be bypassed with the --no-copy option. If a backup folder and `.fpn` file already exist, a new one isn't created and they're left alone.
#[derive(Debug, Parser)]
struct Modify {
    /// Path of the folder containing the timetable files.
    ///
    /// This folder should contain '.trn' and '.timetable.xml' files. An `.fpn` file should exist with the same name as the folder.
    ///
    /// Unfortunately, it is currently not possible to have the `.fpn` file named differently.
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

    /// Delays the entry of trains using an exponential function with the given probability.
    ///
    /// Delays the entry of trains by A(exp(μr)-1) where A is the amplitude and r is a random real in the interval [0, 1).
    ///
    /// See also: --exponential-amplitude, --exponential-lambda.
    #[arg(
        visible_alias = "ep",
        long,
        requires = "exponential_amplitude",
        requires = "exponential_lambda"
    )]
    exponential_probability: Option<f32>,
    /// Amplitude used in the exponential function, see --exponential-probability for further information.
    #[arg(
        visible_alias = "ea",
        long,
        requires = "exponential_probability",
        requires = "exponential_lambda"
    )]
    exponential_amplitude: Option<f32>,
    /// λ parameter of the exponential function, see --exponential-probability for further information.
    #[arg(
        visible_alias = "el",
        long,
        requires = "exponential_probability",
        requires = "exponential_amplitude"
    )]
    exponential_lambda: Option<f32>,

    /// Delays the entry of trains according to a normal distribution with the given mean in minutes.
    ///
    /// See also --bell-deviation.
    #[arg(visible_alias = "bm", long, requires = "bell_deviation")]
    bell_mean: Option<f32>,
    /// Standard deviation used in the normal distribution in minutes, see --bell-mean for further information.
    #[arg(visible_alias = "bd", long, requires = "bell_mean")]
    bell_deviation: Option<f32>,

    /// Delays the entry of trains according to a uniform distribution with the given probability.
    ///
    /// With the given probability, a delay is chosing uniformly in the interval [0, M] where M is the value of --uniform-maximum.
    #[arg(visible_alias = "up", long, requires = "uniform_maximum")]
    uniform_probability: Option<f32>,
    /// Maximum delay used in the uniform distribution in minutes. See --uniform-probability for further information.
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

    /// Remove train from simulation.
    ///
    /// Useful for dealing with trains that would block things
    ///
    /// TODO: implement
    #[arg(short = 'C', long, num_args=0..)]
    cancel_train: Vec<OsString>,
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

        if let Some(p) = modify.exponential_probability {
            let val: f32 = rng.gen();

            if val < p {
                let amplitude = modify
                    .exponential_amplitude
                    .expect("argument required by clap");

                let lambda = modify
                    .exponential_lambda
                    .expect("argument required by clap");

                minutes += amplitude * ((lambda * rng.gen::<f32>()).exp() - 1.0);
            }
        }

        if let Some(bell_mean) = modify.bell_mean {
            minutes += rand_distr::Normal::new(bell_mean, modify.bell_deviation.expect("argument required by clap"))
                .context("unable to generate normal distribution for random number sampling with given parameters")?
                .sample(rng);
        }

        if let Some(p) = modify.uniform_probability {
            if rng.gen::<f32>() < p {
                minutes +=
                    modify.uniform_maximum.expect("argument required by clap") * rng.gen::<f32>();
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

fn duplicate_trains_in_fpn(path: &Path, duplicated: &HashSet<OsString>) -> anyhow::Result<()> {
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

            let file_name = Path::new(OsStr::new(dateiname))
                .file_stem()
                .with_context(|| format!("expected `Dateiname` inside `Zug` to point to a path with a file stem (portion of the file name without the extension), instead it points to {dateiname}"))?;

            if !duplicated.contains(file_name) {
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

/// Returns the file stem of the path if the train was duplicated
fn duplicate_trn(path: &Path, modify: &Modify) -> anyhow::Result<Option<OsString>> {
    let file_stem = path
        .file_stem()
        .context("path to train has no file name")?
        .to_os_string();

    let new_path = {
        let mut file_name = file_stem.clone();
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
    nummer.push('B');

    write_file(&PathBuf::from(&new_path), &tree).context("writing new `.trn` file")?;

    Ok(Some(file_stem))
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
                Ok(file_name) => file_name,
            }
        })
        .collect();

    let _ = duplicate_trains_in_fpn(fahrplan, &duplicated).inspect_err(|err| {
        eprintln!("Failed to duplicate train entries inside `.fpn` file");

        print_stack_trace(err);
    });
}

fn cancel_trains(cmd: &Modify, path: &Path) -> anyhow::Result<()> {
    // This only cancels trains in the `fpl` file.
    //
    // Actually removing the unnecessary `.trn` files is unnecessary and carries risk (as does any file deletion operation).

    let mut tree = read_file(path)?;

    let fahrplan: &mut Element = tree
        .get_mut_child("Fahrplan")
        .context("no tag `Fahrplan`")?;

    let new_children: anyhow::Result<Vec<&XMLNode>> = fahrplan
        .children
        .iter()
        .filter_map(|child| {
            let XMLNode::Element(e) = child else {
                return Some(Ok(child));
            };

            if e.name != "Zug" {
                return Some(Ok(child));
            }

            let datei = match e.get_child("Datei").context("no tag `Datei` inside `Zug`") {
                Ok(ok) => ok,
                Err(err) => return Some(Err(err)),
            };

            let dateiname = match datei
                .attributes
                .get("Dateiname")
                .context("`Datei` inside `Zug` has no attribute `Dateiname`")
            {
                Ok(ok) => ok,
                Err(err) => return Some(Err(err)),
            };

            let train_number = match Path::new(OsStr::new(dateiname))
                .file_stem()
                .with_context(|| format!("expected `Dateiname` inside `Zug` to point to a path with a file stem (portion of the file name without the extension), instead it points to {dateiname}"))
            {
                Ok(ok) => ok,
                Err(err) => return Some(Err(err)),
            };

            dbg!(&train_number);

            // This is a linear search, but optimsation is probably not needed.
            match cmd.cancel_train.iter().any(|t| t == train_number) {
                false => Some(Ok(child)),
                true => None,
            }
        })
        .collect();

    let new_children = new_children?;

    fahrplan.children = new_children.iter().map(|child| (*child).clone()).collect();

    write_file(path, &tree)?;

    Ok(())
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

    if !cmd.cancel_train.is_empty() {
        if let Err(err) = cancel_trains(&cmd, &fahrplan) {
            eprintln!("Failed to cancel trains");
            print_stack_trace(&err);
        }
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
        // For symmetry and in case code gets added below.
        #[expect(clippy::needless_return)]
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
