#![cfg(unix)]

use std::env;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

fn wrapper() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_cargo"))
}

#[test]
fn forwards_to_the_next_cargo_on_path() {
    let fixture = Fixture::new("forwards");

    let output = Command::new(wrapper())
        .args(["check", "--workspace"])
        .env("PATH", fixture.bin_dir())
        .env("CARGO_WRAPPER_FAKE_RECORD", fixture.record_path())
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(43));
    assert_eq!(
        fs::read_to_string(fixture.record_path()).unwrap(),
        "<check>\n<--workspace>\n"
    );
}

#[test]
fn rejects_package_selection_before_running_downstream_cargo() {
    let fixture = Fixture::new("rejects");

    let output = Command::new(wrapper())
        .args(["check", "-p", "demo"])
        .env("PATH", fixture.bin_dir())
        .env("CARGO_WRAPPER_FAKE_RECORD", fixture.record_path())
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(2));
    assert!(!fixture.record_path().exists());

    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(stderr.contains("Never select a subset of the workspace."));
    assert!(stderr.contains("cargo check --workspace --all-targets --all-features"));
    assert!(stderr.contains("cargo nextest run --workspace --no-fail-fast"));
}

#[test]
fn allows_package_like_program_args_after_double_dash() {
    let fixture = Fixture::new("double-dash");

    let output = Command::new(wrapper())
        .args(["run", "--", "-p", "demo"])
        .env("PATH", fixture.bin_dir())
        .env("CARGO_WRAPPER_FAKE_RECORD", fixture.record_path())
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(43));
    assert_eq!(
        fs::read_to_string(fixture.record_path()).unwrap(),
        "<run>\n<-->\n<-p>\n<demo>\n"
    );
}

struct Fixture {
    root: PathBuf,
    bin: PathBuf,
    record: PathBuf,
}

impl Fixture {
    fn new(name: &str) -> Self {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = env::temp_dir().join(format!(
            "cargo-wrapper-{name}-{}-{nanos}",
            std::process::id()
        ));
        let bin = root.join("bin");
        let record = root.join("record");

        fs::create_dir_all(&bin).unwrap();
        write_fake_cargo(&bin.join("cargo"));

        Self { root, bin, record }
    }

    fn bin_dir(&self) -> &Path {
        &self.bin
    }

    fn record_path(&self) -> &Path {
        &self.record
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

fn write_fake_cargo(path: &Path) {
    fs::write(
        path,
        "#!/bin/sh\n\
         for arg do\n\
           printf '<%s>\\n' \"$arg\" >> \"$CARGO_WRAPPER_FAKE_RECORD\"\n\
         done\n\
         exit 43\n",
    )
    .unwrap();

    let mut permissions = fs::metadata(path).unwrap().permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(path, permissions).unwrap();
}
