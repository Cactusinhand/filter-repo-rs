use crate::opts::Options;
use crate::pathutil::{sanitize_invalid_windows_path_bytes, glob_match_bytes, dequote_c_style_bytes, enquote_c_style_bytes, needs_c_style_quote};

// Return Some(new_line) if the filechange should be kept (possibly rebuilt), None to drop.
pub fn handle_file_change_line(line: &[u8], opts: &Options) -> Option<Vec<u8>> {
  // Keep deleteall as-is
  if line == b"deleteall\n" { return Some(line.to_vec()); }

  // Determine if we keep this path based on --path prefixes / --path-glob patterns
  let keep = {
    let path_bytes: &[u8] = if line[0] == b'M' {
      // find third space: M <mode> <id> <path>\n
      let mut spaces = 0usize; let mut idx = 0usize; let bytes = line;
      for (i,b) in bytes.iter().enumerate(){ if *b==b' ' { spaces+=1; if spaces==3 { idx=i+1; break; } } }
      if spaces < 3 { &line[..0] } else { &line[idx..] }
    } else { &line[2..] };
    let mut p = path_bytes;
    if let Some(&last)=p.last(){ if last==b'\n' { p=&p[..p.len()-1]; } }
    let p2: Vec<u8> = if p.len()>=2 && p[0]==b'"' && p[p.len()-1]==b'"' { dequote_c_style_bytes(&p[1..p.len()-1]) } else { p.to_vec() };
    let mut matched = false;
    if !opts.paths.is_empty() {
      if opts.paths.iter().any(|pref| p2.starts_with(pref)) { matched = true; }
    }
    if !opts.path_globs.is_empty() && !matched {
      if opts.path_globs.iter().any(|g| glob_match_bytes(g, &p2)) { matched = true; }
    }
    if opts.paths.is_empty() && opts.path_globs.is_empty() { true }
    else if opts.invert_paths { !matched } else { matched }
  };
  if !keep { return None; }

  // Rebuild path (apply renames and sanitize)
  let path_start = if line[0]==b'M' {
    let mut spaces=0usize; let mut idx=0usize; let bytes = line;
    for (i,b) in bytes.iter().enumerate(){ if *b==b' ' { spaces+=1; if spaces==3 { idx=i+1; break; } } }
    idx
  } else { 2 };
  let (head, tail) = line.split_at(path_start);
  let mut p = tail;
  if let Some(&last)=p.last(){ if last==b'\n' { p=&p[..p.len()-1]; } }
  let was_quoted = p.len()>=2 && p[0]==b'"' && p[p.len()-1]==b'"';
  let p_unquoted: Vec<u8> = if was_quoted { dequote_c_style_bytes(&p[1..p.len()-1]) } else { p.to_vec() };
  let mut newp = p_unquoted;
  if !opts.path_renames.is_empty() {
    for (old,new_) in &opts.path_renames {
      if newp.starts_with(old) {
        let mut tmp = new_.clone();
        tmp.extend_from_slice(&newp[old.len()..]);
        newp = tmp;
      }
    }
  }
  let newp = sanitize_invalid_windows_path_bytes(&newp);
  let mut rebuilt = Vec::with_capacity(line.len()+newp.len());
  rebuilt.extend_from_slice(head);
  if needs_c_style_quote(&newp) {
    let q = enquote_c_style_bytes(&newp);
    rebuilt.extend_from_slice(&q);
  } else {
    rebuilt.extend_from_slice(&newp);
  }
  rebuilt.push(b'\n');
  Some(rebuilt)
}
