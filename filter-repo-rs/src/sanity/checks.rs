use std::collections::HashMap;
use std::io;
use std::path::Path;

use unicode_normalization::UnicodeNormalization;

use super::{
    ConflictType, GitCommandError, GitCommandExecutor, SanityCheckContext, SanityCheckError,
    UnpushedBranch,
};
use crate::gitutil;

pub(super) fn check_git_dir_structure_with_context(
    ctx: &SanityCheckContext,
) -> Result<(), SanityCheckError> {
    if gitutil::validate_git_dir_structure(&ctx.repo_path, ctx.is_bare).is_err() {
        let git_dir = gitutil::git_dir(&ctx.repo_path).map_err(SanityCheckError::from)?;
        let actual = if ctx.is_bare {
            git_dir.display().to_string()
        } else {
            git_dir
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("unknown")
                .to_string()
        };
        let expected = if ctx.is_bare { "." } else { ".git" }.to_string();
        return Err(SanityCheckError::GitDirStructure {
            expected,
            actual,
            is_bare: ctx.is_bare,
        });
    }
    Ok(())
}

pub(super) fn check_reference_conflicts_with_context(
    ctx: &SanityCheckContext,
) -> Result<(), SanityCheckError> {
    if ctx.config.ignore_case {
        check_case_insensitive_conflicts(&ctx.refs)?;
    }
    if ctx.config.precompose_unicode {
        check_unicode_normalization_conflicts(&ctx.refs)?;
    }
    Ok(())
}

pub(super) fn check_case_insensitive_conflicts(
    refs: &HashMap<String, String>,
) -> Result<(), SanityCheckError> {
    let mut case_groups: HashMap<String, Vec<String>> = HashMap::new();
    for refname in refs.keys() {
        let lowercase = refname.to_lowercase();
        case_groups
            .entry(lowercase)
            .or_default()
            .push(refname.clone());
    }
    let mut conflicts = Vec::new();
    for (normalized, group) in case_groups {
        if group.len() > 1 {
            conflicts.push((normalized, group));
        }
    }
    if !conflicts.is_empty() {
        return Err(SanityCheckError::ReferenceConflict {
            conflict_type: ConflictType::CaseInsensitive,
            conflicts,
        });
    }
    Ok(())
}

pub(super) fn check_unicode_normalization_conflicts(
    refs: &HashMap<String, String>,
) -> Result<(), SanityCheckError> {
    let mut normalization_groups: HashMap<String, Vec<String>> = HashMap::new();
    for refname in refs.keys() {
        let normalized: String = refname.nfc().collect();
        normalization_groups
            .entry(normalized)
            .or_default()
            .push(refname.clone());
    }
    let mut conflicts = Vec::new();
    for (normalized, group) in normalization_groups {
        if group.len() > 1 {
            conflicts.push((normalized, group));
        }
    }
    if !conflicts.is_empty() {
        return Err(SanityCheckError::ReferenceConflict {
            conflict_type: ConflictType::UnicodeNormalization,
            conflicts,
        });
    }
    Ok(())
}

pub(super) fn check_reflog_entries_with_context(
    ctx: &SanityCheckContext,
) -> Result<(), SanityCheckError> {
    let reflogs = gitutil::list_all_reflogs(&ctx.repo_path).map_err(SanityCheckError::from)?;
    if reflogs.is_empty() {
        return Ok(());
    }
    let mut problematic_reflogs = Vec::new();
    for reflog_name in &reflogs {
        let entries = gitutil::get_reflog_entries(&ctx.repo_path, reflog_name)
            .map_err(SanityCheckError::from)?;
        if entries.len() > 1 {
            problematic_reflogs.push((reflog_name.clone(), entries.len()));
        }
    }
    if !problematic_reflogs.is_empty() {
        return Err(SanityCheckError::ReflogTooManyEntries {
            problematic_reflogs,
        });
    }
    Ok(())
}

