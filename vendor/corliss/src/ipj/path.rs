//! Resolving project paths (spec 19.4): a relative path is joined to the folder of the .ipj
//! with a backslash and canonicalized, as Inventor's `GetFullPathForProjectPath` does [R-15].
//! Paths stay Windows paths whatever the host, because they name folders on Inventor's machine.

/// The full Windows path of project path `path` for a project file in folder `project_dir`.
/// Absolute paths (`C:\…`, `\\server\…`) are kept and canonicalized; a rooted path (`\X`)
/// takes the drive of `project_dir`. `.` and `..` segments are folded, `/` becomes `\`, and
/// a trailing separator is kept.
///
/// ```
/// use corliss::ipj::resolve_path;
/// assert_eq!(resolve_path(r"C:\Work\Design", "."), r"C:\Work\Design");
/// assert_eq!(resolve_path(r"C:\Work\Design", r"..\Library"), r"C:\Work\Library");
/// assert_eq!(resolve_path(r"C:\Work\Design", r"D:\Std\"), r"D:\Std\");
/// assert_eq!(resolve_path(r"C:\Work\Design", r"\\srv\share\x"), r"\\srv\share\x");
/// ```
pub fn resolve_path(project_dir: &str, path: &str) -> String {
    let path = path.replace('/', "\\");
    let joined = if is_absolute(&path) {
        path
    } else if let Some(rooted) = path.strip_prefix('\\') {
        format!("{}\\{rooted}", root_of(&project_dir.replace('/', "\\")))
    } else {
        format!(
            "{}\\{path}",
            project_dir.replace('/', "\\").trim_end_matches('\\')
        )
    };
    canonicalize(&joined)
}

/// `X:\…` or a UNC path `\\server\share…`.
fn is_absolute(p: &str) -> bool {
    let b = p.as_bytes();
    p.starts_with("\\\\")
        || (b.len() >= 3 && b[0].is_ascii_alphabetic() && b[1] == b':' && b[2] == b'\\')
}

/// The part `..` cannot climb above: `X:` or `\\server\share`.
fn root_of(p: &str) -> String {
    if let Some(unc) = p.strip_prefix("\\\\") {
        let parts: Vec<&str> = unc.splitn(3, '\\').collect();
        return format!("\\\\{}", parts[..parts.len().min(2)].join("\\"));
    }
    p.get(..2)
        .filter(|d| d.ends_with(':'))
        .unwrap_or_default()
        .to_string()
}

/// Folds `.` and `..` below the root; keeps a trailing separator.
fn canonicalize(p: &str) -> String {
    let root = root_of(p);
    let mut parts: Vec<&str> = Vec::new();
    for seg in p[root.len()..].split('\\').filter(|s| !s.is_empty()) {
        match seg {
            "." => {}
            ".." => {
                parts.pop();
            }
            s => parts.push(s),
        }
    }
    let trailing = if p.ends_with('\\') && !parts.is_empty() {
        "\\"
    } else {
        ""
    };
    format!("{root}\\{}{trailing}", parts.join("\\"))
}
