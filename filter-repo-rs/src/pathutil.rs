#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PathCompatPolicy {
    Sanitize,
    Skip,
    Error,
}

impl Default for PathCompatPolicy {
    fn default() -> Self {
        Self::Sanitize
    }
}

impl PathCompatPolicy {
    pub fn as_str(&self) -> &'static str {
        match self {
            PathCompatPolicy::Sanitize => "sanitize",
            PathCompatPolicy::Skip => "skip",
            PathCompatPolicy::Error => "error",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "sanitize" => Some(PathCompatPolicy::Sanitize),
            "skip" => Some(PathCompatPolicy::Skip),
            "error" => Some(PathCompatPolicy::Error),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PathCompatAction {
    Sanitized,
    Skipped,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PathCompatEvent {
    pub action: PathCompatAction,
    pub original: Vec<u8>,
    pub rewritten: Option<Vec<u8>>,
    pub reason: String,
}

fn windows_path_compat_reasons(path: &[u8]) -> Vec<&'static str> {
    let mut reasons = Vec::new();
    if path
        .iter()
        .any(|b| matches!(*b, b'<' | b'>' | b':' | b'"' | b'|' | b'?' | b'*'))
    {
        reasons.push("contains one or more Windows-forbidden characters");
    }
    let trailing_invalid = path
        .rsplit(|&c| c == b'/')
        .next()
        .and_then(|comp| comp.last())
        .is_some_and(|c| *c == b'.' || *c == b' ');
    if trailing_invalid {
        reasons.push("final path component ends with '.' or space");
    }
    reasons
}

fn summarize_windows_path_compat_reason(path: &[u8]) -> String {
    let reasons = windows_path_compat_reasons(path);
    if reasons.is_empty() {
        "path is incompatible with Windows filename rules".to_string()
    } else {
        reasons.join("; ")
    }
}

pub fn format_path_bytes_for_report(path: &[u8]) -> String {
    let mut out = String::with_capacity(path.len() + 2);
    out.push('"');
    for &b in path {
        for c in std::ascii::escape_default(b) {
            out.push(c as char);
        }
    }
    out.push('"');
    out
}

pub fn apply_path_compat_policy(
    path: &[u8],
    policy: PathCompatPolicy,
) -> Result<(Option<Vec<u8>>, Option<PathCompatEvent>), String> {
    if !cfg!(windows) {
        return Ok((Some(path.to_vec()), None));
    }

    let sanitized = sanitize_invalid_windows_path_bytes(path);
    if sanitized == path {
        return Ok((Some(path.to_vec()), None));
    }

    let reason = summarize_windows_path_compat_reason(path);
    match policy {
        PathCompatPolicy::Sanitize => Ok((
            Some(sanitized.clone()),
            Some(PathCompatEvent {
                action: PathCompatAction::Sanitized,
                original: path.to_vec(),
                rewritten: Some(sanitized),
                reason,
            }),
        )),
        PathCompatPolicy::Skip => Ok((
            None,
            Some(PathCompatEvent {
                action: PathCompatAction::Skipped,
                original: path.to_vec(),
                rewritten: None,
                reason,
            }),
        )),
        PathCompatPolicy::Error => Err(format!(
            "--path-compat-policy=error rejected path {} ({})",
            format_path_bytes_for_report(path),
            reason
        )),
    }
}

#[allow(dead_code)]
#[cfg(windows)]
pub fn sanitize_invalid_windows_path_bytes(p: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(p.len());
    for &b in p {
        let nb = match b {
            b'<' | b'>' | b':' | b'"' | b'|' | b'?' | b'*' => b'_',
            _ => b,
        };
        out.push(nb);
    }
    if let Some(pos) = out
        .rsplit(|&c| c == b'/')
        .next()
        .map(|comp| out.len() - comp.len())
    {
        let (head, tail) = out.split_at(pos);
        let mut t = tail.to_vec();
        while t.last().is_some_and(|c| *c == b'.' || *c == b' ') {
            t.pop();
        }
        let mut combined = head.to_vec();
        combined.extend_from_slice(&t);
        return combined;
    }
    let mut o = out;
    while o.last().is_some_and(|c| *c == b'.' || *c == b' ') {
        o.pop();
    }
    o
}

#[allow(dead_code)]
#[cfg(not(windows))]
pub fn sanitize_invalid_windows_path_bytes(p: &[u8]) -> Vec<u8> {
    p.to_vec()
}

#[allow(dead_code)]
pub fn dequote_c_style_bytes(s: &[u8]) -> Vec<u8> {
    // Minimal C-style unescape: handles \\ \" \n \t \r and octal \ooo
    let mut out = Vec::with_capacity(s.len());
    let mut i = 0usize;
    while i < s.len() {
        let b = s[i];
        i += 1;
        if b != b'\\' {
            out.push(b);
            continue;
        }
        if i >= s.len() {
            out.push(b'\\');
            break;
        }
        let c = s[i];
        i += 1;
        match c {
            b'\\' => out.push(b'\\'),
            b'"' => out.push(b'"'),
            b'n' => out.push(b'\n'),
            b't' => out.push(b'\t'),
            b'r' => out.push(b'\r'),
            b'0'..=b'7' => {
                // up to 3 octal digits; we already consumed one
                let mut val: u32 = (c - b'0') as u32;
                let mut count = 0;
                while count < 2 && i < s.len() {
                    let d = s[i];
                    if !(b'0'..=b'7').contains(&d) {
                        break;
                    }
                    i += 1;
                    count += 1;
                    val = (val << 3) | (d - b'0') as u32;
                }
                out.push(val as u8);
            }
            other => {
                out.push(other);
            }
        }
    }
    out
}

#[allow(dead_code)]
pub fn enquote_c_style_bytes(bytes: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(bytes.len() + 2);
    out.push(b'"');
    for &b in bytes {
        match b {
            b'"' => {
                out.extend_from_slice(b"\\\"");
            }
            b'\\' => {
                out.extend_from_slice(b"\\\\");
            }
            b'\n' => {
                out.extend_from_slice(b"\\n");
            }
            b'\t' => {
                out.extend_from_slice(b"\\t");
            }
            b'\r' => {
                out.extend_from_slice(b"\\r");
            }
            0x00..=0x1F | 0x7F..=0xFF => {
                // use 3-digit octal
                let o1 = ((b >> 6) & 0x7) + b'0';
                let o2 = ((b >> 3) & 0x7) + b'0';
                let o3 = (b & 0x7) + b'0';
                out.push(b'\\');
                out.push(o1);
                out.push(o2);
                out.push(o3);
            }
            _ => out.push(b),
        }
    }
    out.push(b'"');
    out
}

enum PathLikeKind {
    Path,
    Glob,
}

impl PathLikeKind {
    fn empty_error(&self) -> &'static str {
        match self {
            PathLikeKind::Path => "empty path not allowed",
            PathLikeKind::Glob => "empty glob not allowed",
        }
    }

    fn absolute_windows_error(&self) -> &'static str {
        match self {
            PathLikeKind::Path => {
                "do not use absolute Windows drive paths; use repo-relative with '/'"
            }
            PathLikeKind::Glob => {
                "do not use absolute Windows drive paths in globs; use repo-relative with '/'"
            }
        }
    }

