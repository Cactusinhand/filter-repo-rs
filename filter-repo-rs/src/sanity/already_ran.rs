use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use super::SanityCheckError;
use crate::gitutil;

#[derive(Debug, PartialEq)]
pub enum AlreadyRanState {
    NotRan,
    RecentRan,
    OldRan { age_hours: u64 },
}

pub struct AlreadyRanChecker {
    pub(super) ran_file: PathBuf,
}

impl AlreadyRanChecker {
    pub fn new(repo_path: &Path) -> io::Result<Self> {
        let git_dir = gitutil::git_dir(repo_path)?;
        let tmp_dir = git_dir.join("filter-repo");
        let ran_file = tmp_dir.join("already_ran");
        if !tmp_dir.exists() {
            fs::create_dir_all(&tmp_dir)?;
        }
        Ok(AlreadyRanChecker { ran_file })
    }

    pub fn check_already_ran(&self) -> io::Result<AlreadyRanState> {
        if !self.ran_file.exists() {
            return Ok(AlreadyRanState::NotRan);
        }
        let timestamp_str = fs::read_to_string(&self.ran_file)?;
        let timestamp: u64 = timestamp_str.trim().parse().map_err(|_| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                "Invalid timestamp in already_ran file",
            )
        })?;
        let current_time = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|_| io::Error::other("System time before Unix epoch"))?
            .as_secs();
        let age_seconds = current_time.saturating_sub(timestamp);
        let age_hours = age_seconds / 3600;
        if age_hours < 24 {
            Ok(AlreadyRanState::RecentRan)
        } else {
            Ok(AlreadyRanState::OldRan { age_hours })
        }
    }

    pub fn mark_as_ran(&self) -> io::Result<()> {
        let current_time = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|_| io::Error::other("System time before Unix epoch"))?
            .as_secs();
        fs::write(&self.ran_file, current_time.to_string())
    }

    pub fn clear_ran_marker(&self) -> io::Result<()> {
        if self.ran_file.exists() {
            fs::remove_file(&self.ran_file)?;
        }
        Ok(())
    }

    pub fn prompt_user_for_old_run(&self, age_hours: u64) -> io::Result<bool> {
        println!(
            "Filter-repo-rs was previously run on this repository {} hours ago.",
            age_hours
        );
        println!("The repository may be in an intermediate state.");
        print!("Do you want to continue with the existing state? [y/N]: ");
        io::stdout().flush()?;
        let mut input = String::new();
        io::stdin().read_line(&mut input)?;
        let response = input.trim().to_lowercase();
        Ok(matches!(response.as_str(), "y" | "yes"))
    }

    pub fn marker_file_exists(&self) -> bool {
        self.ran_file.exists()
    }
}

pub fn check_already_ran_detection(repo_path: &Path, force: bool) -> Result<(), SanityCheckError> {
    if force {
        return Ok(());
    }
    let checker = AlreadyRanChecker::new(repo_path)?;
    let state = checker.check_already_ran()?;
    match state {
        AlreadyRanState::NotRan => {
            checker.mark_as_ran()?;
            Ok(())
        }
        AlreadyRanState::RecentRan => Ok(()),
        AlreadyRanState::OldRan { age_hours } => {
            let user_confirmed = checker.prompt_user_for_old_run(age_hours)?;
            if user_confirmed {
                checker.mark_as_ran()?;
                Ok(())
            } else {
                Err(SanityCheckError::AlreadyRan {
                    ran_file: checker.ran_file.clone(),
                    age_hours,
                    user_confirmed: false,
                })
            }
        }
    }
}
