use anyhow::{bail, Context, Result};
use base64::{engine::general_purpose::STANDARD, Engine};
use clap::{Args, Parser, Subcommand};
use ed25519_dalek::{pkcs8::DecodePrivateKey, Signer, SigningKey};
use reqwest::{blocking::Client, header::HeaderValue};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::{fs, path::PathBuf, time::Duration};
use yam::v2::{Manager, PlatformProfile};

const DEFAULT_UPLOAD_URL: &str = "https://repo.yse.wtf/api/v2/upload";

#[derive(Parser)]
#[command(
    name = "yam-publish",
    about = "Build and publish a YAM package from a PC"
)]
struct Cli {
    #[command(subcommand)]
    command: CommandKind,
}

#[derive(Subcommand)]
enum CommandKind {
    /// Build a .yampkg with a host YAM CLI.
    Pack(PackArgs),
    /// Sign and upload an already-built .yampkg.
    Publish(PublishArgs),
    /// Build, then sign and upload one target artifact.
    Release(ReleaseArgs),
}

#[derive(Args, Clone)]
struct PackArgs {
    /// Directory containing yam.yaml.
    #[arg(long, default_value = ".")]
    source: PathBuf,
    /// Platform ID from the profiles directory.
    #[arg(long, env = "YAM_PLATFORM")]
    platform: String,
    /// Directory containing platform YAML profiles.
    #[arg(long, env = "YAM_PROFILES", default_value = "platforms")]
    profiles: PathBuf,
    /// Destination .yampkg path.
    #[arg(short, long)]
    output: PathBuf,
}

#[derive(Args, Clone)]
struct PublishArgs {
    /// Built .yampkg archive.
    #[arg(long)]
    artifact: PathBuf,
    /// Directory containing the source yam.yaml used to create the artifact.
    #[arg(long, default_value = ".")]
    source: PathBuf,
    /// Target used for this artifact.
    #[arg(long)]
    target: String,
    /// Registry upload endpoint, not the client /v2 index URL.
    #[arg(long, env = "YAM_UPLOAD_URL", default_value = DEFAULT_UPLOAD_URL)]
    upload_url: String,
    /// Publish token created in the registry profile.
    #[arg(long, env = "YAM_PUBLISH_TOKEN")]
    token: String,
    /// PEM PKCS#8 Ed25519 private key downloaded when claiming the namespace.
    #[arg(long, env = "YAM_AUTHOR_KEY_FILE")]
    key: PathBuf,
}

#[derive(Args, Clone)]
struct ReleaseArgs {
    #[command(flatten)]
    pack: PackArgs,
    /// Registry upload endpoint, not the client /v2 index URL.
    #[arg(long, env = "YAM_UPLOAD_URL", default_value = DEFAULT_UPLOAD_URL)]
    upload_url: String,
    /// Publish token created in the registry profile.
    #[arg(long, env = "YAM_PUBLISH_TOKEN")]
    token: String,
    /// PEM PKCS#8 Ed25519 private key downloaded when claiming the namespace.
    #[arg(long, env = "YAM_AUTHOR_KEY_FILE")]
    key: PathBuf,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SourceManifest {
    name: String,
    version: String,
}

fn main() -> Result<()> {
    match Cli::parse().command {
        CommandKind::Pack(args) => pack(&args),
        CommandKind::Publish(args) => publish(&args),
        CommandKind::Release(args) => {
            pack(&args.pack)?;
            publish(&PublishArgs {
                artifact: args.pack.output.clone(),
                source: args.pack.source,
                target: args.pack.platform,
                upload_url: args.upload_url,
                token: args.token,
                key: args.key,
            })
        }
    }
}

fn pack(args: &PackArgs) -> Result<()> {
    let profile = PlatformProfile::load(&args.profiles, &args.platform)?;
    Manager::pack(&args.source, &profile, &args.output)
}

fn publish(args: &PublishArgs) -> Result<()> {
    let manifest_path = args.source.join("yam.yaml");
    let manifest: SourceManifest = serde_yaml::from_slice(
        &fs::read(&manifest_path).with_context(|| format!("read {}", manifest_path.display()))?,
    )?;
    let artifact =
        fs::read(&args.artifact).with_context(|| format!("read {}", args.artifact.display()))?;
    if artifact.is_empty() {
        bail!("artifact is empty")
    }
    let sha256 = format!("{:x}", Sha256::digest(&artifact));
    let key = SigningKey::from_pkcs8_pem(
        &fs::read_to_string(&args.key).with_context(|| format!("read {}", args.key.display()))?,
    )
    .context("author key must be an Ed25519 PKCS#8 PEM private key")?;
    let public_key = format!(
        "ed25519:{}",
        STANDARD.encode(key.verifying_key().to_bytes())
    );
    let signature = STANDARD.encode(
        key.sign(
            upload_message(&manifest.name, &manifest.version, &args.target, &sha256).as_bytes(),
        )
        .to_bytes(),
    );
    let response = Client::builder()
        .timeout(Duration::from_secs(120))
        .user_agent("yam-publish")
        .build()?
        .post(&args.upload_url)
        .header("authorization", format!("Bearer {}", args.token))
        .header("content-type", "application/octet-stream")
        .header("x-yam-name", header(&manifest.name)?)
        .header("x-yam-version", header(&manifest.version)?)
        .header("x-yam-target", header(&args.target)?)
        .header("x-yam-sha256", &sha256)
        .header("x-yam-author-key", public_key)
        .header("x-yam-author-signature", signature)
        .body(artifact)
        .send()?;
    let status = response.status();
    let body = response.text().unwrap_or_default();
    if !status.is_success() {
        bail!("registry rejected publication ({status}): {}", body.trim())
    }
    println!(
        "Published {} {} for {}.",
        manifest.name, manifest.version, args.target
    );
    Ok(())
}

fn header(value: &str) -> Result<HeaderValue> {
    HeaderValue::from_str(value).context("metadata contains an invalid HTTP header value")
}

fn upload_message(name: &str, version: &str, target: &str, sha256: &str) -> String {
    format!("yamreg-upload-v1\n{name}\n{version}\n{target}\n{sha256}\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn upload_signature_binds_all_artifact_identity_fields() {
        assert_eq!(
            upload_message("author/package", "1.2.3", "yandexmini_2", "abc"),
            "yamreg-upload-v1\nauthor/package\n1.2.3\nyandexmini_2\nabc\n"
        );
    }
}
