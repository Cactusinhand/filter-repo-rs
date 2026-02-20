use std::collections::{HashMap, HashSet};
use std::io::{self, BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use regex::bytes::Regex;

use crate::Options;

const OUTPUT_FILE_NAME: &str = "detected-secrets.txt";
const REDACTION: &str = "***REMOVED***";
const MAX_SCAN_BLOB_BYTES: u64 = 2 * 1024 * 1024;
const MAX_DETECTED_VALUES: usize = 500;

struct SecretPattern {
    name: String,
    regex: Regex,
    capture_group: Option<usize>,
}

#[derive(Debug, Clone)]
struct BlobCandidate {
    oid: String,
    path: Option<String>,
}

#[derive(Debug, Clone)]
struct Detection {
    value: String,
    pattern: String,
    oid: String,
    path: Option<String>,
}

pub fn run(opts: &Options) -> io::Result<()> {
    let patterns = build_patterns(opts)?;
    let candidates = collect_blob_candidates(&opts.source)?;
    let detections = scan_blob_candidates(&opts.source, &candidates, &patterns)?;
    let output_path = write_detection_draft(&opts.source, &detections)?;

    println!(
        "Detected {} potential secrets, wrote {}",
        detections.len(),
        output_path.display()
    );

    Ok(())
}

fn build_patterns(opts: &Options) -> io::Result<Vec<SecretPattern>> {
    let mut patterns = Vec::new();
    patterns.push(SecretPattern {
        name: "aws_access_key_id".to_string(),
        regex: Regex::new(r"\b(?:AKIA|ASIA)[0-9A-Z]{16}\b")
            .map_err(|e| io::Error::other(format!("invalid aws_access_key_id regex: {e}")))?,
        capture_group: None,
    });
    patterns.push(SecretPattern {
        name: "aws_secret_access_key".to_string(),
        regex: Regex::new(
            r#"(?i)\baws(?:_|-)?secret(?:_|-)?access(?:_|-)?key\b\s*[:=]\s*["']?([A-Za-z0-9/+=]{40})["']?"#,
        )
        .map_err(|e| io::Error::other(format!("invalid aws_secret_access_key regex: {e}")))?,
        capture_group: Some(1),
    });
    patterns.push(SecretPattern {
        name: "github_token".to_string(),
        regex: Regex::new(r"\bgh[pousr]_[A-Za-z0-9]{36}\b")
            .map_err(|e| io::Error::other(format!("invalid github_token regex: {e}")))?,
        capture_group: None,
    });
    patterns.push(SecretPattern {
        name: "github_pat".to_string(),
        regex: Regex::new(r"\bgithub_pat_[A-Za-z0-9_]{20,255}\b")
            .map_err(|e| io::Error::other(format!("invalid github_pat regex: {e}")))?,
        capture_group: None,
    });
    patterns.push(SecretPattern {
        name: "slack_token".to_string(),
        regex: Regex::new(r"\bxox[baprs]-[A-Za-z0-9-]{10,128}\b")
            .map_err(|e| io::Error::other(format!("invalid slack_token regex: {e}")))?,
        capture_group: None,
    });
    patterns.push(SecretPattern {
        name: "slack_webhook_url".to_string(),
        regex: Regex::new(
            r"https://hooks\.slack\.com/services/T[A-Z0-9]{8,}/B[A-Z0-9]{8,}/[A-Za-z0-9]{24,}",
        )
        .map_err(|e| io::Error::other(format!("invalid slack_webhook_url regex: {e}")))?,
        capture_group: None,
    });
    patterns.push(SecretPattern {
        name: "google_api_key".to_string(),
        regex: Regex::new(r"\bAIza[0-9A-Za-z_-]{35}\b")
            .map_err(|e| io::Error::other(format!("invalid google_api_key regex: {e}")))?,
        capture_group: None,
    });
    patterns.push(SecretPattern {
        name: "google_oauth_refresh_token".to_string(),
        regex: Regex::new(r"\b1//[0-9A-Za-z_-]{20,}\b").map_err(|e| {
            io::Error::other(format!("invalid google_oauth_refresh_token regex: {e}"))
        })?,
        capture_group: None,
    });
    patterns.push(SecretPattern {
        name: "gitlab_pat".to_string(),
        regex: Regex::new(r"\bglpat-[0-9A-Za-z_-]{20,}\b")
            .map_err(|e| io::Error::other(format!("invalid gitlab_pat regex: {e}")))?,
        capture_group: None,
    });
    patterns.push(SecretPattern {
        name: "npm_token".to_string(),
        regex: Regex::new(r"\bnpm_[A-Za-z0-9]{36}\b")
            .map_err(|e| io::Error::other(format!("invalid npm_token regex: {e}")))?,
        capture_group: None,
    });
    patterns.push(SecretPattern {
        name: "pypi_token".to_string(),
        regex: Regex::new(r"\bpypi-[A-Za-z0-9_-]{40,}\b")
            .map_err(|e| io::Error::other(format!("invalid pypi_token regex: {e}")))?,
        capture_group: None,
    });
    patterns.push(SecretPattern {
        name: "stripe_secret_or_restricted_key".to_string(),
        regex: Regex::new(r"\b(?:sk|rk)_(?:live|test)_[0-9A-Za-z]{16,}\b").map_err(|e| {
            io::Error::other(format!(
                "invalid stripe_secret_or_restricted_key regex: {e}"
            ))
        })?,
        capture_group: None,
    });
    patterns.push(SecretPattern {
        name: "jwt".to_string(),
        regex: Regex::new(r"\beyJ[A-Za-z0-9_-]{10,}\.[A-Za-z0-9._-]{10,}\.[A-Za-z0-9._-]{10,}\b")
            .map_err(|e| io::Error::other(format!("invalid jwt regex: {e}")))?,
        capture_group: None,
    });
    // OpenAI API keys: sk-... or sk-proj-...
    patterns.push(SecretPattern {
        name: "openai_api_key".to_string(),
        regex: Regex::new(r"\b(?:sk-|sk-proj-)[A-Za-z0-9_-]{20,200}\b")
            .map_err(|e| io::Error::other(format!("invalid openai_api_key regex: {e}")))?,
        capture_group: None,
    });
    patterns.push(SecretPattern {
        name: "anthropic_api_key".to_string(),
        regex: Regex::new(r"\bsk-ant-[A-Za-z0-9_-]{16,256}\b")
            .map_err(|e| io::Error::other(format!("invalid anthropic_api_key regex: {e}")))?,
        capture_group: None,
    });
    patterns.push(SecretPattern {
        name: "xai_api_key".to_string(),
        regex: Regex::new(r"\bxai-[A-Za-z0-9_-]{16,256}\b")
            .map_err(|e| io::Error::other(format!("invalid xai_api_key regex: {e}")))?,
        capture_group: None,
    });
    patterns.push(SecretPattern {
        name: "zai_api_key".to_string(),
        regex: Regex::new(r"\bzai-[A-Za-z0-9_-]{16,256}\b")
            .map_err(|e| io::Error::other(format!("invalid zai_api_key regex: {e}")))?,
        capture_group: None,
    });
    patterns.push(SecretPattern {
        name: "llm_vendor_key_assignment".to_string(),
        regex: Regex::new(
            r#"(?i)\b(?:gemini|google[_-]?ai|anthropic|claude|xai|grok|deepseek|z\.?ai|glm|minimax|moonshot|kimi|qwen|dashscope)(?:[_-]?(?:api|access))?[_-]?(?:key|token)\b\s*[:=]\s*["']?([A-Za-z0-9._-]{16,256})["']?"#,
        )
        .map_err(|e| io::Error::other(format!("invalid llm_vendor_key_assignment regex: {e}")))?,
        capture_group: Some(1),
    });
    patterns.push(SecretPattern {
        name: "azure_storage_account_key".to_string(),
        regex: Regex::new(r#"(?i)\baccountkey\b\s*[:=]\s*["']?([A-Za-z0-9+/]{40,120}={0,2})["']?"#)
            .map_err(|e| {
                io::Error::other(format!("invalid azure_storage_account_key regex: {e}"))
            })?,
        capture_group: Some(1),
    });
    patterns.push(SecretPattern {
        name: "authorization_bearer".to_string(),
        regex: Regex::new(r"(?i)\bauthorization\b\s*[:=]\s*bearer\s+([A-Za-z0-9._-]{20,})")
            .map_err(|e| io::Error::other(format!("invalid authorization_bearer regex: {e}")))?,
        capture_group: Some(1),
    });
    patterns.push(SecretPattern {
        name: "db_url_password".to_string(),
        regex: Regex::new(r"\b[a-z][a-z0-9+.-]*://[^/\s:@]+:([^/\s@]{8,})@[^/\s]+")
            .map_err(|e| io::Error::other(format!("invalid db_url_password regex: {e}")))?,
        capture_group: Some(1),
    });
    patterns.push(SecretPattern {
        name: "assignment_value".to_string(),
        regex: Regex::new(
            r#"(?i)\b(?:api[_-]?key|token|secret|password|passwd)\b\s*[:=]\s*["']?([A-Za-z0-9_./+=:@-]{8,256})["']?"#,
        )
        .map_err(|e| io::Error::other(format!("invalid assignment_value regex: {e}")))?,
        capture_group: Some(1),
    });

    for (idx, raw) in opts.detect_patterns.iter().enumerate() {
        let regex = Regex::new(raw).map_err(|e| {
            io::Error::other(format!(
                "invalid --detect-pattern #{} '{}': {}",
                idx + 1,
                raw,
                e
            ))
        })?;
        let capture_group = if regex.captures_len() > 1 {
            Some(1)
        } else {
            None
        };
        patterns.push(SecretPattern {
            name: format!("custom_pattern_{}", idx + 1),
            regex,
            capture_group,
        });
    }
    Ok(patterns)
}

fn collect_blob_candidates(repo: &Path) -> io::Result<Vec<BlobCandidate>> {
    let rev_list = run_git_capture(repo, &["rev-list", "--objects", "--all"])?;
    if !rev_list.status.success() {
        let stderr = String::from_utf8_lossy(&rev_list.stderr);
        return Err(io::Error::other(format!(
            "git rev-list --objects --all failed: {}",
            stderr.trim()
        )));
    }

    let mut seen = HashSet::new();
    let mut object_lines = Vec::new();
    let mut path_by_oid: HashMap<String, Option<String>> = HashMap::new();

    for line in String::from_utf8_lossy(&rev_list.stdout).lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let (oid, path) = if let Some((oid, path)) = trimmed.split_once(' ') {
            (oid, Some(path.to_string()))
        } else {
            (trimmed, None)
        };
        if !is_hex_oid(oid) {
            continue;
        }
        if seen.insert(oid.to_string()) {
            object_lines.push(oid.to_string());
            path_by_oid.insert(oid.to_string(), path);
        }
    }

    if object_lines.is_empty() {
        return Ok(Vec::new());
    }

    let mut cmd = Command::new("git");
    cmd.arg("-C")
        .arg(repo)
        .arg("cat-file")
        .arg("--batch-check=%(objectname) %(objecttype) %(objectsize)")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = cmd.spawn()?;
    {
        let mut stdin = child
            .stdin
            .take()
            .ok_or_else(|| io::Error::other("failed to open git cat-file stdin"))?;
        for oid in &object_lines {
            stdin.write_all(oid.as_bytes())?;
            stdin.write_all(b"\n")?;
        }
    }

    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| io::Error::other("failed to read git cat-file stdout"))?;
    let mut reader = BufReader::new(stdout);
    let mut blobs = Vec::new();
    let mut line = String::new();
    while reader.read_line(&mut line)? > 0 {
        let entry = line.trim_end();
        if !entry.is_empty() {
            let mut parts = entry.split_whitespace();
            let oid = parts.next().unwrap_or_default();
            let object_type = parts.next().unwrap_or_default();
            let size = parts.next().unwrap_or_default().parse::<u64>().unwrap_or(0);
            if object_type == "blob" && size > 0 && size <= MAX_SCAN_BLOB_BYTES {
                blobs.push(BlobCandidate {
                    oid: oid.to_string(),
                    path: path_by_oid.get(oid).cloned().flatten(),
                });
            }
        }
        line.clear();
    }

    let status = child.wait()?;
    if !status.success() {
        return Err(io::Error::other(
            "git cat-file --batch-check failed while collecting blob metadata",
        ));
    }

    Ok(blobs)
}

