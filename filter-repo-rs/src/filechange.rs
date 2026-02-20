use crate::opts::Options;
use crate::pathutil::{
    dequote_c_style_bytes, encode_path_for_fi_with_policy, glob_match_bytes, PathCompatEvent,
};

#[derive(Debug)]
enum FileChange {
    DeleteAll,
    Modify {
        mode: Vec<u8>,
        id: Vec<u8>,
        path: Vec<u8>,
    },
    Delete {
        path: Vec<u8>,
    },
    Copy {
        src: Vec<u8>,
        dst: Vec<u8>,
    },
    Rename {
        src: Vec<u8>,
        dst: Vec<u8>,
    },
}

// Parse a fast-export filechange line we care about. Returns None if the line
// is not recognized as a supported filechange directive.
fn parse_file_change_line(line: &[u8]) -> Option<FileChange> {
    if line == b"deleteall\n" {
        return Some(FileChange::DeleteAll);
    }
    if line.len() < 2 {
        return None;
    }
    match line[0] {
        b'M' => {
            if line.get(1).copied() != Some(b' ') {
                return None;
            }
            let rest = &line[2..];
            let space1 = rest.iter().position(|&b| b == b' ')?;
            let mode = rest[..space1].to_vec();
            let rest = &rest[space1 + 1..];
            let space2 = rest.iter().position(|&b| b == b' ')?;
            let id = rest[..space2].to_vec();
            let rest = &rest[space2 + 1..];
            let (path, tail) = parse_path(rest)?;
            if !is_line_end(tail) {
                return None;
            }
            Some(FileChange::Modify { mode, id, path })
        }
        b'D' => {
            if line.get(1).copied() != Some(b' ') {
                return None;
            }
            let rest = &line[2..];
            let (path, tail) = parse_path(rest)?;
            if !is_line_end(tail) {
                return None;
            }
            Some(FileChange::Delete { path })
        }
        b'C' => {
            if line.get(1).copied() != Some(b' ') {
                return None;
            }
            let rest = &line[2..];
            let (src, tail) = parse_path(rest)?;
            let tail = tail.strip_prefix(b" ")?;
            let (dst, tail) = parse_path(tail)?;
            if !is_line_end(tail) {
                return None;
            }
            Some(FileChange::Copy { src, dst })
        }
        b'R' => {
            if line.get(1).copied() != Some(b' ') {
                return None;
            }
            let rest = &line[2..];
            let (src, tail) = parse_path(rest)?;
            let tail = tail.strip_prefix(b" ")?;
            let (dst, tail) = parse_path(tail)?;
            if !is_line_end(tail) {
                return None;
            }
            Some(FileChange::Rename { src, dst })
        }
        _ => None,
    }
}