    fn absolute_prefix_error(&self) -> &'static str {
        match self {
            PathLikeKind::Path => {
                "do not use absolute paths; paths are relative to the repo toplevel and must not start with '/' or '//'"
            }
            PathLikeKind::Glob => "do not use absolute paths in globs; patterns are repo-relative",
        }
    }

    fn absolute_after_normalization_error(&self) -> &'static str {
        match self {
            PathLikeKind::Path => {
                "do not use absolute paths; paths are relative to the repo toplevel"
            }
            PathLikeKind::Glob => "do not use absolute paths in globs; patterns are repo-relative",
        }
    }

    fn dot_segment_error(&self) -> &'static str {
        match self {
            PathLikeKind::Path => {
                "do not use '.' or '..' in paths; specify repo-relative canonical paths"
            }
            PathLikeKind::Glob => {
                "do not use '.' or '..' in globs; specify repo-relative canonical patterns"
            }
        }
    }
}

fn normalize_cli_path_like_str(
    s: &str,
    allow_empty: bool,
    kind: PathLikeKind,
) -> Result<Vec<u8>, String> {
    if s.is_empty() {
        if allow_empty {
            return Ok(Vec::new());
        }
        return Err(kind.empty_error().to_string());
    }

    let bytes = s.as_bytes();
    if bytes.len() >= 2 && bytes[1] == b':' && bytes[0].is_ascii_alphabetic() {
        return Err(kind.absolute_windows_error().to_string());
    }
    if s.starts_with("//") || s.starts_with("\\\\") || s.starts_with('/') {
        return Err(kind.absolute_prefix_error().to_string());
    }

    let out: Vec<u8> = bytes
        .iter()
        .map(|&b| if b == b'\\' { b'/' } else { b })
        .collect();

    if out.first() == Some(&b'/') {
        return Err(kind.absolute_after_normalization_error().to_string());
    }

    for seg in out.split(|&b| b == b'/') {
        if seg == b"." || seg == b".." {
            return Err(kind.dot_segment_error().to_string());
        }
    }

    Ok(out)
}

/// Normalize CLI-supplied path-like strings to Git's internal style.
///
/// Rules:
/// - Convert '\\' to '/' for all inputs (across platforms)
/// - Disallow absolute paths (leading '/', Windows drive letters like 'C:', or UNC '//'/'\\\\')
/// - Disallow any '.' or '..' segments
/// - Return normalized bytes on success
pub fn normalize_cli_path_str(s: &str, allow_empty: bool) -> Result<Vec<u8>, String> {
    normalize_cli_path_like_str(s, allow_empty, PathLikeKind::Path)
}

