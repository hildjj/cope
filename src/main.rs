use std::collections::BTreeSet;
use std::env;
use std::fmt::Write as FmtWrite;
use std::path::{Component, Path, PathBuf};
use std::process::{Command, Stdio};

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

fn convert_path(
    input: String,
    ddash: &mut bool,
    prev: &mut bool,
    param_flags: &BTreeSet<String>,
) -> String {
    let p = normalize(&input);
    let mut seen: BTreeSet<String> = BTreeSet::new();
    let mut root = p.clone();
    let mut found = false;

    if !*ddash {
        if input.eq("--") {
            *ddash = true;
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
    // TODO: Need special handling for:
    // -g --goto

    for _comp in p.components().rev() {
        let s = root.display().to_string();
        // Prevent loops, including the root.
        if seen.contains(&s) {
            break;
        }
        seen.insert(s);

        root.push(".devcontainer");
        if root.is_dir() {
            found = true;
            root.pop();
            break;
        }
        root.pop();

        root.push(".git");
        if root.is_dir() {
            break;
        }
        root.pop();

        if !root.pop() {
            break;
        }
    }
    if found {
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

fn main() {
    let mut ddash = false;
    let mut prev = false;

    // TODO: I would rather this be thread-local global, so it doesn't
    // need to be passed around.
    let param_flags = bset! {
        "--locale",
        "--profile",
        "--category",
        "--install-extension", // No install from dir in container
        "--uninstall-extension",
        "--enable-proposed-api",
        "--add-mcp",
        "--log",
        "--disable-extension",
        "--inspect-extensions",
        "--inspect-brk-extensions",
        "--locate-shell-integration-path"
    };

    // TODO: handle chat, serve-web, and tunnel

    let args: Vec<String> = env::args()
        .skip(1)
        .map(|arg| convert_path(arg, &mut ddash, &mut prev, &param_flags))
        .collect();
    Command::new("code")
        .args(args)
        .stdout(Stdio::inherit())
        .stdin(Stdio::inherit())
        .stderr(Stdio::inherit())
        .spawn()
        .expect("Spawning 'code'");
}
