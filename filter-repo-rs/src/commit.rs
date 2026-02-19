use std::collections::{BTreeSet, HashMap};
use std::fs::File;
use std::io::{self, BufRead, BufReader, Read, Write};
use std::path::Path;
use std::process::ChildStdout;

use aho_corasick::AhoCorasick;

use crate::filechange;
use crate::limits::parse_data_size_header;
use crate::message::{msg_regex, MessageReplacer, ShortHashMapper};
use crate::opts::Options;

pub fn rename_commit_header_ref(
    line: &[u8],
    opts: &Options,
    ref_renames: &mut BTreeSet<(Vec<u8>, Vec<u8>)>,
) -> Vec<u8> {
    if !line.starts_with(b"commit ") {
        return line.to_vec();
    }
    let mut refname = &line[b"commit ".len()..];
    if let Some(&last) = refname.last() {
        if last == b'\n' {
            refname = &refname[..refname.len() - 1];
        }
    }
    // tags
    if refname.starts_with(b"refs/tags/") {
        if let Some((ref old, ref new_)) = opts.tag_rename {
            let name = &refname[b"refs/tags/".len()..];
            if name.starts_with(&old[..]) {
                let mut rebuilt = Vec::with_capacity(
                    7 + b"refs/tags/".len() + new_.len() + (name.len() - old.len()) + 1,
                );
                rebuilt.extend_from_slice(b"commit ");
                rebuilt.extend_from_slice(b"refs/tags/");
                rebuilt.extend_from_slice(new_);
                rebuilt.extend_from_slice(&name[old.len()..]);
                rebuilt.push(b'\n');
                let new_full =
                    [b"refs/tags/".as_ref(), new_.as_slice(), &name[old.len()..]].concat();
                ref_renames.insert((refname.to_vec(), new_full));
                return rebuilt;
            }
        }
    }
    // branches
    if refname.starts_with(b"refs/heads/") {
        if let Some((ref old, ref new_)) = opts.branch_rename {
            let name = &refname[b"refs/heads/".len()..];
            if name.starts_with(&old[..]) {
                let mut rebuilt = Vec::with_capacity(
                    7 + b"refs/heads/".len() + new_.len() + (name.len() - old.len()) + 1,
                );
                rebuilt.extend_from_slice(b"commit ");
                rebuilt.extend_from_slice(b"refs/heads/");
                rebuilt.extend_from_slice(new_);
                rebuilt.extend_from_slice(&name[old.len()..]);
                rebuilt.push(b'\n');
                let new_full =
                    [b"refs/heads/".as_ref(), new_.as_slice(), &name[old.len()..]].concat();
                ref_renames.insert((refname.to_vec(), new_full));
                return rebuilt;
            }
        }
    }
    line.to_vec()
}

pub enum CommitAction {
    Consumed,
    Ended,
}

pub struct ParentLine {
    start: usize,
    end: usize,
    mark: Option<u32>,
    kind: ParentKind,
}

impl ParentLine {
    fn new(start: usize, end: usize, mark: Option<u32>, kind: ParentKind) -> Self {
        Self {
            start,
            end,
            mark,
            kind,
        }
    }
}

#[derive(Copy, Clone)]
pub enum ParentKind {
    From,
    Merge,
}

#[allow(dead_code, clippy::too_many_arguments)]
pub fn start_commit(
    line: &[u8],
    opts: &Options,
    ref_renames: &mut BTreeSet<(Vec<u8>, Vec<u8>)>,
    commit_buf: &mut Vec<u8>,
    commit_has_changes: &mut bool,
    commit_mark: &mut Option<u32>,
    first_parent_mark: &mut Option<u32>,
    parent_lines: &mut Vec<ParentLine>,
) -> bool {
    if !line.starts_with(b"commit ") {
        return false;
    }
    *commit_has_changes = false;
    *commit_mark = None;
    *first_parent_mark = None;
    parent_lines.clear();
    commit_buf.clear();
    let hdr = rename_commit_header_ref(line, opts, ref_renames);
    commit_buf.extend_from_slice(&hdr);
    true
}