/// Normalize CLI-supplied glob patterns.
///
/// We convert '\\' to '/' and reject absolute prefixes and '.'/'..' segments
/// similar to plain paths. Regex-specific escaping is not relevant here.
pub fn normalize_cli_glob_str(s: &str) -> Result<Vec<u8>, String> {
    normalize_cli_path_like_str(s, /*allow_empty=*/ false, PathLikeKind::Glob)
}

/// Encode a repository path for git fast-import:
/// - Apply Windows filename sanitization (on Windows builds)
/// - Apply C-style quoting if needed (spaces, control, non-ASCII, quotes, backslashes)
#[allow(dead_code)]
pub fn encode_path_for_fi(bytes: &[u8]) -> Vec<u8> {
    let safe = sanitize_fast_import_path_bytes(bytes);
    if needs_c_style_quote(&safe) {
        enquote_c_style_bytes(&safe)
    } else {
        safe
    }
}

#[allow(dead_code)]
pub fn encode_path_for_fi_with_policy(
    bytes: &[u8],
    policy: PathCompatPolicy,
) -> Result<(Option<Vec<u8>>, Option<PathCompatEvent>), String> {
    let (maybe_path, event) = apply_path_compat_policy(bytes, policy)?;
    let encoded = maybe_path.map(|p| encode_path_for_fi(&p));
    Ok((encoded, event))
}
/// Sanitize bytes that git fast-import rejects in pathnames.
///
/// Map ASCII control bytes (0x00..=0x1F, 0x7F) to underscores. This avoids
/// fast-import fatal errors like "invalid path" caused by control characters,
/// while preserving other bytes which are re-quoted later if needed.
#[allow(dead_code)]
pub fn sanitize_fast_import_path_bytes(p: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(p.len());
    for &b in p {
        let mapped = match b {
            0x00..=0x1F | 0x7F => b'_',
            _ => b,
        };
        out.push(mapped);
    }
    out
}

#[allow(dead_code)]
pub fn sanitize_and_encode_path_for_import(path: &[u8]) -> Vec<u8> {
    encode_path_for_fi(path)
}

#[allow(dead_code)]
pub fn decode_fast_export_path_bytes(path: &[u8]) -> Vec<u8> {
    let mut trimmed = path;
    if let Some(last) = trimmed.last() {
        if *last == b'\n' {
            trimmed = &trimmed[..trimmed.len() - 1];
        }
    }
    if let (Some(first), Some(last)) = (trimmed.first(), trimmed.last()) {
        if *first == b'"' && *last == b'"' && trimmed.len() >= 2 {
            return dequote_c_style_bytes(&trimmed[1..trimmed.len() - 1]);
        }
    }
    if trimmed.first() == Some(&b'"') {
        return dequote_c_style_bytes(&trimmed[1..]);
    }
    trimmed.to_vec()
}

#[allow(dead_code)]
pub fn needs_c_style_quote(bytes: &[u8]) -> bool {
    // Quote conservatively for fast-import: any space/control/non-ASCII, backslash or quotes
    for &b in bytes {
        if b <= 0x20 || b >= 0x7F || b == b'"' || b == b'\\' {
            return true;
        }
    }
    false
}

#[allow(dead_code)]
pub fn glob_match_bytes(pat: &[u8], text: &[u8]) -> bool {
    fn match_from(p: &[u8], t: &[u8]) -> bool {
        // Fast path: exact match
        if p.is_empty() {
            return t.is_empty();
        }

        // Handle '**' (may be followed by a '/')
        if p[0] == b'*' && p.get(1) == Some(&b'*') {
            let mut rest = &p[2..];
            if rest.first() == Some(&b'/') {
                rest = &rest[1..];
            }
            // Try to match rest at every position (including current), advancing through any chars
            let mut i = 0usize;
            loop {
                if match_from(rest, &t[i..]) {
                    return true;
                }
                if i >= t.len() {
                    break;
                }
                i += 1;
            }
            return false;
        }

        // Handle single '*': match any run of non-'/' chars
        if p[0] == b'*' {
            let rest = &p[1..];
            let mut i = 0usize;
            loop {
                if match_from(rest, &t[i..]) {
                    return true;
                }
                if i >= t.len() || t[i] == b'/' {
                    break;
                }
                i += 1;
            }
            return false;
        }

        // Handle '?'
        if p[0] == b'?' {
            if t.is_empty() || t[0] == b'/' {
                return false;
            }
            return match_from(&p[1..], &t[1..]);
        }

        // Literal byte
        if !t.is_empty() && p[0] == t[0] {
            return match_from(&p[1..], &t[1..]);
        }
        false
    }
    match_from(pat, text)
}