fn scan_blob_candidates(
    repo: &Path,
    candidates: &[BlobCandidate],
    patterns: &[SecretPattern],
) -> io::Result<Vec<Detection>> {
    if candidates.is_empty() {
        return Ok(Vec::new());
    }

    let mut cmd = Command::new("git");
    cmd.arg("-C")
        .arg(repo)
        .arg("cat-file")
        .arg("--batch")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = cmd.spawn()?;
    {
        let mut stdin = child
            .stdin
            .take()
            .ok_or_else(|| io::Error::other("failed to open git cat-file stdin"))?;
        for candidate in candidates {
            stdin.write_all(candidate.oid.as_bytes())?;
            stdin.write_all(b"\n")?;
        }
    }

    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| io::Error::other("failed to read git cat-file stdout"))?;
    let mut reader = BufReader::new(stdout);
    let mut dedup = HashSet::new();
    let mut detections = Vec::new();

    for candidate in candidates {
        let mut header = String::new();
        if reader.read_line(&mut header)? == 0 {
            break;
        }
        let header = header.trim_end();
        if header.ends_with(" missing") {
            continue;
        }

        let mut header_parts = header.split_whitespace();
        let oid = header_parts.next().unwrap_or_default();
        let object_type = header_parts.next().unwrap_or_default();
        let size = header_parts
            .next()
            .unwrap_or_default()
            .parse::<usize>()
            .unwrap_or(0);

        let mut payload = vec![0u8; size];
        reader.read_exact(&mut payload)?;
        let mut _delimiter = [0u8; 1];
        reader.read_exact(&mut _delimiter)?;

        if object_type != "blob" || looks_binary_blob(&payload) {
            continue;
        }

        collect_blob_detections(
            &payload,
            oid,
            candidate.path.as_deref(),
            patterns,
            &mut dedup,
            &mut detections,
        );
    }

    let status = child.wait()?;
    if !status.success() {
        return Err(io::Error::other(
            "git cat-file --batch failed while scanning blobs",
        ));
    }

    detections.sort_by(|a, b| a.value.cmp(&b.value));
    Ok(detections)
}