#[allow(clippy::too_many_arguments)]
pub fn process_commit_line(
    line: &[u8],
    opts: &Options,
    fe_out: &mut BufReader<ChildStdout>,
    orig_file: Option<&mut dyn Write>,
    filt_file: &mut dyn Write,
    mut fi_in: Option<&mut dyn Write>,
    replacer: &Option<MessageReplacer>,
    msg_regex: Option<&msg_regex::RegexReplacer>,
    short_mapper: Option<&ShortHashMapper>,
    commit_buf: &mut Vec<u8>,
    commit_has_changes: &mut bool,
    commit_mark: &mut Option<u32>,
    first_parent_mark: &mut Option<u32>,
    commit_original_oid: &mut Option<Vec<u8>>,
    parent_count: &mut usize,
    commit_pairs: &mut Vec<(Vec<u8>, Option<u32>)>,
    import_broken: &mut bool,
    parent_lines: &mut Vec<ParentLine>,
    alias_map: &mut HashMap<u32, u32>,
    emitted_marks: &std::collections::HashSet<u32>,
) -> io::Result<CommitAction> {
    // mark line
    if let Some(m) = parse_mark_number(line) {
        commit_buf.extend_from_slice(line);
        *commit_mark = Some(m);
        return Ok(CommitAction::Consumed);
    }
    // capture original-oid
    if line.starts_with(b"original-oid ") {
        let mut v = line[b"original-oid ".len()..].to_vec();
        if let Some(last) = v.last() {
            if *last == b'\n' {
                v.pop();
            }
        }
        *commit_original_oid = Some(v);
        commit_buf.extend_from_slice(line);
        return Ok(CommitAction::Consumed);
    }
    // commit message data
    if line.starts_with(b"data ") {
        handle_commit_data(
            line,
            fe_out,
            orig_file,
            commit_buf,
            replacer,
            msg_regex,
            short_mapper,
        )?;
        return Ok(CommitAction::Consumed);
    }
    // parents
    if line.starts_with(b"from ") {
        if first_parent_mark.is_none() {
            if let Some(m) = parse_from_mark(line) {
                *first_parent_mark = Some(m);
            }
        }
        let start = commit_buf.len();
        commit_buf.extend_from_slice(line);
        let end = commit_buf.len();
        parent_lines.push(ParentLine::new(
            start,
            end,
            parse_from_mark(line),
            ParentKind::From,
        ));
        *parent_count = parent_lines.len();
        return Ok(CommitAction::Consumed);
    }
    if line.starts_with(b"merge ") {
        let start = commit_buf.len();
        commit_buf.extend_from_slice(line);
        let end = commit_buf.len();
        parent_lines.push(ParentLine::new(
            start,
            end,
            parse_merge_mark(line),
            ParentKind::Merge,
        ));
        *parent_count = parent_lines.len();
        return Ok(CommitAction::Consumed);
    }
    // file changes with path filtering
    if line.starts_with(b"M ")
        || line.starts_with(b"D ")
        || line.starts_with(b"C ")
        || line.starts_with(b"R ")
        || line == b"deleteall\n"
    {
        if let Some(newline) = filechange::handle_file_change_line(line, opts) {
            commit_buf.extend_from_slice(&newline);
            *commit_has_changes = true;
        }
        return Ok(CommitAction::Consumed);
    }
    // end of commit (blank line)
    if line == b"\n" {
        let original_parents = parent_lines.len();
        let kept_parents = finalize_parent_lines(
            commit_buf,
            parent_lines,
            first_parent_mark,
            emitted_marks,
            alias_map,
        );
        *parent_count = kept_parents;
        let was_merge = original_parents >= 2;
        let is_degenerate = was_merge && kept_parents < 2;
        if should_keep_commit(
            *commit_has_changes,
            *first_parent_mark,
            *commit_mark,
            *parent_count,
            was_merge,
            is_degenerate,
            opts,
        ) {
            // keep commit
            commit_buf.extend_from_slice(b"\n");
            filt_file.write_all(commit_buf)?;
            if let Some(ref mut fi) = fi_in {
                if let Err(e) = fi.write_all(commit_buf) {
                    if e.kind() == io::ErrorKind::BrokenPipe {
                        *import_broken = true;
                    } else {
                        return Err(e);
                    }
                }
            }
            // Record mark and original id for later resolution via marks file
            if let Some(old) = commit_original_oid.take() {
                if let Some(m) = *commit_mark {
                    commit_pairs.push((old, Some(m)));
                }
            }
        } else {
            if let Some(old) = commit_original_oid.take() {
                commit_pairs.push((old, None));
            }
            // prune commit: only alias if we have both marks and parent mark has been emitted
            if let (Some(old_mark), Some(parent_mark)) = (*commit_mark, *first_parent_mark) {
                let canonical = resolve_canonical_mark(parent_mark, alias_map);
                if emitted_marks.contains(&canonical) {
                    alias_map.insert(old_mark, canonical);
                    let alias = build_alias(old_mark, canonical);
                    filt_file.write_all(&alias)?;
                    if let Some(ref mut fi) = fi_in {
                        if let Err(e) = fi.write_all(&alias) {
                            if e.kind() == io::ErrorKind::BrokenPipe {
                                *import_broken = true;
                            } else {
                                return Err(e);
                            }
                        }
                    }
                }
            }
            // If no alias possible, just skip the commit entirely (mark becomes invalid)
        }
        return Ok(CommitAction::Ended);
    }
    // other commit lines: buffer as-is
    commit_buf.extend_from_slice(line);
    Ok(CommitAction::Consumed)
}

