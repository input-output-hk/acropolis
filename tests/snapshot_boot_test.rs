// Integration test for end-to-end snapshot boot

use std::process::Command;

#[test]
#[ignore] // Slow test: counts 11.2M UTXOs (~2s). Run with: cargo test --test snapshot_boot_test -- --ignored
fn test_boot_with_valid_snapshot() {
    // Skip test if Amaru snapshot doesn't exist
    let amaru_path = "tests/fixtures/134092758.670ca68c3de580f8469677754a725e86ca72a7be381d3108569f0704a5fca327.cbor";
    if !std::path::Path::new(amaru_path).exists() {
        eprintln!("Skipping test: Amaru snapshot not found at {}", amaru_path);
        return;
    }

    let output = Command::new("cargo")
        .args(&[
            "run",
            "--",
            "--snapshot",
            amaru_path,
            "--manifest",
            "tests/fixtures/134092758.670ca68c3de580f8469677754a725e86ca72a7be381d3108569f0704a5fca327.json",
        ])
        .output()
        .expect("Failed to execute command");

    if !output.status.success() {
        eprintln!("STDOUT:\n{}", String::from_utf8_lossy(&output.stdout));
        eprintln!("STDERR:\n{}", String::from_utf8_lossy(&output.stderr));
    }

    assert!(output.status.success());

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Status: Starting"));
    assert!(stdout.contains("Status: LoadingSnapshot"));
    assert!(stdout.contains("Status: Ready"));
    assert!(stdout.contains("Node READY"));
}

#[test]
fn test_boot_with_wrong_era() {
    let output = Command::new("cargo")
        .args(&[
            "run",
            "--",
            "--snapshot",
            "tests/fixtures/snapshot-small.cbor",
            "--manifest",
            "tests/fixtures/wrong-era-manifest.json",
        ])
        .output()
        .expect("Failed to execute command");

    assert!(!output.status.success());

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("Era mismatch"));
}

#[test]
fn test_boot_without_snapshot_arg() {
    let output = Command::new("cargo")
        .args(&[
            "run",
            "--",
            "--manifest",
            "tests/fixtures/test-manifest.json",
        ])
        .output()
        .expect("Failed to execute command");

    assert!(!output.status.success());

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("--snapshot is required"));
}
