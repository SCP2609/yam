use anyhow::{bail, Context, Result};
use base64::{engine::general_purpose::STANDARD, Engine};
use ed25519_dalek::{Signature, Verifier, VerifyingKey};
use reqwest::blocking::Client;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    io::{Read, Write},
    path::Path,
};
use url::Url;

use super::core::PlatformProfile;
use super::version;

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
#[derive(Debug, Clone, Deserialize)]
pub struct Catalogue {
    pub format: u32,
    pub packages: Vec<CatalogueEntry>,
}
#[derive(Debug, Clone, Deserialize)]
pub struct CatalogueEntry {
    pub name: String,
    pub summary: String,
    pub latest: String,
    #[serde(default)]
    pub targets: Vec<String>,
    #[serde(default)]
    pub keywords: Vec<String>,
    pub homepage: Option<String>,
    /// Virtual names this package's latest version provides. Lets the
    /// short-name resolver find a candidate from the catalogue alone,
    /// without fetching every package's full index (B2). Absent in an
    /// older index, read as "provides nothing".
    #[serde(default)]
    pub provides: Vec<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PackageIndex {
    pub format: u32,
    pub name: String,
    pub versions: BTreeMap<String, VersionMetadata>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
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
    /// Packages this version replaces, mirroring
    /// `PackageManifest::replaces`. Not consumed by the resolver in this
    /// PR; present so a future resolver-side replaces check (A4-adjacent)
    /// doesn't need another index format bump.
    #[serde(default)]
    pub replaces: Vec<String>,
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
            .filter_map(|key| version::parse_version(key).ok().map(|parsed| (parsed, key)))
            .filter(|(parsed, _)| parsed.pre.is_empty())
            .max_by(|left, right| left.0.cmp(&right.0))
            .map(|(_, key)| key.clone())
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
/// Download `artifact` straight into `destination`, hashing it as it's
/// written rather than buffering the whole thing in memory first (B4):
/// an artifact can be large, and nothing here should hold more than one
/// chunk of it in RAM at a time. Bails as soon as more bytes arrive than
/// the signed index declared, rather than after the fact.
pub fn download_artifact(
    registry: &Registry,
    artifact: &Artifact,
    destination: &Path,
) -> Result<()> {
    let url = Url::parse(&artifact.url)
        .or_else(|_| Url::parse(&format!("{}/", registry.url))?.join(&artifact.url))?;
    if url.scheme() != "https" {
        bail!("artifact URL must use HTTPS")
    }
    let mut response = Client::builder()
        .timeout(std::time::Duration::from_secs(120))
        .build()?
        .get(url)
        .send()?
        .error_for_status()?;
    let mut file = fs::File::create(destination)
        .with_context(|| format!("create {}", destination.display()))?;
    let mut hasher = Sha256::new();
    let mut buffer = [0u8; 64 * 1024];
    let mut downloaded: u64 = 0;
    loop {
        let read = response.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        downloaded += read as u64;
        if downloaded > artifact.size {
            bail!("artifact exceeded its declared size while downloading")
        }
        hasher.update(&buffer[..read]);
        file.write_all(&buffer[..read])?;
    }
    if downloaded != artifact.size {
        bail!("artifact size mismatch")
    }
    if format!("{:x}", hasher.finalize()) != artifact.sha256 {
        bail!("artifact checksum mismatch")
    }
    Ok(())
}

/// Source of registry data, abstracted so the resolution algorithm
/// (`resolve_with`) can be exercised against fixed catalogues/indexes in
/// tests instead of a live HTTPS registry (B2).
pub trait IndexSource {
    fn catalogue(&mut self) -> Result<&Catalogue>;
    fn package_index(&mut self, name: &str) -> Result<&PackageIndex>;
}

pub struct LiveSource<'a> {
    registry: &'a Registry,
    catalogue: Option<Catalogue>,
    indices: BTreeMap<String, PackageIndex>,
}
impl<'a> LiveSource<'a> {
    pub fn new(registry: &'a Registry) -> Self {
        Self {
            registry,
            catalogue: None,
            indices: BTreeMap::new(),
        }
    }
}
impl IndexSource for LiveSource<'_> {
    fn catalogue(&mut self) -> Result<&Catalogue> {
        if self.catalogue.is_none() {
            self.catalogue = Some(catalogue(self.registry)?);
        }
        Ok(self.catalogue.as_ref().expect("just populated"))
    }
    fn package_index(&mut self, name: &str) -> Result<&PackageIndex> {
        if !self.indices.contains_key(name) {
            let index = package_index(self.registry, name)?;
            self.indices.insert(name.to_owned(), index);
        }
        Ok(self.indices.get(name).expect("just populated"))
    }
}

