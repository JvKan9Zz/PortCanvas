//! Release packaging readback using the exact native updater verifier.
use anyhow::{ensure, Context, Result};
use portcanvas_native_probe::update_client::Plan;
use std::path::PathBuf;
fn main() -> Result<()> {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    ensure!(args.len() == 3, "installer, manifest and public key paths required");
    let bytes = std::fs::read(PathBuf::from(&args[0]))?;
    let manifest: serde_json::Value = serde_json::from_slice(&std::fs::read(PathBuf::from(&args[1]))?)?;
    let key = std::fs::read_to_string(PathBuf::from(&args[2]))?;
    let plan = Plan::from_manifest(&manifest, "0.5.95")?.context("newer preview required")?;
    portcanvas_native_probe::update::verify(&bytes, key.trim(), &plan.signature, "0.5.95", &plan.version)?;
    println!("Verified {} installer bytes bound to {} with the native updater.", bytes.len(), plan.version);
    Ok(())
}
