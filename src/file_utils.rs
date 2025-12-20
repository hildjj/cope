use std::collections::BTreeSet;
use std::env;
use std::ffi::OsStr;
use std::fs;
use std::iter::{empty, once, Iterator};
use std::path::{Component, Path, PathBuf};
use std::boxed::Box;

/// Normalize a string into a fully-qualified path that has no . or .. in it.
pub fn normalize(input: &OsStr) -> PathBuf {
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

/// If Is there a directory called dir in pth?
pub fn has_dir(pth: &Path, dir: &str) -> bool {
    pth.join(dir).is_dir()
}

pub struct FindOptions<'a> {
    pub dir: &'a str,
    pub stop: Option<&'a str>,
}

/// Look for the given directory, starting at pth, searching each parent
/// directory. If desired, return None when stop directory found.
pub fn find_dir_up(pth: &Path, opts: FindOptions) -> Option<PathBuf> {
    let mut seen: BTreeSet<String> = BTreeSet::new();
    let mut root = pth.to_path_buf();

    for _comp in pth.components().rev() {
        let s = root.display().to_string();
        // Prevent loops, including the root.
        if seen.contains(&s) {
            break;
        }
        seen.insert(s);

        let dc = root.join(opts.dir);
        if dc.is_dir() {
            return Some(dc);
        }

        match opts.stop {
            // Found .git before .devcontainer, which means we are unlikely to
            // be in a devcontainer directory.
            Some(stop) => {
                if has_dir(&root, stop) {
                    break;
                }
            }
            None => {}
        }

        if !root.pop() {
            break;
        }
    }
    None
}

/// Search this directory, and all subdirs (but just one level!) for file_name.
pub fn files_matching<'a>(dir: &'a Path, file_name: &'a str) -> Box<dyn Iterator<Item = PathBuf> + 'a> {
    if dir.is_dir() {
        let subs = fs::read_dir(dir)
            .expect("Error reading directory")
            .filter_map(|f| {
                let p = f.expect("Bad path").path();
                p.is_dir().then_some(p)
            });
        let res = once(dir.into())
            .chain(subs)
            .filter_map(move |f| {
                let p = f.join(file_name);
                p.is_file().then_some(p)
            });
        Box::new(res)
    } else {
        Box::new(empty::<PathBuf>())
    }
}

#[cfg(test)]
mod tests {
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
    fn test_has_dir() {
        assert!(has_dir(&normalize(OsStr::new(".")), "src"));
    }

    #[test]
    fn test_files_matching() {
        let cur = normalize(OsStr::new(std::file!()));
        let dc = find_dir_up(
            &cur,
            FindOptions {
                dir: ".devcontainer",
                stop: None,
            },
        )
        .expect("No devcontainer");

        let res: Vec<PathBuf> = files_matching(&dc, "devcontainer.json").collect();
        assert_eq!(res.len(), 1);
    }

    #[test]
    fn test_not_dir() {
        let cur = normalize(OsStr::new(std::file!()));
        let ret: Vec<PathBuf> = files_matching(&cur, "NO_SUCH_FILE").collect();
        let expected: Vec<PathBuf> = vec![];
        assert_eq!(ret, expected);
    }    

    #[test]
    fn test_no_container() {
        let cur = normalize(OsStr::new(std::file!()));
        let dc = find_dir_up(
            &cur,
            FindOptions {
                dir: "___BAD_DIR_DOESNT_EXIT_____HOPEFULLY...",
                stop: None,
            },
        );
        assert_eq!(dc, None);
    }

    #[test]
    fn test_stop_dir() {
        let cur = normalize(OsStr::new(std::file!()));
        let dc = find_dir_up(
            &cur,
            FindOptions {
                dir: "___BAD_DIR_DOESNT_EXIT_____HOPEFULLY...",
                stop: Some(".git"),
            },
        );
        assert_eq!(dc, None);
    }    
}
