mod file_utils;
mod string_utils;

use dialoguer::Select;
use nix::unistd::execvp;
use phf::{phf_map, phf_set};
use serde::Deserialize;
use std::collections::BTreeMap;
use std::env;
use std::ffi::{CStr, CString, OsStr, OsString};
use std::fs;
use std::path::{Path, PathBuf};

use crate::file_utils::FindOptions;
pub use crate::file_utils::{files_matching, find_dir_up, normalize};
pub use crate::string_utils::{debug_arg, debug_args, hex, to_cstring};

const DEVCONTAINER_DIR: &str = ".devcontainer";
const CONFIG_FILE: &str = "devcontainer.json";

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

/// All params after one of these are not file names, as far as I can tell.
static TERMINAL_PARAM: phf::Set<&'static str> = phf_set! [
    "--",
    "chat",
    "serve-web",
    "tunnel"
];

#[derive(Deserialize, Debug)]
struct DevContainer {
    name: Option<String>,

    #[serde(rename = "workspaceFolder")]
    workspace_folder: Option<String>,
}

struct JsonResults {
    pub file_name: PathBuf,
    pub dev_container: DevContainer,
}

struct DirProperties {
    pub hex: String,
    pub folder: String,
}

fn read_json(file_name: PathBuf) -> JsonResults {
    let json_data = fs::read_to_string(file_name.clone())
        .unwrap_or_else(|er| panic!("Error reading file {file_name:?} {er}"));
    let dev_container = serde_jsonc::from_str(&json_data)
        .unwrap_or_else(|er| panic!("Error parsing JSON {file_name:?} {er}"));
    JsonResults {
        file_name: file_name,
        dev_container,
    }
}

/// Ask on stderr which of the given items is desired
fn choose<'a>(matches: &'a Vec<JsonResults>, root: &Path) -> &'a JsonResults {
    // See https://github.com/console-rs/console/pull/173 for testing
    let items = matches.iter().map(|m| {
        format!(
            "{} ({:?})",
            m.dev_container
                .name
                .clone()
                .unwrap_or("<no name>".to_string()),
            m.file_name.strip_prefix(root).expect("Relative to root")
        )
    });

    let selection = Select::new()
        .with_prompt("Which container?")
        .items(items)
        .default(0)
        .interact()
        .expect("Selection failed");
    &matches[selection]
}

/// Convert the given config file into the internal format that the Dev
/// Containers extension expects.  This is an undocumented interface, so I
/// expect it to be brittle. To find examples for this, open the Developer
/// Tools in VScode with Cmd-P, "Developer: Toggle Developer Tools", go to the
/// console, and enter: `window.vscode.context.configuration().workspace.uri`.
/// Fiddle around with the results to find value of the _formatted field, then
/// hex decode.
pub fn container_id(root: &Path, chosen: &Path) -> String {
    // Maintain compatibility with the URI that the `devcontainer` CLI uses,
    // if we are just opening the default config.
    if chosen.eq(&root.join(DEVCONTAINER_DIR).join(CONFIG_FILE)) {
        root.to_string_lossy().into()
    } else {
        // This string is incredibly picky.  It just looks like JSON but it
        // apparently isn't.  All of the weird bits need to be there, in this
        // order, with no additional whitespace.
        format!(
            r#"{{"hostPath":{root:?},"localDocker":false,"settings":{{"context":"desktop-linux"}},"configFile":{{"$mid":1,"fsPath":{chosen:?},"external":"file://{}","path":{chosen:?},"scheme":"file"}}}}"#,
            chosen.to_string_lossy()
        )
    }
}

/// Compute the hex bits for the devcontainer URI, as well as the name of the 
/// project folder *inside* the container.
fn dir_properties(root: &Path) -> Option<DirProperties> {
    let matches: Vec<JsonResults> = files_matching(root, CONFIG_FILE).map(read_json).collect();

    let chosen = match matches.len() {
        0 => {
            // No devcontainer.json found in .devcontainer/
            return None;
        }
        1 => {
            // Only one.  The most common case.
            &matches[0]
        }
        _ => choose(&matches, &root),
    };

    // Remove .devcontainer
    let mut root = root.to_path_buf();
    root.pop();
    let id = container_id(&root, &chosen.file_name);
    debug_arg(env::var("COPE_VERBOSE").is_ok(), &id);
    let hex = hex(id.as_bytes());

    let folder = chosen
        .dev_container
        .workspace_folder
        .clone()
        .unwrap_or_else(|| {
            format!(
                "/workspaces/{}",
                root.iter()
                    .next_back()
                    .expect("Last path segment")
                    .to_string_lossy()
            )
        });
    Some(DirProperties { hex, folder })
}

