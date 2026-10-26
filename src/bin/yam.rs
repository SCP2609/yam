use anyhow::{bail, Context, Result};
use clap::{Parser, Subcommand};
use std::{
    fs,
    io::Read,
    path::{Component, Path, PathBuf},
};
use yam::v2::registry;
use yam::v2::{Manager, PlatformProfile};

#[derive(Parser)]
#[command(name = "yam", about = "YAM v2 package manager")]
struct Cli {
    /// Platform profile ID, such as yandexmicro.
    #[arg(long, env = "YAM_PLATFORM")]
    platform: String,
    /// Directory containing platform YAML profiles.
    #[arg(long, default_value = "platforms")]
    profiles: PathBuf,
    /// Override the profile's persistent package root (useful for development).
    #[arg(long, env = "YAM_ROOT")]
    root: Option<PathBuf>,
    /// Configured registry used by package-name installation.
    #[arg(long, default_value = "official")]
    registry: String,
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    Pack {
        source: PathBuf,
        #[arg(short, long)]
        output: PathBuf,
    },
    Install {
        package: String,
        #[arg(short = 'y', long)]
        yes: bool,
    },
    Remove {
        name: String,
    },
    List,
    Files {
        name: String,
    },
    Which {
        path: String,
    },
    Doctor,
    Entware {
        #[command(subcommand)]
        command: EntwareCommand,
    },
    Registry {
        #[command(subcommand)]
        command: RegistryCommand,
    },
    Search {
        query: String,
    },
    Info {
        name: String,
    },
    /// Convert a legacy .yap archive into a reviewable v2 source tree.
    ConvertYap {
        archive: PathBuf,
        output: PathBuf,
    },
    /// The v1 HTTP client remains available during migration.
    Legacy {
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },
}
#[derive(Subcommand)]
enum EntwareCommand {
    Status,
    List,
}
#[derive(Subcommand)]
enum RegistryCommand {
    List,
    Add {
        name: String,
        url: String,
        #[arg(long)]
        key: String,
    },
    Remove {
        name: String,
    },
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    if let Command::Legacy { args } = &cli.command {
        let executable = std::env::current_exe()?.with_file_name("yam-legacy");
        let status = std::process::Command::new(executable).args(args).status()?;
        if !status.success() {
            bail!("yam-legacy exited with {status}")
        }
        return Ok(());
    }
    let profile = PlatformProfile::load(&cli.profiles, &cli.platform)?;
    match cli.command {
        Command::Pack { source, output } => Manager::pack(&source, &profile, &output),
        Command::ConvertYap { archive, output } => convert_yap(&archive, &output),
        command => {
            let root = cli.root.unwrap_or_else(|| profile.paths.root.clone());
            let mut manager = Manager::open(root, profile)?;
            match command {
                Command::Install { package, yes } => {
                    let manifest = if Path::new(&package).is_file() {
                        manager.install(Path::new(&package), yes)?
                    } else {
                        let (name, version) = split_package_version(&package)?;
                        let selected = registry::registry(&manager_root(&manager), &cli.registry)?;
                        let index = registry::package_index(&selected, name)?;
                        let (_, artifact) =
                            registry::select_artifact(&index, version, manager.profile())?;
                        let bytes = registry::download_artifact(&selected, &artifact)?;
                        let temporary =
                            manager_root(&manager).join(".staging/registry-download.yampkg");
                        fs::write(&temporary, bytes)?;
                        let result = manager.install(&temporary, yes);
                        let _ = fs::remove_file(&temporary);
                        result?
                    };
                    println!(
                        "Installed {} {} for {}.",
                        manifest.name, manifest.version, manifest.target
                    );
                }
                Command::Remove { name } => {
                    manager.remove(&name)?;
                    println!("Removed {name}.");
                }
                Command::List => {
                    for package in manager.list()? {
                        println!("{}\t{}\t{}", package.name, package.version, package.target);
                    }
                }
                Command::Files { name } => {
                    for path in manager.files(&name)? {
                        println!("{path}");
                    }
                }
                Command::Which { path } => match manager.which(&path)? {
                    Some(package) => println!("{package}"),
                    None => bail!("{path} is not owned by a YAM package"),
                },
                Command::Doctor => {
                    let problems = manager.doctor()?;
                    if problems.is_empty() {
                        println!("YAM v2 state is healthy.");
                    } else {
                        for problem in problems {
                            eprintln!("problem: {problem}");
                        }
                        bail!("doctor found inconsistencies")
                    }
                }
                Command::Entware { command } => match command {
                    EntwareCommand::Status => println!(
                        "{}",
                        if manager.entware_available() {
                            "available"
                        } else {
                            "unavailable"
                        }
                    ),
                    EntwareCommand::List => print!("{}", manager.entware_list()?),
                },
                Command::Registry { command } => match command {
                    RegistryCommand::List => {
                        for (name, entry) in
                            registry::load_config(&manager_root(&manager))?.registries
                        {
                            println!("{name}\t{}", entry.url);
                        }
                    }
                    RegistryCommand::Add { name, url, key } => {
                        registry::add_registry(&manager_root(&manager), &name, &url, &key)?;
                        println!("Added registry {name}.");
                    }
                    RegistryCommand::Remove { name } => {
                        registry::remove_registry(&manager_root(&manager), &name)?;
                        println!("Removed registry {name}.");
                    }
                },
                Command::Search { query } => {
                    let selected = registry::registry(&manager_root(&manager), &cli.registry)?;
                    for package in
                        registry::catalogue(&selected)?
                            .packages
                            .into_iter()
                            .filter(|entry| {
                                entry.name.contains(&query)
                                    || entry.summary.to_lowercase().contains(&query.to_lowercase())
                                    || entry
                                        .keywords
                                        .iter()
                                        .any(|keyword| keyword.contains(&query))
                            })
                    {
                        println!("{}\t{}\t{}", package.name, package.latest, package.summary);
                    }
                }
                Command::Info { name } => {
                    let selected = registry::registry(&manager_root(&manager), &cli.registry)?;
                    println!(
                        "{}",
                        serde_json::to_string_pretty(&registry::package_index(&selected, &name)?)?
                    );
                }
                Command::Pack { .. } | Command::ConvertYap { .. } | Command::Legacy { .. } => {
                    unreachable!()
                }
            }
            Ok(())
        }
    }
}
fn manager_root(manager: &Manager) -> PathBuf {
    manager.root().to_path_buf()
}
fn split_package_version(value: &str) -> Result<(&str, Option<&str>)> {
    let (name, version) = value
        .rsplit_once('@')
        .map_or((value, None), |(name, version)| (name, Some(version)));
    if name.is_empty() || version.is_some_and(str::is_empty) {
        bail!("package name must be name or name@version")
    }
    Ok((name, version))
}

