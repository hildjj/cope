use nix::unistd::execvp;
use std::collections::BTreeSet;
use std::env;
use std::ffi::{CStr, CString, OsStr, OsString};
use std::fmt::Write as FmtWrite;
use std::os::unix::ffi::OsStrExt;
use std::path::{Component, Path, PathBuf};
use phf::phf_map;

const CODE: &CStr = c"code";
static PARAM_SIZE: phf::Map<&'static str, usize> = phf_map! {
    "--add-mcp" => 1,
    "--add" => 1,
    "--category" => 1,
    "--diff" => 2,
    "--disable-extension" => 1,
    "--enable-proposed-api" => 1,
    "--extensions-dir" => 1,
    "--goto" => 1,
    "--inspect-brk-extensions" => 1,
    "--inspect-extensions" => 1,
    "--install-extension" => 1,
    "--locale" => 1,
    "--locate-shell-integration-path" => 1,
    "--log" => 1,
    "--merge" => 4,
    "--profile" => 1,
    "--remove" => 1,
    "--sync" => 1,
    "--uninstall-extension" => 1,
    "--user-data-dir" => 1,
    "-a" => 1,
    "-d" => 2,
    "-g" => 1,
    "-m" => 4,
};

/// Normalize a string into a fully-qualified path that has no . or .. in it.
fn normalize(input: &OsStr) -> PathBuf {
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

fn to_devcontainer_uri(arg: &OsStr) -> CString {
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

        // Found .git before .devcontainer, which means we are unlikely to
        // be in a devcontainer directory.
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
        CString::new(format!(
            "--{}-uri=vscode-remote://dev-container+{}/workspaces/{}",
            if p.is_dir() { "folder" } else { "file" },
            hx,
            p.strip_prefix(root).expect("stripping prefix").display()
        )).expect("Bad CString from format")
    } else {
        CString::new(arg.as_bytes()).expect("Bad CString from OsString")
    }
}

fn process_args(args: impl ExactSizeIterator<Item = OsString>) -> Vec<CString> {
    let mut result: Vec<CString> = Vec::with_capacity(args.len());
    let mut it = args.into_iter();
    
    result.push(CODE.to_owned());
    it.next().expect("Always expect 'cope' as the 0th param");
    while let Some(a) = it.next() {
        if let Some(b) = a.clone().to_str() {
            if let Some(sz) = PARAM_SIZE.get(b) {
                result.push(to_cstring(a));
                // If we don't have enough parameters, `code` will complain
                // for us, so no need to check that we have enough.
                result.extend(it.by_ref().take(*sz).map(to_cstring));
            } else if b == "--" {
                result.push(to_cstring(a));
                result.extend(it.by_ref().map(|s| to_devcontainer_uri(s.as_os_str())));
                break;
            } else if b.starts_with("--") {
                // Other parameters are passed through unmodified, 
                // and they don't have follow-on parameters.
                result.push(to_cstring(a));
            } else if b.starts_with("-") {
                if (b.len() > 2) && (
                    b.contains('a') || b.contains('d') || b.contains('g') || b.contains('m')
                ) {
                    eprintln!("cope does not handle coalesced single letter flags with parameters cleanly yet")
                }
                // Single-letter parameters, skipped
                result.push(to_cstring(a));
            } else {
                // This must be a filename, since everything else will
                // have been caught above.
                result.push(to_devcontainer_uri(a.as_os_str()));
            }
        } else {
            // Invalid UTF-8, can still be used as a path.  It can't be a
            // valid parameter flag.
            result.push(to_devcontainer_uri(a.as_os_str()));
        }
    }

    // TODO: handle chat, serve-web, and tunnel

    debug_args(env::var("COPE_VERBOSE").is_ok(), &result);
    result
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

// fn to_cstring(s: String) -> CString {
//     CString::new(s).expect("Creating CString")
// }

fn to_cstring(s: OsString) -> CString {
    CString::new(s.as_bytes()).expect("Creating CString")
}

fn debug_args(write: bool, args: &[CString]) {
    if write {
        args.iter().for_each(|a| eprint!("{:?} ", a));
        eprintln!();
    }
}

fn main() {    
    // Just exec here, rather than doing a fork.  This allows the existing
    // stdin and stdout to work, along with their existing pty's.
    match execvp(CODE, &process_args(env::args_os())) {
        Err(_) => eprintln!("execvp failed launching {:?}", CODE),
    }
}

#[cfg(test)]
mod tests {
    use std::os::unix::ffi::OsStringExt;

    use super::*;

    #[test]
    fn test_normalize() {
        assert_eq!(normalize(OsStr::new("/foo")), PathBuf::from("/foo"));
        assert_eq!(
            normalize(OsStr::new("./foo/./.././Cargo.toml")),
            env::current_dir().unwrap().join("Cargo.toml")
        );
    }

    #[test]
    fn test_hex() {
        assert_eq!(hex("\x01\x7f".to_string()), "017f");
    }

    #[test]
    fn test_has_dir() {
        assert!(has_dir(&normalize(OsStr::new(".")), "src"));
    }

    fn convert_args(args: &[&str]) -> Vec<String> {
        let oa: Vec<OsString> = args.iter().map(|&s| OsString::from(s)).collect();
        process_args(oa.into_iter())
            .iter()
            .map(|s| s.to_str().unwrap().into())
            .collect()
    }

    fn assert_file_uri(arg: &str) {
        assert!(
            arg.to_owned()
                .starts_with("--file-uri=vscode-remote://dev-container+"),
            "{:?}", 
            arg
        );
    }

    #[test]
    fn test_convert_path() {
        let actual = convert_args(&["cope", "Cargo.toml", "/", "--log", "info", "--", "--log"]);

        assert_eq!(&actual[0], "code");
        assert_file_uri(&actual[1]);
        let last = actual.len() - 1;
        assert_file_uri(&actual[last]);
        let expected: Vec<String> = ["/", "--log", "info", "--"]
            .iter()
            .map(|&s| s.into())
            .collect();
        assert_eq!(actual[2..last], expected);
    }

    #[test]
    fn test_invalid_trailing_needfile() {
        let actual = convert_args(&["cope", "--log"]);
        // Expect eprintln
        assert_eq!(actual[1], "--log");
    }

    #[test]
    fn test_needfile() {
        let actual = convert_args(&["cope", "-d", "one", "two"]);
        // Expect eprintln
        assert_eq!(actual[1], "-d");
        assert_eq!(actual[2], "one");
        assert_eq!(actual[3], "two");
    }

    #[test]
    fn test_unknown_ddash() {
        let actual = convert_args(&["cope", "--foo"]);
        assert_eq!(actual[1], "--foo");
    }

    #[test]
    fn test_multi_sdash() {
        let actual = convert_args(&["cope", "-wa", "foo"]);
        // Expect eprintf
        assert_file_uri(&actual[2]);
    }

    #[test]
    fn test_invalid_utf8() {
        let good = OsString::from("cope");
        let bad = OsString::from_vec(vec![0xff]);

        let oa = vec![good, bad];
        let actual = process_args(oa.into_iter());
        assert_file_uri(actual[1].to_str().unwrap());
    }

    #[test]
    fn test_to_cstring() {
        let res = to_cstring(OsString::from("foo"));
        assert_eq!(res, c"foo".to_owned());
    }

    #[test]
    fn test_debug() {
        debug_args(true, &[c"foo".to_owned()]);
    }

    #[test]
    #[should_panic]
    fn test_to_cstring_fail() {
        to_cstring(OsString::from("foo\0"));
    }
}
