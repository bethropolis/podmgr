use std::path::PathBuf;
use std::process::Command;

fn main() {
    let version = std::env::var("PODBOX_VERSION")
        .ok()
        .filter(|s| !s.is_empty())
        .or_else(git_describe)
        .unwrap_or_else(|| std::env::var("CARGO_PKG_VERSION").expect("CARGO_PKG_VERSION not set"));
    println!("cargo:rustc-env=PODBOX_VERSION={version}");

    embed_guest();
}

fn git_describe() -> Option<String> {
    let output = Command::new("git")
        .args(["describe", "--tags", "--always", "--dirty=-dirty"])
        .output()
        .ok()?;
    if output.status.success() {
        String::from_utf8(output.stdout)
            .ok()
            .map(|s| s.trim().to_string())
    } else {
        None
    }
}

fn embed_guest() {
    println!("cargo:rerun-if-changed=../podbox-guest/src/");
    println!("cargo:rerun-if-changed=../podbox-guest/Cargo.toml");

    let manifest_dir = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap());
    let workspace_root = manifest_dir
        .parent()
        .expect("CARGO_MANIFEST_DIR has no parent")
        .parent()
        .expect("crates/podbox should have a grandparent workspace root");
    let guest_target = workspace_root.join("target").join("guest-build");
    let out_dir = PathBuf::from(std::env::var("OUT_DIR").unwrap());

    // Prefer a fully static musl build so the embedded guest works in any
    // container regardless of libc. Fall back to the host default target if
    // the musl target isn't installed.
    let musl_target = "x86_64-unknown-linux-musl";
    let musl_available = Command::new("rustup")
        .args(["target", "list", "--installed"])
        .output()
        .ok()
        .map(|o| String::from_utf8_lossy(&o.stdout).contains(musl_target))
        .unwrap_or(false);

    let (guest_path, target_label) = if musl_available {
        let path = guest_target
            .join(musl_target)
            .join("release")
            .join("podbox-guest");
        let status = Command::new("cargo")
            .args([
                "build",
                "--release",
                "--target",
                musl_target,
                "-p",
                "podbox-guest",
                "--target-dir",
            ])
            .arg(&guest_target)
            .status()
            .expect("Failed to launch cargo build for podbox-guest");
        assert!(status.success(), "podbox-guest musl build failed");
        (path, "musl / static")
    } else {
        let path = guest_target.join("release").join("podbox-guest");
        let status = Command::new("cargo")
            .args(["build", "--release", "-p", "podbox-guest", "--target-dir"])
            .arg(&guest_target)
            .status()
            .expect("Failed to launch cargo build for podbox-guest");
        assert!(status.success(), "podbox-guest build failed");
        (path, "dynamic")
    };

    println!("cargo:warning=podbox-guest binary built ({target_label})");

    let guest_bytes = std::fs::read(&guest_path).expect("failed to read podbox-guest binary");

    std::fs::write(out_dir.join("podbox-guest"), &guest_bytes)
        .expect("failed to copy podbox-guest to OUT_DIR");

    let size = guest_bytes.len();
    let code = format!(
        r#"
pub static PODBOX_GUEST_BINARY: &[u8] = {{
    const RAW: &[u8; {size}] = include_bytes!(concat!(env!("OUT_DIR"), "/podbox-guest"));
    RAW
}};
"#,
    );
    std::fs::write(out_dir.join("podbox_guest.rs"), code).expect("failed to write podbox_guest.rs");
}
