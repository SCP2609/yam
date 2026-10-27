use anyhow::{Context, Result};
use base64::{engine::general_purpose::STANDARD, Engine};
use clap::Parser;
use ed25519_dalek::{Signer, SigningKey};
use std::{fs, path::PathBuf};

#[derive(Parser)]
struct Args {
    #[arg(long, env = "YAM_UPDATE_SIGNING_KEY")]
    key: String,
    input: PathBuf,
    output: PathBuf,
}
fn main() -> Result<()> {
    let args = Args::parse();
    let bytes = STANDARD.decode(args.key.strip_prefix("ed25519:").unwrap_or(&args.key))?;
    let key = SigningKey::from_bytes(
        bytes
            .as_slice()
            .try_into()
            .context("signing key must contain 32 bytes")?,
    );
    fs::write(
        args.output,
        STANDARD.encode(key.sign(&fs::read(args.input)?).to_bytes()),
    )?;
    Ok(())
}
