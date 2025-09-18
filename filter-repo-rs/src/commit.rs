use std::collections::BTreeSet;
use std::fs::File;
use std::io::{self, Read, Write};
use std::io::BufReader;
use std::process::{ChildStdout, ChildStdin};

use crate::opts::Options;
use crate::message::MessageReplacer;
use crate::filechange;

pub fn rename_commit_header_ref(
  line: &[u8],
  opts: &Options,
  ref_renames: &mut BTreeSet<(Vec<u8>, Vec<u8>)>,
) -> Vec<u8> {
  if !line.starts_with(b"commit ") { return line.to_vec(); }
  let mut refname = &line[b"commit ".len()..];
  if let Some(&last) = refname.last() { if last == b'\n' { refname = &refname[..refname.len()-1]; } }
  // tags
  if refname.starts_with(b"refs/tags/") {
    if let Some((ref old, ref new_)) = opts.tag_rename {
      let name = &refname[b"refs/tags/".len()..];
      if name.starts_with(&old[..]) {
        let mut rebuilt = Vec::with_capacity(7 + b"refs/tags/".len() + new_.len() + (name.len()-old.len()) + 1);
        rebuilt.extend_from_slice(b"commit ");
        rebuilt.extend_from_slice(b"refs/tags/");
        rebuilt.extend_from_slice(&new_);
        rebuilt.extend_from_slice(&name[old.len()..]);
        rebuilt.push(b'\n');
        let new_full = [b"refs/tags/".as_ref(), new_.as_slice(), &name[old.len()..]].concat();
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
        let mut rebuilt = Vec::with_capacity(7 + b"refs/heads/".len() + new_.len() + (name.len()-old.len()) + 1);
        rebuilt.extend_from_slice(b"commit ");
        rebuilt.extend_from_slice(b"refs/heads/");
        rebuilt.extend_from_slice(&new_);
        rebuilt.extend_from_slice(&name[old.len()..]);
        rebuilt.push(b'\n');
        let new_full = [b"refs/heads/".as_ref(), new_.as_slice(), &name[old.len()..]].concat();
        ref_renames.insert((refname.to_vec(), new_full));
        return rebuilt;
      }
    }
  }
  line.to_vec()
}

pub enum CommitAction { Consumed, Ended }

#[allow(dead_code)]
pub fn start_commit(
  line: &[u8],
  opts: &Options,
  ref_renames: &mut BTreeSet<(Vec<u8>, Vec<u8>)>,
  commit_buf: &mut Vec<u8>,
  commit_has_changes: &mut bool,
  commit_mark: &mut Option<u32>,
  first_parent_mark: &mut Option<u32>,
) -> bool {
  if !line.starts_with(b"commit ") { return false; }
  *commit_has_changes = false;
  *commit_mark = None;
  *first_parent_mark = None;
  commit_buf.clear();
  let hdr = rename_commit_header_ref(line, opts, ref_renames);
  commit_buf.extend_from_slice(&hdr);
  true
}

