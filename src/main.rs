use std::env;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::{self, Command};

use cargo_wrapper::{denied_invocation, deny_message};

fn main() {
    let args = env::args_os().skip(1).collect::<Vec<_>>();

    if let Some(denied) = denied_invocation(&args) {
        write_stderr(&deny_message(&denied));
        process::exit(2);
    }

    let cargo = match find_next_cargo() {
        Ok(cargo) => cargo,
        Err(err) => {
            write_stderr(&format!("cargo-wrapper: {err}\n"));
            process::exit(127);
        }
    };

    let status = match Command::new(cargo).args(&args).status() {
        Ok(status) => status,
        Err(err) => {
            write_stderr(&format!(
                "cargo-wrapper: failed to run downstream cargo: {err}\n"
            ));
            process::exit(127);
        }
    };

    process::exit(status.code().unwrap_or(1));
}

fn find_next_cargo() -> Result<PathBuf, String> {
    let current_exe =
        env::current_exe().map_err(|err| format!("cannot resolve current executable: {err}"))?;
    let current_exe = canonical_or_original(current_exe);
    let path = env::var_os("PATH")
        .ok_or_else(|| "PATH is not set; cannot find downstream cargo".to_owned())?;

    for dir in env::split_paths(&path) {
        for candidate in cargo_candidates(&dir) {
            if !is_executable_file(&candidate) {
                continue;
            }

            let candidate_canonical = canonical_or_original(candidate.clone());
            if candidate_canonical == current_exe {
                continue;
            }

            return Ok(candidate);
        }
    }

    Err("could not find another cargo executable later on PATH".to_owned())
}

fn cargo_candidates(dir: &Path) -> Vec<PathBuf> {
    #[cfg(windows)]
    {
        ["cargo.exe", "cargo.cmd", "cargo.bat", "cargo"]
            .into_iter()
            .map(|name| dir.join(name))
            .collect()
    }

    #[cfg(not(windows))]
    {
        vec![dir.join("cargo")]
    }
}

fn canonical_or_original(path: PathBuf) -> PathBuf {
    path.canonicalize().unwrap_or(path)
}

#[cfg(unix)]
fn is_executable_file(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;

    let Ok(metadata) = path.metadata() else {
        return false;
    };

    metadata.is_file() && metadata.permissions().mode() & 0o111 != 0
}

#[cfg(windows)]
fn is_executable_file(path: &Path) -> bool {
    path.is_file()
}

fn write_stderr(message: &str) {
    let _ = io::stderr().write_all(message.as_bytes());
}
