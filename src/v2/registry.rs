use anyhow::{bail, Context, Result};
use base64::{engine::general_purpose::STANDARD, Engine};
use ed25519_dalek::{Signature, Verifier, VerifyingKey};
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
    get_signed_json(registry, "index.json")
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
    let index: PackageIndex = get_signed_json(
        registry,
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
fn get_signed_json<T: for<'a> Deserialize<'a>>(registry: &Registry, path: &str) -> Result<T> {
    let base = Url::parse(&format!("{}/", registry.url.trim_end_matches('/')))?;
    let client = Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()?;
    let body = client
        .get(base.join(path)?)
        .send()?
        .error_for_status()?
        .bytes()?
        .to_vec();
    let signature = client
        .get(base.join(&format!("{path}.sig"))?)
        .send()?
        .error_for_status()?
        .text()?;
    verify_signature(&registry.key, &body, signature.trim())?;
    Ok(serde_json::from_slice(&body)?)
}
fn verify_signature(key: &str, body: &[u8], signature: &str) -> Result<()> {
    let encoded = key
        .strip_prefix("ed25519:")
        .context("registry key must use ed25519: format")?;
    let bytes = STANDARD
        .decode(encoded)
        .context("invalid registry public key encoding")?;
    let key = VerifyingKey::from_bytes(
        bytes
            .as_slice()
            .try_into()
            .context("registry public key must be 32 bytes")?,
    )?;
    let signature = Signature::from_slice(
        &STANDARD
            .decode(signature)
            .context("invalid registry signature encoding")?,
    )?;
    key.verify(body, &signature)
        .context("registry index signature verification failed")
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

#[cfg(test)]
mod tests {
    use super::*;

    fn index_with(versions: &[&str]) -> PackageIndex {
        PackageIndex {
            format: 2,
            name: "yam/base".into(),
            versions: versions
                .iter()
                .map(|version| {
                    (
                        version.to_string(),
                        VersionMetadata {
                            artifacts: BTreeMap::new(),
                            published_at: None,
                            depends: Vec::new(),
                            recommends: Vec::new(),
                            provides: Vec::new(),
                            conflicts: Vec::new(),
                        },
                    )
                })
                .collect(),
        }
    }

    #[test]
    fn an_explicit_prerelease_pin_can_be_selected_as_a_dependency() {
        // docs/PACKAGING.md and AGENTS.md agree a pre-release is only ever
        // selected when pinned explicitly. select_version's own filter
        // (`!version.contains('-')`) runs unconditionally, even for an
        // exact `=` pin, so a dependency on "=1.5.0-rc.1" can never
        // resolve even though the registry published exactly that
        // version.
        let index = index_with(&["1.5.0-rc.1"]);
        let result = select_version(&index, &["=1.5.0-rc.1"]);
        assert!(
            result.is_ok(),
            "an explicit =X.Y.Z-pre dependency constraint must resolve to that pre-release, got {result:?}"
        );
        assert_eq!(result.unwrap(), "1.5.0-rc.1");
    }

    // Characterization tests: this behavior is already correct today and
    // must stay correct once select_version is switched to version.rs.
    #[test]
    fn caret_excludes_the_next_minor_for_a_zero_major() {
        let index = index_with(&["0.2.5", "0.3.0"]);
        assert_eq!(select_version(&index, &["^0.2"]).unwrap(), "0.2.5");
    }

    #[test]
    fn tilde_excludes_the_next_minor() {
        let index = index_with(&["1.2.9", "1.3.0"]);
        assert_eq!(select_version(&index, &["~1.2"]).unwrap(), "1.2.9");
    }

    #[test]
    fn compound_constraints_pick_the_highest_match() {
        let index = index_with(&["1.0.0", "1.9.9", "2.0.0"]);
        assert_eq!(select_version(&index, &[">=1", "<2"]).unwrap(), "1.9.9");
    }

    #[test]
    fn default_selection_never_picks_a_prerelease() {
        let index = index_with(&["1.0.0", "1.1.0-rc.1"]);
        assert_eq!(select_version(&index, &[]).unwrap(), "1.0.0");
    }
}
