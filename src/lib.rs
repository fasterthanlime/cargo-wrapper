use std::ffi::{OsStr, OsString};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InvocationDecision {
    Forward,
    Deny(DeniedInvocation),
    CannotParse(ParseFailure),
}

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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseFailure {
    pub state: ParseState,
    pub index: usize,
    pub reason: String,
    pub args: Vec<String>,
    pub trace: Vec<ParseTraceEntry>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ParseState {
    CargoGlobal,
    CargoCommand,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseTraceEntry {
    pub state: ParseState,
    pub index: usize,
    pub arg: Option<String>,
    pub action: String,
}

pub fn classify_invocation(args: &[OsString]) -> InvocationDecision {
    Parser::new(args).parse()
}

pub fn deny_message(denied: &DeniedInvocation) -> String {
    match denied {
        DeniedInvocation::CargoTest { package } => cargo_test_deny_message(package.as_ref()),
        DeniedInvocation::PackageSelection(package) => package_selection_deny_message(package),
    }
}

pub fn parse_failure_message(failure: &ParseFailure) -> String {
    let mut message = String::from(
        "cargo-wrapper: refusing to forward an invocation it cannot parse\n\n\
         The wrapper only forwards commands after it has identified Cargo's \
         command boundary and checked the command arguments for forbidden \
         workspace-subset selectors. This invocation hit syntax the parser does \
         not know how to classify, so forwarding it would be guessing.\n\n",
    );

    message.push_str("Failure:\n");
    message.push_str(&format!("  state: {:?}\n", failure.state));
    message.push_str(&format!("  index: {}\n", failure.index));
    message.push_str(&format!("  reason: {}\n\n", failure.reason));

    message.push_str("argv:\n");
    for (index, arg) in failure.args.iter().enumerate() {
        message.push_str(&format!("  [{index}] {arg}\n"));
    }

    message.push_str("\nparser trace:\n");
    for entry in &failure.trace {
        let arg = entry.arg.as_deref().unwrap_or("<end>");
        message.push_str(&format!(
            "  state={:?} index={} arg={} action={}\n",
            entry.state, entry.index, arg, entry.action
        ));
    }

    message
}

struct Parser<'a> {
    args: &'a [OsString],
    index: usize,
    state: ParseState,
    trace: Vec<ParseTraceEntry>,
}

impl<'a> Parser<'a> {
    fn new(args: &'a [OsString]) -> Self {
        Self {
            args,
            index: 0,
            state: ParseState::CargoGlobal,
            trace: Vec::new(),
        }
    }

    fn parse(mut self) -> InvocationDecision {
        let command = match self.parse_cargo_global_args() {
            Ok(Some(command)) => command,
            Ok(None) => return InvocationDecision::Forward,
            Err(failure) => return InvocationDecision::CannotParse(failure),
        };

        self.state = ParseState::CargoCommand;
        self.trace(format!("entered command `{command}`"));

        let command_args = match self.scan_command_args() {
            Ok(command_args) => command_args,
            Err(failure) => {
                if command == "test" {
                    return InvocationDecision::Deny(DeniedInvocation::CargoTest { package: None });
                }

                return InvocationDecision::CannotParse(failure);
            }
        };

        if command_args.has_target {
            return InvocationDecision::Forward;
        }

        if command == "test" {
            return InvocationDecision::Deny(DeniedInvocation::CargoTest {
                package: command_args.package,
            });
        }

        if let Some(package) = command_args.package {
            return InvocationDecision::Deny(DeniedInvocation::PackageSelection(package));
        }

        InvocationDecision::Forward
    }

    fn parse_cargo_global_args(&mut self) -> Result<Option<String>, ParseFailure> {
        while self.index < self.args.len() {
            let arg = self.args[self.index].clone();

            if arg == OsStr::new("--") {
                self.trace("found `--` before cargo subcommand; cargo has no command to classify");
                self.index += 1;
                return Ok(None);
            }

            let Some(text) = arg.to_str() else {
                return Err(self.failure("cargo global argument is not valid UTF-8"));
            };

            if text.starts_with('+') && text.len() > 1 {
                self.trace(format!("accepted rustup toolchain selector `{text}`"));
                self.index += 1;
                continue;
            }

            if !text.starts_with('-') || text == "-" {
                self.trace(format!("identified cargo subcommand `{text}`"));
                self.index += 1;
                return Ok(Some(text.to_owned()));
            }

            if let Some(option) = text.strip_prefix("--") {
                match parse_long_global_option(option) {
                    ParsedOption::Flag => {
                        self.trace(format!("accepted cargo global flag `{text}`"));
                        self.index += 1;
                    }
                    ParsedOption::ValueInline => {
                        self.trace(format!(
                            "accepted cargo global option with inline value `{text}`"
                        ));
                        self.index += 1;
                    }
                    ParsedOption::ValueSeparate => {
                        if self.index + 1 >= self.args.len() {
                            return Err(self.failure(format!(
                                "cargo global option `{text}` requires a value"
                            )));
                        }
                        self.trace(format!(
                            "accepted cargo global option `{text}` with next argument as value"
                        ));
                        self.index += 2;
                    }
                    ParsedOption::Unknown => {
                        return Err(self.failure(format!("unknown cargo global option `{text}`")));
                    }
                }
                continue;
            }

            match parse_short_global_option(text) {
                ParsedOption::Flag => {
                    self.trace(format!("accepted cargo global short flag `{text}`"));
                    self.index += 1;
                }
                ParsedOption::ValueInline => {
                    self.trace(format!(
                        "accepted cargo global short option with inline value `{text}`"
                    ));
                    self.index += 1;
                }
                ParsedOption::ValueSeparate => {
                    if self.index + 1 >= self.args.len() {
                        return Err(
                            self.failure(format!("cargo global option `{text}` requires a value"))
                        );
                    }
                    self.trace(format!(
                        "accepted cargo global short option `{text}` with next argument as value"
                    ));
                    self.index += 2;
                }
                ParsedOption::Unknown => {
                    return Err(self.failure(format!("unknown cargo global short option `{text}`")));
                }
            }
        }

        self.trace("reached end of argv before cargo subcommand");
        Ok(None)
    }

    fn scan_command_args(&mut self) -> Result<CommandArgs, ParseFailure> {
        let mut command_args = CommandArgs::default();

        while self.index < self.args.len() {
            let arg = self.args[self.index].clone();

            if arg == OsStr::new("--") {
                self.trace(
                    "found `--`; remaining arguments belong to executed program/test harness",
                );
                return Ok(command_args);
            }

            let Some(text) = arg.to_str() else {
                return Err(self.failure("command argument is not valid UTF-8"));
            };

            if text == "--package" {
                self.trace("detected forbidden `--package` selector");
                command_args.package = Some(DeniedPackageSelection {
                    flag: text.to_owned(),
                    value: self.args.get(self.index + 1).map(display_os),
                });
                self.index += if self.index + 1 < self.args.len() {
                    2
                } else {
                    1
                };
                continue;
            }

            if let Some(value) = text.strip_prefix("--package=") {
                self.trace("detected forbidden `--package=...` selector");
                command_args.package = Some(DeniedPackageSelection {
                    flag: "--package".to_owned(),
                    value: Some(value.to_owned()),
                });
                self.index += 1;
                continue;
            }

            if text == "-p" {
                self.trace("detected forbidden `-p` selector");
                command_args.package = Some(DeniedPackageSelection {
                    flag: text.to_owned(),
                    value: self.args.get(self.index + 1).map(display_os),
                });
                self.index += if self.index + 1 < self.args.len() {
                    2
                } else {
                    1
                };
                continue;
            }

            if let Some(value) = text.strip_prefix("-p")
                && !value.is_empty()
            {
                self.trace("detected forbidden `-p...` selector");
                command_args.package = Some(DeniedPackageSelection {
                    flag: "-p".to_owned(),
                    value: Some(value.to_owned()),
                });
                self.index += 1;
                continue;
            }

            if text.starts_with("--") {
                if text == "--target" {
                    if self.index + 1 >= self.args.len() {
                        return Err(
                            self.failure(format!("command option `{text}` requires a value"))
                        );
                    }
                    self.trace("accepted command `--target`; target-specific build forwards");
                    command_args.has_target = true;
                    self.index += 2;
                    continue;
                }

                if text.starts_with("--target=") {
                    self.trace("accepted command `--target=...`; target-specific build forwards");
                    command_args.has_target = true;
                    self.index += 1;
                    continue;
                }

                match parse_long_command_option(text) {
                    ParsedOption::Flag => {
                        self.trace(format!("accepted command flag `{text}`"));
                        self.index += 1;
                    }
                    ParsedOption::ValueInline => {
                        self.trace(format!(
                            "accepted command option with inline value `{text}`"
                        ));
                        self.index += 1;
                    }
                    ParsedOption::ValueSeparate => {
                        if self.index + 1 >= self.args.len() {
                            return Err(
                                self.failure(format!("command option `{text}` requires a value"))
                            );
                        }
                        self.trace(format!(
                            "accepted command option `{text}` with next argument as value"
                        ));
                        self.index += 2;
                    }
                    ParsedOption::Unknown => {
                        return Err(self.failure(format!("unknown command option `{text}`")));
                    }
                }
                continue;
            }

            if text.starts_with('-') && text.len() > 1 {
                match parse_short_command_option(text, self.args.get(self.index + 1)) {
                    ShortCommandOption::Flag => {
                        self.trace(format!("accepted command short flag `{text}`"));
                        self.index += 1;
                    }
                    ShortCommandOption::Value => {
                        if self.index + 1 >= self.args.len() {
                            return Err(
                                self.failure(format!("command option `{text}` requires a value"))
                            );
                        }
                        self.trace(format!(
                            "accepted command short option `{text}` with next argument as value"
                        ));
                        self.index += 2;
                    }
                    ShortCommandOption::InlineValue => {
                        self.trace(format!(
                            "accepted command short option with inline value `{text}`"
                        ));
                        self.index += 1;
                    }
                    ShortCommandOption::PackageSelection(package) => {
                        self.trace(format!("detected forbidden `{text}` selector"));
                        command_args.package = Some(package);
                        self.index += 1;
                    }
                    ShortCommandOption::Unknown => {
                        return Err(self.failure(format!("unknown command short option `{text}`")));
                    }
                }
                continue;
            }

            self.trace(format!("accepted command positional `{text}`"));
            self.index += 1;
        }

        Ok(command_args)
    }

    fn trace(&mut self, action: impl Into<String>) {
        self.trace.push(ParseTraceEntry {
            state: self.state,
            index: self.index,
            arg: self.args.get(self.index).map(display_os),
            action: action.into(),
        });
    }

    fn failure(&mut self, reason: impl Into<String>) -> ParseFailure {
        let reason = reason.into();
        self.trace(format!("cannot parse: {reason}"));
        ParseFailure {
            state: self.state,
            index: self.index,
            reason,
            args: self.args.iter().map(display_os).collect(),
            trace: self.trace.clone(),
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct CommandArgs {
    package: Option<DeniedPackageSelection>,
    has_target: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ParsedOption {
    Flag,
    ValueInline,
    ValueSeparate,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum ShortCommandOption {
    Flag,
    Value,
    InlineValue,
    PackageSelection(DeniedPackageSelection),
    Unknown,
}

fn parse_long_global_option(option: &str) -> ParsedOption {
    let name = option.split_once('=').map_or(option, |(name, _)| name);

    match name {
        "color" | "config" | "explain" | "jobs" | "lockfile-path" | "manifest-path"
        | "message-format" | "target" | "target-dir" => {
            if option.contains('=') {
                ParsedOption::ValueInline
            } else {
                ParsedOption::ValueSeparate
            }
        }
        "frozen" | "help" | "list" | "locked" | "offline" | "quiet" | "verbose" | "version" => {
            if option.contains('=') {
                ParsedOption::Unknown
            } else {
                ParsedOption::Flag
            }
        }
        _ => ParsedOption::Unknown,
    }
}

fn parse_short_global_option(text: &str) -> ParsedOption {
    let Some(short) = text.strip_prefix('-') else {
        return ParsedOption::Unknown;
    };

    if short.is_empty() || short.starts_with('-') {
        return ParsedOption::Unknown;
    }

    if short.chars().all(|ch| matches!(ch, 'h' | 'q' | 'v' | 'V')) {
        return ParsedOption::Flag;
    }

    let mut chars = short.chars();
    let Some(first) = chars.next() else {
        return ParsedOption::Unknown;
    };

    if matches!(first, 'C' | 'Z' | 'j') {
        if chars.next().is_some() {
            ParsedOption::ValueInline
        } else {
            ParsedOption::ValueSeparate
        }
    } else {
        ParsedOption::Unknown
    }
}

fn parse_long_command_option(text: &str) -> ParsedOption {
    let Some(option) = text.strip_prefix("--") else {
        return ParsedOption::Unknown;
    };

    let name = option.split_once('=').map_or(option, |(name, _)| name);

    match name {
        "all"
        | "all-targets"
        | "all-features"
        | "bins"
        | "benches"
        | "check"
        | "examples"
        | "frozen"
        | "help"
        | "lib"
        | "locked"
        | "no-default-features"
        | "no-fail-fast"
        | "no-run"
        | "offline"
        | "quiet"
        | "release"
        | "tests"
        | "verbose"
        | "workspace"
        | "force" => {
            if option.contains('=') {
                ParsedOption::Unknown
            } else {
                ParsedOption::Flag
            }
        }
        "bench" | "bin" | "branch" | "color" | "config" | "example" | "exclude" | "features"
        | "git" | "index" | "jobs" | "manifest-path" | "message-format" | "path" | "profile"
        | "registry" | "rev" | "root" | "tag" | "target" | "target-dir" | "test" | "version" => {
            if option.contains('=') {
                ParsedOption::ValueInline
            } else {
                ParsedOption::ValueSeparate
            }
        }
        _ => ParsedOption::Unknown,
    }
}

fn parse_short_command_option(text: &str, next: Option<&OsString>) -> ShortCommandOption {
    let Some(short) = text.strip_prefix('-') else {
        return ShortCommandOption::Unknown;
    };

    if short.is_empty() || short.starts_with('-') {
        return ShortCommandOption::Unknown;
    }

    let mut chars = short.char_indices();
    while let Some((byte_index, ch)) = chars.next() {
        match ch {
            'p' => {
                let value = &short[byte_index + ch.len_utf8()..];
                return ShortCommandOption::PackageSelection(DeniedPackageSelection {
                    flag: "-p".to_owned(),
                    value: if value.is_empty() {
                        next.map(display_os)
                    } else {
                        Some(value.to_owned())
                    },
                });
            }
            'h' | 'q' | 'r' | 'v' => continue,
            'F' | 'Z' | 'j' => {
                if chars.next().is_some() {
                    return ShortCommandOption::InlineValue;
                }
                return ShortCommandOption::Value;
            }
            _ => return ShortCommandOption::Unknown,
        }
    }

    ShortCommandOption::Flag
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
           cargo check --workspace\n\
           cargo clippy --workspace -- -D warnings\n\
           cargo nextest run --workspace --no-fail-fast\n\n\
         If the full workspace is broken, report the full failing set and fix it. \
         Do not hide it with -p or --package.\n",
    );

    message
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
    fn forwards_workspace_command() {
        assert_eq!(
            classify_invocation(&args(&["check", "--workspace"])),
            InvocationDecision::Forward
        );
    }

    #[test]
    fn denies_cargo_test_without_package_selection() {
        assert_eq!(
            classify_invocation(&args(&["test"])),
            InvocationDecision::Deny(DeniedInvocation::CargoTest { package: None })
        );
    }

    #[test]
    fn denies_cargo_test_with_package_selection() {
        assert_eq!(
            classify_invocation(&args(&["test", "-p", "demo"])),
            InvocationDecision::Deny(DeniedInvocation::CargoTest {
                package: Some(DeniedPackageSelection {
                    flag: "-p".to_owned(),
                    value: Some("demo".to_owned())
                })
            })
        );
    }

    #[test]
    fn denies_long_package_with_separate_value() {
        assert_eq!(
            classify_invocation(&args(&["check", "--package", "demo"])),
            InvocationDecision::Deny(DeniedInvocation::PackageSelection(DeniedPackageSelection {
                flag: "--package".to_owned(),
                value: Some("demo".to_owned())
            }))
        );
    }

    #[test]
    fn denies_short_package_with_attached_value() {
        assert_eq!(
            classify_invocation(&args(&["nextest", "run", "-pdemo"])),
            InvocationDecision::Deny(DeniedInvocation::PackageSelection(DeniedPackageSelection {
                flag: "-p".to_owned(),
                value: Some("demo".to_owned())
            }))
        );
    }

    #[test]
    fn denies_short_package_in_flag_cluster_with_separate_value() {
        assert_eq!(
            classify_invocation(&args(&["nextest", "run", "-vp", "demo"])),
            InvocationDecision::Deny(DeniedInvocation::PackageSelection(DeniedPackageSelection {
                flag: "-p".to_owned(),
                value: Some("demo".to_owned())
            }))
        );
    }

    #[test]
    fn allows_program_args_after_double_dash() {
        assert_eq!(
            classify_invocation(&args(&["run", "--", "-p", "demo"])),
            InvocationDecision::Forward
        );
    }

    #[test]
    fn ignores_attached_values_for_other_short_flags() {
        assert_eq!(
            classify_invocation(&args(&["check", "-Zpanic-abort-tests"])),
            InvocationDecision::Forward
        );
        assert_eq!(
            classify_invocation(&args(&["check", "-Fparallel"])),
            InvocationDecision::Forward
        );
    }

    #[test]
    fn finds_test_subcommand_after_global_options() {
        assert_eq!(
            classify_invocation(&args(&["--color", "always", "-q", "test"])),
            InvocationDecision::Deny(DeniedInvocation::CargoTest { package: None })
        );
        assert_eq!(
            classify_invocation(&args(&["+nightly", "-Zunstable-options", "test"])),
            InvocationDecision::Deny(DeniedInvocation::CargoTest { package: None })
        );
        assert_eq!(
            classify_invocation(&args(&["--frozen", "test"])),
            InvocationDecision::Deny(DeniedInvocation::CargoTest { package: None })
        );
    }

    #[test]
    fn package_selection_on_other_commands_keeps_workspace_subset_diagnostic() {
        assert_eq!(
            classify_invocation(&args(&["check", "-p", "demo"])),
            InvocationDecision::Deny(DeniedInvocation::PackageSelection(DeniedPackageSelection {
                flag: "-p".to_owned(),
                value: Some("demo".to_owned())
            }))
        );
    }

    #[test]
    fn target_specific_package_selection_forwards_with_separate_target_value() {
        assert_eq!(
            classify_invocation(&args(&[
                "build",
                "-p",
                "demo",
                "--target",
                "wasm32-unknown-unknown"
            ])),
            InvocationDecision::Forward
        );
    }

    #[test]
    fn target_specific_package_selection_forwards_with_inline_target_value() {
        assert_eq!(
            classify_invocation(&args(&[
                "build",
                "--target=wasm32-unknown-unknown",
                "--package",
                "demo"
            ])),
            InvocationDecision::Forward
        );
    }

    #[test]
    fn target_specific_cargo_test_forwards() {
        assert_eq!(
            classify_invocation(&args(&[
                "test",
                "-p",
                "demo",
                "--target",
                "wasm32-unknown-unknown"
            ])),
            InvocationDecision::Forward
        );
    }

    #[test]
    fn install_from_path_forwards() {
        assert_eq!(
            classify_invocation(&args(&[
                "install",
                "--path",
                ".",
                "--root",
                "/Users/amos/.local/cargo-wrapper",
                "--force"
            ])),
            InvocationDecision::Forward
        );
    }

    #[test]
    fn unknown_global_option_fails_closed_with_trace() {
        let InvocationDecision::CannotParse(failure) =
            classify_invocation(&args(&["--mystery", "check"]))
        else {
            panic!("expected parse failure");
        };

        assert_eq!(failure.state, ParseState::CargoGlobal);
        assert_eq!(failure.index, 0);
        assert_eq!(failure.args, ["--mystery", "check"]);
        assert!(failure.reason.contains("unknown cargo global option"));
        assert_eq!(failure.trace.len(), 1);
    }

    #[test]
    fn unknown_command_option_fails_closed_with_trace() {
        let InvocationDecision::CannotParse(failure) =
            classify_invocation(&args(&["check", "--mystery"]))
        else {
            panic!("expected parse failure");
        };

        assert_eq!(failure.state, ParseState::CargoCommand);
        assert_eq!(failure.index, 1);
        assert!(failure.reason.contains("unknown command option"));
        assert!(
            failure
                .trace
                .iter()
                .any(|entry| entry.action == "entered command `check`")
        );
    }
}
