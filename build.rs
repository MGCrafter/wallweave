fn main() {
    // Installed builds ship the Steamworks runtime beside the executable.
    // Cargo supplies the dependency's library path for `cargo run` and tests.
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("linux") {
        println!("cargo:rustc-link-arg=-Wl,-rpath,$ORIGIN");
    }
}
