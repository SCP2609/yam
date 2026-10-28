use anyhow::{bail, Context, Result};
use base64::{engine::general_purpose::STANDARD, Engine};
use ed25519_dalek::{Signature, Verifier, VerifyingKey};
use reqwest::blocking::Client;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::{fs, time::Duration};

const RELEASES_API: &str = "https://api.github.com/repos/nyakokitsu/yam/releases?per_page=100";

#[derive(Debug, Deserialize)]
struct Release {
    tag_name: String,
    draft: bool,
    prerelease: bool,
    assets: Vec<ReleaseAsset>,
}
#[derive(Debug, Deserialize)]
struct ReleaseAsset {
    name: String,
    browser_download_url: String,
}
#[derive(Debug, Deserialize)]
struct Manifest {
    format: u32,
    version: String,
    target: String,
    artifact: String,
    sha256: String,
    size: u64,
}

pub enum UpdateResult {
    Current(String),
    Updated(String),
}

pub fn apply() -> Result<UpdateResult> {
    let target = update_target()?;
    let key = update_key()?;
    let client = Client::builder()
        .timeout(Duration::from_secs(60))
        .user_agent("yam-updater")
        .build()?;
    let release = client
        .get(RELEASES_API)
        .send()?
        .error_for_status()?
        .json::<Vec<Release>>()?
        .into_iter()
        .find(|release| {
            !release.draft && !release.prerelease && release.tag_name.starts_with("yam-v")
        })
        .context("нет опубликованного релиза YAM для колонки")?;
    let manifest_name = format!("yam-update-{target}.json");
    let signature_name = format!("{manifest_name}.sig");
    let manifest_url = asset_url(&release, &manifest_name)?;
    let signature_url = asset_url(&release, &signature_name)?;
    let manifest_bytes = client
        .get(manifest_url)
        .send()?
        .error_for_status()?
        .bytes()?
        .to_vec();
    let signature = client
        .get(signature_url)
        .send()?
        .error_for_status()?
        .text()?;
    verify(&key, &manifest_bytes, signature.trim())?;
    let manifest: Manifest = serde_json::from_slice(&manifest_bytes)?;
    if manifest.format != 1 || manifest.target != target {
        bail!("некорректный манифест обновления")
    }
    if manifest.version == env!("CARGO_PKG_VERSION") {
        return Ok(UpdateResult::Current(manifest.version));
    }
    if manifest.sha256.len() != 64 || !manifest.sha256.bytes().all(|byte| byte.is_ascii_hexdigit())
    {
        bail!("некорректный SHA-256 в манифесте обновления")
    }
    let artifact = client
        .get(&manifest.artifact)
        .send()?
        .error_for_status()?
        .bytes()?
        .to_vec();
    if artifact.len() as u64 != manifest.size
        || format!("{:x}", Sha256::digest(&artifact)) != manifest.sha256
    {
        bail!("проверка загруженного обновления не пройдена")
    }
    let executable = std::env::current_exe()?;
    if executable.file_name().and_then(|value| value.to_str()) != Some("yam") {
        bail!("самообновление требует запуска установленного файла yam")
    }
    let staged = executable.with_extension("update");
    fs::write(&staged, artifact)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&staged, fs::Permissions::from_mode(0o755))?;
    }
    fs::rename(staged, &executable)?;
    Ok(UpdateResult::Updated(manifest.version))
}

fn update_target() -> Result<&'static str> {
    match (std::env::consts::OS, std::env::consts::ARCH) {
        ("linux", "aarch64") => Ok("aarch64-unknown-linux-musl"),
        ("linux", "arm") => Ok("armv7-unknown-linux-musleabihf"),
        _ => bail!("самообновление доступно только для сборок YAM под Linux ARM"),
    }
}

fn asset_url(release: &Release, name: &str) -> Result<String> {
    release
        .assets
        .iter()
        .find(|asset| asset.name == name)
        .map(|asset| asset.browser_download_url.clone())
        .with_context(|| format!("в релизе {} нет {name}", release.tag_name))
}
fn update_key() -> Result<VerifyingKey> {
    let value = option_env!("YAM_UPDATE_PUBLIC_KEY")
        .filter(|value| !value.is_empty())
        .context("в этой сборке не задан ключ обновлений")?;
    let encoded = value
        .strip_prefix("ed25519:")
        .context("ключ обновлений должен иметь формат ed25519:<base64>")?;
    let bytes = STANDARD.decode(encoded)?;
    Ok(VerifyingKey::from_bytes(
        bytes
            .as_slice()
            .try_into()
            .context("ключ обновлений должен содержать 32 байта")?,
    )?)
}
fn verify(key: &VerifyingKey, body: &[u8], signature: &str) -> Result<()> {
    let signature = Signature::from_slice(&STANDARD.decode(signature)?)?;
    key.verify(body, &signature)
        .context("подпись манифеста обновления недействительна")
}
