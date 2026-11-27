//! A real signed update runs through independent application/helper subprocesses.
//! This is a test host, not a shipped executable. All files and keys are disposable.
#[cfg(all(target_os = "macos", debug_assertions))]
mod macos {
    use anyhow::{ensure, Context, Result};
    use base64::{engine::general_purpose::STANDARD, Engine};
    use portcanvas_native_probe::macos_update;
    use ring::signature::{Ed25519KeyPair, KeyPair};
    use std::{
        fs,
        path::{Path, PathBuf},
        process::{Command, Stdio},
        time::{Duration, Instant},
    };
    use tokio_util::sync::CancellationToken;

    struct Sandbox(PathBuf);
    impl Sandbox {
        fn new() -> Result<Self> {
            let root = PathBuf::from(
                std::env::var_os("WCS_TEST_ROOT")
                    .context("WCS_TEST_ROOT is required for isolated updater tests")?,
            );
            let path = root.join(format!("macos-update-process-{}", uuid::Uuid::new_v4()));
            fs::create_dir_all(&path)?;
            Ok(Self(path))
        }
    }
    impl Drop for Sandbox {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
    fn codesign(app: &Path) -> Result<()> {
        let result = Command::new("/usr/bin/codesign")
            .args(["--force", "--sign", "-", "--timestamp=none"])
            .arg(app)
            .output()?;
        ensure!(
            result.status.success(),
            "codesign fixture: {}",
            String::from_utf8_lossy(&result.stderr)
        );
        Ok(())
    }
    fn bundle(root: &Path, version: &str, fail: bool) -> Result<PathBuf> {
        let app = root.join("PortCanvas.app");
        let contents = app.join("Contents");
        fs::create_dir_all(contents.join("MacOS"))?;
        fs::create_dir(contents.join("Resources"))?;
        fs::copy(
            std::env::current_exe()?,
            contents.join("MacOS/PortCanvasNative"),
        )?;
        let base = version.split('-').next().unwrap();
        let (id, extra) = if version.contains("-beta.") {
            ("com.portcanvas.desktop.beta", format!("<key>PortCanvasVersion</key><string>{version}</string>"))
        } else {
            ("com.portcanvas.desktop", String::new())
        };
        fs::write(
            contents.join("Info.plist"),
            format!(
                r#"<?xml version="1.0" encoding="UTF-8"?><plist version="1.0"><dict><key>CFBundleIdentifier</key><string>{id}</string><key>CFBundleExecutable</key><string>PortCanvasNative</string><key>CFBundleShortVersionString</key><string>{base}</string><key>CFBundleVersion</key><string>{base}</string>{extra}<key>CFBundlePackageType</key><string>APPL</string></dict></plist>"#
            ),
        )?;
        if fail {
            fs::write(
                contents.join("Resources/fail-start"),
                b"test startup failure",
            )?;
        }
        codesign(&app)?;
        Ok(app)
    }
    fn signed(bytes: &[u8], version: &str) -> (String, String) {
        let key = Ed25519KeyPair::from_seed_unchecked(&[42; 32]).unwrap();
        let id = b"TESTKEY1";
        let mut public = b"Ed".to_vec();
        public.extend_from_slice(id);
        public.extend_from_slice(key.public_key().as_ref());
        let public = STANDARD.encode(format!(
            "untrusted comment: disposable test public key\n{}\n",
            STANDARD.encode(public)
        ));
        let raw = key.sign(bytes);
        let comment = format!("timestamp:1\tfile:fixture.app.tar.gz\tversion:{version}");
        let mut message = raw.as_ref().to_vec();
        message.extend_from_slice(comment.as_bytes());
        let global = key.sign(&message);
        let mut signature = b"Ed".to_vec();
        signature.extend_from_slice(id);
        signature.extend_from_slice(raw.as_ref());
        let signature = STANDARD.encode(format!(
            "untrusted comment: disposable test signature\n{}\ntrusted comment: {comment}\n{}\n",
            STANDARD.encode(signature),
            STANDARD.encode(global.as_ref())
        ));
        (public, signature)
    }
    fn app_root() -> Result<PathBuf> {
        let executable = std::env::current_exe()?;
        Ok(executable
            .parent()
            .and_then(Path::parent)
            .and_then(Path::parent)
            .context("test bundle path")?
            .to_owned())
    }
    fn fixture_process() -> Result<bool> {
        let mode = std::env::args().nth(1).unwrap_or_default();
        if mode == "--portcanvas-update-ack" {
            let app = app_root()?;
            if app.join("Contents/Resources/fail-start").exists() {
                std::process::exit(9);
            }
            macos_update::acknowledge_launch()?;
            std::thread::sleep(Duration::from_millis(150));
            return Ok(true);
        }
        if mode == "--portcanvas-update-failed" {
            let app = app_root()?;
            fs::write(
                app.parent().unwrap().join("rollback-visible.txt"),
                std::env::args().nth(2).unwrap_or_default(),
            )?;
            return Ok(true);
        }
        if mode == "--fixture-install" {
            let app = app_root()?;
            let root = app.parent().unwrap();
            let input: serde_json::Value =
                serde_json::from_slice(&fs::read(root.join("input.json"))?)?;
            let payload = fs::read(root.join("fixture.tar.gz"))?;
            let cancel = CancellationToken::new();
            if input["cancel"] == true {
                cancel.cancel();
            }
            let result = macos_update::start_update(
                &payload,
                input["key"].as_str().unwrap(),
                "0.6.0",
                env!("CARGO_PKG_VERSION"),
                input["signature"].as_str().unwrap(),
                &cancel,
            );
            if input["reject"] == true {
                ensure!(result.is_err(), "unsafe update was accepted");
                fs::write(root.join("rejected.txt"), b"rejected")?;
            } else {
                result?;
                // Helper must not exchange a still-running application's bundle.
                std::thread::sleep(Duration::from_millis(250));
                let plist = fs::read_to_string(app.join("Contents/Info.plist"))?;
                ensure!(
                    plist.contains("<string>0.6.0</string>"),
                    "application changed before parent exit"
                );
                fs::write(
                    root.join("waited-for-parent.txt"),
                    b"old bundle remained active",
                )?;
            }
            return Ok(true);
        }
        Ok(false)
    }
    fn exercise(failure: bool, tamper: bool, cancel: bool) -> Result<()> {
        let s = Sandbox::new()?;
        let installed = s.0.join("installed");
        fs::create_dir(&installed)?;
        let app = bundle(&installed, "0.6.0", false)?;
        let incoming = s.0.join("incoming");
        fs::create_dir(&incoming)?;
        let new = bundle(&incoming, env!("CARGO_PKG_VERSION"), failure)?;
        let gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
        let mut archive = tar::Builder::new(gz);
        archive.append_dir_all("PortCanvas.app", &new)?;
        let mut payload = archive.into_inner()?.finish()?;
        let (key, signature) = signed(&payload, env!("CARGO_PKG_VERSION"));
        if tamper {
            payload[20] ^= 1;
        }
        fs::write(installed.join("fixture.tar.gz"), payload)?;
        fs::write(
            installed.join("input.json"),
            serde_json::to_vec(
                &serde_json::json!({"key":key,"signature":signature,"cancel":cancel,"reject":tamper||cancel}),
            )?,
        )?;
        fs::write(s.0.join("user-data.json"), b"user data preserved")?;
        let mut old = Command::new(app.join("Contents/MacOS/PortCanvasNative"))
            .arg("--fixture-install")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::inherit())
            .spawn()?;
        let started = Instant::now();
        loop {
            if let Some(status) = old.try_wait()? {
                ensure!(status.success(), "old test app failed: {status}");
                break;
            }
            if started.elapsed() > Duration::from_secs(45) {
                let _ = old.kill();
                bail_timeout()?;
            }
            std::thread::sleep(Duration::from_millis(25));
        }
        let reject = tamper || cancel;
        let started = Instant::now();
        loop {
            let stages = fs::read_dir(&installed)?
                .filter_map(|v| v.ok())
                .filter(|v| {
                    v.file_name()
                        .as_encoded_bytes()
                        .starts_with(b".PortCanvas-update-")
                })
                .count();
            let text = fs::read_to_string(app.join("Contents/Info.plist"))?;
            let finished = if reject {
                installed.join("rejected.txt").exists() && stages == 0
            } else if failure {
                installed.join("rollback-visible.txt").exists()
                    && stages == 0
                    && text.contains("<string>0.6.0</string>")
            } else {
                stages == 0
                    && text.contains(&format!("<string>{}</string>", env!("CARGO_PKG_VERSION")))
            };
            if finished {
                break;
            }
            ensure!(
                started.elapsed() < Duration::from_secs(45),
                "helper did not finish replacement/rollback in {:?}",
                installed
            );
            std::thread::sleep(Duration::from_millis(25));
        }
        if !reject {
            ensure!(
                installed.join("waited-for-parent.txt").exists(),
                "parent wait was not exercised"
            );
        }
        ensure!(
            fs::read(s.0.join("user-data.json"))? == b"user data preserved",
            "user data changed"
        );
        println!(
            "signed helper process: fail_start={failure}, tamper={tamper}, cancel={cancel}: passed"
        );
        // Allow the acknowledged test app to leave before the fixture is reclaimed.
        std::thread::sleep(Duration::from_millis(200));
        Ok(())
    }
    fn bail_timeout() -> Result<()> {
        anyhow::bail!("test application failed to exit")
    }
    pub fn main() -> Result<()> {
        if std::env::args_os().nth(1).as_deref()
            == Some(std::ffi::OsStr::new("--portcanvas-update-helper"))
        {
            macos_update::dispatch_helper()?;
            return Ok(());
        }
        if fixture_process()? {
            return Ok(());
        }
        exercise(false, false, false)?;
        exercise(true, false, false)?;
        exercise(false, true, false)?;
        exercise(false, false, true)?;
        Ok(())
    }
}
fn main() {
    #[cfg(all(target_os = "macos", not(debug_assertions)))]
    println!("Skipped disposable-key macOS update subprocess scenarios: release builds keep the publisher key pinned.");
    #[cfg(all(target_os = "macos", debug_assertions))]
    if let Err(error) = macos::main() {
        eprintln!("{error:#}");
        std::process::exit(1);
    }
}
