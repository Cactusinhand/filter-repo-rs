use crate::opts::Options;
use crate::pathutil::{
  dequote_c_style_bytes, enquote_c_style_bytes, glob_match_bytes, needs_c_style_quote,
  sanitize_invalid_windows_path_bytes,
};

#[derive(Debug)]
enum FileChange {
  DeleteAll,
  Modify { mode: Vec<u8>, id: Vec<u8>, path: Vec<u8> },
  Delete { path: Vec<u8> },
  Copy { src: Vec<u8>, dst: Vec<u8> },
  Rename { src: Vec<u8>, dst: Vec<u8> },
}

// Parse a fast-export filechange line we care about. Returns None if the line
// is not recognized as a supported filechange directive.
fn parse_file_change_line(line: &[u8]) -> Option<FileChange> {
  if line == b"deleteall\n" { return Some(FileChange::DeleteAll); }
  if line.len() < 2 { return None; }
  match line[0] {
    b'M' => {
      if line.get(1).copied() != Some(b' ') { return None; }
      let rest = &line[2..];
      let space1 = rest.iter().position(|&b| b == b' ')?;
      let mode = rest[..space1].to_vec();
      let rest = &rest[space1 + 1..];
      let space2 = rest.iter().position(|&b| b == b' ')?;
      let id = rest[..space2].to_vec();
      let rest = &rest[space2 + 1..];
      let (path, tail) = parse_path(rest)?;
      if !is_line_end(tail) { return None; }
      Some(FileChange::Modify { mode, id, path })
    }
    b'D' => {
      if line.get(1).copied() != Some(b' ') { return None; }
      let rest = &line[2..];
      let (path, tail) = parse_path(rest)?;
      if !is_line_end(tail) { return None; }
      Some(FileChange::Delete { path })
    }
    b'C' => {
      if line.get(1).copied() != Some(b' ') { return None; }
      let rest = &line[2..];
      let (src, tail) = parse_path(rest)?;
      let tail = tail.strip_prefix(b" ")?;
      let (dst, tail) = parse_path(tail)?;
      if !is_line_end(tail) { return None; }
      Some(FileChange::Copy { src, dst })
    }
    b'R' => {
      if line.get(1).copied() != Some(b' ') { return None; }
      let rest = &line[2..];
      let (src, tail) = parse_path(rest)?;
      let tail = tail.strip_prefix(b" ")?;
      let (dst, tail) = parse_path(tail)?;
      if !is_line_end(tail) { return None; }
      Some(FileChange::Rename { src, dst })
    }
    _ => None,
  }
}

fn parse_path(input: &[u8]) -> Option<(Vec<u8>, &[u8])> {
  if input.is_empty() { return None; }
  if input[0] == b'"' {
    let mut idx = 1usize;
    while idx < input.len() {
      if input[idx] == b'"' {
        let mut backslashes = 0usize;
        let mut j = idx;
        while j > 0 && input[j - 1] == b'\\' { backslashes += 1; j -= 1; }
        if backslashes % 2 == 1 { idx += 1; continue; }
        let decoded = dequote_c_style_bytes(&input[1..idx]);
        let rest = &input[idx + 1..];
        return Some((decoded, rest));
      }
      idx += 1;
    }
    None
  } else {
    let mut idx = 0usize;
    while idx < input.len() {
      let b = input[idx];
      if b == b' ' || b == b'\n' { return Some((input[..idx].to_vec(), &input[idx..])); }
      idx += 1;
    }
    Some((input.to_vec(), &input[input.len()..]))
  }
}

fn is_line_end(rest: &[u8]) -> bool {
  if rest.is_empty() { return true; }
  if rest[0] != b'\n' { return false; }
  rest[1..].is_empty()
}

fn path_matches(path: &[u8], opts: &Options) -> bool {
  if !opts.paths.is_empty() {
    if opts.paths.iter().any(|pref| path.starts_with(pref)) { return true; }
  }
  if !opts.path_globs.is_empty() {
    if opts.path_globs.iter().any(|g| glob_match_bytes(g, path)) { return true; }
  }
  false
}