// Parse a 'mark :<num>' line and return the numeric mark
pub fn parse_mark_number(line: &[u8]) -> Option<u32> {
    if !line.starts_with(b"mark :") {
        return None;
    }
    let mut num: u32 = 0;
    let mut seen = false;
    for &b in line[b"mark :".len()..].iter() {
        if b.is_ascii_digit() {
            seen = true;
            num = num.saturating_mul(10).saturating_add((b - b'0') as u32);
        } else {
            break;
        }
    }
    if seen {
        Some(num)
    } else {
        None
    }
}

// Parse a 'from :<num>' line and return the numeric mark
pub fn parse_from_mark(line: &[u8]) -> Option<u32> {
    if !line.starts_with(b"from ") {
        return None;
    }
    if line.get(b"from ".len()).copied() != Some(b':') {
        return None;
    }
    let mut num: u32 = 0;
    let mut seen = false;
    for &b in line[b"from :".len()..].iter() {
        if b.is_ascii_digit() {
            seen = true;
            num = num.saturating_mul(10).saturating_add((b - b'0') as u32);
        } else {
            break;
        }
    }
    if seen {
        Some(num)
    } else {
        None
    }
}

fn parse_merge_mark(line: &[u8]) -> Option<u32> {
    if !line.starts_with(b"merge ") {
        return None;
    }
    if line.get(b"merge ".len()).copied() != Some(b':') {
        return None;
    }
    let mut num: u32 = 0;
    let mut seen = false;
    for &b in line[b"merge :".len()..].iter() {
        if b.is_ascii_digit() {
            seen = true;
            num = num.saturating_mul(10).saturating_add((b - b'0') as u32);
        } else {
            break;
        }
    }
    if seen {
        Some(num)
    } else {
        None
    }
}

// Handle a commit message 'data <n>' header line: read payload from fe_out,
// mirror to orig_file, apply replacer, and append to commit_buf.
pub fn handle_commit_data(
    header_line: &[u8],
    fe_out: &mut BufReader<ChildStdout>,
    orig_file: Option<&mut dyn Write>,
    commit_buf: &mut Vec<u8>,
    replacer: &Option<MessageReplacer>,
    msg_regex: Option<&msg_regex::RegexReplacer>,
    short_mapper: Option<&ShortHashMapper>,
) -> io::Result<()> {
    if !header_line.starts_with(b"data ") {
        return Ok(());
    }
    let n = parse_data_size_header(header_line)?;
    let mut payload = vec![0u8; n];
    fe_out.read_exact(&mut payload)?;
    if let Some(f) = orig_file {
        f.write_all(&payload)?;
    }
    let mut new_payload = if let Some(r) = replacer {
        r.apply(payload)
    } else {
        payload
    };
    if let Some(rr) = msg_regex {
        new_payload = rr.apply_regex(new_payload);
    }
    if let Some(mapper) = short_mapper {
        new_payload = mapper.rewrite(new_payload);
    }
    let header = format!("data {}\n", new_payload.len());
    commit_buf.extend_from_slice(header.as_bytes());
    commit_buf.extend_from_slice(&new_payload);
    Ok(())
}

