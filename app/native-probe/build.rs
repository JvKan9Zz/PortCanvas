fn main() {
    // The generated native view constructor exceeds the MSVC default main stack
    // in debug builds. Reserve address space; commit remains demand-paged.
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        println!("cargo:rustc-link-arg-bin=PortCanvasNative=/STACK:8388608");
    }
}
