use std::process;
use clap::{Parser, Subcommand};
use chrono::{Duration, Utc};
use tracing::{info, warn, error};

use linux_permission_manager::{
    Config,
    PermissionManager,
    error::{Result, PermissionError},
};

#[derive(Parser)]
#[command(
    name = "permctl",
    about = "Linux Permission Manager CLI",
    version,
    author,
    long_about = "A command-line tool for managing temporary elevated permissions in Linux"
)]
struct Cli {
    /// Path to config file
    #[arg(short, long, global = true)]
    config: Option<String>,

    /// Enable debug logging
    #[arg(short, long, global = true)]
    debug: bool,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Grant temporary permission to a user
    Grant {
        /// Username to grant permission to
        username: String,
        
        /// Command to grant permission for
        command: String,
        
        /// Duration in minutes
        #[arg(short, long, default_value = "60")]
        duration: i64,
    },

    /// Revoke permission from a user
    Revoke {
        /// Username to revoke permission from
        username: String,
        
        /// Command to revoke permission for
        command: String,
    },

    /// List permissions
    List {
        /// Show all permissions, including expired ones
        #[arg(short, long)]
        all: bool,

        /// Show permissions for specific user
        #[arg(short, long)]
        user: Option<String>,
    },

    /// Show allowed commands
    Commands {
        /// Show detailed information about commands
        #[arg(short, long)]
        verbose: bool,
    },

    /// Clean up expired permissions
    Cleanup,

    /// Initialize configuration
    Init {
        /// Force overwrite existing configuration
        #[arg(short, long)]
        force: bool,
    },

    /// Verify configuration and permissions
    Verify,
}

#[tokio::main]
async fn main() -> Result<()> {
    // Parse command line arguments
    let cli = Cli::parse();

    // Setup logging
    setup_logging(cli.debug)?;

    // Load configuration
    let config = match &cli.config {
        Some(path) => Config::load_from(path),
        None => Config::load(),
    }?;

    // Initialize permission manager
    let manager = PermissionManager::new(config).await?;

    // Process commands
    match cli.command {
        Commands::Grant { username, command, duration } => {
            grant_permission(&manager, &username, &command, duration).await?;
        }

        Commands::Revoke { username, command } => {
            revoke_permission(&manager, &username, &command).await?;
        }

        Commands::List { all, user } => {
            list_permissions(&manager, all, user).await?;
        }

        Commands::Commands { verbose } => {
            show_commands(&manager, verbose)?;
        }

        Commands::Cleanup => {
            cleanup_expired(&manager).await?;
        }

        Commands::Init { force } => {
            initialize_config(force)?;
        }

        Commands::Verify => {
            verify_setup(&manager).await?;
        }
    }

    Ok(())
}

fn setup_logging(debug: bool) -> Result<()> {
    let level = if debug { "debug" } else { "info" };
    
    tracing_subscriber::fmt()
        .with_env_filter(format!("permctl={},linux_permission_manager={}", level, level))
        .try_init()
        .map_err(|e| PermissionError::Config(format!("Failed to initialize logging: {}", e)))?;

    Ok(())
}

async fn grant_permission(
    manager: &PermissionManager,
    username: &str,
    command: &str,
    duration_mins: i64,
) -> Result<()> {
    let duration = Duration::minutes(duration_mins);
    let granted_by = whoami::username();

    match manager.grant_permission(username, command, duration, &granted_by).await {
        Ok(id) => {
            println!("✓ Permission granted successfully");
            println!("  ID: {}", id);
            println!("  User: {}", username);
            println!("  Command: {}", command);
            println!("  Duration: {} minutes", duration_mins);
            println!("  Expires: {}", Utc::now() + duration);
            Ok(())
        }
        Err(e) => {
            println!("✗ Failed to grant permission");
            println!("  Error: {}", e);
            Err(e)
        }
    }
}

async fn revoke_permission(
    manager: &PermissionManager,
    username: &str,
    command: &str,
) -> Result<()> {
    let revoked_by = whoami::username();

    match manager.revoke_permission(username, command, &revoked_by).await {
        Ok(true) => {
            println!("✓ Permission revoked successfully");
            println!("  User: {}", username);
            println!("  Command: {}", command);
            Ok(())
        }
        Ok(false) => {
            println!("! No active permission found to revoke");
            Ok(())
        }
        Err(e) => {
            println!("✗ Failed to revoke permission");
            println!("  Error: {}", e);
            Err(e)
        }
    }
}

async fn list_permissions(
    manager: &PermissionManager,
    all: bool,
    user: Option<String>,
) -> Result<()> {
    if let Some(username) = user {
        let permissions = manager.list_user_permissions(&username).await?;
        if permissions.is_empty() {
            println!("No permissions found for user {}", username);
            return Ok(());
        }

        println!("Permissions for user {}:", username);
        for perm in permissions {
            println!("  Command: {}", perm.command);
            println!("    Granted: {}", perm.granted_at);
            println!("    Expires: {}", perm.expires_at);
            if let Some(last_used) = perm.last_used {
                println!("    Last used: {}", last_used);
            }
            println!();
        }
    } else {
        // TODO: Implement listing all permissions
        println!("Listing all permissions not implemented yet");
    }

    Ok(())
}

fn show_commands(manager: &PermissionManager, verbose: bool) -> Result<()> {
    println!("Allowed commands:");
    
    for (cmd, config) in &manager.config().allowed_commands {
        if verbose {
            println!("\n{}", cmd);
            println!("  Description: {}", config.description);
            println!("  Max duration: {} minutes", config.max_duration);
            println!("  Required groups: {}", config.required_groups.join(", "));
            if config.audit_usage {
                println!("  Auditing: enabled");
            }
            println!("  Max concurrent users: {}", config.max_concurrent_users);
        } else {
            println!("  {}", cmd);
        }
    }

    Ok(())
}

async fn cleanup_expired(manager: &PermissionManager) -> Result<()> {
    let count = manager.cleanup_expired().await?;
    if count > 0 {
        println!("✓ Cleaned up {} expired permission(s)", count);
    } else {
        println!("No expired permissions to clean up");
    }
    Ok(())
}

fn initialize_config(force: bool) -> Result<()> {
    let config_path = Config::default_config_path()?;
    
    if config_path.exists() && !force {
        println!("! Configuration file already exists at {:?}", config_path);
        println!("  Use --force to overwrite");
        return Ok(());
    }

    let config = Config::default();
    config.save_to(config_path.clone())?;

    println!("✓ Created default configuration at {:?}", config_path);
    println!("  Please review and customize before using");
    
    Ok(())
}

async fn verify_setup(manager: &PermissionManager) -> Result<()> {
    println!("Verifying setup...");

    // Check sudoers file
    if !manager.config().sudoers_path.exists() {
        println!("✗ Sudoers file not found");
        return Err(PermissionError::Config("Sudoers file not found".to_string()));
    }

    // Check database
    // This will fail if database connection fails
    manager.list_user_permissions("test").await?;
    println!("✓ Database connection successful");

    // Check directories
    for path in [
        manager.config().sudoers_path.parent(),
        manager.config().db_path.parent(),
        manager.config().log_path.parent(),
    ].iter().flatten() {
        if !path.exists() {
            println!("✗ Required directory not found: {:?}", path);
            return Err(PermissionError::Config(format!(
                "Required directory not found: {:?}", 
                path
            )));
        }
    }

    println!("✓ All directories present");

    // Verify current process permissions
    if !nix::unistd::Uid::effective().is_root() {
        println!("! Warning: Not running as root");
        println!("  Some operations may fail");
    }

    println!("✓ Setup verification complete");
    Ok(())
}