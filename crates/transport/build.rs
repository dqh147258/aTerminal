fn main() {
    println!("cargo:rerun-if-changed=build.rs");

    // Build scripts run on the host, so inspect Cargo's target configuration.
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows")
        && std::env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("msvc")
    {
        // datachannel-sys 0.23.0+0.23.2 does not propagate all Win32 import
        // libraries required by vendored libdatachannel and static OpenSSL.
        // Link through this library so tests and downstream binaries inherit it.
        for library in [
            "bcrypt", "ws2_32", "iphlpapi", "advapi32", "user32", "crypt32",
        ] {
            println!("cargo:rustc-link-lib=dylib={library}");
        }
    }
}
