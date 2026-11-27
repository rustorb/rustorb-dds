// The version of RustDDS in Cargo.lock, for the "version" of the JSON.
fn main() {
    println!("cargo:rerun-if-changed=Cargo.lock");
    let lock = std::fs::read_to_string("Cargo.lock").unwrap_or_default();
    let mut version = "?".to_string();
    let mut in_rustdds = false;
    for line in lock.lines() {
        if line.starts_with("name = ") {
            in_rustdds = line == "name = \"rustdds\"";
        } else if in_rustdds && line.starts_with("version = ") {
            version = line
                .trim_start_matches("version = ")
                .trim_matches('"')
                .to_string();
            break;
        }
    }
    println!("cargo:rustc-env=RUSTDDS_VERSION={version}");
}