fn should_keep(paths: &[&[u8]], opts: &Options) -> bool {
  if opts.paths.is_empty() && opts.path_globs.is_empty() { return true; }
  let matched = paths.iter().copied().any(|p| path_matches(p, opts));
  if opts.invert_paths { !matched } else { matched }
}

fn rewrite_path(mut path: Vec<u8>, opts: &Options) -> Vec<u8> {
  if !opts.path_renames.is_empty() {
    for (old, new_) in &opts.path_renames {
      if path.starts_with(old) {
        let mut tmp = new_.clone();
        tmp.extend_from_slice(&path[old.len()..]);
        path = tmp;
      }
    }
  }
  sanitize_invalid_windows_path_bytes(&path)
}

fn encode_path(path: &[u8]) -> Vec<u8> {
  if needs_c_style_quote(path) { enquote_c_style_bytes(path) } else { path.to_vec() }
}

// Return Some(new_line) if the filechange should be kept (possibly rebuilt), None to drop.
pub fn handle_file_change_line(line: &[u8], opts: &Options) -> Option<Vec<u8>> {
  let parsed = match parse_file_change_line(line) {
    Some(p) => p,
    None => return Some(line.to_vec()),
  };

  let keep = match &parsed {
    FileChange::DeleteAll => true,
    FileChange::Modify { path, .. } => should_keep(&[path.as_slice()], opts),
    FileChange::Delete { path } => should_keep(&[path.as_slice()], opts),
    FileChange::Copy { src, dst } | FileChange::Rename { src, dst } => {
      should_keep(&[src.as_slice(), dst.as_slice()], opts)
    }
  };
  if !keep { return None; }

  match parsed {
    FileChange::DeleteAll => Some(line.to_vec()),
    FileChange::Modify { mode, id, path } => {
      let new_path = rewrite_path(path, opts);
      let mut rebuilt = Vec::with_capacity(line.len() + new_path.len());
      rebuilt.extend_from_slice(b"M ");
      rebuilt.extend_from_slice(&mode);
      rebuilt.push(b' ');
      rebuilt.extend_from_slice(&id);
      rebuilt.push(b' ');
      let enc = encode_path(&new_path);
      rebuilt.extend_from_slice(&enc);
      rebuilt.push(b'\n');
      Some(rebuilt)
    }
    FileChange::Delete { path } => {
      let new_path = rewrite_path(path, opts);
      let mut rebuilt = Vec::with_capacity(2 + new_path.len() + 2);
      rebuilt.extend_from_slice(b"D ");
      let enc = encode_path(&new_path);
      rebuilt.extend_from_slice(&enc);
      rebuilt.push(b'\n');
      Some(rebuilt)
    }
    FileChange::Copy { src, dst } => {
      let new_src = rewrite_path(src, opts);
      let new_dst = rewrite_path(dst, opts);
      let mut rebuilt = Vec::with_capacity(line.len() + new_src.len() + new_dst.len());
      rebuilt.extend_from_slice(b"C ");
      let enc_src = encode_path(&new_src);
      rebuilt.extend_from_slice(&enc_src);
      rebuilt.push(b' ');
      let enc_dst = encode_path(&new_dst);
      rebuilt.extend_from_slice(&enc_dst);
      rebuilt.push(b'\n');
      Some(rebuilt)
    }
    FileChange::Rename { src, dst } => {
      let new_src = rewrite_path(src, opts);
      let new_dst = rewrite_path(dst, opts);
      let mut rebuilt = Vec::with_capacity(line.len() + new_src.len() + new_dst.len());
      rebuilt.extend_from_slice(b"R ");
      let enc_src = encode_path(&new_src);
      rebuilt.extend_from_slice(&enc_src);
      rebuilt.push(b' ');
      let enc_dst = encode_path(&new_dst);
      rebuilt.extend_from_slice(&enc_dst);
      rebuilt.push(b'\n');
      Some(rebuilt)
    }
  }
}
