use std::io;

#[derive(Clone, Debug, Default)]
pub struct MessageReplacer { pub pairs: Vec<(Vec<u8>, Vec<u8>)> }

impl MessageReplacer {
    pub fn from_file(path: &std::path::Path) -> io::Result<Self> {
        let content = std::fs::read(path)?;
        let mut pairs = Vec::new();
        for raw in content.split(|&b| b == b'\n') {
            if raw.is_empty() { continue; }
            if raw.starts_with(b"#") { continue; }
            if let Some(pos) = find_subslice(raw, b"==>") {
                let from = raw[..pos].to_vec();
                let to = raw[pos+3..].to_vec();
                if !from.is_empty() { pairs.push((from, to)); }
            } else {
                let from = raw.to_vec();
                if !from.is_empty() { pairs.push((from, b"***REMOVED***".to_vec())); }
            }
        }
        Ok(Self { pairs })
    }

    pub fn apply(&self, mut data: Vec<u8>) -> Vec<u8> {
        for (from, to) in &self.pairs { data = replace_all_bytes(&data, from, to); }
        data
    }
}

pub fn find_subslice(h: &[u8], n: &[u8]) -> Option<usize> {
    if n.is_empty() { return Some(0); }
    h.windows(n.len()).position(|w| w == n)
}

pub fn replace_all_bytes(h: &[u8], n: &[u8], r: &[u8]) -> Vec<u8> {
    if n.is_empty() { return h.to_vec(); }
    let mut out = Vec::with_capacity(h.len());
    let mut i = 0;
    while i + n.len() <= h.len() {
        if &h[i..i+n.len()] == n { out.extend_from_slice(r); i += n.len(); }
        else { out.push(h[i]); i += 1; }
    }
    out.extend_from_slice(&h[i..]);
    out
}

// Regex support for blob replacements reuses the same replacement file syntax,
// where lines starting with "regex:" are treated as regex rules.
pub mod blob_regex {
    use super::*;
    use regex::bytes::{Regex, Captures};

    #[derive(Clone, Debug, Default)]
    pub struct RegexReplacer { pub rules: Vec<(Regex, Vec<u8>, bool)> }

    impl RegexReplacer {
        pub fn from_file(path: &std::path::Path) -> io::Result<Option<Self>> {
            let content = std::fs::read(path)?;
            let mut rules: Vec<(Regex, Vec<u8>, bool)> = Vec::new();
            for raw in content.split(|&b| b == b'\n') {
                if raw.is_empty() { continue; }
                if raw.starts_with(b"#") { continue; }
                if let Some(rest) = raw.strip_prefix(b"regex:") {
                    // Split at first occurrence of ==> for pattern/replacement
                    if let Some(pos) = super::find_subslice(rest, b"==>") {
                        let pat = &rest[..pos];
                        let rep = &rest[pos+3..];
                        // Pattern is bytes; interpret as UTF-8 for regex parser
                        // (regex bytes API still requires UTF-8 pattern text)
                        if let Ok(pat_str) = std::str::from_utf8(pat) {
                            if let Ok(re) = Regex::new(pat_str) {
                                let rep = rep.to_vec();
                                let has_dollar = rep.contains(&b'$');
                                rules.push((re, rep, has_dollar));
                            }
                        }
                    } else {
                        // No replacement specified; default to ***REMOVED***
                        if let Ok(pat_str) = std::str::from_utf8(rest) {
                            if let Ok(re) = Regex::new(pat_str) {
                                let rep = b"***REMOVED***".to_vec();
                                let has_dollar = rep.contains(&b'$');
                                rules.push((re, rep, has_dollar));
                            }
                        }
                    }
                }
            }
            if rules.is_empty() { Ok(None) } else { Ok(Some(Self { rules })) }
        }

        pub fn apply_regex(&self, data: Vec<u8>) -> Vec<u8> {
            let mut cur = data;
            for (re, rep, has_dollar) in &self.rules {
                if *has_dollar {
                    let tpl = rep.clone();
                    cur = re.replace_all(&cur, |caps: &Captures| {
                        expand_bytes_template(&tpl, caps)
                    }).into_owned();
                } else {
                    cur = re.replace_all(&cur, regex::bytes::NoExpand(rep)).into_owned();
                }
            }
            cur
        }
    }

    fn expand_bytes_template(tpl: &[u8], caps: &Captures) -> Vec<u8> {
        // Minimal $1..$9 expansion with $$ -> literal '$'
        let mut out = Vec::with_capacity(tpl.len() + 16);
        let mut i = 0;
        while i < tpl.len() {
            let b = tpl[i];
            if b == b'$' {
                i += 1;
                if i < tpl.len() {
                    let nb = tpl[i];
                    if nb == b'$' { out.push(b'$'); i += 1; continue; }
                    // parse number
                    let mut num: usize = 0; let mut seen = false;
                    while i < tpl.len() {
                        let c = tpl[i];
                        if c >= b'0' && c <= b'9' { seen = true; num = num*10 + (c - b'0') as usize; i += 1; }
                        else { break; }
                    }
                    if seen && num > 0 {
                        if let Some(m) = caps.get(num) { out.extend_from_slice(m.as_bytes()); }
                        continue;
                    }
                    // No valid group number; treat as literal '$' + nb
                    out.push(b'$');
                    out.push(nb);
                    i += 1;
                    continue;
                } else {
                    // Trailing '$'
                    out.push(b'$');
                    break;
                }
            } else {
                out.push(b);
                i += 1;
            }
        }
        out
    }
}
