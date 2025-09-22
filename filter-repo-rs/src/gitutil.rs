use std::collections::{HashMap, HashSet};
use std::io;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

pub fn git_dir(repo: &Path) -> io::Result<PathBuf> {
    let out = Command::new("git")
        .arg("-C")
        .arg(repo)
        .arg("rev-parse")
        .arg("--git-dir")
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .output()?;
    if !out.status.success() {
        return Err(io::Error::new(
            io::ErrorKind::Other,
            format!("'git -C {:?} rev-parse --git-dir' failed", repo),
        ));
    }
    let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
    let p = PathBuf::from(&s);
    if p.is_absolute() {
        Ok(p)
    } else {
        // Make relative .git paths absolute to the repo directory
        Ok(repo.join(p))
    }
}

/// Get all references in the repository
///
/// Retrieves all Git references (branches, tags, etc.) and their corresponding
/// object hashes using `git for-each-ref`.
///
/// # Arguments
///
/// * `repo_path` - Path to the Git repository
///
/// # Returns
///
/// Returns a HashMap mapping reference names to their object hashes.
///
/// # Examples
///
/// ```rust,no_run
/// use filter_repo_rs::gitutil;
/// use std::path::Path;
///
/// let refs = gitutil::get_all_refs(Path::new(".")).unwrap();
/// for (refname, hash) in refs {
///     println!("{}: {}", refname, hash);
/// }
/// ```
pub fn get_all_refs(repo_path: &Path) -> io::Result<HashMap<String, String>> {
    let output = Command::new("git")
        .arg("-C")
        .arg(repo_path)
        .arg("for-each-ref")
        .arg("--format=%(refname) %(objectname)")
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .output()?;

    if !output.status.success() {
        return Err(io::Error::new(
            io::ErrorKind::Other,
            format!("'git -C {:?} for-each-ref' failed", repo_path),
        ));
    }

    let mut refs = HashMap::new();
    let output_str = String::from_utf8_lossy(&output.stdout);

    for line in output_str.lines() {
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() >= 2 {
            let refname = parts[0].to_string();
            let hash = parts[1].to_string();
            refs.insert(refname, hash);
        }
    }

    Ok(refs)
}

/// Check if the repository is bare
///
/// Determines whether the repository is a bare repository (no working directory)
/// using `git rev-parse --is-bare-repository`.
///
/// # Arguments
///
/// * `repo_path` - Path to the Git repository
///
/// # Returns
///
/// Returns `true` if the repository is bare, `false` otherwise.
pub fn is_bare_repository(repo_path: &Path) -> io::Result<bool> {
    let output = Command::new("git")
        .arg("-C")
        .arg(repo_path)
        .arg("rev-parse")
        .arg("--is-bare-repository")
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .output()?;

    if !output.status.success() {
        return Err(io::Error::new(
            io::ErrorKind::Other,
            format!(
                "'git -C {:?} rev-parse --is-bare-repository' failed",
                repo_path
            ),
        ));
    }

    let result = String::from_utf8_lossy(&output.stdout)
        .trim()
        .to_lowercase();
    Ok(result == "true")
}

/// Get reflog entries for a specific reference
///
/// Retrieves all reflog entries for a given reference using `git reflog show`.
/// Returns an empty vector if the reflog doesn't exist.
///
/// # Arguments
///
/// * `repo_path` - Path to the Git repository
/// * `refname` - Name of the reference (e.g., "HEAD", "refs/heads/main")
///
/// # Returns
///
/// Returns a vector of reflog entry hashes, or empty vector if no reflog exists.
pub fn get_reflog_entries(repo_path: &Path, refname: &str) -> io::Result<Vec<String>> {
    let output = Command::new("git")
        .arg("-C")
        .arg(repo_path)
        .arg("reflog")
        .arg("show")
        .arg("--format=%H")
        .arg(refname)
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .output()?;

    if !output.status.success() {
        // Reflog might not exist, return empty vector
        return Ok(Vec::new());
    }

    let output_str = String::from_utf8_lossy(&output.stdout);
    let entries: Vec<String> = output_str
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| line.trim().to_string())
        .collect();

    Ok(entries)
}

