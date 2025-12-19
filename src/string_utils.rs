use std::ffi::{CString, OsString};
use std::fmt::Write as FmtWrite;
use std::os::unix::ffi::OsStrExt;

pub fn to_cstring(s: OsString) -> CString {
    CString::new(s.as_bytes()).expect("Creating CString")
}

pub fn debug_arg(write: bool, a: &str) {
    if write {
        eprint!("{} ", a);
    }
}
pub fn debug_args(write: bool, args: &[CString]) {
    if write {
        args.iter()
            .for_each(|a| debug_arg(true, a.to_str().unwrap()));
        eprintln!();
    }
}

pub fn hex(input: &[u8]) -> String {
    let mut res = String::new();
    for b in input {
        write!(res, "{:02x}", b).expect("String write");
    }
    res
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hex() {
        assert_eq!(hex("\x01\x7f".as_bytes()), "017f");
    }

    #[test]
    fn test_to_cstring() {
        let res = to_cstring(OsString::from("foo"));
        assert_eq!(res, c"foo".to_owned());
    }

    #[test]
    #[should_panic]
    fn test_to_cstring_fail() {
        to_cstring(OsString::from("foo\0"));
    }

    #[test]
    fn test_debug() {
        debug_args(true, &[c"foo".to_owned()]);
    }
}
