use nix::unistd::execvp;
use std::collections::BTreeSet;
use std::env;
use std::ffi::{CStr, CString};
use std::fmt::Write as FmtWrite;
use std::iter::once;
use std::path::{Component, Path, PathBuf};

const CODE: &CStr = c"code";

fn to_devcontainer_uri(arg: &String) -> String {
    let p = normalize(arg);
    let mut seen: BTreeSet<String> = BTreeSet::new();
    let mut root = p.clone();
    let mut found_devcontainer = false;

    for _comp in p.components().rev() {
        let s = root.display().to_string();
        // Prevent loops, including the root.
        if seen.contains(&s) {
            break;
        }
        seen.insert(s);

        if has_dir(&root, ".devcontainer") {
            found_devcontainer = true;
            break;
        }
        if has_dir(&root, ".git") {
            break;
        }

        if !root.pop() {
            break;
        }
    }
    if found_devcontainer {
        let hx = hex(root.display().to_string());
        root.pop();
        format!(
            "--{}-uri=vscode-remote://dev-container+{}/workspaces/{}",
            if p.is_dir() { "folder" } else { "file" },
            hx,
            p.strip_prefix(root).expect("stripping prefix").display()
        )
    } else {
        arg.clone()
    }
}

// State machine for argument processing.
enum ArgState {
    /// Normal argument processing
    Normal,
    /// After seeing `--`, all remaining args are potential paths
    AfterDoubleDash,
    /// Previous arg was a flag that expects a non-filename value
    ExpectingNonFilename(String),
}

struct Cli<'a> {
    flags: BTreeSet<&'a str>,
}

impl Cli<'_> {
    fn new<'a>() -> Cli<'a> {
        Cli {
            flags: BTreeSet::from([
                "--add-mcp",
                "--category",
                "--disable-extension",
                "--enable-proposed-api",
                "--inspect-brk-extensions",
                "--inspect-extensions",
                "--install-extension", // No install from dir in container
                "--locale",
                "--locate-shell-integration-path",
                "--log",
                "--profile",
                "--sync",
                "--uninstall-extension",
            ]),
        }
    }
    fn process_args(&mut self, args: impl IntoIterator<Item = String>) -> Vec<CString> {
        let (result, final_state) = args.into_iter().fold(
            (Vec::new(), ArgState::Normal),
            |(mut result, state), arg| match state {
                // TODO: Add another state for -g --goto
                ArgState::ExpectingNonFilename(_) => {
                    result.push(arg);
                    (result, ArgState::Normal)
                }
                ArgState::AfterDoubleDash => {
                    result.push(to_devcontainer_uri(&arg));
                    (result, ArgState::AfterDoubleDash)
                }
                ArgState::Normal if arg == "--" => {
                    result.push(arg);
                    (result, ArgState::AfterDoubleDash)
                }
                ArgState::Normal if arg.starts_with('-') => {
                    let needs_value = self.flags.contains(&arg.as_str());
                    let next = if needs_value {
                        ArgState::ExpectingNonFilename(arg.clone())
                    } else {
                        ArgState::Normal
                    };
                    result.push(arg);
                    (result, next)
                }
                ArgState::Normal => {
                    result.push(to_devcontainer_uri(&arg));
                    (result, ArgState::Normal)
                }
            },
        );
        if let ArgState::ExpectingNonFilename(flag) = final_state {
            eprintln!("Warning(cope): flag {} expects a value", flag);
        }
        result.into_iter().map(to_cstring).collect()
    }
}

/// Normalize a string into a fully-qualified path that has no . or .. in it.
fn normalize(input: &String) -> PathBuf {
    let p = Path::new(&input);
    let abs = if p.is_absolute() {
        p.to_path_buf()
    } else {
        env::current_dir()
            .expect("failed to get current directory")
            .join(p)
    };
    let mut normalized = PathBuf::new();
    for comp in abs.components() {
        match comp {
            Component::CurDir => {
                // skip
            }
            Component::ParentDir => {
                // pop one component if possible; if at root, ignore ParentDir so it
                // won't escape above the absolute root.
                normalized.pop();
            }
            other => normalized.push(other.as_os_str()),
        }
    }

    normalized
}

fn hex(input: String) -> String {
    let mut res = String::new();
    for b in input.as_bytes() {
        write!(res, "{:02x}", b).expect("String write");
    }
    res
}

fn has_dir(pth: &Path, dir: &str) -> bool {
    pth.join(dir).is_dir()
}

fn to_cstring(s: String) -> CString {
    CString::new(s).expect("Creating CString")
}

fn main() {
    let mut cli = Cli::new();
    // TODO: handle chat, serve-web, and tunnel

    // The first arg must match the exec filename
    let args: Vec<CString> = once(CODE.to_owned())
        .chain(cli.process_args(env::args().skip(1)))
        .collect();

    // Just exec here, rather than doing a fork.  This allows the existing
    // stdin and stdout to work, along with their existing pty's.
    match execvp(CODE, &args) {
        Err(_) => eprintln!("execvp failed launching {:?}", CODE),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_normalize() {
        assert_eq!(normalize(&"/foo".to_string()), PathBuf::from("/foo"));
        assert_eq!(
            normalize(&"./foo/.././Cargo.toml".to_string()),
            env::current_dir().unwrap().join("Cargo.toml")
        );
    }

    #[test]
    fn test_hex() {
        assert_eq!(hex("\x01\x7f".to_string()), "017f");
    }

    #[test]
    fn test_has_dir() {
        assert!(has_dir(&normalize(&".".to_string()), "src"));
    }

    fn convert_args(args: &[&str]) -> Vec<String> {
        let args: Vec<String> = args.iter().map(|&s| s.into()).collect();
        let mut cli = Cli::new();
        cli.process_args(args)
            .iter()
            .map(|cs| cs.clone().into_string().unwrap())
            .collect()
    }

    fn assert_file_uri(arg: impl AsRef<str>) {
        assert!(
            arg.as_ref()
                .starts_with("--file-uri=vscode-remote://dev-container+")
        );
    }

    #[test]
    fn test_convert_path() {
        let actual = convert_args(&["Cargo.toml", "/", "--log", "info", "--", "--log"]);

        assert_file_uri(&actual[0]);
        let last = actual.len() - 1;
        assert_file_uri(&actual[last]);
        let expected: Vec<String> = ["/", "--log", "info", "--"]
            .iter()
            .map(|&s| s.into())
            .collect();
        assert_eq!(actual[1..last], expected);
    }

    #[test]
    fn test_invalid_trailing_needfile() {
        let actual = convert_args(&["--log"]);
        // Expect eprintln
        assert_eq!(actual[0], "--log");
    }

    #[test]
    fn test_needfile() {
        let actual = convert_args(&["-d", "one", "two"]);
        // Expect eprintln
        assert_eq!(actual[0], "-d");
        assert_file_uri(&actual[1]);
        assert_file_uri(&actual[2]);
    }

    #[test]
    fn test_to_cstring() {
        let res = to_cstring("foo".to_owned());
        assert_eq!(res, c"foo".to_owned());
    }

    #[test]
    #[should_panic]
    fn test_to_cstring_fail() {
        to_cstring("foo\0".to_string());
    }
}