fn parse_path(input: &[u8]) -> Option<(Vec<u8>, &[u8])> {
    if input.is_empty() {
        return None;
    }
    if input[0] == b'"' {
        let mut idx = 1usize;
        while idx < input.len() {
            if input[idx] == b'"' {
                let mut backslashes = 0usize;
                let mut j = idx;
                while j > 0 && input[j - 1] == b'\\' {
                    backslashes += 1;
                    j -= 1;
                }
                if backslashes % 2 == 1 {
                    idx += 1;
                    continue;
                }
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
            if b == b' ' || b == b'\n' {
                return Some((input[..idx].to_vec(), &input[idx..]));
            }
            idx += 1;
        }
        Some((input.to_vec(), &input[input.len()..]))
    }
}

fn is_line_end(rest: &[u8]) -> bool {
    if rest.is_empty() {
        return true;
    }
    if rest[0] != b'\n' {
        return false;
    }
    rest[1..].is_empty()
}

fn path_matches(path: &[u8], opts: &Options) -> bool {
    if !opts.paths.is_empty() && opts.paths.iter().any(|pref| path.starts_with(pref)) {
        return true;
    }
    if !opts.path_globs.is_empty() && opts.path_globs.iter().any(|g| glob_match_bytes(g, path)) {
        return true;
    }
    if !opts.path_regexes.is_empty() && opts.path_regexes.iter().any(|re| re.is_match(path)) {
        return true;
    }
    false
}

fn should_keep(paths: &[&[u8]], opts: &Options) -> bool {
    if opts.paths.is_empty() && opts.path_globs.is_empty() && opts.path_regexes.is_empty() {
        return true;
    }
    let matched = paths.iter().copied().any(|p| path_matches(p, opts));
    opts.invert_paths ^ matched
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
    // Path renames are applied. Further sanitization and encoding is handled by `encode_path_for_fi`.
    path
}

// Return Some(new_line) if the filechange should be kept (possibly rebuilt), None to drop.
pub struct HandleFileChangeOutcome {
    pub line: Option<Vec<u8>>,
    pub path_compat_events: Vec<PathCompatEvent>,
}

fn encode_path_with_policy(
    path: &[u8],
    opts: &Options,
    path_compat_events: &mut Vec<PathCompatEvent>,
) -> Result<Option<Vec<u8>>, String> {
    let (encoded, event) = encode_path_for_fi_with_policy(path, opts.path_compat_policy)?;
    if let Some(e) = event {
        path_compat_events.push(e);
    }
    Ok(encoded)
}

// Return Some(new_line) if the filechange should be kept (possibly rebuilt), None to drop.
pub fn handle_file_change_line(
    line: &[u8],
    opts: &Options,
) -> Result<HandleFileChangeOutcome, String> {
    let parsed = match parse_file_change_line(line) {
        Some(p) => p,
        None => {
            return Ok(HandleFileChangeOutcome {
                line: Some(line.to_vec()),
                path_compat_events: Vec::new(),
            });
        }
    };

    let keep = match &parsed {
        FileChange::DeleteAll => true,
        FileChange::Modify { path, .. } => should_keep(&[path.as_slice()], opts),
        FileChange::Delete { path } => should_keep(&[path.as_slice()], opts),
        FileChange::Copy { src, dst } | FileChange::Rename { src, dst } => {
            should_keep(&[src.as_slice(), dst.as_slice()], opts)
        }
    };
    if !keep {
        return Ok(HandleFileChangeOutcome {
            line: None,
            path_compat_events: Vec::new(),
        });
    }

    let mut path_compat_events = Vec::new();
    match parsed {
        FileChange::DeleteAll => Ok(HandleFileChangeOutcome {
            line: Some(line.to_vec()),
            path_compat_events,
        }),
        FileChange::Modify { mode, id, path } => {
            let new_path = rewrite_path(path, opts);
            let enc = match encode_path_with_policy(&new_path, opts, &mut path_compat_events)? {
                Some(enc) => enc,
                None => {
                    return Ok(HandleFileChangeOutcome {
                        line: None,
                        path_compat_events,
                    });
                }
            };
            let mut rebuilt = Vec::with_capacity(line.len() + new_path.len());
            rebuilt.extend_from_slice(b"M ");
            rebuilt.extend_from_slice(&mode);
            rebuilt.push(b' ');
            rebuilt.extend_from_slice(&id);
            rebuilt.push(b' ');
            rebuilt.extend_from_slice(&enc);
            rebuilt.push(b'\n');
            Ok(HandleFileChangeOutcome {
                line: Some(rebuilt),
                path_compat_events,
            })
        }
        FileChange::Delete { path } => {
            let new_path = rewrite_path(path, opts);
            let enc = match encode_path_with_policy(&new_path, opts, &mut path_compat_events)? {
                Some(enc) => enc,
                None => {
                    return Ok(HandleFileChangeOutcome {
                        line: None,
                        path_compat_events,
                    });
                }
            };
            let mut rebuilt = Vec::with_capacity(2 + new_path.len() + 2);
            rebuilt.extend_from_slice(b"D ");
            rebuilt.extend_from_slice(&enc);
            rebuilt.push(b'\n');
            Ok(HandleFileChangeOutcome {
                line: Some(rebuilt),
                path_compat_events,
            })
        }
        FileChange::Copy { src, dst } => {
            let new_src = rewrite_path(src, opts);
            let new_dst = rewrite_path(dst, opts);
            let enc_src = match encode_path_with_policy(&new_src, opts, &mut path_compat_events)? {
                Some(enc) => enc,
                None => {
                    return Ok(HandleFileChangeOutcome {
                        line: None,
                        path_compat_events,
                    });
                }
            };
            let enc_dst = match encode_path_with_policy(&new_dst, opts, &mut path_compat_events)? {
                Some(enc) => enc,
                None => {
                    return Ok(HandleFileChangeOutcome {
                        line: None,
                        path_compat_events,
                    });
                }
            };
            let mut rebuilt = Vec::with_capacity(line.len() + new_src.len() + new_dst.len());
            rebuilt.extend_from_slice(b"C ");
            rebuilt.extend_from_slice(&enc_src);
            rebuilt.push(b' ');
            rebuilt.extend_from_slice(&enc_dst);
            rebuilt.push(b'\n');
            Ok(HandleFileChangeOutcome {
                line: Some(rebuilt),
                path_compat_events,
            })
        }
        FileChange::Rename { src, dst } => {
            let new_src = rewrite_path(src, opts);
            let new_dst = rewrite_path(dst, opts);
            let enc_src = match encode_path_with_policy(&new_src, opts, &mut path_compat_events)? {
                Some(enc) => enc,
                None => {
                    return Ok(HandleFileChangeOutcome {
                        line: None,
                        path_compat_events,
                    });
                }
            };
            let enc_dst = match encode_path_with_policy(&new_dst, opts, &mut path_compat_events)? {
                Some(enc) => enc,
                None => {
                    return Ok(HandleFileChangeOutcome {
                        line: None,
                        path_compat_events,
                    });
                }
            };
            let mut rebuilt = Vec::with_capacity(line.len() + new_src.len() + new_dst.len());
            rebuilt.extend_from_slice(b"R ");
            rebuilt.extend_from_slice(&enc_src);
            rebuilt.push(b' ');
            rebuilt.extend_from_slice(&enc_dst);
            rebuilt.push(b'\n');
            Ok(HandleFileChangeOutcome {
                line: Some(rebuilt),
                path_compat_events,
            })
        }
    }
}
