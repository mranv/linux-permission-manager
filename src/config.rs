use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::fs;
use serde::{Deserialize, Serialize};
use directories::ProjectDirs;
use chrono::Duration;

use crate::error::{PermissionError, Result};

/// Default configuration values
const DEFAULT_CONFIG_FILENAME: &str = "config.yaml";
const DEFAULT_SUDOERS_PATH: &str = "/etc/sudoers.d/permctl";
const DEFAULT_DB_PATH: &str = "/var/lib/permctl/permissions.db";
const DEFAULT_LOG_PATH: &str = "/var/log/permctl/access.log";

/// Configuration for a specific command
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommandConfig {
    /// Description of what the command does
    pub description: String,
    /// Maximum duration in minutes that this command can be granted for
    pub max_duration: i64,
    /// Groups that a user must be a member of to be granted this command
    pub required_groups: Vec<String>,
    /// Whether to audit all uses of this command
    #[serde(default)]
    pub audit_usage: bool,
    /// Maximum concurrent users allowed for this command
    #[serde(default = "default_max_users")]
    pub max_concurrent_users: usize,
}

impl CommandConfig {
    /// Validate the command configuration
    pub fn validate(&self) -> Result<()> {
        if self.max_duration <= 0 {
            return Err(PermissionError::Config(
                format!("max_duration must be positive, got {}", self.max_duration)
            ));
        }
        if self.max_concurrent_users == 0 {
            return Err(PermissionError::Config(
                "max_concurrent_users must be at least 1".to_string()
            ));
        }
        Ok(())
    }

    /// Convert max_duration to chrono::Duration
    pub fn max_duration_as_duration(&self) -> Duration {
        Duration::minutes(self.max_duration)
    }
}

fn default_max_users() -> usize {
    10
}

/// Main configuration structure
#[derive(Debug, Serialize, Deserialize)]
pub struct Config {
    /// Map of command paths to their configurations
    pub allowed_commands: HashMap<String, CommandConfig>,
    
    /// Path to the sudoers.d file for this application
    #[serde(default = "default_sudoers_path")]
    pub sudoers_path: PathBuf,
    
    /// Path to the SQLite database
    #[serde(default = "default_db_path")]
    pub db_path: PathBuf,
    
    /// Path to the log file
    #[serde(default = "default_log_path")]
    pub log_path: PathBuf,
    
    /// Whether to enable debug logging
    #[serde(default)]
    pub debug: bool,
    
    /// Number of days to keep audit logs
    #[serde(default = "default_log_retention")]
    pub log_retention_days: u32,
}

fn default_sudoers_path() -> PathBuf {
    PathBuf::from(DEFAULT_SUDOERS_PATH)
}

fn default_db_path() -> PathBuf {
    PathBuf::from(DEFAULT_DB_PATH)
}

fn default_log_path() -> PathBuf {
    PathBuf::from(DEFAULT_LOG_PATH)
}

fn default_log_retention() -> u32 {
    30
}

impl Config {
    /// Load configuration from the default location
    pub fn load() -> Result<Self> {
        let config_path = Self::default_config_path()?;
        Self::load_from(config_path)
    }

    /// Load configuration from a specific path
    pub fn load_from<P: AsRef<Path>>(path: P) -> Result<Self> {
        let content = fs::read_to_string(&path).map_err(|e| {
            PermissionError::io_error(e, path.as_ref().to_path_buf())
        })?;

        let config: Config = serde_yaml::from_str(&content)
            .map_err(|e| PermissionError::Config(format!("Invalid config format: {}", e)))?;

        config.validate()?;
        Ok(config)
    }

    /// Get the default configuration path
    pub fn default_config_path() -> Result<PathBuf> {
        let proj_dirs = ProjectDirs::from("com", "yourorg", "permctl")
            .ok_or_else(|| PermissionError::Config(
                "Could not determine config directory".to_string()
            ))?;

        Ok(proj_dirs.config_dir().join(DEFAULT_CONFIG_FILENAME))
    }

    /// Validate the entire configuration
    pub fn validate(&self) -> Result<()> {
        // Validate command configurations
        for (cmd, config) in &self.allowed_commands {
            if !cmd.starts_with('/') {
                return Err(PermissionError::Config(
                    format!("Command path must be absolute: {}", cmd)
                ));
            }
            config.validate()?;
        }

        // Validate paths
        for path in &[&self.sudoers_path, &self.db_path, &self.log_path] {
            if !path.is_absolute() {
                return Err(PermissionError::Config(
                    format!("Path must be absolute: {:?}", path)
                ));
            }
        }

        Ok(())
    }

    /// Save configuration to a file
    pub fn save_to<P: AsRef<Path>>(&self, path: P) -> Result<()> {
        let content = serde_yaml::to_string(self)
            .map_err(|e| PermissionError::Config(format!("Failed to serialize config: {}", e)))?;

        fs::write(&path, content).map_err(|e| PermissionError::io_error(e, path.as_ref().to_path_buf()))
    }

    /// Create a default configuration
    pub fn default() -> Self {
        let mut allowed_commands = HashMap::new();
        allowed_commands.insert(
            "/usr/bin/docker".to_string(),
            CommandConfig {
                description: "Docker command access".to_string(),
                max_duration: 480, // 8 hours
                required_groups: vec!["docker".to_string()],
                audit_usage: true,
                max_concurrent_users: 5,
            },
        );

        Config {
            allowed_commands,
            sudoers_path: default_sudoers_path(),
            db_path: default_db_path(),
            log_path: default_log_path(),
            debug: false,
            log_retention_days: default_log_retention(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_command_config_validation() {
        let valid_config = CommandConfig {
            description: "test".to_string(),
            max_duration: 60,
            required_groups: vec!["test".to_string()],
            audit_usage: true,
            max_concurrent_users: 5,
        };
        assert!(valid_config.validate().is_ok());

        let invalid_duration = CommandConfig {
            max_duration: 0,
            ..valid_config.clone()
        };
        assert!(invalid_duration.validate().is_err());
    }

    #[test]
    fn test_config_serialization() {
        let temp_dir = TempDir::new().unwrap();
        let config_path = temp_dir.path().join("config.yaml");
        
        let config = Config::default();
        config.save_to(&config_path).unwrap();
        
        let loaded_config = Config::load_from(&config_path).unwrap();
        assert_eq!(
            loaded_config.allowed_commands.len(),
            config.allowed_commands.len()
        );
    }

    #[test]
    fn test_config_validation() {
        let mut config = Config::default();
        
        // Test invalid command path
        config.allowed_commands.insert(
            "invalid-path".to_string(),
            CommandConfig {
                description: "test".to_string(),
                max_duration: 60,
                required_groups: vec![],
                audit_usage: false,
                max_concurrent_users: 1,
            },
        );
        assert!(config.validate().is_err());
    }
}