/// Fixed-index source for tests: never touches the network.
#[cfg(test)]
pub struct FakeSource {
    pub catalogue: Catalogue,
    pub indices: BTreeMap<String, PackageIndex>,
}
#[cfg(test)]
impl IndexSource for FakeSource {
    fn catalogue(&mut self) -> Result<&Catalogue> {
        Ok(&self.catalogue)
    }
    fn package_index(&mut self, name: &str) -> Result<&PackageIndex> {
        self.indices
            .get(name)
            .with_context(|| format!("no index published for {name}"))
    }
}

/// What the local database already has installed, so the resolver can
/// skip a dependency (or a root request) that's already satisfied
/// instead of planning to redownload or reinstall it (B2).
#[derive(Debug, Clone, Default)]
pub struct InstalledSnapshot {
    pub packages: BTreeMap<String, semver::Version>,
    /// Virtual name -> installed packages that provide it.
    pub provides: BTreeMap<String, BTreeSet<String>>,
}
impl InstalledSnapshot {
    pub fn empty() -> Self {
        Self::default()
    }
    fn satisfies(&self, name: &str, constraints: &version::Constraints) -> bool {
        if let Some(version) = self.packages.get(name) {
            if constraints.matches(version) {
                return true;
            }
        }
        if let Some(providers) = self.provides.get(name) {
            for provider in providers {
                if let Some(version) = self.packages.get(provider) {
                    if constraints.matches(version) {
                        return true;
                    }
                }
            }
        }
        false
    }
}

/// `resolve`'s replacement, built on `IndexSource` so it can be unit
/// tested against fixed data (B2). A full `ns/name` request resolves to
/// exactly that package; a short name's resolution against `installed`
/// and the catalogue's `yam/` providers is not implemented yet in this
/// commit -- see the `TODO(B2)` in `resolve_one_with`.
pub fn resolve_with<S: IndexSource>(
    source: &mut S,
    requested: &str,
    requested_version: Option<&str>,
    profile: &PlatformProfile,
    installed: &InstalledSnapshot,
) -> Result<Vec<ResolvedPackage>> {
    let mut resolved = Vec::new();
    let mut completed = BTreeSet::new();
    let mut visiting = BTreeSet::new();
    resolve_one_with(
        source,
        requested,
        requested_version,
        profile,
        installed,
        &mut resolved,
        &mut completed,
        &mut visiting,
    )?;
    Ok(resolved)
}