/// List all reflogs in the repository
///
/// Discovers all reflog files by traversing the `.git/logs/refs` directory.
/// Returns an empty vector if no reflogs exist.
///
/// # Arguments
///
/// * `repo_path` - Path to the Git repository
///
/// # Returns
///
/// Returns a vector of reflog names (e.g., "refs/heads/main", "refs/remotes/origin/main").
pub fn list_all_reflogs(repo_path: &Path) -> io::Result<Vec<String>> {
    let git_dir = git_dir(repo_path)?;
    let logs_dir = git_dir.join("logs").join("refs");

    if !logs_dir.exists() {
        return Ok(Vec::new());
    }

    let mut reflogs = Vec::new();
    collect_reflogs(&logs_dir, "refs", &mut reflogs)?;

    Ok(reflogs)
}

/// Recursively collect reflog names from the logs directory
fn collect_reflogs(dir: &Path, prefix: &str, reflogs: &mut Vec<String>) -> io::Result<()> {
    if !dir.is_dir() {
        return Ok(());
    }

    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        let name = entry.file_name();
        let name_str = name.to_string_lossy();

        if path.is_dir() {
            let new_prefix = format!("{}/{}", prefix, name_str);
            collect_reflogs(&path, &new_prefix, reflogs)?;
        } else {
            let reflog_name = format!("{}/{}", prefix, name_str);
            reflogs.push(reflog_name);
        }
    }

    Ok(())
}

/// Get replace references in the repository
///
/// Discovers all Git replace references by traversing the `.git/refs/replace` directory.
/// Replace references are used to replace one object with another in Git's object database.
///
/// # Arguments
///
/// * `repo_path` - Path to the Git repository
///
/// # Returns
///
/// Returns a set of replace reference object IDs, or empty set if none exist.
pub fn get_replace_refs(repo_path: &Path) -> io::Result<HashSet<String>> {
    let git_dir = git_dir(repo_path)?;
    let replace_dir = git_dir.join("refs").join("replace");

    if !replace_dir.exists() {
        return Ok(HashSet::new());
    }

    let mut replace_refs = HashSet::new();
    collect_replace_refs(&replace_dir, &mut replace_refs)?;

    Ok(replace_refs)
}

/// Recursively collect replace reference object IDs
fn collect_replace_refs(dir: &Path, replace_refs: &mut HashSet<String>) -> io::Result<()> {
    if !dir.is_dir() {
        return Ok(());
    }

    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();

        if path.is_dir() {
            collect_replace_refs(&path, replace_refs)?;
        } else {
            let name = entry.file_name();
            let name_str = name.to_string_lossy().to_string();
            replace_refs.insert(name_str);
        }
    }

    Ok(())
}