// Should the commit be kept based on observed properties
pub fn should_keep_commit(
    commit_has_changes: bool,
    first_parent_mark: Option<u32>,
    commit_mark: Option<u32>,
    parent_count: usize,
    was_merge: bool,
    is_degenerate: bool,
    opts: &crate::opts::Options,
) -> bool {
    // Always keep roots and malformed commits for safety
    if first_parent_mark.is_none() || commit_mark.is_none() {
        return true;
    }

    // If there were any file changes, keep regardless of prune settings
    if commit_has_changes {
        return true;
    }

    // No file changes
    let is_merge_after = parent_count >= 2;
    if is_merge_after {
        // Non-degenerate merge (still 2+ parents): keep
        return true;
    }

    // If commit started as a merge but became degenerate
    if was_merge && is_degenerate {
        if opts.no_ff {
            // Respect --no-ff: keep degenerate merges
            return true;
        }
        return match opts.prune_degenerate {
            crate::opts::PruneMode::Never => true,
            crate::opts::PruneMode::Auto | crate::opts::PruneMode::Always => false,
        };
    }

    // Non-merge (0 or 1 parent) empty commit
    match opts.prune_empty {
        crate::opts::PruneMode::Never => true,
        crate::opts::PruneMode::Auto | crate::opts::PruneMode::Always => false,
    }
}

// Build an alias stanza to map an old mark to its first parent mark
pub fn build_alias(old_mark: u32, first_parent_mark: u32) -> Vec<u8> {
    format!("alias\nmark :{}\nto :{}\n\n", old_mark, first_parent_mark).into_bytes()
}

fn finalize_parent_lines(
    commit_buf: &mut Vec<u8>,
    parent_lines: &mut Vec<ParentLine>,
    first_parent_mark: &mut Option<u32>,
    emitted_marks: &std::collections::HashSet<u32>,
    alias_map: &HashMap<u32, u32>,
) -> usize {
    if parent_lines.is_empty() {
        *first_parent_mark = None;
        return 0;
    }

    enum ParentReplacement {
        Canonical { canonical: u32, kind: ParentKind },
        Raw(Vec<u8>),
    }

    let mut replacements: Vec<Option<ParentReplacement>> = Vec::with_capacity(parent_lines.len());
    let mut seen_canonical: BTreeSet<u32> = BTreeSet::new();
    let mut first_kept_mark: Option<u32> = None;
    let mut first_kept_idx: Option<usize> = None;
    let mut kept_count: usize = 0;

    for (idx, parent) in parent_lines.iter().enumerate() {
        if let Some(mark) = parent.mark {
            let canonical = resolve_canonical_mark(mark, alias_map);
            if !emitted_marks.contains(&canonical) {
                replacements.push(None);
                continue;
            }
            if !seen_canonical.insert(canonical) {
                replacements.push(None);
                continue;
            }
            if first_kept_idx.is_none() {
                first_kept_idx = Some(idx);
                first_kept_mark = Some(canonical);
            }
            replacements.push(Some(ParentReplacement::Canonical {
                canonical,
                kind: parent.kind,
            }));
            kept_count += 1;
        } else {
            let line = commit_buf[parent.start..parent.end].to_vec();
            if first_kept_idx.is_none() {
                first_kept_idx = Some(idx);
            }
            replacements.push(Some(ParentReplacement::Raw(line)));
            kept_count += 1;
        }
    }

    let mut new_buf = Vec::with_capacity(commit_buf.len());
    let mut cursor = 0usize;
    for (idx, (parent, replacement)) in parent_lines
        .iter()
        .zip(replacements.into_iter())
        .enumerate()
    {
        if cursor < parent.start {
            new_buf.extend_from_slice(&commit_buf[cursor..parent.start]);
        }
        if let Some(replacement) = replacement {
            match replacement {
                ParentReplacement::Canonical { canonical, kind } => {
                    let effective_kind = if Some(idx) == first_kept_idx {
                        ParentKind::From
                    } else {
                        kind
                    };
                    new_buf.extend_from_slice(&rebuild_parent_line(effective_kind, canonical));
                }
                ParentReplacement::Raw(mut bytes) => {
                    if Some(idx) == first_kept_idx && bytes.starts_with(b"merge ") {
                        let mut rebuilt = Vec::with_capacity(bytes.len());
                        rebuilt.extend_from_slice(b"from ");
                        rebuilt.extend_from_slice(&bytes[b"merge ".len()..]);
                        bytes = rebuilt;
                    }
                    new_buf.extend_from_slice(&bytes);
                }
            }
        }
        cursor = parent.end;
    }
    if cursor < commit_buf.len() {
        new_buf.extend_from_slice(&commit_buf[cursor..]);
    }

    *commit_buf = new_buf;
    parent_lines.clear();
    *first_parent_mark = first_kept_mark;
    kept_count
}

