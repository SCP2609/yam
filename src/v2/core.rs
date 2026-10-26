use anyhow::{bail, Context, Result};
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    fs,
    io::{Cursor, Read, Write},
    path::{Component, Path, PathBuf},
    process::Command,
};
use tar::{Archive, Builder, Header};
use uuid::Uuid;

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PlatformProfile {
    pub id: String,
    pub arch: String,
    pub triple: String,
    pub paths: PlatformPaths,
    #[serde(default)]
    pub entware: Option<EntwareProfile>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PlatformPaths {
    pub root: PathBuf,
    pub etc: PathBuf,
    pub var: PathBuf,
    pub run: PathBuf,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct EntwareProfile {
    pub opkg: PathBuf,
}

impl PlatformProfile {
    pub fn load(directory: &Path, id: &str) -> Result<Self> {
        validate_platform_id(id)?;
        let path = directory.join(format!("{id}.yaml"));
        let profile: Self = serde_yaml::from_slice(
            &fs::read(&path).with_context(|| format!("read {}", path.display()))?,
        )?;
        if profile.id != id {
            bail!(
                "profile {} has mismatched id {}",
                path.display(),
                profile.id
            )
        }
        validate_platform_id(&profile.id)?;
        Ok(profile)
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PackageSource {
    pub name: String,
    pub version: String,
    pub license: String,
    pub description: String,
    #[serde(default)]
    pub homepage: Option<String>,
    pub targets: Vec<String>,
    pub files: BTreeMap<String, String>,
    #[serde(default)]
    pub depends: Vec<String>,
    #[serde(default)]
    pub recommends: Vec<String>,
    #[serde(default)]
    pub enhances: Vec<String>,
    #[serde(default)]
    pub provides: Vec<String>,
    #[serde(default)]
    pub conflicts: Vec<String>,
    #[serde(default)]
    pub replaces: Vec<String>,
    #[serde(default)]
    pub permissions: Vec<String>,
    #[serde(default)]
    pub entware: Option<EntwareRequest>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct EntwareRequest {
    pub packages: Vec<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PackageManifest {
    pub name: String,
    pub version: String,
    pub license: String,
    pub description: String,
    #[serde(default)]
    pub homepage: Option<String>,
    pub target: String,
    pub arch: String,
    #[serde(default)]
    pub depends: Vec<String>,
    #[serde(default)]
    pub recommends: Vec<String>,
    #[serde(default)]
    pub enhances: Vec<String>,
    #[serde(default)]
    pub provides: Vec<String>,
    #[serde(default)]
    pub conflicts: Vec<String>,
    #[serde(default)]
    pub replaces: Vec<String>,
    #[serde(default)]
    pub permissions: Vec<String>,
    #[serde(default)]
    pub entware: Option<EntwareRequest>,
    pub files_sha256: BTreeMap<String, String>,
}

#[derive(Debug, Clone)]
pub struct InstalledPackage {
    pub name: String,
    pub version: String,
    pub target: String,
}

pub struct Manager {
    root: PathBuf,
    profile: PlatformProfile,
    db: Connection,
}

impl Manager {
    pub fn open(root: PathBuf, profile: PlatformProfile) -> Result<Self> {
        for directory in [
            root.join("pkgs"),
            root.join("bin"),
            root.join("share"),
            root.join("db"),
            root.join(".staging"),
        ] {
            fs::create_dir_all(directory)?;
        }
        let db = Connection::open(root.join("db/state.sqlite3"))?;
        db.execute_batch(
            "PRAGMA foreign_keys = ON;
             CREATE TABLE IF NOT EXISTS packages (
               name TEXT PRIMARY KEY, version TEXT NOT NULL, target TEXT NOT NULL,
               payload_path TEXT NOT NULL
             );
             CREATE TABLE IF NOT EXISTS package_files (
               package_name TEXT NOT NULL REFERENCES packages(name) ON DELETE CASCADE,
               path TEXT NOT NULL, sha256 TEXT NOT NULL,
               PRIMARY KEY(package_name, path)
             );
             CREATE TABLE IF NOT EXISTS links (
               path TEXT PRIMARY KEY, package_name TEXT NOT NULL REFERENCES packages(name) ON DELETE CASCADE,
               target TEXT NOT NULL
             );",
        )?;
        Ok(Self { root, profile, db })
    }

    pub fn profile(&self) -> &PlatformProfile {
        &self.profile
    }

    pub fn pack(source_dir: &Path, profile: &PlatformProfile, output: &Path) -> Result<()> {
        let descriptor = source_dir.join("yam.yaml");
        let source: PackageSource = serde_yaml::from_slice(
            &fs::read(&descriptor).with_context(|| format!("read {}", descriptor.display()))?,
        )?;
        validate_name(&source.name)?;
        validate_version(&source.version)?;
        if source.description.trim().is_empty() || source.license.trim().is_empty() {
            bail!("name, license and non-empty description are required")
        }
        if !source
            .targets
            .iter()
            .any(|target| target == &profile.id || target == "any")
        {
            bail!("{} does not support target {}", source.name, profile.id)
        }
        let target = if source.targets.iter().any(|target| target == &profile.id) {
            profile.id.clone()
        } else {
            "any".into()
        };
        let mut payload = Vec::<(String, PathBuf)>::new();
        let mut hashes = BTreeMap::new();
        for (destination, template) in &source.files {
            validate_payload_path(destination)?;
            let source_path = source_dir.join(expand(template, profile));
            if source_path.is_dir() {
                if !destination.ends_with('/') {
                    bail!("directory source {template:?} requires a destination ending in /")
                }
                for entry in walkdir::WalkDir::new(&source_path).sort_by_file_name() {
                    let entry = entry?;
                    if entry.file_type().is_symlink() {
                        bail!("symlink in source payload: {}", entry.path().display())
                    }
                    if !entry.file_type().is_file() {
                        continue;
                    }
                    let relative = entry.path().strip_prefix(&source_path)?;
                    let item = format!(
                        "{}{}",
                        destination.trim_end_matches('/'),
                        if destination.ends_with('/') {
                            format!("/{}", relative.display())
                        } else {
                            String::new()
                        }
                    );
                    validate_payload_path(&item)?;
                    insert_payload(&mut payload, &mut hashes, item, entry.path().to_path_buf())?;
                }
            } else {
                if !source_path.is_file() {
                    bail!("payload source is not a file: {}", source_path.display())
                }
                insert_payload(&mut payload, &mut hashes, destination.clone(), source_path)?;
            }
        }
        if payload.is_empty() {
            bail!("package has no payload files")
        }
        let manifest = PackageManifest {
            name: source.name,
            version: source.version,
            license: source.license,
            description: source.description,
            homepage: source.homepage,
            target,
            arch: profile.arch.clone(),
            depends: source.depends,
            recommends: source.recommends,
            enhances: source.enhances,
            provides: source.provides,
            conflicts: source.conflicts,
            replaces: source.replaces,
            permissions: source.permissions,
            entware: source.entware,
            files_sha256: hashes,
        };
        validate_manifest(&manifest)?;
        let file =
            fs::File::create(output).with_context(|| format!("create {}", output.display()))?;
        let encoder = zstd::Encoder::new(file, 19)?;
        let mut archive = Builder::new(encoder.auto_finish());
        append_bytes(
            &mut archive,
            "MANIFEST.yaml",
            &serde_yaml::to_string(&manifest)?.into_bytes(),
        )?;
        for (destination, source) in payload {
            archive.append_path_with_name(source, format!("files/{destination}"))?;
        }
        archive.finish()?;
        Ok(())
    }

    pub fn install(&mut self, artifact: &Path, yes: bool) -> Result<PackageManifest> {
        let (manifest, payload) = read_artifact(artifact)?;
        validate_manifest(&manifest)?;
        if manifest.target != "any" && manifest.target != self.profile.id {
            bail!(
                "package target {} does not match platform {}",
                manifest.target,
                self.profile.id
            )
        }
        if manifest.arch != self.profile.arch {
            bail!(
                "package architecture {} does not match {}",
                manifest.arch,
                self.profile.arch
            )
        }
        if !manifest.depends.is_empty()
            || !manifest.recommends.is_empty()
            || !manifest.enhances.is_empty()
            || !manifest.provides.is_empty()
            || !manifest.conflicts.is_empty()
            || !manifest.replaces.is_empty()
        {
            bail!("dependency resolution is not implemented yet; refusing a partial install")
        }
        if manifest
            .entware
            .as_ref()
            .is_some_and(|request| !request.packages.is_empty())
            && !yes
        {
            bail!(
                "{} requests Entware packages; rerun with --yes after reviewing them",
                manifest.name
            )
        }
        self.ensure_entware(&manifest, yes)?;
        let staging = self.root.join(".staging").join(Uuid::new_v4().to_string());
        fs::create_dir_all(&staging)?;
        let result = self.install_payload(&manifest, &payload, &staging);
        if result.is_err() {
            let _ = fs::remove_dir_all(&staging);
        }
        result?;
        Ok(manifest)
    }

    fn install_payload(
        &mut self,
        manifest: &PackageManifest,
        payload: &[(String, Vec<u8>)],
        staging: &Path,
    ) -> Result<()> {
        let (namespace, package) = split_name(&manifest.name)?;
        let destination = self
            .root
            .join("pkgs")
            .join(namespace)
            .join(package)
            .join(&manifest.version);
        let files_dir = staging.join("files");
        for (path, bytes) in payload {
            let out = files_dir.join(path);
            if let Some(parent) = out.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::write(&out, bytes)?;
            set_payload_mode(&out, path)?;
        }
        let transaction = self.db.transaction()?;
        let existing: Option<(String, String)> = transaction
            .query_row(
                "SELECT version, payload_path FROM packages WHERE name=?1",
                [&manifest.name],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?;
        if let Some((old_version, old_payload)) = existing {
            if old_version == manifest.version {
                bail!(
                    "{} {} is already installed",
                    manifest.name,
                    manifest.version
                )
            }
            let _ = old_payload;
        }
        for path in manifest.files_sha256.keys().filter(|path| linkable(path)) {
            let owner: Option<String> = transaction
                .query_row(
                    "SELECT package_name FROM links WHERE path=?1",
                    [path],
                    |row| row.get(0),
                )
                .optional()?;
            if let Some(owner) = owner {
                if owner != manifest.name && !manifest.replaces.iter().any(|name| name == &owner) {
                    bail!("{} is already owned by {owner}", path)
                }
            }
        }
        if destination.exists() {
            bail!(
                "payload destination already exists: {}",
                destination.display()
            )
        }
        if let Some(parent) = destination.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::rename(files_dir, &destination)?;
        let package_root = destination.parent().context("package root")?;
        replace_link(&package_root.join("current"), Path::new(&manifest.version))?;
        transaction.execute(
            "DELETE FROM package_files WHERE package_name=?1",
            [&manifest.name],
        )?;
        transaction.execute("DELETE FROM links WHERE package_name=?1", [&manifest.name])?;
        transaction.execute(
            "INSERT INTO packages(name,version,target,payload_path) VALUES(?1,?2,?3,?4)
             ON CONFLICT(name) DO UPDATE SET version=excluded.version,target=excluded.target,payload_path=excluded.payload_path",
            params![manifest.name, manifest.version, manifest.target, destination.to_string_lossy()],
        )?;
        for (path, hash) in &manifest.files_sha256 {
            transaction.execute(
                "INSERT INTO package_files(package_name,path,sha256) VALUES(?1,?2,?3)",
                params![manifest.name, path, hash],
            )?;
            if linkable(path) {
                let link = self.root.join(path);
                if let Some(parent) = link.parent() {
                    fs::create_dir_all(parent)?;
                }
                let target = destination.join(path);
                if link.exists() || link.is_symlink() {
                    fs::remove_file(&link)?;
                }
                make_link(&link, &target)?;
                transaction.execute(
                    "INSERT OR REPLACE INTO links(path,package_name,target) VALUES(?1,?2,?3)",
                    params![path, manifest.name, target.to_string_lossy()],
                )?;
            }
        }
        transaction.commit()?;
        let _ = fs::remove_dir_all(staging);
        Ok(())
    }

    pub fn list(&self) -> Result<Vec<InstalledPackage>> {
        let mut statement = self
            .db
            .prepare("SELECT name,version,target FROM packages ORDER BY name")?;
        let packages = statement
            .query_map([], |row| {
                Ok(InstalledPackage {
                    name: row.get(0)?,
                    version: row.get(1)?,
                    target: row.get(2)?,
                })
            })?
            .collect::<rusqlite::Result<_>>()?;
        Ok(packages)
    }

    pub fn files(&self, name: &str) -> Result<Vec<String>> {
        validate_name(name)?;
        let mut statement = self
            .db
            .prepare("SELECT path FROM package_files WHERE package_name=?1 ORDER BY path")?;
        let files = statement
            .query_map([name], |row| row.get(0))?
            .collect::<rusqlite::Result<_>>()?;
        Ok(files)
    }

    pub fn which(&self, path: &str) -> Result<Option<String>> {
        validate_payload_path(path)?;
        Ok(self
            .db
            .query_row(
                "SELECT package_name FROM links WHERE path=?1",
                [path],
                |row| row.get(0),
            )
            .optional()?)
    }

    pub fn remove(&mut self, name: &str) -> Result<()> {
        validate_name(name)?;
        let payload: String = self
            .db
            .query_row(
                "SELECT payload_path FROM packages WHERE name=?1",
                [name],
                |row| row.get(0),
            )
            .optional()?
            .context("package is not installed")?;
        let links = {
            let mut statement = self
                .db
                .prepare("SELECT path,target FROM links WHERE package_name=?1")?;
            let links = statement
                .query_map([name], |row| {
                    Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
                })?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            links
        };
        let transaction = self.db.transaction()?;
        for (path, target) in links {
            let link = self.root.join(path);
            if link.is_symlink()
                && fs::read_link(&link)
                    .ok()
                    .is_some_and(|value| value == PathBuf::from(&target))
            {
                fs::remove_file(link)?;
            }
        }
        transaction.execute("DELETE FROM packages WHERE name=?1", [name])?;
        transaction.commit()?;
        fs::remove_dir_all(payload)?;
        Ok(())
    }

    pub fn doctor(&self) -> Result<Vec<String>> {
        let mut problems = Vec::new();
        let mut statement = self
            .db
            .prepare("SELECT path,package_name,target FROM links ORDER BY path")?;
        for row in statement.query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
            ))
        })? {
            let (path, package, target) = row?;
            let actual = fs::read_link(self.root.join(&path));
            if actual.as_ref().ok() != Some(&PathBuf::from(&target)) {
                problems.push(format!(
                    "{path}: link owned by {package} is missing or points elsewhere"
                ));
            }
        }
        Ok(problems)
    }

    pub fn entware_available(&self) -> bool {
        self.profile
            .entware
            .as_ref()
            .is_some_and(|entry| entry.opkg.is_file())
    }

    pub fn entware_list(&self) -> Result<String> {
        let opkg = self
            .profile
            .entware
            .as_ref()
            .context("Entware is not configured for this platform")?;
        let output = Command::new(&opkg.opkg)
            .arg("list-installed")
            .output()
            .with_context(|| format!("run {}", opkg.opkg.display()))?;
        if !output.status.success() {
            bail!(
                "opkg list-installed failed: {}",
                String::from_utf8_lossy(&output.stderr)
            )
        }
        Ok(String::from_utf8_lossy(&output.stdout).into())
    }

    fn ensure_entware(&self, manifest: &PackageManifest, yes: bool) -> Result<()> {
        let Some(request) = &manifest.entware else {
            return Ok(());
        };
        if request.packages.is_empty() {
            return Ok(());
        }
        let adapter = self
            .profile
            .entware
            .as_ref()
            .context("platform has no Entware adapter")?;
        if !adapter.opkg.is_file() {
            bail!(
                "Entware is not installed: {} is missing",
                adapter.opkg.display()
            )
        }
        if !yes {
            bail!("Entware package installation requires --yes")
        }
        let status = Command::new(&adapter.opkg)
            .arg("install")
            .args(&request.packages)
            .status()
            .with_context(|| format!("run {}", adapter.opkg.display()))?;
        if !status.success() {
            bail!("Entware package installation failed")
        }
        Ok(())
    }
}

fn insert_payload(
    payload: &mut Vec<(String, PathBuf)>,
    hashes: &mut BTreeMap<String, String>,
    destination: String,
    source: PathBuf,
) -> Result<()> {
    if hashes.contains_key(&destination) {
        bail!("duplicate payload path {destination}")
    }
    hashes.insert(destination.clone(), digest_file(&source)?);
    payload.push((destination, source));
    Ok(())
}

fn read_artifact(path: &Path) -> Result<(PackageManifest, Vec<(String, Vec<u8>)>)> {
    let file = fs::File::open(path).with_context(|| format!("open {}", path.display()))?;
    let decoder =
        zstd::Decoder::new(file).context(".yampkg must be a zstd-compressed tar archive")?;
    let mut archive = Archive::new(decoder);
    let mut manifest = None;
    let mut payload = BTreeMap::new();
    for entry in archive.entries()? {
        let mut entry = entry?;
        let path = entry.path()?.into_owned();
        safe_tar_path(&path)?;
        if entry.header().entry_type().is_dir() {
            continue;
        }
        if !entry.header().entry_type().is_file() {
            bail!("unsupported archive entry {}", path.display())
        }
        let name = path.to_string_lossy().to_string();
        let mut bytes = Vec::new();
        entry.read_to_end(&mut bytes)?;
        if name == "MANIFEST.yaml" {
            if manifest.is_some() {
                bail!("duplicate MANIFEST.yaml")
            }
            manifest = Some(serde_yaml::from_slice(&bytes)?);
        } else if let Some(relative) = name.strip_prefix("files/") {
            validate_payload_path(relative)?;
            if payload.insert(relative.to_string(), bytes).is_some() {
                bail!("duplicate payload path {relative}")
            }
        } else if name != "SOURCE" {
            bail!("unexpected archive entry {name}")
        }
    }
    let manifest: PackageManifest = manifest.context("missing MANIFEST.yaml")?;
    if payload.len() != manifest.files_sha256.len() {
        bail!("manifest file list differs from payload")
    }
    for (path, expected) in &manifest.files_sha256 {
        let bytes = payload
            .get(path)
            .with_context(|| format!("manifest file missing from payload: {path}"))?;
        if digest_bytes(bytes) != *expected {
            bail!("checksum mismatch for {path}")
        }
    }
    Ok((manifest, payload.into_iter().collect()))
}

fn validate_manifest(manifest: &PackageManifest) -> Result<()> {
    validate_name(&manifest.name)?;
    validate_version(&manifest.version)?;
    validate_platform_id(&manifest.target)?;
    if manifest.arch.is_empty()
        || manifest.license.is_empty()
        || manifest.description.trim().is_empty()
    {
        bail!("manifest identity fields are incomplete")
    }
    for (path, hash) in &manifest.files_sha256 {
        validate_payload_path(path)?;
        if hash.len() != 64 || !hash.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            bail!("invalid SHA-256 for {path}")
        }
    }
    Ok(())
}

fn validate_name(name: &str) -> Result<()> {
    let (namespace, package) = split_name(name)?;
    for part in [namespace, package] {
        if part.is_empty()
            || part.len() > 64
            || !part
                .bytes()
                .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
        {
            bail!("invalid package name {name:?}")
        }
    }
    Ok(())
}
fn split_name(name: &str) -> Result<(&str, &str)> {
    let mut parts = name.split('/');
    let namespace = parts
        .next()
        .context("package name must be namespace/name")?;
    let package = parts
        .next()
        .context("package name must be namespace/name")?;
    if parts.next().is_some() {
        bail!("package name must contain exactly one slash")
    }
    Ok((namespace, package))
}
fn validate_platform_id(value: &str) -> Result<()> {
    if value == "any" {
        return Ok(());
    }
    if value.is_empty()
        || !value.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_' || byte == b'-'
        })
    {
        bail!("invalid platform id {value:?}")
    }
    Ok(())
}
fn validate_version(version: &str) -> Result<()> {
    let (main, build) = version
        .split_once('+')
        .map_or((version, None), |(main, build)| (main, Some(build)));
    let (main, pre) = main
        .split_once('-')
        .map_or((main, None), |(main, pre)| (main, Some(pre)));
    let numeric = |part: &str| {
        !part.is_empty()
            && part.bytes().all(|byte| byte.is_ascii_digit())
            && (part == "0" || !part.starts_with('0'))
    };
    let main_parts = main.split('.').collect::<Vec<_>>();
    let identifier = |part: &str| {
        !part.is_empty()
            && part
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
    };
    if main_parts.len() != 3
        || !main_parts.iter().all(|part| numeric(part))
        || pre.is_some_and(|value| !value.split('.').all(identifier))
        || build.is_some_and(|value| !value.split('.').all(identifier))
    {
        bail!("version must be SemVer: {version:?}")
    }
    Ok(())
}
fn validate_payload_path(value: &str) -> Result<()> {
    let path = Path::new(value);
    if value.is_empty()
        || path.is_absolute()
        || path.components().any(|part| {
            matches!(
                part,
                Component::ParentDir | Component::RootDir | Component::Prefix(_)
            )
        })
    {
        bail!("unsafe payload path {value:?}")
    }
    Ok(())
}
fn safe_tar_path(path: &Path) -> Result<()> {
    validate_payload_path(path.to_str().context("non-UTF-8 archive path")?)
}
fn expand(value: &str, profile: &PlatformProfile) -> String {
    value
        .replace("$YAM_TARGET", &profile.id)
        .replace("$YAM_TRIPLE", &profile.triple)
}
fn digest_file(path: &Path) -> Result<String> {
    Ok(digest_bytes(&fs::read(path)?))
}
fn digest_bytes(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
fn linkable(path: &str) -> bool {
    path.starts_with("bin/") || path.starts_with("share/")
}
fn append_bytes<W: Write>(archive: &mut Builder<W>, path: &str, bytes: &[u8]) -> Result<()> {
    let mut header = Header::new_gnu();
    header.set_size(bytes.len() as u64);
    header.set_mode(0o644);
    header.set_cksum();
    archive.append_data(&mut header, path, Cursor::new(bytes))?;
    Ok(())
}
#[cfg(unix)]
fn make_link(link: &Path, target: &Path) -> Result<()> {
    std::os::unix::fs::symlink(target, link)?;
    Ok(())
}
#[cfg(not(unix))]
fn make_link(_: &Path, _: &Path) -> Result<()> {
    bail!("v2 package links require a Unix host")
}
fn replace_link(path: &Path, target: &Path) -> Result<()> {
    let temporary = path.with_extension(format!("new-{}", Uuid::new_v4()));
    make_link(&temporary, target)?;
    fs::rename(temporary, path)?;
    Ok(())
}
fn set_payload_mode(path: &Path, payload_path: &str) -> Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = if payload_path.starts_with("bin/") || payload_path.starts_with("hooks/") {
            0o755
        } else {
            0o644
        };
        fs::set_permissions(path, fs::Permissions::from_mode(mode))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn package_names_and_payload_paths_are_strict() {
        assert!(validate_name("nikita/soloist").is_ok());
        assert!(validate_name("soloist").is_err());
        assert!(validate_payload_path("share/host/a.lua").is_ok());
        assert!(validate_payload_path("../etc/passwd").is_err());
    }
}
