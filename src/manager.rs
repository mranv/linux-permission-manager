use std::process::Command;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use chrono::{Utc, Duration};
use tracing::{info, warn, error};

use crate::config::Config;
use crate::db::{Database, PermissionGrant};
use crate::error::{Result, PermissionError};

/// Core permission manager that handles all permission-related operations
pub struct PermissionManager {
    config: Config,
    db: Database,
}

impl PermissionManager {
    /// Create a new permission manager instance with the provided configuration
    pub async fn new(config: Config) -> Result<Self> {
        // Validate the configuration before proceeding
        config.validate()?;

        // Set up the required directory structure
        Self::setup_directories(&config)?;

        // Initialize the database connection
        let db = Database::new(&config.db_path).await?;
        
        let manager = Self { config, db };
        manager.initialize().await?;
        
        Ok(manager)
    }

    /// Initialize the permission manager and set up required components
    async fn initialize(&self) -> Result<()> {
        // Create and set up required directories
        for path in [
            self.config.sudoers_path.parent(),
            self.config.db_path.parent(),
            self.config.log_path.parent(),
        ].iter().flatten() {
            fs::create_dir_all(path)
                .map_err(|e| PermissionError::io_error(e, path.to_path_buf()))?;
            
            // Set appropriate directory permissions
            let mut perms = fs::metadata(path)?.permissions();
            perms.set_mode(0o755);
            fs::set_permissions(path, perms)
                .map_err(|e| PermissionError::io_error(e, path.to_path_buf()))?;
        }

        // Initialize sudoers file configuration
        self.update_sudoers_file().await?;

        Ok(())
    }

    /// Get a reference to the current configuration
    pub fn config(&self) -> &Config {
        &self.config
    }

    /// Grant permission to a user for a specific command
    pub async fn grant_permission(
        &self,
        username: &str,
        command: &str,
        duration: Duration,
        granted_by: &str,
    ) -> Result<i64> {
        // Validate command is allowed
        let cmd_config = self.config.allowed_commands.get(command)
            .ok_or_else(|| PermissionError::CommandNotAllowed(command.to_string()))?;

        // Validate duration
        if duration > cmd_config.max_duration_as_duration() {
            return Err(PermissionError::InvalidDuration(format!(
                "Duration exceeds maximum allowed ({} minutes)",
                cmd_config.max_duration
            )));
        }

        // Validate user exists on system
        if !self.user_exists(username)? {
            return Err(PermissionError::UserNotFound(username.to_string()));
        }

        // Check user group requirements
        for group in &cmd_config.required_groups {
            if !self.user_in_group(username, group)? {
                return Err(PermissionError::GroupRequirementNotMet {
                    user: username.to_string(),
                    group: group.to_string(),
                });
            }
        }

        // Calculate expiration time
        let expires_at = Utc::now() + duration;

        // Grant permission in database
        let id = self.db.grant_permission(username, command, expires_at, granted_by).await?;

        // Update sudoers configuration
        self.update_sudoers_file().await?;

        info!(
            "Granted permission: id={}, user={}, command={}, expires={}",
            id, username, command, expires_at
        );

        Ok(id)
    }

    /// Revoke permission from a user for a specific command
    pub async fn revoke_permission(
        &self,
        username: &str,
        command: &str,
        revoked_by: &str,
    ) -> Result<bool> {
        // Revoke in database
        let revoked = self.db.revoke_permission(username, command, revoked_by).await?;

        if revoked {
            // Update sudoers configuration
            self.update_sudoers_file().await?;
            info!("Revoked permission: user={}, command={}", username, command);
        } else {
            warn!("No active permission found to revoke: user={}, command={}", username, command);
        }

        Ok(revoked)
    }

    /// List all active permissions for a user
    pub async fn list_user_permissions(&self, username: &str) -> Result<Vec<PermissionGrant>> {
        self.db.list_user_permissions(username).await
    }

    /// Clean up expired permissions
    pub async fn cleanup_expired(&self) -> Result<u64> {
        let count = self.db.cleanup_expired().await?;
        if count > 0 {
            self.update_sudoers_file().await?;
            info!("Cleaned up {} expired permission(s)", count);
        }
        Ok(count)
    }

