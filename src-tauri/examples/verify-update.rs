//! Verify the exact public key and archive used by a release before publishing.
use base64::Engine;
use minisign_verify::{PublicKey, Signature};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args.len() != 2 {
        return Err("usage: verify-update <archive> <signature>".into());
    }
    let config: serde_json::Value = serde_json::from_str(include_str!("../tauri.conf.json"))?;
    let key = config["plugins"]["updater"]["pubkey"]
        .as_str()
        .ok_or("missing updater public key")?;
    let decoded_key = base64::engine::general_purpose::STANDARD.decode(key.trim())?;
    let key = PublicKey::decode(std::str::from_utf8(&decoded_key)?)?;
    let encoded_signature = std::fs::read_to_string(&args[1])?;
    let signature = base64::engine::general_purpose::STANDARD.decode(encoded_signature.trim())?;
    let signature = Signature::decode(std::str::from_utf8(&signature)?)?;
    key.verify(&std::fs::read(&args[0])?, &signature, false)?;
    let signed_version = signature
        .trusted_comment()
        .split('\t')
        .find_map(|field| field.strip_prefix("version:"))
        .ok_or("signature must carry the application version")?;
    if semver::Version::parse(signed_version.trim_start_matches('v'))?
        != semver::Version::parse(env!("CARGO_PKG_VERSION"))?
    {
        return Err("signed application version differs from the release version".into());
    }
    println!("Verified update signature: {}", args[0]);
    Ok(())
}
