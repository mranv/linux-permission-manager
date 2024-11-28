use std::process::Command;
use std::fs;
use chrono::{Utc, Duration};
// use tracing::info;

use crate::config::Config;
use crate::db::{Database, PermissionGrant};
use crate::error::{Result, PermissionError};

/// Core permission manager that handles all permission-related operations
pub struct PermissionManager {
    config: Config,
    db: Database,
}

impl PermissionManager {
    /// Create a new permission manager instance
    pub async fn new(config: Config) -> Result<Self> {
        let db = Database::new(&config.db_path).await?;
        let manager = Self { config, db };
        manager.initialize().await?;
        Ok(manager)
    }

    /// Initialize the permission manager
    async fn initialize(&self) -> Result<()> {
        // Ensure required directories exist
        for path in [
            self.config.sudoers_path.parent(),
            self.config.db_path.parent(),
            self.config.log_path.parent(),
        ].iter().flatten() {
            fs::create_dir_all(path).map_err(|e| PermissionError::io_error(e, path.to_path_buf()))?;
        }

        // Initialize sudoers file
        self.update_sudoers_file().await?;

        Ok(())
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

        // Validate user exists
        if !self.user_exists(username)? {
            return Err(PermissionError::UserNotFound(username.to_string()));
        }

        // Check user groups
        for group in &cmd_config.required_groups {
            if !self.user_in_group(username, group)? {
                return Err(PermissionError::GroupRequirementNotMet {
                    user: username.to_string(),
                    group: group.to_string(),
                });
            }
        }

        // Calculate expiration
        let expires_at = Utc::now() + duration;

        // Grant permission in database
        let id = self.db.grant_permission(username, command, expires_at, granted_by).await?;

        // Update sudoers file
        self.update_sudoers_file().await?;

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
            // Update sudoers file
            self.update_sudoers_file().await?;
        }

        Ok(revoked)
    }

    /// List all active permissions for a user
    pub async fn list_user_permissions(&self, username: &str) -> Result<Vec<PermissionGrant>> {
        self.db.list_user_permissions(username).await
    }

    /// Check if a user has permission for a specific command
    pub async fn check_permission(&self, username: &str, command: &str) -> Result<bool> {
        self.db.check_permission(username, command).await
    }

    /// Update the usage timestamp for a permission
    pub async fn record_usage(&self, username: &str, command: &str) -> Result<()> {
        self.db.update_last_used(username, command).await
    }

    /// Clean up expired permissions
    pub async fn cleanup_expired(&self) -> Result<u64> {
        let count = self.db.cleanup_expired().await?;
        if count > 0 {
            self.update_sudoers_file().await?;
        }
        Ok(count)
    }

    /// Update the sudoers file with current permissions
async fn update_sudoers_file(&self) -> Result<()> {
    let header = "# This file is managed by permctl. Do not edit manually.\n\n";
    let mut content = String::from(header);

    // Get all active permissions from database
    let all_permissions = sqlx::query!(
        r#"
        SELECT *
        FROM permission_grants
        WHERE NOT revoked
            AND expires_at > ?
        ORDER BY username, command
        "#,
        Utc::now()
    )
    .fetch_all(self.db.get_pool())
    .await
    .map_err(PermissionError::Database)?;

    // Group permissions by user for better organization
    use std::collections::HashMap;
    let mut user_permissions: HashMap<String, Vec<String>> = HashMap::new();

    for row in all_permissions {
        user_permissions
            .entry(row.username)
            .or_default()
            .push(row.command);
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
    Command::new("chmod")
        .arg("0440")
        .arg(&temp_path)
        .status()
        .map_err(|e| PermissionError::system_command(e, "chmod"))?;

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

        // Add a test command
        use crate::config::CommandConfig;
        config.allowed_commands.insert(
            "/test/command".to_string(),
            CommandConfig {
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
    async fn test_grant_and_check_permission() {
        let (manager, _temp) = create_test_manager().await;
        
        // Mock user_exists and user_in_group for testing
        // In a real environment, these would check against the system
        
        let id = manager.grant_permission(
            "testuser",
            "/test/command",
            Duration::minutes(30),
            "admin"
        ).await.unwrap();

        assert!(id > 0);
        assert!(manager.check_permission("testuser", "/test/command").await.unwrap());
    }

    #[tokio::test]
    async fn test_revoke_permission() {
        let (manager, _temp) = create_test_manager().await;

        manager.grant_permission(
            "testuser",
            "/test/command",
            Duration::minutes(30),
            "admin"
        ).await.unwrap();

        assert!(manager.revoke_permission(
            "testuser",
            "/test/command",
            "admin"
        ).await.unwrap());

        assert!(!manager.check_permission("testuser", "/test/command").await.unwrap());
    }
}