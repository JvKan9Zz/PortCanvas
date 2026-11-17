fn main() {
    // Preserve the Windows debug constructor's stack reservation.
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref()==Ok("windows") {
        println!("cargo:rustc-link-arg-bin=PortCanvasNative=/STACK:8388608");
    }
    println!("cargo:rerun-if-changed=native/macos/media.m");
    println!("cargo:rerun-if-changed=native/macos/window.m");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref()!=Ok("macos") {return;}
    let out=std::path::PathBuf::from(std::env::var_os("OUT_DIR").unwrap());
    let arch=if std::env::var("CARGO_CFG_TARGET_ARCH").unwrap()=="aarch64" {"arm64"} else {"x86_64"};
    let mut objects=vec![];
    for source in ["media", "window"] {
        let path=format!("native/macos/{source}.m");
        if !std::path::Path::new(&path).exists() {continue;}
        let object=out.join(format!("{source}.o"));
        assert!(std::process::Command::new("xcrun").args(["clang","-arch",arch,"-mmacosx-version-min=13.0","-fobjc-arc","-fmodules","-Wno-deprecated-declarations","-O2","-c",&path,"-o"]).arg(&object).status().unwrap().success(),"macOS bridge compilation failed");
        objects.push(object);
    }
    let library=out.join("libportcanvas_macos.a");
    assert!(std::process::Command::new("xcrun").args(["ar","crs"]).arg(&library).args(objects).status().unwrap().success());
    println!("cargo:rustc-link-search=native={}",out.display());
    println!("cargo:rustc-link-lib=static=portcanvas_macos");
    // Objective-C availability guards use Apple's compiler-rt shim, also on macOS 13.
    let runtime=std::process::Command::new("xcrun").args(["clang","--print-file-name=libclang_rt.osx.a"]).output().unwrap();
    let runtime=std::path::PathBuf::from(String::from_utf8(runtime.stdout).unwrap().trim());
    assert!(runtime.is_file(),"Apple availability runtime not found");
    println!("cargo:rustc-link-search=native={}",runtime.parent().unwrap().display());
    println!("cargo:rustc-link-lib=static=clang_rt.osx");
    for name in ["Foundation","AppKit","AVFoundation","AudioToolbox","CoreMedia","CoreVideo","QuartzCore","CoreGraphics"] {println!("cargo:rustc-link-lib=framework={name}");}
}