pub(super) fn check_unpushed_changes_with_context(
    ctx: &SanityCheckContext,
) -> Result<(), SanityCheckError> {
    if ctx.is_bare {
        return Ok(());
    }
    let branch_mappings = build_branch_mappings(&ctx.refs)?;
    if branch_mappings.remote_branches.is_empty() {
        return Ok(());
    }
    let mut unpushed_branches = Vec::new();
    for (local_branch, local_hash) in &branch_mappings.local_branches {
        let remote_branch = format!(
            "refs/remotes/origin/{}",
            local_branch
                .strip_prefix("refs/heads/")
                .unwrap_or(local_branch)
        );
        if let Some(remote_hash) = branch_mappings.remote_branches.get(&remote_branch) {
            if local_hash != remote_hash {
                unpushed_branches.push(UnpushedBranch {
                    branch_name: local_branch.clone(),
                    local_hash: local_hash.clone(),
                    remote_hash: Some(remote_hash.clone()),
                });
            }
        } else {
            unpushed_branches.push(UnpushedBranch {
                branch_name: local_branch.clone(),
                local_hash: local_hash.clone(),
                remote_hash: None,
            });
        }
    }
    if !unpushed_branches.is_empty() {
        return Err(SanityCheckError::UnpushedChanges { unpushed_branches });
    }
    Ok(())
}

pub(super) fn check_replace_refs_in_loose_objects_with_context(
    ctx: &SanityCheckContext,
    packs: usize,
    loose_count: usize,
) -> bool {
    let replace_refs = &ctx.replace_refs;
    if replace_refs.is_empty() {
        return (packs == 1 && loose_count == 0) || (packs == 0 && loose_count < 100);
    }
    if loose_count <= replace_refs.len() {
        return (packs <= 1) || (packs == 0 && 0 < 100);
    }
    let non_replace_loose_count = loose_count.saturating_sub(replace_refs.len());
    (packs == 1 && non_replace_loose_count == 0) || (packs == 0 && non_replace_loose_count < 100)
}

pub(super) fn check_remote_configuration_with_context(
    ctx: &SanityCheckContext,
) -> Result<(), SanityCheckError> {
    let executor = GitCommandExecutor::new(&ctx.repo_path);
    let remotes = match executor.run_command(&["remote"]) {
        Ok(output) => output,
        Err(GitCommandError::ExecutionFailed { stderr, .. }) if stderr.is_empty() => String::new(),
        Err(e) => {
            return Err(SanityCheckError::IoError(io::Error::other(format!(
                "Failed to get remote configuration: {e}"
            ))));
        }
    };
    let remote_trim = remotes.trim();
    if remote_trim != "origin" && !remote_trim.is_empty() {
        let remote_list: Vec<String> = remotes.lines().map(|s| s.trim().to_string()).collect();
        return Err(SanityCheckError::InvalidRemotes {
            remotes: remote_list,
        });
    }
    Ok(())
}

pub(super) fn check_stash_presence_with_context(
    ctx: &SanityCheckContext,
) -> Result<(), SanityCheckError> {
    let executor = GitCommandExecutor::new(&ctx.repo_path);
    match executor.run_command(&["rev-parse", "--verify", "--quiet", "refs/stash"]) {
        Ok(_) => Err(SanityCheckError::StashedChanges),
        Err(GitCommandError::ExecutionFailed { exit_code, .. }) if exit_code != 0 => Ok(()),
        Err(e) => Err(SanityCheckError::IoError(io::Error::other(format!(
            "Failed to check stash status: {e}"
        )))),
    }
}

pub(super) fn check_working_tree_cleanliness_with_context(
    ctx: &SanityCheckContext,
) -> Result<(), SanityCheckError> {
    let executor = GitCommandExecutor::new(&ctx.repo_path);
    let staged_dirty = match executor.run_command(&["diff", "--staged", "--quiet"]) {
        Ok(_) => false,
        Err(GitCommandError::ExecutionFailed { exit_code: 1, .. }) => true,
        Err(e) => {
            return Err(SanityCheckError::IoError(io::Error::other(format!(
                "Failed to check staged changes: {e}"
            ))));
        }
    };
    let unstaged_dirty = match executor.run_command(&["diff", "--quiet"]) {
        Ok(_) => false,
        Err(GitCommandError::ExecutionFailed { exit_code: 1, .. }) => true,
        Err(e) => {
            return Err(SanityCheckError::IoError(io::Error::other(format!(
                "Failed to check unstaged changes: {e}"
            ))));
        }
    };
    if staged_dirty || unstaged_dirty {
        return Err(SanityCheckError::WorkingTreeNotClean {
            staged_dirty,
            unstaged_dirty,
        });
    }
    Ok(())
}

