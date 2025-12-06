use nix::unistd::execvp;
use std::collections::BTreeSet;
use std::env;
use std::ffi::{CStr, CString};
use std::fmt::Write as FmtWrite;
use std::iter::once;
use std::path::{Component, Path, PathBuf};

const CODE: &CStr = c"code";

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

fn has_dir(pth: &PathBuf, dir: &str) -> bool {
    let mut copy = pth.clone();
    copy.push(dir);
    copy.is_dir()
}

/// Convert a path to a vscode-remote://dev-container URI with the appropriate
/// flag (--file-uri= or --folder-uri) if needed, otherwise return the original
/// parameter.
fn convert_path(
    input: String,
    ddash: &mut bool,
    prev: &mut bool,
    param_flags: &BTreeSet<String>,
) -> String {
    let p = normalize(&input);
    let mut seen: BTreeSet<String> = BTreeSet::new();
    let mut root = p.clone();
    let mut found_devcontainer = false;

    // If there is a --, stop processing flags
    if !*ddash {
        if input.eq("--") {
            *ddash = true;
            *prev = false;
            return input;
        }
        if input.starts_with("-") {
            if param_flags.contains(&input) {
                *prev = true;
            }
            return input;
        }
        if *prev {
            *prev = false;
            return input;
        }
    }
    *prev = false;

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
        format!(
            "--{}-uri=vscode-remote://dev-container+{}/workspaces/{}",
            if p.is_dir() { "folder" } else { "file" },
            hx,
            p.strip_prefix(root).expect("stripping prefix").display()
        )
    } else {
        input
    }
}

// Cargo-cult copied from
// https://docs.rs/literally/0.1.3/src/literally/lib.rs.html#95
macro_rules! bset {
    ( $($key:expr),* $(,)? ) => {
        {
            let mut _set = ::std::collections::BTreeSet::new();
            $(
                _set.insert($key.into());
            )*
            _set
        }
    };
}

fn to_cstring(s: String) -> CString {
    CString::new(s).expect("Creating CString")
}

fn main() {
    let mut ddash = false;
    let mut prev = false;

    // TODO: I would rather this be thread-local global, so it doesn't
    // need to be passed around.
    let param_flags = bset! {
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
    };

    // TODO: handle chat, serve-web, and tunnel

    // The first arg must match the exec filename
    let args: Vec<CString> = once(CODE.to_owned())
        .chain(
            env::args()
                .skip(1)
                .map(|arg| to_cstring(convert_path(arg, &mut ddash, &mut prev, &param_flags))),
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
            PathBuf::from(env::current_dir().unwrap()).join("Cargo.toml")
        );
    }

    #[test]
    fn test_hex() {
        assert_eq!(hex("\x01\x7f".to_string()), "017f");
    }

    #[test]
    fn test_has_dir() {
        assert_eq!(has_dir(&normalize(&".".to_string()), "src"), true);
    }

    #[test]
    fn test_convert_path() {
        let norm = normalize(&".".to_string()).to_str().unwrap().to_string();
        let mut ddash = false;
        let mut prev = false;
        let param_flags = bset! {
            "--log",
        };

        let params: Vec<String> = vec![
            norm.as_str(),
            "--log",
            "info",
            "--",
            "--log",
            "foo"
        ].iter()
        .map(|s| convert_path(s.to_string(), &mut ddash, &mut prev, &param_flags))
        .collect();
        let expected: Vec<String> = vec![
            norm.as_str(),
            "--log",
            "info",
            "--",
            "--log",
            "foo"
        ].iter().map(|s| s.to_string()).collect();
        assert_eq!(params, expected);
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
