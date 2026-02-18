use std::cell::RefCell;
use std::collections::HashMap;
use std::io::{self, BufRead, Read, Write};
use std::path::Path;

use aho_corasick::AhoCorasick;
use regex::bytes::RegexBuilder;

const AHO_CORASICK_THRESHOLD: usize = 3;

pub const STREAMING_THRESHOLD: usize = 1024 * 1024;

#[derive(Clone, Debug, Default)]
pub struct MessageReplacer {
    pub pairs: Vec<(Vec<u8>, Vec<u8>)>,
    ac: Option<AhoCorasick>,
    replacements: Vec<Vec<u8>>,
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

        if pairs.is_empty() {
            return Ok(Self::default());
        }

        let (ac, replacements) = if pairs.len() >= AHO_CORASICK_THRESHOLD {
            let patterns: Vec<&[u8]> = pairs.iter().map(|(p, _)| p.as_slice()).collect();
            let replacements: Vec<Vec<u8>> = pairs.iter().map(|(_, r)| r.clone()).collect();
            let ac = AhoCorasick::new(&patterns).ok();
            (ac, replacements)
        } else {
            (None, Vec::new())
        };

        Ok(Self {
            pairs,
            ac,
            replacements,
        })
    }

    pub fn apply(&self, data: Vec<u8>) -> Vec<u8> {
        if let Some(ref ac) = self.ac {
            let (result, _changed) = self.apply_ac_replacements(ac, &data);
            result
        } else {
            let mut result = data;
            for (from, to) in &self.pairs {
                result = replace_all_bytes(&result, from, to);
            }
            result
        }
    }

    pub fn supports_streaming(&self) -> bool {
        self.ac.is_some()
    }

    pub fn apply_streaming<R: Read, W: Write>(
        &self,
        reader: &mut R,
        writer: &mut W,
    ) -> io::Result<bool> {
        let Some(ref ac) = self.ac else {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "streaming not supported for simple byte replacement",
            ));
        };

        let mut changed = false;
        ac.try_stream_replace_all_with(reader, writer, |mat, _matched, wtr| {
            changed = true;
            let idx = mat.pattern().as_usize();
            wtr.write_all(
                self.replacements
                    .get(idx)
                    .expect("replacement index should exist"),
            )
        })?;

        Ok(changed)
    }

    fn apply_ac_replacements(&self, ac: &AhoCorasick, data: &[u8]) -> (Vec<u8>, bool) {
        if ac.find(data).is_none() {
            return (data.to_vec(), false);
        }

        let mut out = Vec::with_capacity(data.len());
        let mut last = 0usize;
        for m in ac.find_iter(data) {
            out.extend_from_slice(&data[last..m.start()]);
            let idx = m.pattern().as_usize();
            out.extend_from_slice(
                self.replacements
                    .get(idx)
                    .expect("replacement index should exist"),
            );
            last = m.end();
        }
        out.extend_from_slice(&data[last..]);
        (out, true)
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
        let regex = RegexBuilder::new(r"(?i)\b[0-9a-f]{7,40}\b")
            .size_limit(10 << 20)
            .dfa_size_limit(10 << 20)
            .build()
            .map_err(|e| {
                io::Error::other(
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
        let entries = self.prefix_index.get(&key)?;
        let mut matches_iter = entries
            .iter()
            .filter(|full| full.len() >= orig_len && &full[..orig_len] == short);
        let full_old = matches_iter.next()?;
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
    use regex::bytes::{Captures, Regex, RegexBuilder};

    const REGEX_SIZE_LIMIT: usize = 10 << 20;
    const DFA_SIZE_LIMIT: usize = 10 << 20;

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
                    let (pat, rep) = if let Some(pos) = super::find_subslice(rest, b"==>") {
                        (&rest[..pos], rest[pos + 3..].to_vec())
                    } else {
                        (rest, b"***REMOVED***".to_vec())
                    };
                    let pat_str = std::str::from_utf8(pat).map_err(|e| {
                        io::Error::new(
                            io::ErrorKind::InvalidInput,
                            format!("invalid UTF-8 in regex rule: {e}"),
                        )
                    })?;
                    let re = RegexBuilder::new(pat_str)
                        .size_limit(REGEX_SIZE_LIMIT)
                        .dfa_size_limit(DFA_SIZE_LIMIT)
                        .build()
                        .map_err(|e| {
                            io::Error::new(
                                io::ErrorKind::InvalidInput,
                                format!("invalid regex pattern: {e}"),
                            )
                        })?;
                    let has_dollar = rep.contains(&b'$');
                    rules.push((re, rep, has_dollar));
                    continue;
                }
                if let Some(rest) = raw.strip_prefix(b"glob:") {
                    // Split at first ==> for replacement; default to ***REMOVED*** if missing
                    let (pat, rep) = if let Some(pos) = super::find_subslice(rest, b"==>") {
                        (&rest[..pos], rest[pos + 3..].to_vec())
                    } else {
                        (rest, b"***REMOVED***".to_vec())
                    };
                    let glob_str = std::str::from_utf8(pat).map_err(|e| {
                        io::Error::new(
                            io::ErrorKind::InvalidInput,
                            format!("invalid UTF-8 in glob rule: {e}"),
                        )
                    })?;
                    // Convert a simple glob pattern to a bytes regex:
                    // * -> .*, ? -> ., everything else regex-escaped. No anchors
                    let mut rx = String::with_capacity(glob_str.len() + 8);
                    for ch in glob_str.chars() {
                        match ch {
                            '*' => rx.push_str(".*"),
                            '?' => rx.push('.'),
                            // escape regex meta characters
                            '.' | '+' | '(' | ')' | '|' | '{' | '}' | '[' | ']' | '^' | '$'
                            | '\\' => {
                                rx.push('\\');
                                rx.push(ch);
                            }
                            _ => rx.push(ch),
                        }
                    }
                    let re = RegexBuilder::new(&rx)
                        .size_limit(REGEX_SIZE_LIMIT)
                        .dfa_size_limit(DFA_SIZE_LIMIT)
                        .build()
                        .map_err(|e| {
                            io::Error::new(
                                io::ErrorKind::InvalidInput,
                                format!("invalid glob-derived regex: {e}"),
                            )
                        })?;
                    // For glob-derived rules, treat '$' literally in replacement (no capture groups)
                    let has_dollar = false;
                    rules.push((re, rep, has_dollar));
                    continue;
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
                        if c.is_ascii_digit() {
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

// Regex support for commit/tag message replacements: support lines beginning with
// "regex:" in the --replace-message FILE. Patterns are Rust regex (bytes), so use
// (?m) for multi-line when matching whole lines.
pub mod msg_regex {
    use super::*;
    use regex::bytes::{Captures, Regex, RegexBuilder};

    const REGEX_SIZE_LIMIT: usize = 10 << 20;
    const DFA_SIZE_LIMIT: usize = 10 << 20;

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
                    let (pat, rep) = if let Some(pos) = super::find_subslice(rest, b"==>") {
                        (&rest[..pos], rest[pos + 3..].to_vec())
                    } else {
                        (rest, b"***REMOVED***".to_vec())
                    };
                    let pat_str = std::str::from_utf8(pat).map_err(|e| {
                        io::Error::new(
                            io::ErrorKind::InvalidInput,
                            format!("invalid UTF-8 in regex rule: {e}"),
                        )
                    })?;
                    let re = RegexBuilder::new(pat_str)
                        .size_limit(REGEX_SIZE_LIMIT)
                        .dfa_size_limit(DFA_SIZE_LIMIT)
                        .build()
                        .map_err(|e| {
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
                        if c.is_ascii_digit() {
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

#[cfg(test)]
mod tests {
    use super::*;

    fn write_file(path: &std::path::Path, content: &[u8]) {
        std::fs::write(path, content).expect("write test file");
    }

    fn hex40(ch: u8) -> Vec<u8> {
        vec![ch; 40]
    }

    #[test]
    fn message_replacer_parses_rules_and_applies_defaults() {
        let dir = tempfile::tempdir().expect("create tempdir");
        let path = dir.path().join("rules.txt");
        write_file(&path, b"# comment\nFOO==>BAR\nBAZ\n==>IGNORED\n\n");

        let replacer = MessageReplacer::from_file(&path).expect("parse rules");
        assert_eq!(replacer.pairs.len(), 2);
        assert_eq!(replacer.pairs[0], (b"FOO".to_vec(), b"BAR".to_vec()));
        assert_eq!(
            replacer.pairs[1],
            (b"BAZ".to_vec(), b"***REMOVED***".to_vec())
        );

        let out = replacer.apply(b"FOO + BAZ".to_vec());
        assert_eq!(out, b"BAR + ***REMOVED***".to_vec());
    }

    #[test]
    fn replace_all_bytes_handles_empty_and_multiple_matches() {
        assert_eq!(replace_all_bytes(b"abcdef", b"", b"X"), b"abcdef".to_vec());
        assert_eq!(
            replace_all_bytes(b"foo foo foo", b"foo", b"bar"),
            b"bar bar bar".to_vec()
        );
    }

    #[test]
    fn message_replacer_aho_corasick_preserves_non_utf8_bytes_without_match() {
        let dir = tempfile::tempdir().expect("create tempdir");
        let path = dir.path().join("rules-ac.txt");
        write_file(&path, b"foo==>bar\nbaz==>qux\nhello==>world\n");

        let replacer = MessageReplacer::from_file(&path).expect("parse rules");
        assert!(
            replacer.supports_streaming(),
            "3+ rules should enable aho-corasick path"
        );

        let input = vec![0xff, 0x00, 0xfe, b'A', 0x80, b'B', b'C'];
        let out = replacer.apply(input.clone());
        assert_eq!(out, input, "non-utf8 bytes should remain byte-identical");
    }

    #[test]
    fn message_replacer_streaming_matches_across_chunk_boundary() {
        const CHUNK_SIZE: usize = 64 * 1024;
        let dir = tempfile::tempdir().expect("create tempdir");
        let path = dir.path().join("rules-streaming.txt");
        write_file(&path, b"ABCDE==>Z\nunused1==>u\nunused2==>v\n");

        let replacer = MessageReplacer::from_file(&path).expect("parse rules");
        assert!(
            replacer.supports_streaming(),
            "3+ rules should enable aho-corasick path"
        );

        let mut input = vec![b'x'; CHUNK_SIZE - 2];
        input.extend_from_slice(b"ABCDE-end");
        let mut reader = std::io::Cursor::new(input);
        let mut out = Vec::new();

        let changed = replacer
            .apply_streaming(&mut reader, &mut out)
            .expect("streaming replacement should succeed");
        assert!(changed, "cross-chunk replacement should mark content as changed");

        let mut expected = vec![b'x'; CHUNK_SIZE - 2];
        expected.extend_from_slice(b"Z-end");
        assert_eq!(
            out, expected,
            "pattern split across read boundary should still be replaced"
        );
    }

    #[test]
    fn short_hash_mapper_from_debug_dir_handles_missing_or_empty_map() {
        let dir = tempfile::tempdir().expect("create tempdir");
        assert!(ShortHashMapper::from_debug_dir(dir.path())
            .expect("missing map should not fail")
            .is_none());

        write_file(&dir.path().join("commit-map"), b"\n");
        assert!(ShortHashMapper::from_debug_dir(dir.path())
            .expect("empty map should not fail")
            .is_none());
    }

    #[test]
    fn short_hash_mapper_rewrites_full_and_unique_short_hashes() {
        let dir = tempfile::tempdir().expect("create tempdir");
        let old_a = hex40(b'a');
        let new_b = hex40(b'b');
        let old_c = hex40(b'c');
        let map = format!(
            "{} {}\n{} {}\n",
            String::from_utf8_lossy(&old_a),
            String::from_utf8_lossy(&new_b),
            String::from_utf8_lossy(&old_c),
            String::from_utf8_lossy(NULL_OID),
        );
        write_file(&dir.path().join("commit-map"), map.as_bytes());
        let mapper = ShortHashMapper::from_debug_dir(dir.path())
            .expect("load map")
            .expect("mapper should exist");

        let input = format!(
            "full={} short={} removed={}",
            String::from_utf8_lossy(&old_a),
            String::from_utf8_lossy(&old_a[..7]),
            String::from_utf8_lossy(&old_c[..7]),
        );
        let out = mapper.rewrite(input.into_bytes());
        let out = String::from_utf8(out).expect("utf8 output");
        let new_b_full = String::from_utf8_lossy(&new_b);
        let new_b_short = String::from_utf8_lossy(&new_b[..7]);
        let old_c_short = String::from_utf8_lossy(&old_c[..7]);
        assert!(out.contains(new_b_full.as_ref()));
        assert!(out.contains(new_b_short.as_ref()));
        assert!(
            out.contains(old_c_short.as_ref()),
            "null target should remain unchanged"
        );
    }

    #[test]
    fn short_hash_mapper_keeps_ambiguous_prefix_and_updates_cache() {
        let dir = tempfile::tempdir().expect("create tempdir");
        let old1 = b"1111111aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".to_vec();
        let old2 = b"1111111bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb".to_vec();
        let new1 = b"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".to_vec();
        let new2 = b"cccccccccccccccccccccccccccccccccccccccc".to_vec();
        let map = format!(
            "{} {}\n{} {}\n",
            String::from_utf8_lossy(&old1),
            String::from_utf8_lossy(&new1),
            String::from_utf8_lossy(&old2),
            String::from_utf8_lossy(&new2),
        );
        write_file(&dir.path().join("commit-map"), map.as_bytes());
        let mut mapper = ShortHashMapper::from_debug_dir(dir.path())
            .expect("load mapper")
            .expect("mapper should exist");

        let ambiguous = mapper.rewrite(b"1111111".to_vec());
        assert_eq!(ambiguous, b"1111111".to_vec());

        let old_unique = b"2222222aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".to_vec();
        let first_new = b"dddddddddddddddddddddddddddddddddddddddd".to_vec();
        let second_new = b"eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee".to_vec();
        mapper.update_mapping(&old_unique, &first_new);
        let first = mapper.rewrite(b"2222222".to_vec());
        assert_eq!(first, b"ddddddd".to_vec());

        mapper.update_mapping(&old_unique, &second_new);
        let second = mapper.rewrite(b"2222222".to_vec());
        assert_eq!(second, b"eeeeeee".to_vec());
    }

    #[test]
    fn blob_regex_parses_rules_and_expands_templates() {
        let dir = tempfile::tempdir().expect("create tempdir");
        let rules_path = dir.path().join("blob-rules.txt");
        write_file(
            &rules_path,
            b"regex:(foo)(bar)==>$2-$1\n\
glob:sec*et==>REDACTED\n\
glob:cash$==>$100\n",
        );

        let replacer = blob_regex::RegexReplacer::from_file(&rules_path)
            .expect("parse blob regex rules")
            .expect("rules should exist");
        let out = replacer.apply_regex(b"foobar secret cash$".to_vec());
        assert_eq!(out, b"bar-foo REDACTED $100".to_vec());
    }

    #[test]
    fn blob_regex_ignores_non_regex_lines_and_reports_invalid_input() {
        let dir = tempfile::tempdir().expect("create tempdir");
        let no_rules = dir.path().join("no-rules.txt");
        write_file(&no_rules, b"FOO==>BAR\n");
        assert!(blob_regex::RegexReplacer::from_file(&no_rules)
            .expect("parse should succeed")
            .is_none());

        let bad_utf8 = dir.path().join("bad-utf8.txt");
        write_file(&bad_utf8, b"regex:\xFF==>x\n");
        let err = blob_regex::RegexReplacer::from_file(&bad_utf8).expect_err("invalid utf8");
        assert_eq!(err.kind(), io::ErrorKind::InvalidInput);
    }

    #[test]
    fn msg_regex_expands_captures_literal_dollar_and_trailing_dollar() {
        let dir = tempfile::tempdir().expect("create tempdir");
        let rules = dir.path().join("msg-rules.txt");
        write_file(
            &rules,
            b"regex:(ID)-(\\d+)==>$1:$2:$$:$x\nregex:foo==>bar$\n",
        );

        let replacer = msg_regex::RegexReplacer::from_file(&rules)
            .expect("parse msg regex rules")
            .expect("rules should exist");
        let out = replacer.apply_regex(b"ID-42 and foo".to_vec());
        assert_eq!(out, b"ID:42:$:$x and bar$".to_vec());
    }

    #[test]
    fn msg_regex_returns_none_without_regex_rules() {
        let dir = tempfile::tempdir().expect("create tempdir");
        let rules = dir.path().join("plain-rules.txt");
        write_file(&rules, b"glob:foo==>bar\nFOO==>BAR\n");

        assert!(msg_regex::RegexReplacer::from_file(&rules)
            .expect("parse should succeed")
            .is_none());
    }
}