/// Validate Git directory structure based on repository type
///
/// Ensures that the Git directory structure matches the repository type.
/// For bare repositories, GIT_DIR should be "." (the repository root).
/// For non-bare repositories, GIT_DIR should be ".git".
///
/// # Arguments
///
/// * `repo_path` - Path to the Git repository
/// * `is_bare` - Whether the repository is bare
///
/// # Returns
///
/// Returns `Ok(())` if structure is valid, or an error describing the problem.
pub fn validate_git_dir_structure(repo_path: &Path, is_bare: bool) -> io::Result<()> {
    let git_dir = git_dir(repo_path)?;
    let git_dir_name = git_dir
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("");

    if is_bare {
        // For bare repositories, GIT_DIR should be "."
        if git_dir != repo_path {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!(
                    "Bare repository GIT_DIR should be '.', but found '{}'",
                    git_dir.display()
                ),
            ));
        }
    } else {
        // For non-bare repositories, GIT_DIR should be ".git"
        if git_dir_name != ".git" {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!(
                    "Non-bare repository GIT_DIR should be '.git', but found '{}'",
                    git_dir_name
                ),
            ));
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::process::Command;
    use tempfile::TempDir;

    fn create_test_repo() -> io::Result<TempDir> {
        let temp_dir = TempDir::new()?;

        // Initialize git repository
        let output = Command::new("git")
            .arg("init")
            .current_dir(temp_dir.path())
            .output()?;

        if !output.status.success() {
            return Err(io::Error::new(
                io::ErrorKind::Other,
                "Failed to initialize test git repository",
            ));
        }

        // Configure git user for commits
        Command::new("git")
            .arg("config")
            .arg("user.name")
            .arg("Test User")
            .current_dir(temp_dir.path())
            .output()?;

        Command::new("git")
            .arg("config")
            .arg("user.email")
            .arg("test@example.com")
            .current_dir(temp_dir.path())
            .output()?;

        Ok(temp_dir)
    }

    fn create_bare_repo() -> io::Result<TempDir> {
        let temp_dir = TempDir::new()?;

        // Initialize bare git repository
        let output = Command::new("git")
            .arg("init")
            .arg("--bare")
            .current_dir(temp_dir.path())
            .output()?;

        if !output.status.success() {
            return Err(io::Error::new(
                io::ErrorKind::Other,
                "Failed to initialize bare test git repository",
            ));
        }

        Ok(temp_dir)
    }

    fn create_commit(repo_path: &Path) -> io::Result<()> {
        // Create a test file
        fs::write(repo_path.join("test.txt"), "test content")?;

        // Add and commit
        Command::new("git")
            .arg("add")
            .arg("test.txt")
            .current_dir(repo_path)
            .output()?;

        Command::new("git")
            .arg("commit")
            .arg("-m")
            .arg("Test commit")
            .current_dir(repo_path)
            .output()?;

        Ok(())
    }

    #[test]
    fn test_get_all_refs_empty_repo() -> io::Result<()> {
        let temp_repo = create_test_repo()?;

        let refs = get_all_refs(temp_repo.path())?;

        // Empty repo should have no refs
        assert!(refs.is_empty());

        Ok(())
    }

    #[test]
    fn test_get_all_refs_with_commits() -> io::Result<()> {
        let temp_repo = create_test_repo()?;
        create_commit(temp_repo.path())?;

        let refs = get_all_refs(temp_repo.path())?;

        // Should have at least HEAD and refs/heads/master (or main)
        assert!(!refs.is_empty());
        assert!(refs.keys().any(|k| k.contains("refs/heads/")));

        Ok(())
    }

    #[test]
    fn test_is_bare_repository_false() -> io::Result<()> {
        let temp_repo = create_test_repo()?;

        let is_bare = is_bare_repository(temp_repo.path())?;

        assert_eq!(is_bare, false);

        Ok(())
    }

    #[test]
    fn test_is_bare_repository_true() -> io::Result<()> {
        let temp_repo = create_bare_repo()?;

        let is_bare = is_bare_repository(temp_repo.path())?;

        assert_eq!(is_bare, true);

        Ok(())
    }

    #[test]
    fn test_get_reflog_entries_nonexistent() -> io::Result<()> {
        let temp_repo = create_test_repo()?;

        let entries = get_reflog_entries(temp_repo.path(), "refs/heads/nonexistent")?;

        // Should return empty vector for nonexistent reflog
        assert!(entries.is_empty());

        Ok(())
    }

    #[test]
    fn test_get_reflog_entries_with_commits() -> io::Result<()> {
        let temp_repo = create_test_repo()?;
        create_commit(temp_repo.path())?;

        let entries = get_reflog_entries(temp_repo.path(), "HEAD")?;

        // Should have at least one entry after commit
        assert!(!entries.is_empty());

        Ok(())
    }

    #[test]
    fn test_list_all_reflogs_empty_repo() -> io::Result<()> {
        let temp_repo = create_test_repo()?;

        let result = list_all_reflogs(temp_repo.path());

        // Should succeed even for empty repo
        assert!(result.is_ok());

        Ok(())
    }

    #[test]
    fn test_list_all_reflogs_with_commits() -> io::Result<()> {
        let temp_repo = create_test_repo()?;
        create_commit(temp_repo.path())?;

        let reflogs = list_all_reflogs(temp_repo.path())?;

        // Should have reflogs after commit
        assert!(!reflogs.is_empty());

        Ok(())
    }

    #[test]
    fn test_get_replace_refs_empty() -> io::Result<()> {
        let temp_repo = create_test_repo()?;

        let replace_refs = get_replace_refs(temp_repo.path())?;

        // Should be empty for normal repo
        assert!(replace_refs.is_empty());

        Ok(())
    }

    #[test]
    fn test_validate_git_dir_structure_non_bare() -> io::Result<()> {
        let temp_repo = create_test_repo()?;

        let result = validate_git_dir_structure(temp_repo.path(), false);

        // Should succeed for non-bare repo
        assert!(result.is_ok());

        Ok(())
    }

    #[test]
    fn test_validate_git_dir_structure_bare() -> io::Result<()> {
        let temp_repo = create_bare_repo()?;

        let result = validate_git_dir_structure(temp_repo.path(), true);

        // Should succeed for bare repo
        assert!(result.is_ok());

        Ok(())
    }

    #[test]
    fn test_validate_git_dir_structure_mismatch() -> io::Result<()> {
        let temp_repo = create_test_repo()?;

        // Try to validate non-bare repo as bare - should fail
        let result = validate_git_dir_structure(temp_repo.path(), true);

        assert!(result.is_err());

        Ok(())
    }
}