fn rebuild_parent_line(kind: ParentKind, mark: u32) -> Vec<u8> {
    match kind {
        ParentKind::From => format!("from :{}\n", mark).into_bytes(),
        ParentKind::Merge => format!("merge :{}\n", mark).into_bytes(),
    }
}

fn resolve_canonical_mark(mark: u32, alias_map: &HashMap<u32, u32>) -> u32 {
    let mut current = mark;
    let mut seen = std::collections::HashSet::new();
    while let Some(&next) = alias_map.get(&current) {
        if !seen.insert(current) {
            break;
        }
        if next == current {
            break;
        }
        current = next;
    }
    current
}

pub struct AuthorRewriter {
    patterns: Vec<String>,
    replacements: Vec<String>,
    ac: AhoCorasick,
}

impl AuthorRewriter {
    pub fn from_file<P: AsRef<Path>>(path: P) -> io::Result<Self> {
        let file = File::open(path)?;
        let reader = BufReader::new(file);
        Self::from_reader(reader)
    }

    pub fn from_reader<R: BufRead>(reader: R) -> io::Result<Self> {
        let mut patterns = Vec::new();
        let mut replacements = Vec::new();

        for line in reader.lines() {
            let line = line?;
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }

            if let Some((old, new)) = line.split_once("==>") {
                let old = old.trim();
                let new = new.trim();
                if !old.is_empty() {
                    patterns.push(old.to_string());
                    replacements.push(new.to_string());
                }
            }
        }

        if patterns.is_empty() {
            return Ok(Self {
                patterns: vec![String::new()],
                replacements: vec![String::new()],
                ac: AhoCorasick::new([""]).unwrap(),
            });
        }

        let ac = AhoCorasick::new(&patterns)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;

        Ok(Self {
            patterns,
            replacements,
            ac,
        })
    }

    pub fn rewrite(&self, text: &[u8]) -> Vec<u8> {
        if self.patterns.is_empty() || (self.patterns.len() == 1 && self.patterns[0].is_empty()) {
            return text.to_vec();
        }
        let text_str = match std::str::from_utf8(text) {
            Ok(s) => s,
            Err(_) => return text.to_vec(),
        };
        let result = self.ac.replace_all(text_str, &self.replacements);
        result.into_bytes()
    }

    pub fn is_empty(&self) -> bool {
        self.patterns.is_empty() || (self.patterns.len() == 1 && self.patterns[0].is_empty())
    }
}

impl Clone for AuthorRewriter {
    fn clone(&self) -> Self {
        Self {
            patterns: self.patterns.clone(),
            replacements: self.replacements.clone(),
            ac: AhoCorasick::new(&self.patterns).unwrap(),
        }
    }
}

use regex::Regex as RegexStr;

pub struct MailmapRewriter {
    parser: RegexStr,
    old_email_patterns: Vec<RegexStr>,
    new_names: Vec<String>,
    new_emails: Vec<String>,
}

impl MailmapRewriter {
    pub fn from_file<P: AsRef<Path>>(path: P) -> io::Result<Self> {
        let file = File::open(path)?;
        let reader = BufReader::new(file);
        Self::from_reader(reader)
    }

