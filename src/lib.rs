use std::ffi::{OsStr, OsString};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeniedPackageSelection {
    pub flag: String,
    pub value: Option<String>,
}

pub fn denied_package_selection(args: &[OsString]) -> Option<DeniedPackageSelection> {
    let mut index = 0;

    while index < args.len() {
        let arg = &args[index];

        if arg == OsStr::new("--") {
            return None;
        }

        if let Some(text) = arg.to_str() {
            if text == "--package" {
                return Some(DeniedPackageSelection {
                    flag: text.to_owned(),
                    value: args.get(index + 1).map(display_os),
                });
            }

            if let Some(value) = text.strip_prefix("--package=") {
                return Some(DeniedPackageSelection {
                    flag: "--package".to_owned(),
                    value: Some(value.to_owned()),
                });
            }

            if text == "-p" {
                return Some(DeniedPackageSelection {
                    flag: text.to_owned(),
                    value: args.get(index + 1).map(display_os),
                });
            }

            if let Some(value) = text.strip_prefix("-p")
                && !value.is_empty()
            {
                return Some(DeniedPackageSelection {
                    flag: "-p".to_owned(),
                    value: Some(value.to_owned()),
                });
            }

            if let Some(denied) = denied_short_cluster(text, args.get(index + 1)) {
                return Some(denied);
            }
        }

        index += 1;
    }

    None
}

pub fn deny_message(denied: &DeniedPackageSelection) -> String {
    let mut message = String::from(
        "cargo-wrapper: refusing workspace subset selection\n\n\
         Detected forbidden Cargo package selector: ",
    );

    message.push_str(&denied.flag);
    if let Some(value) = &denied.value {
        message.push(' ');
        message.push_str(value);
    }

    message.push_str(
        "\n\n\
         Never select a subset of the workspace.\n\n\
         Cargo's package selection flags split the workspace into smaller shards, \
         which hides failures in crates outside that shard and throws away the \
         build-cache shape that a full workspace command would have reused. \
         Agents must keep the production path visible instead of narrowing the \
         command until it looks green.\n\n\
         Do this instead:\n\
           cargo check --workspace --all-targets --all-features\n\
           cargo clippy --workspace --all-targets --all-features -- -D warnings\n\
           cargo nextest run --workspace --no-fail-fast\n\n\
         If the full workspace is broken, report the full failing set and fix it. \
         Do not hide it with -p or --package.\n",
    );

    message
}

fn denied_short_cluster(text: &str, next: Option<&OsString>) -> Option<DeniedPackageSelection> {
    let cluster = text.strip_prefix('-')?;
    if cluster.starts_with('-') || cluster.is_empty() {
        return None;
    }

    let mut chars = cluster.char_indices();
    let (_, first) = chars.next()?;

    if short_option_takes_attached_value(first) {
        return None;
    }

    for (byte_index, ch) in chars {
        if ch == 'p' {
            let value = &cluster[byte_index + ch.len_utf8()..];
            return Some(DeniedPackageSelection {
                flag: "-p".to_owned(),
                value: if value.is_empty() {
                    next.map(display_os)
                } else {
                    Some(value.to_owned())
                },
            });
        }

        if short_option_takes_attached_value(ch) {
            return None;
        }
    }

    None
}

fn short_option_takes_attached_value(ch: char) -> bool {
    matches!(ch, 'C' | 'F' | 'Z' | 'j')
}

fn display_os(value: &OsString) -> String {
    value.to_string_lossy().into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(items: &[&str]) -> Vec<OsString> {
        items.iter().map(OsString::from).collect()
    }

    #[test]
    fn denies_long_package_with_separate_value() {
        let denied = denied_package_selection(&args(&["check", "--package", "demo"])).unwrap();

        assert_eq!(
            denied,
            DeniedPackageSelection {
                flag: "--package".to_owned(),
                value: Some("demo".to_owned())
            }
        );
    }

    #[test]
    fn denies_short_package_with_attached_value() {
        let denied = denied_package_selection(&args(&["nextest", "run", "-pdemo"])).unwrap();

        assert_eq!(
            denied,
            DeniedPackageSelection {
                flag: "-p".to_owned(),
                value: Some("demo".to_owned())
            }
        );
    }

    #[test]
    fn allows_program_args_after_double_dash() {
        assert_eq!(
            denied_package_selection(&args(&["run", "--", "-p", "demo"])),
            None
        );
    }

    #[test]
    fn ignores_attached_values_for_other_short_flags() {
        assert_eq!(
            denied_package_selection(&args(&["check", "-Zpanic-abort-tests"])),
            None
        );
        assert_eq!(
            denied_package_selection(&args(&["check", "-Fparallel"])),
            None
        );
    }
}