    /// Update the sudoers file with current permissions
    async fn update_sudoers_file(&self) -> Result<()> {
        let header = "# This file is managed by permctl. Do not edit manually.\n\n";
        let mut content = String::from(header);

        // Get all active permissions
        let all_permissions = self.db.list_active_permissions().await?;

        // Group permissions by user for better organization
        use std::collections::HashMap;
        let mut user_permissions: HashMap<String, Vec<String>> = HashMap::new();

        for grant in all_permissions {
            user_permissions
                .entry(grant.username)
                .or_default()
                .push(grant.command);
        }

        // Build sudoers content
        for (username, commands) in user_permissions {
            for command in commands {
                content.push_str(&format!(
                    "{} ALL=(ALL) NOPASSWD: {}\n",
                    username, command
                ));
            }
        }

        // Write to temporary file first
        let temp_path = self.config.sudoers_path.with_extension("tmp");
        fs::write(&temp_path, content.as_bytes())
            .map_err(|e| PermissionError::io_error(e, temp_path.clone()))?;

        // Set correct permissions (0440)
        let mut perms = fs::metadata(&temp_path)?.permissions();
        perms.set_mode(0o440);
        fs::set_permissions(&temp_path, perms)
            .map_err(|e| PermissionError::io_error(e, temp_path.clone()))?;

        // Move temporary file to final location
        fs::rename(&temp_path, &self.config.sudoers_path)
            .map_err(|e| PermissionError::io_error(e, self.config.sudoers_path.clone()))?;

        Ok(())
    }

    /// Check if a user exists on the system
    fn user_exists(&self, username: &str) -> Result<bool> {
        let output = Command::new("id")
            .arg(username)
            .output()
            .map_err(|e| PermissionError::system_command(e, "id"))?;

        Ok(output.status.success())
    }

    /// Check if a user is member of a group
    fn user_in_group(&self, username: &str, group: &str) -> Result<bool> {
        let output = Command::new("groups")
            .arg(username)
            .output()
            .map_err(|e| PermissionError::system_command(e, "groups"))?;

        let groups = String::from_utf8_lossy(&output.stdout);
        Ok(groups.split_whitespace().any(|g| g == group))
    }

    /// Set up required directories with appropriate permissions
    fn setup_directories(config: &Config) -> Result<()> {
        let dirs = [
            config.db_path.parent(),
            config.log_path.parent(),
            config.sudoers_path.parent(),
        ];

        for dir in dirs.iter().flatten() {
            fs::create_dir_all(dir)
                .map_err(|e| PermissionError::io_error(e, dir.to_path_buf()))?;
            
            let mut perms = fs::metadata(dir)?.permissions();
            perms.set_mode(0o755);
            fs::set_permissions(dir, perms)
                .map_err(|e| PermissionError::io_error(e, dir.to_path_buf()))?;
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;
    use std::collections::HashMap;

    async fn create_test_manager() -> (PermissionManager, TempDir) {
        let temp_dir = TempDir::new().unwrap();
        
        let mut config = Config {
            allowed_commands: HashMap::new(),
            sudoers_path: temp_dir.path().join("sudoers"),
            db_path: temp_dir.path().join("test.db"),
            log_path: temp_dir.path().join("test.log"),
            debug: false,
            log_retention_days: 30,
        };

        config.allowed_commands.insert(
            "/test/command".to_string(),
            crate::config::CommandConfig {
                description: "Test command".to_string(),
                max_duration: 60,
                required_groups: vec!["users".to_string()],
                audit_usage: true,
                max_concurrent_users: 5,
            },
        );

        let manager = PermissionManager::new(config).await.unwrap();
        (manager, temp_dir)
    }

    #[tokio::test]
    async fn test_grant_and_revoke_permission() {
        let (manager, _temp) = create_test_manager().await;
        
        let id = manager.grant_permission(
            "testuser",
            "/test/command",
            Duration::minutes(30),
            "admin"
        ).await.unwrap();

        assert!(id > 0);

        let revoked = manager.revoke_permission(
            "testuser",
            "/test/command",
            "admin"
        ).await.unwrap();

        assert!(revoked);
    }
}