pub(super) fn check_untracked_files_with_context(
    ctx: &SanityCheckContext,
) -> Result<(), SanityCheckError> {
    if ctx.is_bare {
        return Ok(());
    }
    let executor = GitCommandExecutor::new(&ctx.repo_path);
    match executor.run_command(&["ls-files", "-o", "--exclude-standard", "--directory"]) {
        Ok(output) => {
            let untracked_files: Vec<String> = output
                .lines()
                .map(|line| line.trim().to_string())
                .filter(|l| !l.is_empty())
                .collect();
            if !untracked_files.is_empty() {
                return Err(SanityCheckError::UntrackedFiles {
                    files: untracked_files,
                });
            }
        }
        Err(GitCommandError::ExecutionFailed { .. }) => {}
        Err(e) => {
            return Err(SanityCheckError::IoError(io::Error::other(format!(
                "Failed to check untracked files: {e}"
            ))));
        }
    }
    Ok(())
}

pub(super) fn check_worktree_count_with_context(
    ctx: &SanityCheckContext,
) -> Result<(), SanityCheckError> {
    let executor = GitCommandExecutor::new(&ctx.repo_path);
    match executor.run_command(&["worktree", "list"]) {
        Ok(output) => {
            let worktree_count = output.lines().count();
            if worktree_count > 1 {
                return Err(SanityCheckError::MultipleWorktrees {
                    count: worktree_count,
                });
            }
        }
        Err(GitCommandError::ExecutionFailed { .. }) => {}
        Err(e) => {
            return Err(SanityCheckError::IoError(io::Error::other(format!(
                "Failed to check worktree count: {e}"
            ))));
        }
    }
    Ok(())
}

pub(super) fn quick_repo_checks(target: &Path) -> Result<(), SanityCheckError> {
    let _ = gitutil::git_dir(target).map_err(SanityCheckError::from)?;
    Ok(())
}

pub(super) fn early_worktree_checks(dir: &Path) -> Result<(), SanityCheckError> {
    let is_bare = gitutil::is_bare_repository(dir).unwrap_or(false);
    if is_bare {
        return Ok(());
    }
    let executor = GitCommandExecutor::new(dir);
    let staged_dirty = match executor.run_command(&["diff", "--staged", "--quiet"]) {
        Ok(_) => false,
        Err(GitCommandError::ExecutionFailed { exit_code: 1, .. }) => true,
        Err(e) => {
            return Err(SanityCheckError::IoError(io::Error::other(format!(
                "Failed to check staged changes: {e}"
            ))));
        }
    };
    let unstaged_dirty = match executor.run_command(&["diff", "--quiet"]) {
        Ok(_) => false,
        Err(GitCommandError::ExecutionFailed { exit_code: 1, .. }) => true,
        Err(e) => {
            return Err(SanityCheckError::IoError(io::Error::other(format!(
                "Failed to check unstaged changes: {e}"
            ))));
        }
    };
    if staged_dirty || unstaged_dirty {
        return Err(SanityCheckError::WorkingTreeNotClean {
            staged_dirty,
            unstaged_dirty,
        });
    }
    match executor.run_command(&["ls-files", "-o", "--exclude-standard", "--directory"]) {
        Ok(output) => {
            let files: Vec<String> = output
                .lines()
                .map(|l| l.trim().to_string())
                .filter(|l| !l.is_empty())
                .collect();
            if !files.is_empty() {
                return Err(SanityCheckError::UntrackedFiles { files });
            }
        }
        Err(GitCommandError::ExecutionFailed { .. }) => {}
        Err(e) => {
            return Err(SanityCheckError::IoError(io::Error::other(format!(
                "Failed to check untracked files: {e}"
            ))));
        }
    }
    Ok(())
}

pub(super) struct BranchMappings {
    pub(super) local_branches: HashMap<String, String>,
    pub(super) remote_branches: HashMap<String, String>,
}

pub(super) fn build_branch_mappings(refs: &HashMap<String, String>) -> io::Result<BranchMappings> {
    let mut local_branches = HashMap::new();
    let mut remote_branches = HashMap::new();
    for (refname, hash) in refs {
        if refname.starts_with("refs/heads/") {
            local_branches.insert(refname.clone(), hash.clone());
        } else if refname.starts_with("refs/remotes/origin/") {
            remote_branches.insert(refname.clone(), hash.clone());
        }
    }
    Ok(BranchMappings {
        local_branches,
        remote_branches,
    })
}
