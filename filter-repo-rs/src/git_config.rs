use std::io;
use std::path::Path;
use std::process::{Command, Stdio};

/// Git configuration settings relevant to sanity checks
///
/// This struct holds Git configuration values that affect how sanity checks
/// are performed, particularly for filesystem-specific validations and
/// remote repository information.
#[derive(Debug, Clone)]
pub struct GitConfig {
    /// Whether the filesystem is case-insensitive (core.ignorecase)
    ///
    /// When true, reference name conflict detection will check for
    /// case-insensitive conflicts that could cause issues on this filesystem.
    pub ignore_case: bool,

    /// Whether Unicode precomposition is enabled (core.precomposeunicode)
    ///
    /// When true, reference name conflict detection will check for
    /// Unicode normalization conflicts that could cause issues.
    pub precompose_unicode: bool,

    /// Origin remote URL (remote.origin.url)
    ///
    /// Contains the URL of the origin remote if configured. This can be used
    /// for additional validations or informational purposes in error messages.
    /// Currently stored for future use and completeness of Git configuration.
    #[allow(dead_code)] // Reserved for future validation features
    pub origin_url: Option<String>,
}

impl GitConfig {
    /// Read Git configuration from a repository
    ///
    /// Reads relevant Git configuration values that affect sanity check behavior.
    /// Uses safe defaults when configuration values are not set or cannot be read.
    ///
    /// # Arguments
    ///
    /// * `repo_path` - Path to the Git repository
    ///
    /// # Returns
    ///
    /// Returns a `GitConfig` struct with configuration values, using defaults
    /// for any values that cannot be read.
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// use std::path::Path;
    /// use filter_repo_rs::git_config::GitConfig;
    ///
    /// let config = GitConfig::read_from_repo(Path::new(".")).unwrap();
    /// if config.ignore_case {
    ///     println!("Case-insensitive filesystem detected");
    /// }
    /// ```
    pub fn read_from_repo(repo_path: &Path) -> io::Result<Self> {
        let ignore_case = Self::get_bool_config(repo_path, "core.ignorecase")?.unwrap_or(false);

        let precompose_unicode =
            Self::get_bool_config(repo_path, "core.precomposeunicode")?.unwrap_or(false);

        let origin_url = Self::get_string_config(repo_path, "remote.origin.url")?;

        Ok(GitConfig {
            ignore_case,
            precompose_unicode,
            origin_url,
        })
    }

    /// Get a boolean configuration value from Git
    ///
    /// Retrieves a boolean configuration value using `git config --bool`.
    /// Returns `None` if the configuration key doesn't exist or cannot be read.
    ///
    /// # Arguments
    ///
    /// * `repo_path` - Path to the Git repository
    /// * `key` - Git configuration key (e.g., "core.ignorecase")
    ///
    /// # Returns
    ///
    /// * `Ok(Some(true))` - Configuration is set to true
    /// * `Ok(Some(false))` - Configuration is set to false  
    /// * `Ok(None)` - Configuration key doesn't exist or cannot be read
    /// * `Err(_)` - IO error occurred
    pub fn get_bool_config(repo_path: &Path, key: &str) -> io::Result<Option<bool>> {
        let output = Command::new("git")
            .arg("-C")
            .arg(repo_path)
            .arg("config")
            .arg("--bool")
            .arg(key)
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .output()?;

        if !output.status.success() {
            // Config key doesn't exist or other error - return None
            return Ok(None);
        }

        let value = String::from_utf8_lossy(&output.stdout)
            .trim()
            .to_lowercase();
        match value.as_str() {
            "true" => Ok(Some(true)),
            "false" => Ok(Some(false)),
            _ => Ok(None),
        }
    }

