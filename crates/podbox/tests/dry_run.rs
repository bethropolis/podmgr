use std::process::Command;

use assert_cmd::prelude::*;

fn fixtures_dir() -> std::path::PathBuf {
    std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
}

#[test]
fn dry_build_shows_containerfile() {
    let mut cmd = Command::cargo_bin("podbox").unwrap();
    cmd.args([
        "--config",
        &fixtures_dir().join("full.toml").to_string_lossy(),
        "build",
        "--dry-run",
    ]);
    cmd.assert()
        .success()
        .stdout(predicates::str::contains("FROM fedora:41"))
        .stdout(predicates::str::contains("podbox-guest"));
}

#[test]
fn dry_build_shows_podman_build_command() {
    let mut cmd = Command::cargo_bin("podbox").unwrap();
    cmd.args([
        "--config",
        &fixtures_dir().join("full.toml").to_string_lossy(),
        "build",
        "--dry-run",
    ]);
    cmd.assert()
        .success()
        .stdout(predicates::str::contains("podman build"))
        .stdout(predicates::str::contains("localhost/podbox-myenv:latest"));
}

#[test]
fn dry_enable_shows_quadlet_container_section() {
    let mut cmd = Command::cargo_bin("podbox").unwrap();
    cmd.args([
        "--config",
        &fixtures_dir().join("full.toml").to_string_lossy(),
        "enable",
        "--dry-run",
    ]);
    cmd.assert()
        .success()
        .stdout(predicates::str::contains("[Container]"))
        .stdout(predicates::str::contains("Environment=HOME=/home/%u"));
}

#[test]
fn dry_enable_shows_quadlet_socket_section() {
    let mut cmd = Command::cargo_bin("podbox").unwrap();
    cmd.args([
        "--config",
        &fixtures_dir().join("full.toml").to_string_lossy(),
        "enable",
        "--dry-run",
    ]);
    cmd.assert()
        .success()
        .stdout(predicates::str::contains("[Socket]"))
        .stdout(predicates::str::contains(
            "ListenStream=%t/podbox/myenv.sock",
        ));
}

#[test]
fn dry_enable_shows_quadlet_build_section() {
    let mut cmd = Command::cargo_bin("podbox").unwrap();
    cmd.args([
        "--config",
        &fixtures_dir().join("full.toml").to_string_lossy(),
        "enable",
        "--dry-run",
    ]);
    cmd.assert()
        .success()
        .stdout(predicates::str::contains("[Build]"))
        .stdout(predicates::str::contains(
            "ImageTag=localhost/podbox-myenv:latest",
        ));
}

#[test]
fn dry_shell_shows_podman_exec() {
    let mut cmd = Command::cargo_bin("podbox").unwrap();
    cmd.args([
        "--config",
        &fixtures_dir().join("full.toml").to_string_lossy(),
        "shell",
        "--dry-run",
    ]);
    cmd.assert()
        .success()
        .stdout(predicates::str::contains("podman exec"));
}

#[test]
fn dry_stop_shows_podman_stop() {
    let mut cmd = Command::cargo_bin("podbox").unwrap();
    cmd.args([
        "--config",
        &fixtures_dir().join("full.toml").to_string_lossy(),
        "stop",
        "--dry-run",
    ]);
    cmd.assert()
        .success()
        .stdout(predicates::str::contains("podman stop"));
}
