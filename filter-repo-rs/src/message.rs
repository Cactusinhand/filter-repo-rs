use std::cell::RefCell;
use std::collections::HashMap;
use std::io::{self, BufRead};
use std::path::Path;

#[derive(Clone, Debug, Default)]
pub struct MessageReplacer {
    pub pairs: Vec<(Vec<u8>, Vec<u8>)>,
}

impl MessageReplacer {
    pub fn from_file(path: &std::path::Path) -> io::Result<Self> {
        let content = std::fs::read(path)?;
        let mut pairs = Vec::new();
        for raw in content.split(|&b| b == b'\n') {
            if raw.is_empty() {
                continue;
            }
            if raw.starts_with(b"#") {
                continue;
            }
            if let Some(pos) = find_subslice(raw, b"==>") {
                let from = raw[..pos].to_vec();
                let to = raw[pos + 3..].to_vec();
                if !from.is_empty() {
                    pairs.push((from, to));
                }
            } else {
                let from = raw.to_vec();
                if !from.is_empty() {
                    pairs.push((from, b"***REMOVED***".to_vec()));
                }
            }
        }
        Ok(Self { pairs })
    }

    pub fn apply(&self, mut data: Vec<u8>) -> Vec<u8> {
        for (from, to) in &self.pairs {
            data = replace_all_bytes(&data, from, to);
        }
        data
    }
}

const MIN_SHORT_HASH_LEN: usize = 7;

const NULL_OID: &[u8] = b"0000000000000000000000000000000000000000";

pub struct ShortHashMapper {
    lookup: HashMap<Vec<u8>, Option<Vec<u8>>>,
    prefix_index: HashMap<Vec<u8>, Vec<Vec<u8>>>,
    cache: RefCell<HashMap<Vec<u8>, Option<Vec<u8>>>>,
    regex: regex::bytes::Regex,
}

impl ShortHashMapper {
    pub fn from_debug_dir(dir: &Path) -> io::Result<Option<Self>> {
        let map_path = dir.join("commit-map");
        let file = match std::fs::File::open(&map_path) {
            Ok(f) => f,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(e),
        };
        let mut lookup: HashMap<Vec<u8>, Option<Vec<u8>>> = HashMap::new();
        let mut prefix_index: HashMap<Vec<u8>, Vec<Vec<u8>>> = HashMap::new();
        let mut rdr = std::io::BufReader::new(file);
        let mut line = Vec::with_capacity(128);
        let mut has_any = false;
        while rdr.read_until(b'\n', &mut line)? > 0 {
            while line.last().copied() == Some(b'\n') || line.last().copied() == Some(b'\r') {
                line.pop();
            }
            if line.is_empty() {
                line.clear();
                continue;
            }
            let mut parts = line.splitn(2, |&b| b == b' ');
            let old = match parts.next() {
                Some(v) if !v.is_empty() => v,
                _ => {
                    line.clear();
                    continue;
                }
            };
            let new = match parts.next() {
                Some(v) if !v.is_empty() => v,
                _ => {
                    line.clear();
                    continue;
                }
            };
            let old_norm = old.to_ascii_lowercase();
            let new_entry = if new == NULL_OID {
                None
            } else {
                Some(new.to_ascii_lowercase())
            };
            prefix_index
                .entry(old_norm[..MIN_SHORT_HASH_LEN.min(old_norm.len())].to_vec())
                .or_default()
                .push(old_norm.clone());
            lookup.insert(old_norm, new_entry);
            has_any = true;
            line.clear();
        }
        if !has_any {
            return Ok(None);
        }
        let regex = regex::bytes::Regex::new(r"(?i)\b[0-9a-f]{7,40}\b").map_err(|e| {
            io::Error::new(
                io::ErrorKind::Other,
                format!("invalid short-hash regex: {e}"),
            )
        })?;
        Ok(Some(Self {
            lookup,
            prefix_index,
            cache: RefCell::new(HashMap::new()),
            regex,
        }))
    }

    pub fn rewrite(&self, data: Vec<u8>) -> Vec<u8> {
        self.regex
            .replace_all(&data, |caps: &regex::bytes::Captures| {
                let m = caps.get(0).expect("short hash match");
                self.translate(m.as_bytes())
                    .unwrap_or_else(|| m.as_bytes().to_vec())
            })
            .into_owned()
    }

    fn translate(&self, candidate: &[u8]) -> Option<Vec<u8>> {
        if candidate.len() < MIN_SHORT_HASH_LEN {
            return None;
        }
        let key = candidate.to_ascii_lowercase();
        let mut cache = self.cache.borrow_mut();
        if let Some(entry) = cache.get(&key) {
            return entry.clone();
        }
        let resolved = if candidate.len() == 40 {
            self.lookup.get(&key).cloned().flatten()
        } else {
            self.lookup_prefix(&key, candidate.len())
        };
        cache.insert(key, resolved.clone());
        resolved
    }

