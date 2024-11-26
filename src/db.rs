use std::path::Path;
use sqlx::{sqlite::{SqlitePool, SqlitePoolOptions}, Row};
use chrono::{DateTime, Utc};
use serde::{Serialize, Deserialize};
use tracing::{info, warn, error};

use crate::error::{Result, PermissionError};

/// Represents a permission grant in the database
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PermissionGrant {
    pub id: i64,
    pub username: String,
    pub command: String,
    pub granted_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
    pub granted_by: String,
    pub last_used: Option<DateTime<Utc>>,
    pub revoked: bool,
    pub revoked_at: Option<DateTime<Utc>>,
    pub revoked_by: Option<String>,
}

/// Database manager for permission storage
pub struct Database {
    pool: SqlitePool,
}

impl Database {
    /// Create a new database connection
    pub async fn new(db_path: impl AsRef<Path>) -> Result<Self> {
        // Ensure parent directory exists
        if let Some(parent) = db_path.as_ref().parent() {
            std::fs::create_dir_all(parent).map_err(|e| {
                PermissionError::io_error(e, parent.to_path_buf())
            })?;
        }

        let pool = SqlitePoolOptions::new()
            .max_connections(5)
            .connect(&format!("sqlite:{}", db_path.as_ref().display()))
            .await
            .map_err(PermissionError::Database)?;

        let db = Self { pool };
        db.initialize().await?;
        Ok(db)
    }

    /// Initialize the database schema
    async fn initialize(&self) -> Result<()> {
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS permission_grants (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                username TEXT NOT NULL,
                command TEXT NOT NULL,
                granted_at DATETIME NOT NULL,
                expires_at DATETIME NOT NULL,
                granted_by TEXT NOT NULL,
                last_used DATETIME,
                revoked BOOLEAN NOT NULL DEFAULT FALSE,
                revoked_at DATETIME,
                revoked_by TEXT,
                UNIQUE(username, command) ON CONFLICT REPLACE
            );

            CREATE INDEX IF NOT EXISTS idx_permissions_user 
                ON permission_grants(username);
            CREATE INDEX IF NOT EXISTS idx_permissions_expires 
                ON permission_grants(expires_at);
            CREATE INDEX IF NOT EXISTS idx_permissions_active 
                ON permission_grants(username, command, expires_at) 
                WHERE NOT revoked;

            CREATE TABLE IF NOT EXISTS audit_log (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                timestamp DATETIME NOT NULL,
                username TEXT NOT NULL,
                command TEXT NOT NULL,
                action TEXT NOT NULL,
                details TEXT
            );
            "#,
        )
        .execute(&self.pool)
        .await
        .map_err(PermissionError::Database)?;

