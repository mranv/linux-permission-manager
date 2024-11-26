use std::path::PathBuf;
use thiserror::Error;

/// Custom error types for the permission manager
#[derive(Error, Debug)]
pub enum PermissionError {
    #[error("Database error: {0}")]
    Database(#[from] sqlx::Error),

    #[error("IO error at {path:?}: {source}")]
    Io {
        #[source]
        source: std::io::Error,
        path: PathBuf,
    },

    #[error("Configuration error: {0}")]
    Config(String),

    #[error("User error: {0}")]
    User(String),

    #[error("System command error: {cmd} failed with {source}")]
    SystemCommand {
        #[source]
        source: std::io::Error,
        cmd: String,
    },

    #[error("Permission denied: {0}")]
    PermissionDenied(String),

    #[error("Invalid duration: {0}")]
    InvalidDuration(String),

    #[error("Command not allowed: {0}")]
    CommandNotAllowed(String),

    #[error("Group requirement not met: user {user} is not in required group {group}")]
    GroupRequirementNotMet {
        user: String,
        group: String,
    },

    #[error("User not found: {0}")]
    UserNotFound(String),

    #[error("Access expired")]
    AccessExpired,
}

/// Result type alias for Permission operations
pub type Result<T> = std::result::Result<T, PermissionError>;

impl PermissionError {
    /// Creates a new IO error with associated path
    pub fn io_error(source: std::io::Error, path: impl Into<PathBuf>) -> Self {
        Self::Io {
            source,
            path: path.into(),
        }
    }

    /// Creates a new system command error
    pub fn system_command(source: std::io::Error, cmd: impl Into<String>) -> Self {
        Self::SystemCommand {
            source,
            cmd: cmd.into(),
        }
    }

    /// Check if error is due to insufficient permissions
    pub fn is_permission_denied(&self) -> bool {
        matches!(self, Self::PermissionDenied(_))
    }

    /// Check if error is related to user configuration
    pub fn is_user_error(&self) -> bool {
        matches!(self, Self::User(_) | Self::UserNotFound(_) | Self::GroupRequirementNotMet { .. })
    }

    /// Returns true if this is a transient error that might succeed if retried
    pub fn is_transient(&self) -> bool {
        matches!(
            self,
            Self::Database(_) | // Database connection issues might be temporary
            Self::SystemCommand { .. } // System commands might fail temporarily
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Error as IoError, ErrorKind};

    #[test]
    fn test_io_error_creation() {
        let io_err = IoError::new(ErrorKind::NotFound, "file not found");
        let err = PermissionError::io_error(io_err, "/test/path");
        
        match err {
            PermissionError::Io { path, .. } => {
                assert_eq!(path, PathBuf::from("/test/path"));
            }
            _ => panic!("Expected IO error variant"),
        }
    }

    #[test]
    fn test_system_command_error() {
        let io_err = IoError::new(ErrorKind::PermissionDenied, "permission denied");
        let err = PermissionError::system_command(io_err, "test_command");
        
        match err {
            PermissionError::SystemCommand { cmd, .. } => {
                assert_eq!(cmd, "test_command");
            }
            _ => panic!("Expected SystemCommand error variant"),
        }
    }

    #[test]
    fn test_error_categorization() {
        let user_err = PermissionError::UserNotFound("testuser".to_string());
        assert!(user_err.is_user_error());
        assert!(!user_err.is_transient());

        let db_err = PermissionError::Database(sqlx::Error::PoolTimedOut);
        assert!(db_err.is_transient());
        assert!(!db_err.is_user_error());
    }
}