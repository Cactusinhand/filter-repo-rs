use std::collections::BTreeSet;
use std::io::{self, BufRead, BufReader, Read, Write};
use std::process::ChildStdout;

use crate::message::{msg_regex, MessageReplacer, ShortHashMapper};
use crate::opts::Options;

pub struct TagProcessContext<'a> {
    pub fe_out: &'a mut BufReader<ChildStdout>,
    pub orig_file: Option<&'a mut dyn Write>,
    pub filt_file: &'a mut dyn Write,
    pub fi_in: Option<&'a mut dyn Write>,
    pub replacer: &'a Option<MessageReplacer>,
    pub msg_regex: Option<&'a msg_regex::RegexReplacer>,
    pub short_mapper: Option<&'a ShortHashMapper>,
    pub opts: &'a Options,
    pub updated_refs: &'a mut BTreeSet<Vec<u8>>,
    pub annotated_tag_refs: &'a mut BTreeSet<Vec<u8>>,
    pub ref_renames: &'a mut BTreeSet<(Vec<u8>, Vec<u8>)>,
    pub emitted_marks: &'a mut std::collections::HashSet<u32>,
}

pub fn precheck_duplicate_tag(
    line: &[u8],
    opts: &Options,
    updated_refs: &BTreeSet<Vec<u8>>,
) -> bool {
    if !line.starts_with(b"tag ") {
        return false;
    }
    if let Some((ref old, ref new_)) = opts.tag_rename {
        let mut name = &line[b"tag ".len()..];
        if let Some(&last) = name.last() {
            if last == b'\n' {
                name = &name[..name.len() - 1];
            }
        }
        let mut renamed = name.to_vec();
        if renamed.starts_with(&old[..]) {
            let mut v = new_.to_vec();
            v.extend_from_slice(&renamed[old.len()..]);
            renamed = v;
        }
        let target_ref = [b"refs/tags/".as_ref(), renamed.as_slice()].concat();
        return updated_refs.contains(&target_ref);
    }
    false
}

