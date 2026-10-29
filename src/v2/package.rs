use anyhow::{bail, Context, Result};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    fs,
    io::{Read, Write},
    path::{Component, Path, PathBuf},
};
use tar::Archive;

use super::{
    core::{set_payload_mode, ArchiveLimits, PackageManifest, PlatformProfile, ServiceSpec},
    version,
};

/// Split a dependency/conflict/replaces entry into its package or virtual
/// name and its version constraints. Shared with `registry.rs` so the two
/// resolvers parse requirements identically (previously each had its own
/// string-based version comparison that disagreed with the other).
pub(crate) fn parse_requirement(value: &str) -> Result<(&str, version::Constraints)> {
    let mut words = value.split_whitespace();
    let name = words.next().context("empty package requirement")?;
    validate_capability(name)?;
    let rest = value[name.len()..].trim_start();
    let constraints = version::Constraints::parse(rest)?;
    Ok((name, constraints))
}
pub(crate) fn requirement_name(value: &str) -> Result<&str> {
    Ok(parse_requirement(value)?.0)
}

pub(crate) fn insert_payload(
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

/// Stream a `.yampkg`'s tar.zst straight into `staging/files/`, writing
/// and hashing each payload file as its bytes are read rather than
/// buffering the archive or any entry in memory (B4). Every limit is
/// checked against bytes actually read, never a tar header's claimed
/// size, since a small compressed artifact can still decompress to
/// something unbounded. `MANIFEST.yaml` must be the first entry, as
/// `Manager::pack` already writes it.
pub(crate) fn extract_artifact(
    path: &Path,
    staging: &Path,
    limits: &ArchiveLimits,
) -> Result<PackageManifest> {
    let compressed_size = fs::metadata(path)
        .with_context(|| format!("stat {}", path.display()))?
        .len();
    if compressed_size > limits.max_compressed_bytes {
        bail!(
            "{} is {} bytes, over the {}-byte compressed artifact limit",
            path.display(),
            compressed_size,
            limits.max_compressed_bytes
        )
    }
    let file = fs::File::open(path).with_context(|| format!("open {}", path.display()))?;
    let decoder =
        zstd::Decoder::new(file).context(".yampkg must be a zstd-compressed tar archive")?;
    let mut archive = Archive::new(decoder);
    let files_dir = staging.join("files");
    let mut manifest: Option<PackageManifest> = None;
    let mut computed_hashes: BTreeMap<String, String> = BTreeMap::new();
    let mut total_unpacked: u64 = 0;
    let mut entry_count: u64 = 0;
    let mut first_entry = true;

    for entry in archive.entries()? {
        let mut entry = entry?;
        entry_count += 1;
        if entry_count > limits.max_entries {
            bail!(
                "archive has more than the {}-entry limit",
                limits.max_entries
            )
        }
        let entry_path = entry.path()?.into_owned();
        safe_tar_path(&entry_path)?;
        let name = entry_path.to_string_lossy().to_string();
        if first_entry {
            if name != "MANIFEST.yaml" {
                bail!("the first archive entry must be MANIFEST.yaml, found {name}")
            }
            first_entry = false;
        }
        if entry.header().entry_type().is_dir() {
            continue;
        }
        if !entry.header().entry_type().is_file() {
            bail!("unsupported archive entry {}", entry_path.display())
        }
        if name == "MANIFEST.yaml" {
            if manifest.is_some() {
                bail!("duplicate MANIFEST.yaml")
            }
            let mut bytes = Vec::new();
            copy_bounded(
                &mut entry,
                &mut bytes,
                limits.max_file_bytes,
                &mut total_unpacked,
                limits.max_total_unpacked_bytes,
            )?;
            manifest = Some(serde_yaml::from_slice(&bytes)?);
        } else if let Some(relative) = name.strip_prefix("files/") {
            validate_payload_path(relative)?;
            if computed_hashes.contains_key(relative) {
                bail!("duplicate payload path {relative}")
            }
            let out = files_dir.join(relative);
            if let Some(parent) = out.parent() {
                fs::create_dir_all(parent)?;
            }
            let mut hasher = Sha256::new();
            {
                let mut destination =
                    fs::File::create(&out).with_context(|| format!("create {}", out.display()))?;
                let mut writer = HashingWriter {
                    inner: &mut destination,
                    hasher: &mut hasher,
                };
                copy_bounded(
                    &mut entry,
                    &mut writer,
                    limits.max_file_bytes,
                    &mut total_unpacked,
                    limits.max_total_unpacked_bytes,
                )?;
            }
            set_payload_mode(&out, relative, entry.header().mode()?)?;
            computed_hashes.insert(relative.to_string(), format!("{:x}", hasher.finalize()));
        } else if name == "SOURCE" {
            copy_bounded(
                &mut entry,
                &mut std::io::sink(),
                limits.max_file_bytes,
                &mut total_unpacked,
                limits.max_total_unpacked_bytes,
            )?;
        } else {
            bail!("unexpected archive entry {name}")
        }
    }
    let manifest: PackageManifest = manifest.context("missing MANIFEST.yaml")?;
    if computed_hashes.len() != manifest.files_sha256.len() {
        bail!("manifest file list differs from payload")
    }
    for (path, expected) in &manifest.files_sha256 {
        let actual = computed_hashes
            .get(path)
            .with_context(|| format!("manifest file missing from payload: {path}"))?;
        if actual != expected {
            bail!("checksum mismatch for {path}")
        }
    }
    Ok(manifest)
}

/// Copy `reader` into `writer` in chunks, bailing as soon as either the
/// single entry or the running archive total exceeds its limit --
/// checked against bytes actually transferred, not a header's claim.
fn copy_bounded<R: Read, W: Write>(
    reader: &mut R,
    writer: &mut W,
    max_file_bytes: u64,
    total_unpacked: &mut u64,
    max_total_unpacked_bytes: u64,
) -> Result<()> {
    let mut buffer = [0u8; 64 * 1024];
    let mut file_bytes: u64 = 0;
    loop {
        let read = reader.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        file_bytes += read as u64;
        if file_bytes > max_file_bytes {
            bail!("an archive entry is over the {max_file_bytes}-byte single-file limit")
        }
        *total_unpacked += read as u64;
        if *total_unpacked > max_total_unpacked_bytes {
            bail!("archive unpacks to more than the {max_total_unpacked_bytes}-byte limit")
        }
        writer.write_all(&buffer[..read])?;
    }
    Ok(())
}

/// Forwards writes to `inner` while feeding the same bytes to `hasher`,
/// so a payload file is hashed in the same pass that writes it to disk.
struct HashingWriter<'a, W: Write> {
    inner: W,
    hasher: &'a mut Sha256,
}
impl<W: Write> Write for HashingWriter<'_, W> {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        let written = self.inner.write(buf)?;
        self.hasher.update(&buf[..written]);
        Ok(written)
    }
    fn flush(&mut self) -> std::io::Result<()> {
        self.inner.flush()
    }
}

