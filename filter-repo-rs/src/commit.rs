use std::collections::{BTreeSet, HashMap};
use std::io::BufReader;
use std::io::{self, Read, Write};
use std::process::{ChildStdin, ChildStdout};

use crate::filechange;
use crate::message::{MessageReplacer, ShortHashMapper};
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
                rebuilt.extend_from_slice(&new_);
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
                rebuilt.extend_from_slice(&new_);
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

#[allow(dead_code)]
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

pub fn process_commit_line(
    line: &[u8],
    opts: &Options,
    fe_out: &mut BufReader<ChildStdout>,
    orig_file: Option<&mut dyn Write>,
    filt_file: &mut dyn Write,
    mut fi_in: Option<&mut ChildStdin>,
    replacer: &Option<MessageReplacer>,
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
        handle_commit_data(line, fe_out, orig_file, commit_buf, replacer, short_mapper)?;
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
        let kept_parents = finalize_parent_lines(
            commit_buf,
            parent_lines,
            first_parent_mark,
            emitted_marks,
            alias_map,
        );
        *parent_count = kept_parents;
        if should_keep_commit(
            *commit_has_changes,
            *first_parent_mark,
            *commit_mark,
            *parent_count,
        ) {
            // keep commit
            commit_buf.extend_from_slice(b"\n");
            filt_file.write_all(&commit_buf)?;
            if let Some(ref mut fi) = fi_in {
                if let Err(e) = fi.write_all(&commit_buf) {
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
        if b >= b'0' && b <= b'9' {
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
        if b >= b'0' && b <= b'9' {
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
        if b >= b'0' && b <= b'9' {
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
    short_mapper: Option<&ShortHashMapper>,
) -> io::Result<()> {
    if !header_line.starts_with(b"data ") {
        return Ok(());
    }
    let size_bytes = &header_line[b"data ".len()..];
    let n = std::str::from_utf8(size_bytes)
        .ok()
        .map(|s| s.trim())
        .and_then(|s| s.parse::<usize>().ok())
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "invalid data header"))?;
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
) -> bool {
    let is_merge = parent_count >= 2;
    commit_has_changes || first_parent_mark.is_none() || commit_mark.is_none() || is_merge
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

    let mut replacements: Vec<Option<Vec<u8>>> = Vec::with_capacity(parent_lines.len());
    let mut seen_canonical: BTreeSet<u32> = BTreeSet::new();
    let mut first_kept: Option<u32> = None;
    let mut kept_count: usize = 0;

    for parent in parent_lines.iter() {
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
            if first_kept.is_none() {
                first_kept = Some(canonical);
            }
            replacements.push(Some(rebuild_parent_line(parent.kind, canonical)));
            kept_count += 1;
        } else {
            let line = commit_buf[parent.start..parent.end].to_vec();
            replacements.push(Some(line));
            kept_count += 1;
        }
    }

    let mut new_buf = Vec::with_capacity(commit_buf.len());
    let mut cursor = 0usize;
    for (parent, replacement) in parent_lines.iter().zip(replacements.into_iter()) {
        if cursor < parent.start {
            new_buf.extend_from_slice(&commit_buf[cursor..parent.start]);
        }
        if let Some(bytes) = replacement {
            new_buf.extend_from_slice(&bytes);
        }
        cursor = parent.end;
    }
    if cursor < commit_buf.len() {
        new_buf.extend_from_slice(&commit_buf[cursor..]);
    }

    *commit_buf = new_buf;
    parent_lines.clear();
    *first_parent_mark = first_kept;
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