    /// Get a string configuration value from Git
    ///
    /// Retrieves a string configuration value using `git config`.
    /// Returns `None` if the configuration key doesn't exist, is empty, or cannot be read.
    ///
    /// # Arguments
    ///
    /// * `repo_path` - Path to the Git repository
    /// * `key` - Git configuration key (e.g., "remote.origin.url")
    ///
    /// # Returns
    ///
    /// * `Ok(Some(value))` - Configuration contains a non-empty value
    /// * `Ok(None)` - Configuration key doesn't exist, is empty, or cannot be read
    /// * `Err(_)` - IO error occurred
    pub fn get_string_config(repo_path: &Path, key: &str) -> io::Result<Option<String>> {
        let output = Command::new("git")
            .arg("-C")
            .arg(repo_path)
            .arg("config")
            .arg(key)
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .output()?;

        if !output.status.success() {
            // Config key doesn't exist or other error - return None
            return Ok(None);
        }

        let value = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if value.is_empty() {
            Ok(None)
        } else {
            Ok(Some(value))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
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

        Ok(temp_dir)
    }

    fn set_git_config(repo_path: &Path, key: &str, value: &str) -> io::Result<()> {
        let output = Command::new("git")
            .arg("-C")
            .arg(repo_path)
            .arg("config")
            .arg(key)
            .arg(value)
            .output()?;

        if !output.status.success() {
            return Err(io::Error::new(
                io::ErrorKind::Other,
                format!("Failed to set git config {}={}", key, value),
            ));
        }

        Ok(())
    }

    #[test]
    fn test_get_bool_config_true() -> io::Result<()> {
        let temp_repo = create_test_repo()?;
        set_git_config(temp_repo.path(), "core.ignorecase", "true")?;

        let result = GitConfig::get_bool_config(temp_repo.path(), "core.ignorecase")?;
        assert_eq!(result, Some(true));

        Ok(())
    }

    #[test]
    fn test_get_bool_config_false() -> io::Result<()> {
        let temp_repo = create_test_repo()?;
        set_git_config(temp_repo.path(), "core.ignorecase", "false")?;

        let result = GitConfig::get_bool_config(temp_repo.path(), "core.ignorecase")?;
        assert_eq!(result, Some(false));

        Ok(())
    }

    #[test]
    fn test_get_bool_config_missing() -> io::Result<()> {
        let temp_repo = create_test_repo()?;

        let result = GitConfig::get_bool_config(temp_repo.path(), "nonexistent.key")?;
        assert_eq!(result, None);

        Ok(())
    }

    #[test]
    fn test_get_string_config_exists() -> io::Result<()> {
        let temp_repo = create_test_repo()?;
        set_git_config(
            temp_repo.path(),
            "remote.origin.url",
            "https://github.com/example/repo.git",
        )?;

        let result = GitConfig::get_string_config(temp_repo.path(), "remote.origin.url")?;
        assert_eq!(
            result,
            Some("https://github.com/example/repo.git".to_string())
        );

        Ok(())
    }

    #[test]
    fn test_get_string_config_missing() -> io::Result<()> {
        let temp_repo = create_test_repo()?;

        let result = GitConfig::get_string_config(temp_repo.path(), "remote.origin.url")?;
        assert_eq!(result, None);

        Ok(())
    }

    #[test]
    fn test_read_from_repo_with_all_configs() -> io::Result<()> {
        let temp_repo = create_test_repo()?;
        set_git_config(temp_repo.path(), "core.ignorecase", "true")?;
        set_git_config(temp_repo.path(), "core.precomposeunicode", "false")?;
        set_git_config(
            temp_repo.path(),
            "remote.origin.url",
            "https://github.com/example/repo.git",
        )?;

        let config = GitConfig::read_from_repo(temp_repo.path())?;

        assert_eq!(config.ignore_case, true);
        assert_eq!(config.precompose_unicode, false);
        assert_eq!(
            config.origin_url,
            Some("https://github.com/example/repo.git".to_string())
        );

        Ok(())
    }

    #[test]
    fn test_read_from_repo_with_defaults() -> io::Result<()> {
        let temp_repo = create_test_repo()?;

        let config = GitConfig::read_from_repo(temp_repo.path())?;

        // Note: core.ignorecase may be automatically set by Git on case-insensitive filesystems
        // So we just verify that we can read the config without error
        // and that missing configs return None for origin_url
        assert_eq!(config.precompose_unicode, false);
        assert_eq!(config.origin_url, None);

        Ok(())
    }

    #[test]
    fn test_read_from_repo_partial_configs() -> io::Result<()> {
        let temp_repo = create_test_repo()?;
        set_git_config(temp_repo.path(), "core.ignorecase", "true")?;
        // Don't set precomposeunicode or origin.url

        let config = GitConfig::read_from_repo(temp_repo.path())?;

        assert_eq!(config.ignore_case, true);
        assert_eq!(config.precompose_unicode, false); // default
        assert_eq!(config.origin_url, None); // default

        Ok(())
    }
}