    pub fn from_reader<R: BufRead>(reader: R) -> io::Result<Self> {
        let parser =
            RegexStr::new(r"^(?:([^<]*?)\s+)?<([^>]+)>\s+(?:<([^>]+)>|([^<]*?)\s+<([^>]+)>)")
                .unwrap();

        let mut old_email_patterns = Vec::new();
        let mut new_names = Vec::new();
        let mut new_emails = Vec::new();

        for line in reader.lines() {
            let line = line?;
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }

            if let Some(caps) = parser.captures(line) {
                let new_name = caps.get(1).and_then(|m| {
                    let s = m.as_str().trim();
                    if s.is_empty() {
                        None
                    } else {
                        Some(s.to_string())
                    }
                });

                let new_email = caps.get(2).map(|m| m.as_str().trim().to_string());

                let old_email = if let Some(m) = caps.get(3) {
                    Some(m.as_str().trim())
                } else {
                    caps.get(5).map(|m| m.as_str().trim())
                };

                if let Some(old_email_str) = old_email {
                    let escaped = regex::escape(old_email_str);
                    if let Ok(re) = RegexStr::new(&format!("^{}$", escaped)) {
                        old_email_patterns.push(re);
                        new_names.push(new_name.unwrap_or_default());
                        new_emails.push(new_email.unwrap_or_default());
                    }
                }
            }
        }

        Ok(Self {
            parser,
            old_email_patterns,
            new_names,
            new_emails,
        })
    }

    pub fn rewrite_line(&self, line: &[u8]) -> Vec<u8> {
        let line_str = match std::str::from_utf8(line) {
            Ok(s) => s,
            Err(_) => return line.to_vec(),
        };

        let header_len = if line.starts_with(b"author ") {
            b"author ".len()
        } else if line.starts_with(b"committer ") {
            b"committer ".len()
        } else {
            return line.to_vec();
        };
        let identity = &line_str[header_len..];

        if let Some(email_start_rel) = identity.find('<') {
            let email_start = email_start_rel + 1;
            if let Some(close_pos_rel) = identity[email_start_rel..].find('>') {
                let close_pos = email_start_rel + close_pos_rel;
                let old_name = identity[..email_start_rel].trim_end();
                let old_email = &identity[email_start..close_pos];
                let suffix = &identity[close_pos + 1..];

                for (i, pattern) in self.old_email_patterns.iter().enumerate() {
                    if pattern.is_match(old_email) {
                        let mut result = String::new();
                        result.push_str(&line_str[..header_len]);

                        let new_name = &self.new_names[i];
                        let final_name = if new_name.is_empty() {
                            old_name
                        } else {
                            new_name.as_str()
                        };
                        if !final_name.is_empty() {
                            result.push_str(final_name);
                            result.push(' ');
                        }

                        let new_email = &self.new_emails[i];
                        let final_email = if new_email.is_empty() {
                            old_email
                        } else {
                            new_email.as_str()
                        };
                        result.push('<');
                        result.push_str(final_email);
                        result.push('>');

                        result.push_str(suffix);

                        return result.into_bytes();
                    }
                }
            }
        }

        line.to_vec()
    }

    pub fn is_empty(&self) -> bool {
        self.old_email_patterns.is_empty()
    }
}

impl Clone for MailmapRewriter {
    fn clone(&self) -> Self {
        Self {
            parser: RegexStr::new(self.parser.as_str()).unwrap(),
            old_email_patterns: self
                .old_email_patterns
                .iter()
                .map(|r| RegexStr::new(r.as_str()).unwrap())
                .collect(),
            new_names: self.new_names.clone(),
            new_emails: self.new_emails.clone(),
        }
    }
}

pub fn rewrite_author_line(line: &[u8], rewriter: Option<&AuthorRewriter>) -> Vec<u8> {
    if let Some(rw) = rewriter {
        if rw.is_empty() {
            return line.to_vec();
        }
        rw.rewrite(line)
    } else {
        line.to_vec()
    }
}

