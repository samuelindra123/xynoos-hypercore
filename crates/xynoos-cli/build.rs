fn main() {
    println!("cargo:rerun-if-env-changed=XYNOOS_SOURCE_COMMIT");
    println!("cargo:rerun-if-changed=../../.git/HEAD");
    let commit = std::env::var("XYNOOS_SOURCE_COMMIT").unwrap_or_else(|_| {
        std::process::Command::new("git")
            .args(["rev-parse", "HEAD"])
            .output()
            .ok()
            .filter(|o| o.status.success())
            .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_owned())
            .unwrap_or_else(|| "unknown".into())
    });
    println!("cargo:rustc-env=XYNOOS_SOURCE_COMMIT={commit}");
}
