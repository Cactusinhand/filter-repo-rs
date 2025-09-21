use std::io;
use std::path::Path;
use std::process::{Command, Stdio};

use crate::gitutil::git_dir;
use crate::opts::Options;

pub fn build_fast_export_cmd(opts: &Options) -> io::Result<Command> {
    // Test override: if provided in opts, read a prebuilt stream from that file
    if let Some(stream_path) = &opts.fe_stream_override {
        if !opts.debug_mode {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "error: --fe_stream_override is gated behind debug mode. Set FRRS_DEBUG=1 or pass --debug-mode to access debug-only flags.",
            ));
        }
        #[cfg(windows)]
        {
            let mut cmd = Command::new("cmd");
            cmd.arg("/C").arg("type").arg(stream_path);
            cmd.stdout(Stdio::piped());
            cmd.stderr(if opts.quiet {
                Stdio::null()
            } else {
                Stdio::inherit()
            });
            return Ok(cmd);
        }
        #[cfg(not(windows))]
        {
            let mut cmd = Command::new("cat");
            cmd.arg(stream_path);
            cmd.stdout(Stdio::piped());
            cmd.stderr(if opts.quiet {
                Stdio::null()
            } else {
                Stdio::inherit()
            });
            return Ok(cmd);
        }
    }
    let mut cmd = Command::new("git");
    cmd.arg("-C").arg(&opts.source);
    if opts.quotepath {
        cmd.arg("-c").arg("core.quotepath=false");
    }
    cmd.arg("fast-export");
    for r in &opts.refs {
        cmd.arg(r);
    }
    cmd.arg("--show-original-ids")
        .arg("--signed-tags=strip")
        .arg("--tag-of-filtered-object=rewrite")
        .arg("--fake-missing-tagger")
        .arg("--reference-excluded-parents")
        .arg("--use-done-feature");
    if opts.date_order {
        cmd.arg("--date-order");
    }
    if opts.no_data {
        cmd.arg("--no-data");
    }
    if opts.reencode {
        cmd.arg("--reencode=yes");
    }
    if opts.mark_tags {
        cmd.arg("--mark-tags");
    }
    cmd.stdout(Stdio::piped());
    cmd.stderr(if opts.quiet {
        Stdio::null()
    } else {
        Stdio::inherit()
    });
    Ok(cmd)
}

pub fn build_fast_import_cmd(opts: &Options) -> Command {
    let mut cmd = Command::new("git");
    cmd.arg("-C").arg(&opts.target);
    // Config overrides must precede subcommand
    cmd.arg("-c").arg("core.ignorecase=false");
    cmd.arg("fast-import");
    cmd.arg("--force").arg("--quiet");
    cmd.arg("--date-format=raw-permissive");
    // Export marks so we can build commit-map without in-stream get-mark
    if let Ok(gd) = git_dir(&opts.target) {
        let marks_path = Path::new(&gd).join("filter-repo").join("target-marks");
        cmd.arg(format!("--export-marks={}", marks_path.to_string_lossy()));
    }
    cmd.stdin(Stdio::piped());
    cmd.stdout(Stdio::piped());
    cmd.stderr(Stdio::inherit());
    cmd
}