        Ok(())
    }

    /// Grant a new permission
    pub async fn grant_permission(
        &self,
        username: &str,
        command: &str,
        expires_at: DateTime<Utc>,
        granted_by: &str,
    ) -> Result<i64> {
        let now = Utc::now();
        
        let result = sqlx::query(
            r#"
            INSERT INTO permission_grants 
                (username, command, granted_at, expires_at, granted_by)
            VALUES (?, ?, ?, ?, ?)
            RETURNING id
            "#,
        )
        .bind(username)
        .bind(command)
        .bind(now)
        .bind(expires_at)
        .bind(granted_by)
        .fetch_one(&self.pool)
        .await
        .map_err(PermissionError::Database)?;

        let id = result.get::<i64, _>("id");

        self.add_audit_log(
            username,
            command,
            "grant",
            Some(&format!("Granted by {} until {}", granted_by, expires_at)),
        ).await?;

        info!(
            "Granted permission: id={}, user={}, command={}, expires={}",
            id, username, command, expires_at
        );

        Ok(id)
    }

    /// Revoke a permission
    pub async fn revoke_permission(
        &self,
        username: &str,
        command: &str,
        revoked_by: &str,
    ) -> Result<bool> {
        let now = Utc::now();
        
        let result = sqlx::query(
            r#"
            UPDATE permission_grants
            SET revoked = TRUE,
                revoked_at = ?,
                revoked_by = ?
            WHERE username = ?
                AND command = ?
                AND NOT revoked
                AND expires_at > ?
            "#,
        )
        .bind(now)
        .bind(revoked_by)
        .bind(username)
        .bind(command)
        .bind(now)
        .execute(&self.pool)
        .await
        .map_err(PermissionError::Database)?;

        if result.rows_affected() > 0 {
            self.add_audit_log(
                username,
                command,
                "revoke",
                Some(&format!("Revoked by {}", revoked_by)),
            ).await?;

            info!(
                "Revoked permission: user={}, command={}, revoked_by={}",
                username, command, revoked_by
            );
            Ok(true)
        } else {
            Ok(false)
        }
    }

    /// Check if a permission is currently valid
    pub async fn check_permission(
        &self,
        username: &str,
        command: &str,
    ) -> Result<bool> {
        let now = Utc::now();
        
        let count = sqlx::query(
            r#"
            SELECT COUNT(*) as count
            FROM permission_grants
            WHERE username = ?
                AND command = ?
                AND NOT revoked
                AND expires_at > ?
            "#,
        )
        .bind(username)
        .bind(command)
        .bind(now)
        .fetch_one(&self.pool)
        .await
        .map_err(PermissionError::Database)?
        .get::<i64, _>("count");

        Ok(count > 0)
    }

    /// Update last used timestamp for a permission
    pub async fn update_last_used(
        &self,
        username: &str,
        command: &str,
    ) -> Result<()> {
        let now = Utc::now();
        
        sqlx::query(
            r#"
            UPDATE permission_grants
            SET last_used = ?
            WHERE username = ?
                AND command = ?
                AND NOT revoked
                AND expires_at > ?
            "#,
        )
        .bind(now)
        .bind(username)
        .bind(command)
        .bind(now)
        .execute(&self.pool)
        .await
        .map_err(PermissionError::Database)?;

        self.add_audit_log(
            username,
            command,
            "use",
            None,
        ).await?;

        Ok(())
    }

    /// List all active permissions for a user
    pub async fn list_user_permissions(
        &self,
        username: &str,
    ) -> Result<Vec<PermissionGrant>> {
        let now = Utc::now();
        
        let grants = sqlx::query_as!(
            PermissionGrant,
            r#"
            SELECT *
            FROM permission_grants
            WHERE username = ?
                AND NOT revoked
                AND expires_at > ?
            ORDER BY expires_at DESC
            "#,
            username,
            now
        )
        .fetch_all(&self.pool)
        .await
        .map_err(PermissionError::Database)?;

        Ok(grants)
    }

    /// Add an entry to the audit log
    async fn add_audit_log(
        &self,
        username: &str,
        command: &str,
        action: &str,
        details: Option<&str>,
    ) -> Result<()> {
        let now = Utc::now();
        
        sqlx::query(
            r#"
            INSERT INTO audit_log 
                (timestamp, username, command, action, details)
            VALUES (?, ?, ?, ?, ?)
            "#,
        )
        .bind(now)
        .bind(username)
        .bind(command)
        .bind(action)
        .bind(details)
        .execute(&self.pool)
        .await
        .map_err(PermissionError::Database)?;

        Ok(())
    }

    /// Clean up expired permissions
    pub async fn cleanup_expired(&self) -> Result<u64> {
        let now = Utc::now();
        
        let result = sqlx::query(
            r#"
            UPDATE permission_grants
            SET revoked = TRUE,
                revoked_at = ?,
                revoked_by = 'system_cleanup'
            WHERE NOT revoked
                AND expires_at <= ?
            "#,
        )
        .bind(now)
        .bind(now)
        .execute(&self.pool)
        .await
        .map_err(PermissionError::Database)?;

        Ok(result.rows_affected())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    async fn create_test_db() -> (Database, TempDir) {
        let temp_dir = TempDir::new().unwrap();
        let db_path = temp_dir.path().join("test.db");
        let db = Database::new(&db_path).await.unwrap();
        (db, temp_dir)
    }

    #[tokio::test]
    async fn test_grant_and_check_permission() {
        let (db, _temp) = create_test_db().await;
        let expires_at = Utc::now() + chrono::Duration::hours(1);

        let id = db.grant_permission(
            "testuser",
            "/test/command",
            expires_at,
            "admin"
        ).await.unwrap();

        assert!(id > 0);
        assert!(db.check_permission("testuser", "/test/command").await.unwrap());
    }

    #[tokio::test]
    async fn test_revoke_permission() {
        let (db, _temp) = create_test_db().await;
        let expires_at = Utc::now() + chrono::Duration::hours(1);

        db.grant_permission(
            "testuser",
            "/test/command",
            expires_at,
            "admin"
        ).await.unwrap();

        assert!(db.revoke_permission(
            "testuser",
            "/test/command",
            "admin"
        ).await.unwrap());

        assert!(!db.check_permission("testuser", "/test/command").await.unwrap());
    }
}