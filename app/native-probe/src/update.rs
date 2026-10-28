//! Native verification of the existing signed-version contract. Does not download or execute packages.
use anyhow::{ensure, Context, Result};
use base64::{engine::general_purpose::STANDARD, Engine};
use minisign_verify::{PublicKey, Signature};
use semver::Version;

pub fn verify(
    payload: &[u8],
    public_key: &str,
    signature: &str,
    current: &str,
    announced: &str,
) -> Result<()> {
    let current = Version::parse(current.trim_start_matches('v'))?;
    let announced = Version::parse(announced.trim_start_matches('v'))?;
    ensure!(
        announced > current,
        "Update is not newer than the installed version"
    );
    let key = PublicKey::decode(&String::from_utf8(STANDARD.decode(public_key)?)?)?;
    let signature = Signature::decode(&String::from_utf8(STANDARD.decode(signature)?)?)?;
    key.verify(payload, &signature, true)?;
    // Authenticate the trusted comment before reading its version, matching the legacy updater.
    let signed = signature
        .trusted_comment()
        .split('\t')
        .find_map(|field| field.strip_prefix("version:"))
        .context("Update signature does not bind an application version")?;
    ensure!(
        Version::parse(signed.trim_start_matches('v'))? == announced,
        "Signed and announced versions differ"
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture() -> serde_json::Value {
        serde_json::from_str(include_str!("../tests/fixtures/update.json")).unwrap()
    }
    #[test]
    fn accepts_existing_signer_format_without_tauri_runtime() -> Result<()> {
        let f = fixture();
        verify(
            f["payload"].as_str().unwrap().as_bytes(),
            f["publicKey"].as_str().unwrap(),
            f["signature"].as_str().unwrap(),
            "0.5.95",
            "0.6.0",
        )
    }
    #[test]
    fn rejects_tampered_payload_and_wrong_key() {
        let f = fixture();
        let key = f["publicKey"].as_str().unwrap();
        let signature = f["signature"].as_str().unwrap();
        assert!(verify(b"tampered", key, signature, "0.5.95", "0.6.0").is_err());
        assert!(verify(
            f["payload"].as_str().unwrap().as_bytes(),
            f["otherPublicKey"].as_str().unwrap(),
            signature,
            "0.5.95",
            "0.6.0"
        )
        .is_err());
    }
    #[test]
    fn rejects_manifest_relabel_and_missing_signed_version() {
        let f = fixture();
        let bytes = f["payload"].as_str().unwrap().as_bytes();
        let key = f["publicKey"].as_str().unwrap();
        assert!(verify(
            bytes,
            key,
            f["signature"].as_str().unwrap(),
            "0.5.95",
            "0.6.1"
        )
        .unwrap_err()
        .to_string()
        .contains("Signed and announced"));
        assert!(verify(
            bytes,
            key,
            f["signatureWithoutVersion"].as_str().unwrap(),
            "0.5.95",
            "0.6.0"
        )
        .unwrap_err()
        .to_string()
        .contains("does not bind"));
    }
    #[test]
    fn rejects_downgrade_and_equal_version() {
        let f = fixture();
        for current in ["0.6.0", "0.6.1"] {
            assert!(verify(
                f["payload"].as_str().unwrap().as_bytes(),
                f["publicKey"].as_str().unwrap(),
                f["signature"].as_str().unwrap(),
                current,
                "0.6.0"
            )
            .is_err());
        }
    }
}
