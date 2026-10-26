use anyhow::{bail, Context, Result};
use reqwest::blocking::Client;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::Path,
};
use url::Url;

use super::core::PlatformProfile;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct RegistryConfig {
    #[serde(default)]
    pub registries: BTreeMap<String, Registry>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Registry {
    pub url: String,
    pub key: String,
}
#[derive(Debug, Deserialize)]
pub struct Catalogue {
    pub format: u32,
    pub packages: Vec<CatalogueEntry>,
}
#[derive(Debug, Deserialize)]
pub struct CatalogueEntry {
    pub name: String,
    pub summary: String,
    pub latest: String,
    #[serde(default)]
    pub targets: Vec<String>,
    #[serde(default)]
    pub keywords: Vec<String>,
    pub homepage: Option<String>,
}
#[derive(Debug, Serialize, Deserialize)]
pub struct PackageIndex {
    pub format: u32,
    pub name: String,
    pub versions: BTreeMap<String, VersionMetadata>,
}
#[derive(Debug, Serialize, Deserialize)]
pub struct VersionMetadata {
    pub artifacts: BTreeMap<String, Artifact>,
    pub published_at: Option<String>,
    #[serde(default)]
    pub depends: Vec<String>,
    #[serde(default)]
    pub recommends: Vec<String>,
    #[serde(default)]
    pub provides: Vec<String>,
    #[serde(default)]
    pub conflicts: Vec<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Artifact {
    pub arch: String,
    pub url: String,
    pub sha256: String,
    pub size: u64,
}
#[derive(Debug, Clone)]
pub struct ResolvedPackage {
    pub name: String,
    pub version: String,
    pub artifact: Artifact,
}

pub fn config_path(root: &Path) -> std::path::PathBuf {
    root.join("db/registries.yaml")
}
pub fn load_config(root: &Path) -> Result<RegistryConfig> {
    let path = config_path(root);
    match fs::read(&path) {
        Ok(raw) => Ok(serde_yaml::from_slice(&raw)?),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(RegistryConfig::default()),
        Err(error) => Err(error.into()),
    }
}
pub fn save_config(root: &Path, config: &RegistryConfig) -> Result<()> {
    fs::create_dir_all(root.join("db"))?;
    fs::write(config_path(root), serde_yaml::to_string(config)?)?;
    Ok(())
}
pub fn add_registry(root: &Path, name: &str, url: &str, key: &str) -> Result<()> {
    if name.is_empty()
        || !name
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
    {
        bail!("invalid registry name")
    }
    let parsed = Url::parse(url).context("invalid registry URL")?;
    if parsed.scheme() != "https" {
        bail!("registry URL must use HTTPS")
    }
    if !key.starts_with("ed25519:") {
        bail!("registry key must use ed25519: format")
    }
    let mut config = load_config(root)?;
    config.registries.insert(
        name.into(),
        Registry {
            url: url.trim_end_matches('/').into(),
            key: key.into(),
        },
    );
    save_config(root, &config)
}
pub fn remove_registry(root: &Path, name: &str) -> Result<()> {
    let mut config = load_config(root)?;
    if config.registries.remove(name).is_none() {
        bail!("registry not configured")
    };
    save_config(root, &config)
}
pub fn registry(root: &Path, name: &str) -> Result<Registry> {
    load_config(root)?
        .registries
        .remove(name)
        .context("registry not configured")
}
pub fn catalogue(registry: &Registry) -> Result<Catalogue> {
    get_json(&registry.url, "index.json")
}
pub fn package_index(registry: &Registry, name: &str) -> Result<PackageIndex> {
    let mut parts = name.split('/');
    let namespace = parts
        .next()
        .context("package name must be namespace/name")?;
    let package = parts
        .next()
        .context("package name must be namespace/name")?;
    if parts.next().is_some() {
        bail!("package name must be namespace/name")
    }
    let index: PackageIndex = get_json(
        &registry.url,
        &format!("packages/{namespace}/{package}/index.json"),
    )?;
    if index.format != 2 || index.name != name {
        bail!("invalid package index for {name}")
    }
    Ok(index)
}
pub fn select_artifact(
    index: &PackageIndex,
    requested: Option<&str>,
    profile: &PlatformProfile,
) -> Result<(String, Artifact)> {
    let version = match requested {
        Some(version) => version.to_owned(),
        None => index
            .versions
            .keys()
            .filter(|version| !version.contains('-'))
            .max_by(|left, right| version_cmp(left, right))
            .cloned()
            .context("registry has no stable versions")?,
    };
    let metadata = index
        .versions
        .get(&version)
        .context("package version not found")?;
    let artifact = metadata
        .artifacts
        .get(&profile.id)
        .or_else(|| metadata.artifacts.get("any"))
        .context("package version has no artifact for this platform")?
        .clone();
    if artifact.arch != profile.arch {
        bail!(
            "artifact architecture {} does not match {}",
            artifact.arch,
            profile.arch
        )
    }
    if artifact.sha256.len() != 64 || !artifact.sha256.bytes().all(|byte| byte.is_ascii_hexdigit())
    {
        bail!("invalid artifact SHA-256")
    }
    Ok((version, artifact))
}
pub fn download_artifact(registry: &Registry, artifact: &Artifact) -> Result<Vec<u8>> {
    let url = Url::parse(&artifact.url)
        .or_else(|_| Url::parse(&format!("{}/", registry.url))?.join(&artifact.url))?;
    if url.scheme() != "https" {
        bail!("artifact URL must use HTTPS")
    }
    let response = Client::builder()
        .timeout(std::time::Duration::from_secs(120))
        .build()?
        .get(url)
        .send()?
        .error_for_status()?;
    let bytes = response.bytes()?.to_vec();
    if bytes.len() as u64 != artifact.size {
        bail!("artifact size mismatch")
    }
    if format!("{:x}", Sha256::digest(&bytes)) != artifact.sha256 {
        bail!("artifact checksum mismatch")
    }
    Ok(bytes)
}
pub fn resolve(
    registry: &Registry,
    requested: &str,
    requested_version: Option<&str>,
    profile: &PlatformProfile,
) -> Result<Vec<ResolvedPackage>> {
    let mut resolved = Vec::new();
    let mut completed = BTreeSet::new();
    let mut visiting = BTreeSet::new();
    resolve_one(
        registry,
        requested,
        requested_version,
        profile,
        &mut resolved,
        &mut completed,
        &mut visiting,
    )?;
    Ok(resolved)
}
fn resolve_one(
    registry: &Registry,
    name: &str,
    requested_version: Option<&str>,
    profile: &PlatformProfile,
    resolved: &mut Vec<ResolvedPackage>,
    completed: &mut BTreeSet<String>,
    visiting: &mut BTreeSet<String>,
) -> Result<()> {
    if completed.contains(name) {
        return Ok(());
    }
    if !name.contains('/') {
        bail!("virtual dependency {name:?} requires a registry provider policy")
    }
    if !visiting.insert(name.into()) {
        bail!("dependency cycle detected at {name}")
    }
    let index = package_index(registry, name)?;
    let (version, artifact) = select_artifact(&index, requested_version, profile)?;
    let metadata = index
        .versions
        .get(&version)
        .expect("selected version exists");
    for requirement in &metadata.depends {
        let (dependency, constraints) = split_requirement(requirement)?;
        let dependency_index = package_index(registry, dependency)?;
        let selected = select_version(&dependency_index, &constraints)?;
        resolve_one(
            registry,
            dependency,
            Some(&selected),
            profile,
            resolved,
            completed,
            visiting,
        )?;
    }
    visiting.remove(name);
    completed.insert(name.into());
    resolved.push(ResolvedPackage {
        name: name.into(),
        version,
        artifact,
    });
    Ok(())
}
fn split_requirement(value: &str) -> Result<(&str, Vec<&str>)> {
    let mut words = value.split_whitespace();
    let name = words.next().context("empty dependency")?;
    let constraints = words.collect::<Vec<_>>();
    if constraints.iter().any(|constraint| {
        ![">=", "<", "^", "~", "="]
            .iter()
            .any(|operator| constraint.starts_with(operator))
    }) {
        bail!("invalid dependency constraint {value:?}")
    }
    Ok((name, constraints))
}
fn select_version(index: &PackageIndex, constraints: &[&str]) -> Result<String> {
    index
        .versions
        .keys()
        .filter(|version| {
            !version.contains('-')
                && constraints
                    .iter()
                    .all(|constraint| version_matches(version, constraint))
        })
        .max_by(|left, right| version_cmp(left, right))
        .cloned()
        .context("no registry version satisfies dependency")
}
fn version_matches(version: &str, constraint: &str) -> bool {
    let Some((operator, expected)) = [">=", "^", "~", "=", "<"].iter().find_map(|operator| {
        constraint
            .strip_prefix(operator)
            .map(|expected| (*operator, expected))
    }) else {
        return false;
    };
    let compare = version_cmp(version, &normalize_version(expected));
    match operator {
        ">=" => compare.is_ge(),
        "<" => compare.is_lt(),
        "=" => compare.is_eq(),
        "~" => compare.is_ge() && same_parts(version, expected, 2),
        "^" => {
            compare.is_ge()
                && same_parts(
                    version,
                    expected,
                    if expected.starts_with("0.") { 2 } else { 1 },
                )
        }
        _ => false,
    }
}
fn normalize_version(value: &str) -> String {
    let mut parts = value.split('.').collect::<Vec<_>>();
    while parts.len() < 3 {
        parts.push("0");
    }
    parts.join(".")
}
fn same_parts(version: &str, expected: &str, count: usize) -> bool {
    version
        .split('.')
        .zip(expected.split('.'))
        .take(count)
        .all(|(left, right)| left == right)
}
fn get_json<T: for<'a> Deserialize<'a>>(base: &str, path: &str) -> Result<T> {
    let url = Url::parse(&format!("{}/", base.trim_end_matches('/')))?.join(path)?;
    let response = Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()?
        .get(url)
        .send()?
        .error_for_status()?;
    Ok(response.json()?)
}
fn version_cmp(left: &str, right: &str) -> std::cmp::Ordering {
    let parse = |value: &str| {
        value
            .split('.')
            .map(|part| part.parse::<u64>().unwrap_or(0))
            .collect::<Vec<_>>()
    };
    let (left, right) = (parse(left), parse(right));
    for index in 0..left.len().max(right.len()) {
        match left
            .get(index)
            .copied()
            .unwrap_or(0)
            .cmp(&right.get(index).copied().unwrap_or(0))
        {
            std::cmp::Ordering::Equal => {}
            order => return order,
        }
    }
    std::cmp::Ordering::Equal
}