pub fn process_tag_block(first_line: &[u8], mut ctx: TagProcessContext<'_>) -> io::Result<()> {
    // Extract tag name
    let mut tagname = &first_line[b"tag ".len()..];
    if let Some(&last) = tagname.last() {
        if last == b'\n' {
            tagname = &tagname[..tagname.len() - 1];
        }
    }

    // Buffer header lines until data
    let mut hdrs: Vec<Vec<u8>> = Vec::new();
    loop {
        let mut l = Vec::with_capacity(256);
        let read2 = ctx.fe_out.read_until(b'\n', &mut l)?;
        if read2 == 0 {
            break;
        }
        if let Some(f) = ctx.orig_file.as_mut() {
            (*f).write_all(&l)?;
        }
        if l.starts_with(b"data ") {
            // Read payload
            let size_bytes = &l[b"data ".len()..];
            let n = std::str::from_utf8(size_bytes)
                .ok()
                .map(|s| s.trim())
                .and_then(|s| s.parse::<usize>().ok())
                .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "invalid data header"))?;
            let mut payload = vec![0u8; n];
            ctx.fe_out.read_exact(&mut payload)?;
            if let Some(f) = ctx.orig_file.as_mut() {
                (*f).write_all(&payload)?;
            }

            // Rename tag name
            let mut renamed = tagname.to_vec();
            if let Some((ref old, ref new_)) = ctx.opts.tag_rename {
                if renamed.starts_with(&old[..]) {
                    let mut v = new_.clone();
                    v.extend_from_slice(&renamed[old.len()..]);
                    renamed = v;
                }
            }
            let target_ref = [b"refs/tags/".as_ref(), renamed.as_slice()].concat();

            // Dedupe annotated tags
            if ctx.updated_refs.contains(&target_ref) {
                return Ok(()); // skip emitting
            }
            ctx.updated_refs.insert(target_ref.clone());
            ctx.annotated_tag_refs.insert(target_ref.clone());
            if renamed != tagname {
                let old_full = [b"refs/tags/".as_ref(), tagname].concat();
                ctx.ref_renames.insert((old_full, target_ref.clone()));
            }

            // Emit to filtered/import streams
            let mut out = Vec::with_capacity(5 + renamed.len() + 1);
            out.extend_from_slice(b"tag ");
            out.extend_from_slice(&renamed);
            out.push(b'\n');
            ctx.filt_file.write_all(&out)?;
            if let Some(ref mut fi) = ctx.fi_in {
                fi.write_all(&out)?;
            }
            for h in hdrs.into_iter() {
                ctx.filt_file.write_all(&h)?;
                if let Some(ref mut fi) = ctx.fi_in {
                    fi.write_all(&h)?;
                }
                // Record emitted tag mark
                if h.starts_with(b"mark :") {
                    let mut num: u32 = 0;
                    let mut seen = false;
                    for &b in h[b"mark :".len()..].iter() {
                        if b.is_ascii_digit() {
                            seen = true;
                            num = num.saturating_mul(10).saturating_add((b - b'0') as u32);
                        } else {
                            break;
                        }
                    }
                    if seen {
                        ctx.emitted_marks.insert(num);
                    }
                }
            }

            if ctx.replacer.is_none() && ctx.msg_regex.is_none() && ctx.short_mapper.is_none() {
                // No modifications needed; forward header and payload without cloning
                let header = format!("data {}\n", payload.len());
                ctx.filt_file.write_all(header.as_bytes())?;
                ctx.filt_file.write_all(&payload)?;
                if let Some(ref mut fi) = ctx.fi_in {
                    fi.write_all(header.as_bytes())?;
                    fi.write_all(&payload)?;
                }
            } else {
                let mut new_payload = if let Some(r) = ctx.replacer {
                    r.apply(payload)
                } else {
                    payload
                };
                if let Some(rr) = ctx.msg_regex {
                    new_payload = rr.apply_regex(new_payload);
                }
                if let Some(mapper) = ctx.short_mapper {
                    new_payload = mapper.rewrite(new_payload);
                }
                let header = format!("data {}\n", new_payload.len());
                ctx.filt_file.write_all(header.as_bytes())?;
                ctx.filt_file.write_all(&new_payload)?;
                if let Some(ref mut fi) = ctx.fi_in {
                    fi.write_all(header.as_bytes())?;
                    fi.write_all(&new_payload)?;
                }
            }
            return Ok(());
        } else {
            hdrs.push(l.clone());
        }
    }
    Ok(())
}

// If a previous 'reset refs/tags/<name>' was seen, capture the following
// 'from ' line into the buffered_tag_resets list and indicate the line was handled.
pub fn maybe_capture_pending_tag_reset(
    pending_tag_reset: &mut Option<Vec<u8>>,
    line: &[u8],
    buffered_tag_resets: &mut Vec<(Vec<u8>, Vec<u8>)>,
) -> bool {
    if let Some(ref_full) = pending_tag_reset.take() {
        if line.starts_with(b"from ") {
            buffered_tag_resets.push((ref_full, line.to_vec()));
            return true;
        }
    }
    false
}

// Handle 'reset refs/tags/<name>' lines for lightweight tags: apply --tag-rename
// mapping, record ref_renames, and set pending_tag_reset to capture the next 'from '.
pub fn process_reset_header(
    line: &[u8],
    opts: &Options,
    ref_renames: &mut BTreeSet<(Vec<u8>, Vec<u8>)>,
    pending_tag_reset: &mut Option<Vec<u8>>,
) -> bool {
    if !line.starts_with(b"reset ") {
        return false;
    }
    let mut name = &line[b"reset ".len()..];
    if let Some(&last) = name.last() {
        if last == b'\n' {
            name = &name[..name.len() - 1];
        }
    }
    if !name.starts_with(b"refs/tags/") {
        return false;
    }
    let mut ref_full = name.to_vec();
    if let Some((ref old, ref new_)) = opts.tag_rename {
        let tagname = &name[b"refs/tags/".len()..];
        if tagname.starts_with(&old[..]) {
            let new_full = [b"refs/tags/".as_ref(), new_, &tagname[old.len()..]].concat();
            ref_renames.insert((name.to_vec(), new_full.clone()));
            ref_full = new_full;
        }
    }
    *pending_tag_reset = Some(ref_full);
    true
}