pub fn process_commit_line(
  line: &[u8],
  opts: &Options,
  fe_out: &mut BufReader<ChildStdout>,
  orig_file: &mut File,
  filt_file: &mut File,
  mut fi_in: Option<&mut ChildStdin>,
  replacer: &Option<MessageReplacer>,
  commit_buf: &mut Vec<u8>,
  commit_has_changes: &mut bool,
  commit_mark: &mut Option<u32>,
  first_parent_mark: &mut Option<u32>,
  commit_original_oid: &mut Option<Vec<u8>>,
  parent_count: &mut usize,
  commit_pairs: &mut Vec<(Vec<u8>, Option<u32>)>,
  import_broken: &mut bool,
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
    if let Some(last) = v.last() { if *last == b'\n' { v.pop(); } }
    *commit_original_oid = Some(v);
    commit_buf.extend_from_slice(line);
    return Ok(CommitAction::Consumed);
  }
  // commit message data
  if line.starts_with(b"data ") {
    handle_commit_data(line, fe_out, orig_file, commit_buf, replacer)?;
    return Ok(CommitAction::Consumed);
  }
  // parents
  if line.starts_with(b"from ") {
    if first_parent_mark.is_none() {
      if let Some(m) = parse_from_mark(line) { *first_parent_mark = Some(m); }
    }
    *parent_count = 1;
    commit_buf.extend_from_slice(line);
    return Ok(CommitAction::Consumed);
  }
  if line.starts_with(b"merge ") {
    commit_buf.extend_from_slice(line);
    *parent_count = parent_count.saturating_add(1);
    return Ok(CommitAction::Consumed);
  }
  // file changes with path filtering
  if line.starts_with(b"M ") || line.starts_with(b"D ") || line.starts_with(b"C ") || line.starts_with(b"R ") || line == b"deleteall\n" {
    if let Some(newline) = filechange::handle_file_change_line(line, opts) {
      commit_buf.extend_from_slice(&newline);
      *commit_has_changes = true;
    }
    return Ok(CommitAction::Consumed);
  }
  // end of commit (blank line)
  if line == b"\n" {
    if should_keep_commit(*commit_has_changes, *first_parent_mark, *commit_mark, *parent_count) {
      // keep commit
      commit_buf.extend_from_slice(b"\n");
      filt_file.write_all(&commit_buf)?;
      if let Some(ref mut fi) = fi_in { if let Err(e) = fi.write_all(&commit_buf) { if e.kind()==io::ErrorKind::BrokenPipe { *import_broken=true; } else { return Err(e); } } }
      // Record mark and original id for later resolution via marks file
      if let Some(old) = commit_original_oid.take() {
        if let Some(m) = *commit_mark {
          commit_pairs.push((old, Some(m)));
        }
      }
    } else {
      if let Some(old) = commit_original_oid.take() { commit_pairs.push((old, None)); }
      // prune commit: only alias if we have both marks and parent mark has been emitted
      if let (Some(old_mark), Some(parent_mark)) = (*commit_mark, *first_parent_mark) {
        if emitted_marks.contains(&parent_mark) {
          let alias = build_alias(old_mark, parent_mark);
          filt_file.write_all(&alias)?;
          if let Some(ref mut fi) = fi_in { if let Err(e) = fi.write_all(&alias) { if e.kind()==io::ErrorKind::BrokenPipe { *import_broken=true; } else { return Err(e); } } }
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
  if !line.starts_with(b"mark :") { return None; }
  let mut num: u32 = 0; let mut seen = false;
  for &b in line[b"mark :".len()..].iter() {
    if b >= b'0' && b <= b'9' { seen = true; num = num.saturating_mul(10).saturating_add((b - b'0') as u32); }
    else { break; }
  }
  if seen { Some(num) } else { None }
}

// Parse a 'from :<num>' line and return the numeric mark
pub fn parse_from_mark(line: &[u8]) -> Option<u32> {
  if !line.starts_with(b"from ") { return None; }
  if line.get(b"from ".len()).copied() != Some(b':') { return None; }
  let mut num: u32 = 0; let mut seen=false;
  for &b in line[b"from :".len()..].iter() {
    if b >= b'0' && b <= b'9' { seen=true; num=num.saturating_mul(10).saturating_add((b-b'0') as u32);} else {break;}
  }
  if seen { Some(num) } else { None }
}

// Handle a commit message 'data <n>' header line: read payload from fe_out,
// mirror to orig_file, apply replacer, and append to commit_buf.
pub fn handle_commit_data(
  header_line: &[u8],
  fe_out: &mut BufReader<ChildStdout>,
  orig_file: &mut File,
  commit_buf: &mut Vec<u8>,
  replacer: &Option<MessageReplacer>,
) -> io::Result<()> {
  if !header_line.starts_with(b"data ") { return Ok(()); }
  let size_bytes = &header_line[b"data ".len()..];
  let n = std::str::from_utf8(size_bytes)
    .ok().map(|s| s.trim()).and_then(|s| s.parse::<usize>().ok())
    .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "invalid data header"))?;
  let mut payload = vec![0u8; n];
  fe_out.read_exact(&mut payload)?;
  orig_file.write_all(&payload)?;
  let new_payload = if let Some(r) = replacer { r.apply(payload) } else { payload };
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