/// If this is a file in a directory that has a devcontainer, convert it to
/// a vscode-remote: URI.  If not, just convert to a CString.
fn to_devcontainer_uri(
    arg: &OsStr,
    dir: &str,
    cache: &mut BTreeMap<PathBuf, Option<DirProperties>>,
) -> CString {
    let pth = normalize(arg);
    if let Some(mut root) = find_dir_up(
        &pth,
        FindOptions {
            dir,
            stop: Some(".git"),
        },
    ) {
        let cached = cache
            .entry(root.clone())
            .or_insert_with(|| dir_properties(&root));

        if let Some(props) = cached {
            root.pop();
            return CString::new(format!(
                "--{}-uri=vscode-remote://dev-container+{}{}/{}",
                if pth.is_dir() { "folder" } else { "file" },
                props.hex,
                props.folder,
                pth.strip_prefix(root).expect("stripping prefix").display()
            ))
            .expect("Bad CString from format");
        }
    }

    // No .devcontainer/ in the parents, or no devcontainer.json in
    // .devcontainer
    to_cstring(arg.into())
}

/// For each arg, if it might be a file name, see if the file name needs to be
/// converted to a URI.  Otherwise pass the arg through.
fn process_args(args: impl ExactSizeIterator<Item = OsString>) -> Vec<CString> {
    let mut args: Vec<OsString> = args.collect();
    if args.len() == 1 {
        // This is the default in the code CLI, but we need a chance to 
        // permute it into a file URI.
        args.push(OsString::from("."));
    }
    let mut result: Vec<CString> = Vec::with_capacity(args.len());
    let mut it = args.into_iter();

    result.push(CODE.to_owned());
    it.next().expect("Always expect 'cope' as the 0th param");

    // Cache so we don't call `choose` twice for the same directory.
    // The perf is unlikely to matter in practice, but the UX of having to 
    // answer the same question twice is bad.
    let mut cache: BTreeMap<PathBuf, Option<DirProperties>> = BTreeMap::new();
    while let Some(a) = it.next() {
        if let Some(b) = a.clone().to_str() {
            if let Some(sz) = PARAM_SIZE.get(b) {
                result.push(to_cstring(a));
                // If we don't have enough parameters, `code` will complain
                // for us, so no need to check that we have enough.
                result.extend(it.by_ref().take(*sz).map(to_cstring));
            } else if TERMINAL_PARAM.contains(b) {
                // Nothing after a terminal can be processed as a URI. If it's
                // a filename, when the "--" is passed to code, the --file-uri
                // param after it gets treated as a literal filename.
                result.push(to_cstring(a));
                result.extend(it.map(to_cstring));
                break;
            } else if b.starts_with("--") {
                // Other parameters are passed through unmodified,
                // and they don't have follow-on parameters.
                result.push(to_cstring(a));
            } else if b.starts_with("-") {
                if (b.len() > 2)
                    && (b.contains('a') || b.contains('d') || b.contains('g') || b.contains('m'))
                {
                    eprintln!(
                        "cope does not handle coalesced single letter flags with parameters cleanly yet"
                    )
                }
                // Single-letter parameters, skipped
                result.push(to_cstring(a));
            } else {
                // This must be a filename, since everything else will
                // have been caught above.
                result.push(to_devcontainer_uri(
                    a.as_os_str(),
                    DEVCONTAINER_DIR,
                    &mut cache,
                ));
            }
        } else {
            // Invalid UTF-8, can still be used as a path.  It can't be a
            // valid parameter flag.
            result.push(to_devcontainer_uri(
                a.as_os_str(),
                DEVCONTAINER_DIR,
                &mut cache,
            ));
        }
    }

    // TODO: handle chat, serve-web, and tunnel

    debug_args(env::var("COPE_VERBOSE").is_ok(), &result);
    result
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
    use super::*;
    use std::os::unix::ffi::OsStringExt;

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

    fn assert_folder_uri(arg: &str) {
        assert!(
            arg.to_owned()
                .starts_with("--folder-uri=vscode-remote://dev-container+"),
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
        assert_eq!(&actual[last], "--log");
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
    fn test_no_args() {
        let actual = convert_args(&["cope"]);
        assert_folder_uri(&actual[1]);
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
    fn test_complex_container() {
        let id = container_id(
            &PathBuf::from("/foo"),
            &PathBuf::from("/foo/.devcontainer/bar/devcontainer.json"),
        );
        assert_eq!(id.chars().next().unwrap(), '{');
    }

    #[test]
    fn test_empty_dir() {
        let mut cache: BTreeMap<PathBuf, Option<DirProperties>> = BTreeMap::new();
        let u = to_devcontainer_uri(&OsStr::new(std::file!()), "src", &mut cache);
        assert_eq!(u, to_cstring(std::file!().into()));
    }
}
