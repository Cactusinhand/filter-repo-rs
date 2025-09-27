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
        while t.last().map_or(false, |c| *c == b'.' || *c == b' ') {
            t.pop();
        }
        let mut combined = head.to_vec();
        combined.extend_from_slice(&t);
        return combined;
    }
    let mut o = out;
    while o.last().map_or(false, |c| *c == b'.' || *c == b' ') {
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
                    if d < b'0' || d > b'7' {
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
