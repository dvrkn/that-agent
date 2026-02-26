//! Git safety operations for Anvil.
//!
//! Provides checkpoint/restore functionality to protect against edit failures.
//! Uses git2 for repository detection and status reads, and git CLI for
//! write operations (stash, branch) for reliability.

use std::path::Path;
use std::process::Command;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum GitError {
    #[error("not a git repository")]
    NotARepo,
    #[error("git2 error: {0}")]
    Git2(#[from] git2::Error),
    #[error("git command failed: {0}")]
    CommandFailed(String),
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
}

/// A snapshot of git state that can be restored on failure.
pub struct GitCheckpoint {
    pub repo_path: std::path::PathBuf,
    pub original_branch: String,
    pub stash_created: bool,
    pub safety_branch: Option<String>,
}

/// Create a checkpoint before editing.
///
/// 1. Discover the git repository
/// 2. If working tree is dirty, stash changes
/// 3. If `create_branch` is true, create a safety branch
pub fn create_checkpoint(file_path: &Path, create_branch: bool) -> Result<GitCheckpoint, GitError> {
    // Discover the git repository
    let repo = git2::Repository::discover(file_path)?;
    let repo_path = repo.workdir().ok_or(GitError::NotARepo)?.to_path_buf();

    // Get current branch name
    let head = repo.head()?;
    let original_branch = head.shorthand().unwrap_or("HEAD").to_string();

    // Check if working tree is dirty
    let statuses = repo.statuses(Some(
        git2::StatusOptions::new()
            .include_untracked(true)
            .recurse_untracked_dirs(false),
    ))?;

    let is_dirty = !statuses.is_empty();
    let mut stash_created = false;

    if is_dirty {
        // Use git CLI for stash (more reliable with edge cases)
        let output = Command::new("git")
            .args(["stash", "push", "-m", "anvil: pre-edit safety stash"])
            .current_dir(&repo_path)
            .output()?;

        if output.status.success() {
            let stdout = String::from_utf8_lossy(&output.stdout);
            // "No local changes" means nothing was stashed
            stash_created = !stdout.contains("No local changes");
        } else {
            let stderr = String::from_utf8_lossy(&output.stderr);
            tracing::warn!("git stash failed: {}", stderr);
        }
    }

    // Optionally create a safety branch
    let safety_branch = if create_branch {
        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        let branch_name = format!("anvil/edit-{}", timestamp);
        let output = Command::new("git")
            .args(["checkout", "-b", &branch_name])
            .current_dir(&repo_path)
            .output()?;
        if output.status.success() {
            Some(branch_name)
        } else {
            let stderr = String::from_utf8_lossy(&output.stderr);
            tracing::warn!("safety branch creation failed: {}", stderr);
            None
        }
    } else {
        None
    };

    Ok(GitCheckpoint {
        repo_path,
        original_branch,
        stash_created,
        safety_branch,
    })
}

/// Restore a checkpoint (pop stash, switch back to original branch).
pub fn restore_checkpoint(checkpoint: &GitCheckpoint) -> Result<(), GitError> {
    if checkpoint.stash_created {
        let output = Command::new("git")
            .args(["stash", "pop"])
            .current_dir(&checkpoint.repo_path)
            .output()?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(GitError::CommandFailed(format!(
                "stash pop failed: {}",
                stderr
            )));
        }
    }

    if let Some(ref branch) = checkpoint.safety_branch {
        let output = Command::new("git")
            .args(["checkout", &checkpoint.original_branch])
            .current_dir(&checkpoint.repo_path)
            .output()?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(GitError::CommandFailed(format!(
                "checkout back to {} failed: {}",
                checkpoint.original_branch, stderr
            )));
        }

        // Delete the safety branch
        let _ = Command::new("git")
            .args(["branch", "-D", branch])
            .current_dir(&checkpoint.repo_path)
            .output();
    }

    Ok(())
}

/// Check if a path is inside a git repository.
#[allow(dead_code)]
pub fn is_git_repo(path: &Path) -> bool {
    git2::Repository::discover(path).is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn setup_git_repo() -> TempDir {
        let tmp = TempDir::new().unwrap();
        Command::new("git")
            .args(["init"])
            .current_dir(tmp.path())
            .output()
            .unwrap();
        Command::new("git")
            .args(["config", "user.email", "test@test.com"])
            .current_dir(tmp.path())
            .output()
            .unwrap();
        Command::new("git")
            .args(["config", "user.name", "Test"])
            .current_dir(tmp.path())
            .output()
            .unwrap();
        fs::write(tmp.path().join("main.rs"), "fn main() {}\n").unwrap();
        Command::new("git")
            .args(["add", "."])
            .current_dir(tmp.path())
            .output()
            .unwrap();
        Command::new("git")
            .args(["commit", "-m", "init"])
            .current_dir(tmp.path())
            .output()
            .unwrap();
        tmp
    }

    #[test]
    fn test_is_git_repo() {
        let tmp = setup_git_repo();
        assert!(is_git_repo(tmp.path()));
    }

    #[test]
    fn test_not_git_repo() {
        let tmp = TempDir::new().unwrap();
        assert!(!is_git_repo(tmp.path()));
    }

    #[test]
    fn test_checkpoint_clean_repo() {
        let tmp = setup_git_repo();
        let cp = create_checkpoint(tmp.path(), false).unwrap();
        assert!(!cp.stash_created, "clean repo should not create stash");
    }

    #[test]
    fn test_checkpoint_dirty_repo() {
        let tmp = setup_git_repo();
        // Make dirty
        fs::write(tmp.path().join("main.rs"), "fn main() { /* dirty */ }\n").unwrap();

        let cp = create_checkpoint(tmp.path(), false).unwrap();
        assert!(cp.stash_created, "dirty repo should create stash");

        // Restore
        restore_checkpoint(&cp).unwrap();

        // Verify the dirty change is back
        let content = fs::read_to_string(tmp.path().join("main.rs")).unwrap();
        assert!(
            content.contains("dirty"),
            "stash pop should restore changes"
        );
    }

    #[test]
    fn test_checkpoint_not_a_repo() {
        let tmp = TempDir::new().unwrap();
        let result = create_checkpoint(tmp.path(), false);
        assert!(result.is_err());
    }

    #[test]
    fn test_checkpoint_with_safety_branch() {
        let tmp = setup_git_repo();
        let cp = create_checkpoint(tmp.path(), true).unwrap();
        assert!(cp.safety_branch.is_some(), "should create safety branch");
        assert!(cp
            .safety_branch
            .as_ref()
            .unwrap()
            .starts_with("anvil/edit-"));

        // Restore should switch back to original branch
        restore_checkpoint(&cp).unwrap();
    }

    #[test]
    fn test_restore_no_stash() {
        let tmp = setup_git_repo();
        let cp = GitCheckpoint {
            repo_path: tmp.path().to_path_buf(),
            original_branch: "main".to_string(),
            stash_created: false,
            safety_branch: None,
        };
        // Should succeed without doing anything
        restore_checkpoint(&cp).unwrap();
    }
}
