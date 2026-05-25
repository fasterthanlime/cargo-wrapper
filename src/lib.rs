use std::ffi::{OsStr, OsString};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DeniedInvocation {
    CargoTest {
        package: Option<DeniedPackageSelection>,
    },
    PackageSelection(DeniedPackageSelection),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeniedPackageSelection {
    pub flag: String,
    pub value: Option<String>,
}

pub fn denied_invocation(args: &[OsString]) -> Option<DeniedInvocation> {
    let package = denied_package_selection(args);

    if cargo_subcommand(args).is_some_and(|subcommand| subcommand == "test") {
        return Some(DeniedInvocation::CargoTest { package });
    }

    package.map(DeniedInvocation::PackageSelection)
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

pub fn deny_message(denied: &DeniedInvocation) -> String {
    match denied {
        DeniedInvocation::CargoTest { package } => cargo_test_deny_message(package.as_ref()),
        DeniedInvocation::PackageSelection(package) => package_selection_deny_message(package),
    }
}

fn cargo_test_deny_message(package: Option<&DeniedPackageSelection>) -> String {
    let mut message = String::from(
        "cargo-wrapper: refusing cargo test\n\n\
         Use cargo nextest instead of cargo test. Nextest gives better failure \
         reporting, runs the workspace test graph directly, and keeps test \
         selection explicit instead of smuggling workspace slicing through Cargo \
         package flags.\n\n",
    );

    if let Some(package) = package {
        message.push_str("Detected forbidden Cargo package selector: ");
        message.push_str(&package.flag);
        if let Some(value) = &package.value {
            message.push(' ');
            message.push_str(value);
        }

        message.push_str("\n\n");

        if let Some(value) = &package.value {
            message.push_str("For nextest, do not write `-p ");
            message.push_str(value);
            message.push_str(
                "`. Use the filter expression instead:\n  cargo nextest run -E 'package(",
            );
            message.push_str(value);
            message.push_str(")'\n\n");
        } else {
            message.push_str(
                "For nextest, do not use Cargo package selection. Use a nextest \
                 filter expression such as:\n  cargo nextest run -E 'package(crate-name)'\n\n",
            );
        }
    }

    message.push_str(
        "Do this instead:\n\
           cargo nextest run --workspace --no-fail-fast\n",
    );

    message
}

fn package_selection_deny_message(denied: &DeniedPackageSelection) -> String {
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
         Cargo's package selection flags change the set of crates being built, \
         which changes the set of enabled features. That breaks the feature \
         unification shape the workspace is supposed to establish, so Cargo \
         keeps invalidating and overwriting useful build cache entries instead \
         of reusing one stable full-workspace cache shape. Agents must run the \
         workspace as a workspace so the cache stops getting churned by \
         different partial feature sets. Keeping failures outside one crate \
         visible is useful too, but the cache invalidation is the main reason \
         this wrapper exists.\n\n\
         Do this instead:\n\
           cargo check --workspace --all-targets --all-features\n\
           cargo clippy --workspace --all-targets --all-features -- -D warnings\n\
           cargo nextest run --workspace --no-fail-fast\n\n\
         If the full workspace is broken, report the full failing set and fix it. \
         Do not hide it with -p or --package.\n",
    );

    message
}

fn cargo_subcommand(args: &[OsString]) -> Option<&str> {
    let mut index = 0;

    while index < args.len() {
        let arg = &args[index];

        if arg == OsStr::new("--") {
            return None;
        }

        let text = arg.to_str()?;

        if text.starts_with('+') {
            index += 1;
            continue;
        }

        if text == "--" {
            return None;
        }

        if let Some(option) = text.strip_prefix("--") {
            if !option.contains('=') && long_option_takes_separate_value(option) {
                index += 2;
            } else {
                index += 1;
            }
            continue;
        }

        if text.starts_with('-') && text.len() > 1 {
            let skip = short_option_separate_value_count(text, args.get(index + 1));
            index += skip;
            continue;
        }

        return Some(text);
    }

    None
}

fn long_option_takes_separate_value(option: &str) -> bool {
    matches!(
        option,
        "color"
            | "config"
            | "jobs"
            | "lockfile-path"
            | "manifest-path"
            | "message-format"
            | "target"
            | "target-dir"
    )
}

fn short_option_separate_value_count(text: &str, next: Option<&OsString>) -> usize {
    let Some(short) = text.strip_prefix('-') else {
        return 1;
    };

    if short.starts_with('-') {
        return 1;
    }

    if short.len() == 1
        && short
            .chars()
            .next()
            .is_some_and(short_option_takes_attached_value)
        && next.is_some()
    {
        return 2;
    }

    1
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

    #[test]
    fn denies_cargo_test_without_package_selection() {
        assert_eq!(
            denied_invocation(&args(&["test"])),
            Some(DeniedInvocation::CargoTest { package: None })
        );
    }

    #[test]
    fn denies_cargo_test_with_package_selection() {
        assert_eq!(
            denied_invocation(&args(&["test", "-p", "demo"])),
            Some(DeniedInvocation::CargoTest {
                package: Some(DeniedPackageSelection {
                    flag: "-p".to_owned(),
                    value: Some("demo".to_owned())
                })
            })
        );
    }

    #[test]
    fn finds_test_subcommand_after_global_options() {
        assert_eq!(
            denied_invocation(&args(&["--color", "always", "-q", "test"])),
            Some(DeniedInvocation::CargoTest { package: None })
        );
        assert_eq!(
            denied_invocation(&args(&["+nightly", "-Zunstable-options", "test"])),
            Some(DeniedInvocation::CargoTest { package: None })
        );
        assert_eq!(
            denied_invocation(&args(&["--frozen", "test"])),
            Some(DeniedInvocation::CargoTest { package: None })
        );
    }

    #[test]
    fn package_selection_on_other_commands_keeps_workspace_subset_diagnostic() {
        assert_eq!(
            denied_invocation(&args(&["check", "-p", "demo"])),
            Some(DeniedInvocation::PackageSelection(DeniedPackageSelection {
                flag: "-p".to_owned(),
                value: Some("demo".to_owned())
            }))
        );
    }
}