pub(crate) fn validate_manifest(manifest: &PackageManifest) -> Result<()> {
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
    validate_share_namespace(manifest)?;
    for (name, service) in &manifest.services {
        validate_service(name, service, &manifest.files_sha256)?;
    }
    Ok(())
}
/// Every `share/` path for a package outside `yam/` must contain a path
/// component scoped to that package (`<namespace>.<name>` or starting
/// with `<namespace>.`), so one host's shared directory (such as
/// `share/host/plugins/`) can't be polluted by an unscoped file.
/// `yam/` packages are exempt; `bin/` is unaffected (A2 covers its
/// collisions).
fn validate_share_namespace(manifest: &PackageManifest) -> Result<()> {
    let (namespace, _) = split_name(&manifest.name)?;
    if namespace == "yam" {
        return Ok(());
    }
    let prefix = format!("{namespace}.");
    for path in manifest
        .files_sha256
        .keys()
        .filter(|path| path.starts_with("share/"))
    {
        let scoped = Path::new(path).components().any(|component| {
            component
                .as_os_str()
                .to_str()
                .is_some_and(|value| value.starts_with(&prefix))
        });
        if !scoped {
            bail!(
                "{path} is outside yam/ and must contain a path component starting with {prefix:?}"
            )
        }
    }
    Ok(())
}
fn validate_service(
    name: &str,
    service: &ServiceSpec,
    payload: &BTreeMap<String, String>,
) -> Result<()> {
    if name.is_empty()
        || !name
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
    {
        bail!("invalid service name {name:?}")
    }
    let executable = service
        .exec
        .first()
        .context("service exec must not be empty")?;
    validate_payload_path(executable)?;
    if !payload.contains_key(executable) {
        bail!("service {name:?} executable is not a package payload: {executable}")
    }
    if service.exec.iter().any(|argument| argument.contains('\0')) {
        bail!("service {name:?} contains a NUL argument")
    }
    for (key, value) in &service.env {
        if key.is_empty()
            || !key
                .bytes()
                .all(|byte| byte.is_ascii_uppercase() || byte.is_ascii_digit() || byte == b'_')
            || value.contains('\0')
        {
            bail!("invalid environment entry for service {name:?}")
        }
    }
    Ok(())
}

pub(crate) fn validate_name(name: &str) -> Result<()> {
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
pub(crate) fn validate_capability(value: &str) -> Result<()> {
    if value.contains('/') {
        return validate_name(value);
    }
    if value.is_empty()
        || value.len() > 64
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
    {
        bail!("invalid package capability {value:?}")
    }
    Ok(())
}
pub(crate) fn split_name(name: &str) -> Result<(&str, &str)> {
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
pub(crate) fn validate_platform_id(value: &str) -> Result<()> {
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
pub(crate) fn validate_version(value: &str) -> Result<()> {
    // `semver::Version::parse` requires exactly three numeric components
    // (no "1.2" shorthand) and already rejects a leading zero in any
    // numeric identifier, matching the rules this function used to
    // implement by hand.
    version::validate_version(value)
}
pub(crate) fn validate_payload_path(value: &str) -> Result<()> {
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
pub(crate) fn expand(value: &str, profile: &PlatformProfile) -> String {
    value
        .replace("$YAM_TARGET", &profile.id)
        .replace("$YAM_TRIPLE", &profile.triple)
}
pub(crate) fn digest_file(path: &Path) -> Result<String> {
    Ok(digest_bytes(&fs::read(path)?))
}
pub(crate) fn digest_bytes(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
/// Describe whatever already exists at `path`, for an actionable
/// "refusing to overwrite" error (A2).
pub(crate) fn describe_obstruction(path: &Path) -> String {
    match fs::symlink_metadata(path) {
        Ok(meta) if meta.file_type().is_symlink() => format!(
            "a symlink to {}",
            fs::read_link(path)
                .map(|target| target.display().to_string())
                .unwrap_or_else(|_| "<unreadable>".into())
        ),
        Ok(meta) if meta.is_dir() => "a directory".into(),
        Ok(_) => "a regular file".into(),
        Err(_) => "an unknown filesystem entry".into(),
    }
}
pub(crate) fn linkable(path: &str) -> bool {
    path.starts_with("bin/") || path.starts_with("share/")
}