#[allow(clippy::too_many_arguments)]
fn resolve_one_with<S: IndexSource>(
    source: &mut S,
    name: &str,
    requested_version: Option<&str>,
    profile: &PlatformProfile,
    installed: &InstalledSnapshot,
    resolved: &mut Vec<ResolvedPackage>,
    completed: &mut BTreeSet<String>,
    visiting: &mut BTreeSet<String>,
) -> Result<()> {
    if completed.contains(name) {
        return Ok(());
    }
    if !name.contains('/') {
        // TODO(B2): resolve a short/virtual name against `installed`
        // and the catalogue's yam/ providers instead of always
        // refusing. Kept identical to the old resolve_one for now so
        // introducing IndexSource is a pure refactor on its own.
        bail!("virtual dependency {name:?} requires a registry provider policy")
    }
    if !visiting.insert(name.into()) {
        bail!("dependency cycle detected at {name}")
    }
    let index = source.package_index(name)?.clone();
    let version = match requested_version {
        Some(version) => version.to_owned(),
        None => index
            .versions
            .keys()
            .filter_map(|key| version::parse_version(key).ok().map(|parsed| (parsed, key)))
            .filter(|(parsed, _)| parsed.pre.is_empty())
            .max_by(|left, right| left.0.cmp(&right.0))
            .map(|(_, key)| key.clone())
            .context("registry has no stable versions")?,
    };
    let metadata = index
        .versions
        .get(&version)
        .context("package version not found")?
        .clone();
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
    for requirement in &metadata.depends {
        let (dependency, constraint_words) = split_requirement(requirement)?;
        // TODO(B2): skip a dependency installed already satisfies,
        // same as the root (see the short-name TODO above).
        let dependency_index = source.package_index(dependency)?;
        let selected = select_version(dependency_index, &constraint_words)?;
        resolve_one_with(
            source,
            dependency,
            Some(&selected),
            profile,
            installed,
            resolved,
            completed,
            visiting,
        )?;
    }
    visiting.remove(name);
    completed.insert(name.to_owned());
    resolved.push(ResolvedPackage {
        name: name.into(),
        version,
        artifact,
    });
    Ok(())
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
/// Highest published version matching every constraint. A pre-release is
/// only matched when a constraint pins to it explicitly at the same
/// major.minor.patch (`version::Constraints`/`semver::VersionReq`'s own
/// rule); no separate dash-based filter is needed or applied here.
fn select_version(index: &PackageIndex, constraints: &[&str]) -> Result<String> {
    let parsed = version::Constraints::parse(&constraints.join(" "))?;
    index
        .versions
        .keys()
        .filter_map(|key| {
            version::parse_version(key)
                .ok()
                .map(|version| (version, key))
        })
        .filter(|(version, _)| parsed.matches(version))
        .max_by(|left, right| left.0.cmp(&right.0))
        .map(|(_, key)| key.clone())
        .context("no registry version satisfies dependency")
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
                            replaces: Vec::new(),
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

    fn profile() -> PlatformProfile {
        serde_yaml::from_str(
            "id: yandexmicro\narch: aarch64\ntriple: aarch64-unknown-linux-musl\npaths:\n  root: /tmp/root\n  etc: /tmp/etc\n  var: /tmp/var\n  run: /tmp/run\n",
        )
        .unwrap()
    }

    fn artifact() -> Artifact {
        Artifact {
            arch: "aarch64".into(),
            url: "https://example/a.yampkg".into(),
            sha256: "0".repeat(64),
            size: 1,
        }
    }

    /// An index with one or more versions, each with its own `depends`
    /// and `provides`, for B2's cross-package resolution tests.
    fn index(name: &str, versions: &[(&str, Vec<&str>, Vec<&str>)]) -> PackageIndex {
        PackageIndex {
            format: 2,
            name: name.into(),
            versions: versions
                .iter()
                .map(|(version, depends, provides)| {
                    (
                        version.to_string(),
                        VersionMetadata {
                            artifacts: BTreeMap::from([("any".to_string(), artifact())]),
                            published_at: None,
                            depends: depends.iter().map(|d| d.to_string()).collect(),
                            recommends: Vec::new(),
                            provides: provides.iter().map(|p| p.to_string()).collect(),
                            conflicts: Vec::new(),
                            replaces: Vec::new(),
                        },
                    )
                })
                .collect(),
        }
    }

    fn entry(name: &str, provides: &[&str]) -> CatalogueEntry {
        CatalogueEntry {
            name: name.into(),
            summary: String::new(),
            latest: "1.0.0".into(),
            targets: Vec::new(),
            keywords: Vec::new(),
            homepage: None,
            provides: provides.iter().map(|p| p.to_string()).collect(),
        }
    }

    #[test]
    fn short_name_resolves_to_the_official_yam_package() {
        let mut source = FakeSource {
            catalogue: Catalogue {
                format: 1,
                packages: vec![entry("yam/spotify", &[])],
            },
            indices: BTreeMap::from([(
                "yam/spotify".to_string(),
                index("yam/spotify", &[("1.0.0", vec![], vec![])]),
            )]),
        };
        let result = resolve_with(
            &mut source,
            "spotify",
            None,
            &profile(),
            &InstalledSnapshot::empty(),
        );
        assert!(
            result.is_ok(),
            "a short name with a single yam/ provider must resolve, got {result:?}"
        );
        let resolved = result.unwrap();
        assert_eq!(resolved.len(), 1);
        assert_eq!(resolved[0].name, "yam/spotify");
    }

    #[test]
    fn short_name_already_satisfied_by_an_installed_provider_resolves_to_nothing() {
        let mut source = FakeSource {
            catalogue: Catalogue {
                format: 1,
                packages: vec![entry("yam/soloist", &["spotify-connect"])],
            },
            indices: BTreeMap::from([(
                "yam/soloist".to_string(),
                index("yam/soloist", &[("1.0.0", vec![], vec!["spotify-connect"])]),
            )]),
        };
        let mut installed = InstalledSnapshot::empty();
        installed.packages.insert(
            "vasya/librespot".into(),
            semver::Version::parse("1.0.0").unwrap(),
        );
        installed.provides.insert(
            "spotify-connect".into(),
            BTreeSet::from(["vasya/librespot".to_string()]),
        );
        let result = resolve_with(&mut source, "spotify-connect", None, &profile(), &installed);
        assert!(
            result.is_ok(),
            "a short name already satisfied by something installed must resolve to nothing, got {result:?}"
        );
        assert!(result.unwrap().is_empty());
    }

    #[test]
    fn short_name_with_two_yam_providers_is_an_error() {
        let mut source = FakeSource {
            catalogue: Catalogue {
                format: 1,
                packages: vec![
                    entry("yam/soloist", &["spotify-connect"]),
                    entry("yam/alt-player", &["spotify-connect"]),
                ],
            },
            indices: BTreeMap::new(),
        };
        let result = resolve_with(
            &mut source,
            "spotify-connect",
            None,
            &profile(),
            &InstalledSnapshot::empty(),
        );
        assert!(
            result.is_err(),
            "two yam/ providers of the same virtual name must be an error, got {result:?}"
        );
        let message = result.unwrap_err().to_string();
        assert!(
            message.contains("yam/soloist") && message.contains("yam/alt-player"),
            "error should list both providers, got {message:?}"
        );
    }

    #[test]
    fn short_name_provider_outside_yam_is_rejected_with_a_hint() {
        let mut source = FakeSource {
            catalogue: Catalogue {
                format: 1,
                packages: vec![entry("vasya/librespot", &["spotify-connect"])],
            },
            indices: BTreeMap::new(),
        };
        let result = resolve_with(
            &mut source,
            "spotify-connect",
            None,
            &profile(),
            &InstalledSnapshot::empty(),
        );
        assert!(result.is_err());
        let message = result.unwrap_err().to_string();
        assert!(
            message.contains("vasya/librespot"),
            "error should name the non-yam/ provider as a hint to install it explicitly, got {message:?}"
        );
    }

    #[test]
    fn full_name_dependency_cycle_is_still_detected() {
        let mut source = FakeSource {
            catalogue: Catalogue {
                format: 1,
                packages: vec![],
            },
            indices: BTreeMap::from([
                (
                    "yam/a".to_string(),
                    index("yam/a", &[("1.0.0", vec!["yam/b"], vec![])]),
                ),
                (
                    "yam/b".to_string(),
                    index("yam/b", &[("1.0.0", vec!["yam/a"], vec![])]),
                ),
            ]),
        };
        let result = resolve_with(
            &mut source,
            "yam/a",
            None,
            &profile(),
            &InstalledSnapshot::empty(),
        );
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("cycle"));
    }
}
