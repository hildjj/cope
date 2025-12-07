use nix::unistd::execvp;
use std::collections::BTreeSet;
use std::env;
use std::ffi::{CStr, CString};
use std::fmt::Write as FmtWrite;
use std::iter::once;
use std::path::{Component, Path, PathBuf};

const CODE: &CStr = c"code";

struct Cli<'a> {
    ddash: bool,
    prev: bool,
    flags: BTreeSet<&'a str>,
}

impl Cli<'_> {
    fn new<'a>() -> Cli<'a> {
        Cli {
            ddash: false,
            prev: false,
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
    fn convert_path(&mut self, input: String) -> CString {
        let p = normalize(&input);
        let mut seen: BTreeSet<String> = BTreeSet::new();
        let mut root = p.clone();
        let mut found_devcontainer = false;

        // If there is a --, stop processing flags
        if !self.ddash {
            if input.eq("--") {
                self.ddash = true;
                self.prev = false;
                return to_cstring(input);
            }
            if input.starts_with("-") {
                if self.flags.contains(input.as_str()) {
                    self.prev = true;
                }
                return to_cstring(input);
            }
            if self.prev {
                self.prev = false;
                return to_cstring(input);
            }
        }
        self.prev = false;

        // TODO: Need special handling for:
        // -g --goto

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
            to_cstring(format!(
                "--{}-uri=vscode-remote://dev-container+{}/workspaces/{}",
                if p.is_dir() { "folder" } else { "file" },
                hx,
                p.strip_prefix(root).expect("stripping prefix").display()
            ))
        } else {
            to_cstring(input)
        }
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
        .chain(
            env::args()
                .skip(1)
                .map(|arg| cli.convert_path(arg)),
        )
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

    #[test]
    fn test_convert_path() {
        let mut cli = Cli::new();
        let params: Vec<String> = [
            "Cargo.toml",
            "/",
            "--log", 
            "info", 
            "--",
        ]
        .iter()
        .map(|&s| s.into())
        .map(|s| cli.convert_path(s))
        .map(|cs| cs.into_string().unwrap())
        .collect();
        assert!(params[0].starts_with("--file-uri=vscode-remote://dev-container+"));
        let expected: Vec<String> = ["/", "--log", "info", "--"]
            .iter()
            .map(|&s| s.into())
            .collect();
        assert_eq!(params[1..], expected);
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