pub fn rewrite_email_line(line: &[u8], rewriter: Option<&AuthorRewriter>) -> Vec<u8> {
    if let Some(rw) = rewriter {
        if rw.is_empty() {
            return line.to_vec();
        }

        let line_str = match std::str::from_utf8(line) {
            Ok(s) => s,
            Err(_) => return line.to_vec(),
        };

        if let Some(start) = line_str.find('<') {
            if let Some(end) = line_str[start..].find('>') {
                let before = &line[..start];
                let email = &line[start + 1..start + end];
                let after = &line[start + end + 1..];

                let rewritten_email = rw.rewrite(email);
                let mut result =
                    Vec::with_capacity(before.len() + rewritten_email.len() + after.len() + 3);
                result.extend_from_slice(before);
                result.push(b'<');
                result.extend_from_slice(&rewritten_email);
                result.push(b'>');
                result.extend_from_slice(after);
                return result;
            }
        }
    }
    line.to_vec()
}

pub fn rewrite_mailmap_line(line: &[u8], rewriter: Option<&MailmapRewriter>) -> Vec<u8> {
    if let Some(rw) = rewriter {
        if rw.is_empty() {
            return line.to_vec();
        }
        rw.rewrite_line(line)
    } else {
        line.to_vec()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::{HashMap, HashSet};
    use std::io::Cursor;

    #[test]
    fn finalize_promotes_first_remaining_merge_to_from() {
        let mut commit_buf = b"from :1\nmerge :2\n".to_vec();
        let first_line_len = b"from :1\n".len();
        let total_len = commit_buf.len();
        let mut parent_lines = vec![
            ParentLine::new(0, first_line_len, Some(1), ParentKind::From),
            ParentLine::new(first_line_len, total_len, Some(2), ParentKind::Merge),
        ];
        let mut first_parent_mark = Some(1);
        let emitted_marks: HashSet<u32> = [2u32].into_iter().collect();
        let alias_map: HashMap<u32, u32> = HashMap::new();

        let kept = finalize_parent_lines(
            &mut commit_buf,
            &mut parent_lines,
            &mut first_parent_mark,
            &emitted_marks,
            &alias_map,
        );

        assert_eq!(kept, 1);
        assert_eq!(commit_buf, b"from :2\n");
        assert_eq!(first_parent_mark, Some(2));
    }

    #[test]
    fn finalize_promotes_raw_merge_to_from() {
        let mut commit_buf = b"merge deadbeefdeadbeefdeadbeefdeadbeefdeadbeef\n".to_vec();
        let total_len = commit_buf.len();
        let mut parent_lines = vec![ParentLine::new(0, total_len, None, ParentKind::Merge)];
        let mut first_parent_mark = Some(42);
        let emitted_marks: HashSet<u32> = HashSet::new();
        let alias_map: HashMap<u32, u32> = HashMap::new();

        let kept = finalize_parent_lines(
            &mut commit_buf,
            &mut parent_lines,
            &mut first_parent_mark,
            &emitted_marks,
            &alias_map,
        );

        assert_eq!(kept, 1);
        assert_eq!(
            commit_buf,
            b"from deadbeefdeadbeefdeadbeefdeadbeefdeadbeef\n"
        );
        assert_eq!(first_parent_mark, None);
    }

    #[test]
    fn mailmap_rewrite_replaces_name_and_email_in_author_line() {
        let rw = MailmapRewriter::from_reader(Cursor::new(
            "New Name <new@example.com> <old@example.com>\n",
        ))
        .unwrap();
        let line = b"author Old Name <old@example.com> 1700000000 +0800\n";
        let rewritten = rw.rewrite_line(line);
        assert_eq!(
            rewritten,
            b"author New Name <new@example.com> 1700000000 +0800\n"
        );
    }

    #[test]
    fn mailmap_rewrite_preserves_name_when_rule_has_only_new_email() {
        let rw = MailmapRewriter::from_reader(Cursor::new("<new@example.com> <old@example.com>\n"))
            .unwrap();
        let line = b"author Old Name <old@example.com> 1700000000 +0800\n";
        let rewritten = rw.rewrite_line(line);
        assert_eq!(
            rewritten,
            b"author Old Name <new@example.com> 1700000000 +0800\n"
        );
    }
}