fn collect_blob_detections(
    payload: &[u8],
    oid: &str,
    path: Option<&str>,
    patterns: &[SecretPattern],
    dedup: &mut HashSet<String>,
    detections: &mut Vec<Detection>,
) {
    for pattern in patterns {
        for captures in pattern.regex.captures_iter(payload) {
            let matched = if let Some(group_idx) = pattern.capture_group {
                captures.get(group_idx)
            } else {
                captures.get(0)
            };
            let Some(matched) = matched else {
                continue;
            };

            let Some(value) = normalize_detected_value(matched.as_bytes()) else {
                continue;
            };
            if !dedup.insert(value.clone()) {
                continue;
            }
            if detections.len() >= MAX_DETECTED_VALUES {
                continue;
            }

            detections.push(Detection {
                value,
                pattern: pattern.name.clone(),
                oid: oid.to_string(),
                path: path.map(ToOwned::to_owned),
            });
        }
    }
}

fn normalize_detected_value(bytes: &[u8]) -> Option<String> {
    if bytes.len() < 8 || bytes.len() > 256 {
        return None;
    }
    if bytes
        .iter()
        .any(|&b| b == b'\n' || b == b'\r' || b == b'\t' || b == b' ')
    {
        return None;
    }
    let value = std::str::from_utf8(bytes)
        .ok()?
        .trim_matches('"')
        .trim_matches('\'')
        .to_string();
    if value.len() < 8 || value.len() > 256 {
        return None;
    }

    let lowercase = value.to_ascii_lowercase();
    let obvious_placeholders = [
        "example",
        "sample",
        "placeholder",
        "changeme",
        "your_token",
        "your_key",
        "dummy",
    ];
    if obvious_placeholders
        .iter()
        .any(|needle| lowercase.contains(needle))
    {
        return None;
    }
    Some(value)
}