fn convert_yap(archive: &Path, output: &Path) -> Result<()> {
    if output.exists() {
        bail!("output already exists: {}", output.display())
    }
    fs::create_dir_all(output.join("payload"))?;
    let mut manifest = None;
    if archive
        .extension()
        .is_some_and(|extension| extension == "yap")
        && zip::ZipArchive::new(fs::File::open(archive)?).is_ok()
    {
        let mut zip = zip::ZipArchive::new(fs::File::open(archive)?)?;
        for index in 0..zip.len() {
            let mut entry = zip.by_index(index)?;
            let path = entry.enclosed_name().context("unsafe ZIP path")?.to_owned();
            if entry.is_dir() {
                continue;
            }
            let name = path.to_string_lossy().to_string();
            let mut bytes = Vec::new();
            entry.read_to_end(&mut bytes)?;
            extract_legacy_file(output, &name, bytes, &mut manifest)?;
        }
    } else {
        let file = fs::File::open(archive)?;
        let mut tar = tar::Archive::new(flate2::read::GzDecoder::new(file));
        for entry in tar.entries()? {
            let mut entry = entry?;
            if !entry.header().entry_type().is_file() {
                continue;
            }
            let path = entry.path()?.into_owned();
            safe_relative(&path)?;
            let name = path.to_string_lossy().to_string();
            let mut bytes = Vec::new();
            entry.read_to_end(&mut bytes)?;
            extract_legacy_file(output, &name, bytes, &mut manifest)?;
        }
    }
    let manifest = manifest.context("legacy archive has no root plugin.toml")?;
    let value: toml::Value = toml::from_str(&String::from_utf8(manifest)?)?;
    let id = value
        .get("id")
        .and_then(toml::Value::as_str)
        .context("legacy manifest has no id")?;
    let name = id
        .chars()
        .map(|character| {
            if character.is_ascii_lowercase() || character.is_ascii_digit() || character == '-' {
                character
            } else {
                '-'
            }
        })
        .collect::<String>();
    let version = value
        .get("version")
        .and_then(toml::Value::as_str)
        .unwrap_or("0.0.0");
    let description = value
        .get("description")
        .and_then(toml::Value::as_str)
        .unwrap_or("Converted legacy package");
    let mut files = serde_yaml::Mapping::new();
    for entry in walkdir::WalkDir::new(output.join("payload")).min_depth(1) {
        let entry = entry?;
        if !entry.file_type().is_file() {
            continue;
        }
        let path = entry
            .path()
            .strip_prefix(output.join("payload"))?
            .to_string_lossy()
            .replace('\\', "/");
        files.insert(
            serde_yaml::Value::String(path.clone()),
            serde_yaml::Value::String(format!("payload/{path}")),
        );
    }
    let mut document = serde_yaml::Mapping::new();
    document.insert("name".into(), format!("legacy/{name}").into());
    document.insert("version".into(), version.into());
    document.insert("license".into(), "NOASSERTION".into());
    document.insert("description".into(), description.into());
    document.insert(
        "targets".into(),
        serde_yaml::Value::Sequence(vec!["any".into()]),
    );
    document.insert("files".into(), serde_yaml::Value::Mapping(files));
    fs::write(output.join("yam.yaml"), serde_yaml::to_string(&document)?)?;
    fs::write(output.join("MIGRATION.md"), "This tree was converted from a legacy .yap archive. Review yam.yaml, choose real targets, and migrate any Lua runtime metadata before packing. No legacy permissions, launch settings, or hooks were imported.\n")?;
    println!(
        "Converted {} to {}. Review yam.yaml before packing.",
        archive.display(),
        output.display()
    );
    Ok(())
}
fn extract_legacy_file(
    output: &Path,
    name: &str,
    bytes: Vec<u8>,
    manifest: &mut Option<Vec<u8>>,
) -> Result<()> {
    let path = Path::new(name);
    safe_relative(path)?;
    if name == "plugin.toml" {
        if manifest.replace(bytes).is_some() {
            bail!("duplicate plugin.toml")
        };
        return Ok(());
    }
    let target = output.join("payload").join(path);
    if let Some(parent) = target.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(target, bytes)?;
    Ok(())
}
fn safe_relative(path: &Path) -> Result<()> {
    if path.as_os_str().is_empty()
        || path.is_absolute()
        || path.components().any(|component| {
            matches!(
                component,
                Component::ParentDir | Component::RootDir | Component::Prefix(_)
            )
        })
    {
        bail!("unsafe archive path")
    }
    Ok(())
}