    pub fn update_mapping(&mut self, old_full: &[u8], new_full: &[u8]) {
        if old_full.is_empty() || new_full.is_empty() {
            return;
        }
        let old_norm = old_full.to_ascii_lowercase();
        let new_norm = new_full.to_ascii_lowercase();
        let prefix_len = MIN_SHORT_HASH_LEN.min(old_norm.len());
        let prefix = old_norm[..prefix_len].to_vec();
        let entry = self.prefix_index.entry(prefix).or_default();
        if !entry.iter().any(|existing| existing == &old_norm) {
            entry.push(old_norm.clone());
        }
        self.lookup.insert(old_norm, Some(new_norm));
        self.cache.borrow_mut().clear();
    }

    fn lookup_prefix(&self, short: &[u8], orig_len: usize) -> Option<Vec<u8>> {
        if short.len() < MIN_SHORT_HASH_LEN {
            return None;
        }
        let key = short[..MIN_SHORT_HASH_LEN].to_vec();
        let entries = match self.prefix_index.get(&key) {
            Some(v) => v,
            None => return None,
        };
        let mut matches_iter = entries
            .iter()
            .filter(|full| full.len() >= orig_len && &full[..orig_len] == short);
        let full_old = match matches_iter.next() {
            Some(m) => m,
            None => return None,
        };
        if matches_iter.next().is_some() {
            return None;
        }
        match self.lookup.get(full_old) {
            Some(Some(new_full)) => Some(new_full[..orig_len].to_vec()),
            _ => None,
        }
    }
}

pub fn find_subslice(h: &[u8], n: &[u8]) -> Option<usize> {
    if n.is_empty() {
        return Some(0);
    }
    h.windows(n.len()).position(|w| w == n)
}

pub fn replace_all_bytes(h: &[u8], n: &[u8], r: &[u8]) -> Vec<u8> {
    if n.is_empty() {
        return h.to_vec();
    }
    let mut out = Vec::with_capacity(h.len());
    let mut i = 0;
    while i + n.len() <= h.len() {
        if &h[i..i + n.len()] == n {
            out.extend_from_slice(r);
            i += n.len();
        } else {
            out.push(h[i]);
            i += 1;
        }
    }
    out.extend_from_slice(&h[i..]);
    out
}

// Regex support for blob replacements reuses the same replacement file syntax,
// where lines starting with "regex:" are treated as regex rules.
pub mod blob_regex {
    use super::*;
    use regex::bytes::{Captures, Regex};

    #[derive(Clone, Debug, Default)]
    pub struct RegexReplacer {
        pub rules: Vec<(Regex, Vec<u8>, bool)>,
    }

    impl RegexReplacer {
        pub fn from_file(path: &std::path::Path) -> io::Result<Option<Self>> {
            let content = std::fs::read(path)?;
            let mut rules: Vec<(Regex, Vec<u8>, bool)> = Vec::new();
            for raw in content.split(|&b| b == b'\n') {
                if raw.is_empty() {
                    continue;
                }
                if raw.starts_with(b"#") {
                    continue;
                }
                if let Some(rest) = raw.strip_prefix(b"regex:") {
                    // Split at first occurrence of ==> for pattern/replacement
                    let (pat, rep) = if let Some(pos) = super::find_subslice(rest, b"==>") {
                        (&rest[..pos], rest[pos + 3..].to_vec())
                    } else {
                        // No replacement specified; default to ***REMOVED***
                        (&rest[..], b"***REMOVED***".to_vec())
                    };
                    // Pattern is bytes; interpret as UTF-8 for regex parser
                    // (regex bytes API still requires UTF-8 pattern text)
                    let pat_str = std::str::from_utf8(pat).map_err(|e| {
                        io::Error::new(
                            io::ErrorKind::InvalidInput,
                            format!("invalid UTF-8 in regex rule: {e}"),
                        )
                    })?;
                    let re = Regex::new(pat_str).map_err(|e| {
                        io::Error::new(
                            io::ErrorKind::InvalidInput,
                            format!("invalid regex pattern: {e}"),
                        )
                    })?;
                    let has_dollar = rep.contains(&b'$');
                    rules.push((re, rep, has_dollar));
                }
            }
            if rules.is_empty() {
                Ok(None)
            } else {
                Ok(Some(Self { rules }))
            }
        }

        pub fn apply_regex(&self, data: Vec<u8>) -> Vec<u8> {
            let mut cur = data;
            for (re, rep, has_dollar) in &self.rules {
                if *has_dollar {
                    let tpl = rep.clone();
                    cur = re
                        .replace_all(&cur, |caps: &Captures| expand_bytes_template(&tpl, caps))
                        .into_owned();
                } else {
                    cur = re
                        .replace_all(&cur, regex::bytes::NoExpand(rep))
                        .into_owned();
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
                    if nb == b'$' {
                        out.push(b'$');
                        i += 1;
                        continue;
                    }
                    // parse number
                    let mut num: usize = 0;
                    let mut seen = false;
                    while i < tpl.len() {
                        let c = tpl[i];
                        if c >= b'0' && c <= b'9' {
                            seen = true;
                            num = num * 10 + (c - b'0') as usize;
                            i += 1;
                        } else {
                            break;
                        }
                    }
                    if seen && num > 0 {
                        if let Some(m) = caps.get(num) {
                            out.extend_from_slice(m.as_bytes());
                        }
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
