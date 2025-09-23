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
        if opts.git_caps.fast_export_reencode {
            cmd.arg("--reencode=yes");
        } else {
            return Err(io::Error::new(
                io::ErrorKind::Other,
                "error: git fast-export lacks --reencode; need git >= 2.23.0",
            ));
        }
    } else if matches!(opts.reencode_requested, Some(true)) {
        return Err(io::Error::new(
            io::ErrorKind::Other,
            "error: git fast-export lacks --reencode; need git >= 2.23.0",
        ));
    }
    if opts.mark_tags {
        if opts.git_caps.fast_export_mark_tags {
            cmd.arg("--mark-tags");
        } else if matches!(opts.mark_tags_requested, Some(true)) {
            return Err(io::Error::new(
                io::ErrorKind::Other,
                "error: git fast-export lacks --mark-tags; need git >= 2.24.0",
            ));
        }
    } else if matches!(opts.mark_tags_requested, Some(true)) {
        return Err(io::Error::new(
            io::ErrorKind::Other,
            "error: git fast-export lacks --mark-tags; need git >= 2.24.0",
        ));
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
    if opts.git_caps.fast_export_anonymize_map {
        cmd.arg("--date-format=raw-permissive");
    }
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::opts::Options;
    use tempfile::TempDir;

    fn args_as_strings(cmd: &Command) -> Vec<String> {
        cmd.get_args()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect()
    }

    #[test]
    fn fast_export_skips_flags_without_capability() {
        let mut opts = Options::default();
        opts.reencode = false;
        opts.mark_tags = true;
        opts.git_caps.fast_export_reencode = false;
        opts.git_caps.fast_export_mark_tags = false;

        let cmd = build_fast_export_cmd(&opts).expect("command");
        let args = args_as_strings(&cmd);
        assert!(
            !args.iter().any(|arg| arg == "--reencode=yes"),
            "expected --reencode=yes to be omitted"
        );
        assert!(
            !args.iter().any(|arg| arg == "--mark-tags"),
            "expected --mark-tags to be omitted"
        );
    }

    #[test]
    fn fast_export_errors_when_mark_tags_requested_without_support() {
        let mut opts = Options::default();
        opts.git_caps.fast_export_mark_tags = false;
        opts.mark_tags_requested = Some(true);

        let err = build_fast_export_cmd(&opts).expect_err("should fail");
        let msg = err.to_string();
        assert!(msg.contains("git >= 2.24.0"), "unexpected msg: {msg}");
    }

    #[test]
    fn fast_export_errors_when_reencode_requested_without_support() {
        let mut opts = Options::default();
        opts.git_caps.fast_export_reencode = false;
        opts.reencode_requested = Some(true);

        let err = build_fast_export_cmd(&opts).expect_err("should fail");
        let msg = err.to_string();
        assert!(msg.contains("git >= 2.23.0"), "unexpected msg: {msg}");
    }

    #[test]
    fn fast_import_respects_raw_permissive_capability() {
        let temp = TempDir::new().unwrap();
        std::process::Command::new("git")
            .arg("init")
            .arg(".")
            .current_dir(temp.path())
            .status()
            .expect("git init");

        let mut opts = Options::default();
        opts.target = temp.path().to_path_buf();
        opts.git_caps.fast_export_anonymize_map = true;
        let with_cap = build_fast_import_cmd(&opts);
        let args_with = args_as_strings(&with_cap);
        assert!(
            args_with
                .iter()
                .any(|arg| arg == "--date-format=raw-permissive"),
            "expected raw-permissive when supported"
        );

        let mut opts_without = opts.clone();
        opts_without.git_caps.fast_export_anonymize_map = false;
        let without_cap = build_fast_import_cmd(&opts_without);
        let args_without = args_as_strings(&without_cap);
        assert!(
            !args_without
                .iter()
                .any(|arg| arg == "--date-format=raw-permissive"),
            "expected raw-permissive to be skipped"
        );
    }
}
