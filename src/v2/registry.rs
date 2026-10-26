use anyhow::{bail, Context, Result};
use reqwest::blocking::Client;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{collections::BTreeMap, fs, path::Path};
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
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Artifact {
    pub arch: String,
    pub url: String,
    pub sha256: String,
    pub size: u64,
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