fn looks_binary_blob(payload: &[u8]) -> bool {
    if payload.contains(&0) {
        return true;
    }
    let sample = &payload[..payload.len().min(4096)];
    if sample.is_empty() {
        return false;
    }
    let non_text = sample
        .iter()
        .filter(|&&b| !(b == b'\n' || b == b'\r' || b == b'\t' || (32..=126).contains(&b)))
        .count();
    non_text * 5 > sample.len()
}

fn write_detection_draft(repo: &Path, detections: &[Detection]) -> io::Result<PathBuf> {
    let output_path = repo.join(OUTPUT_FILE_NAME);
    let mut out = std::fs::File::create(&output_path)?;

    writeln!(out, "# Auto-generated by filter-repo-rs --detect-secrets")?;
    writeln!(
        out,
        "# Review each entry before using: filter-repo-rs --replace-text {} --sensitive",
        OUTPUT_FILE_NAME
    )?;

    if detections.is_empty() {
        writeln!(out, "# No potential secrets detected.")?;
        return Ok(output_path);
    }

    writeln!(out)?;
    for detection in detections {
        let short_oid = &detection.oid[..detection.oid.len().min(12)];
        let location = detection.path.as_deref().unwrap_or("<unknown-path>");
        writeln!(
            out,
            "# {} @ {} ({})",
            detection.pattern, location, short_oid
        )?;
        writeln!(out, "{}==>{}", detection.value, REDACTION)?;
    }

    Ok(output_path)
}

fn is_hex_oid(s: &str) -> bool {
    s.len() == 40 && s.bytes().all(|b| b.is_ascii_hexdigit())
}

fn run_git_capture(repo: &Path, args: &[&str]) -> io::Result<std::process::Output> {
    Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
}
