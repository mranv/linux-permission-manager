#!/bin/bash

# Exit on any error and undefined variables
set -eu

# Configuration variables
INSTALL_DIR="/usr/sbin"
CONFIG_DIR="/etc/permctl"
DATA_DIR="/var/lib/permctl"
LOG_DIR="/var/log/permctl"
SUDOERS_DIR="/etc/sudoers.d"
SYSTEMD_DIR="/etc/systemd/system"
MAN_DIR="/usr/share/man/man1"

# Color definitions for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m'

# Function to log messages with timestamp
log() {
    echo -e "${2:-}$(date '+%Y-%m-%d %H:%M:%S') - $1${NC}"
}

# Function to check if a command exists
check_command() {
    if ! command -v "$1" >/dev/null 2>&1; then
        log "Required command '$1' not found. Please install it first." "${RED}"
        exit 1
    fi
}

# Function to create directory with proper permissions
create_directory() {
    local dir="$1"
    local perms="${2:-755}"
    
    if [ ! -d "$dir" ]; then
        mkdir -p "$dir"
        log "Created directory: $dir" "${YELLOW}"
    fi
    
    chown root:root "$dir"
    chmod "$perms" "$dir"
}

# Check if running as root
if [ "$EUID" -ne 0 ]; then
    log "This script must be run as root" "${RED}"
    exit 1
}

# Check for required commands
log "Checking prerequisites..." "${YELLOW}"
check_command "cargo"
check_command "systemctl"
check_command "gzip"

# Display installation start message
log "Starting Linux Permission Manager installation..." "${YELLOW}"

# Create required directories with proper permissions
log "Creating and configuring directories..." "${YELLOW}"
create_directory "$CONFIG_DIR"
create_directory "$DATA_DIR"
create_directory "$LOG_DIR"
create_directory "$SUDOERS_DIR" "750"

# Build the binary
log "Building binary..." "${YELLOW}"
if ! cargo build --release; then
    log "Failed to build the binary" "${RED}"
    exit 1
fi

# Install binary
log "Installing binary..." "${YELLOW}"
install -m 755 target/release/permctl "$INSTALL_DIR/permctl" || {
    log "Failed to install binary" "${RED}"
    exit 1
}

# Initialize configuration
if [ ! -f "$CONFIG_DIR/config.yaml" ]; then
    log "Installing default configuration..." "${YELLOW}"
    "$INSTALL_DIR/permctl" init || {
        log "Failed to initialize configuration" "${RED}"
        exit 1
    }
    chmod 644 "$CONFIG_DIR/config.yaml"
fi

# Install man page if it exists
if [ -f "docs/permctl.1" ]; then
    log "Installing man page..." "${YELLOW}"
    install -m 644 docs/permctl.1 "$MAN_DIR/permctl.1"
    gzip -f "$MAN_DIR/permctl.1"
else
    log "Man page not found, skipping..." "${YELLOW}"
fi

# Install and configure systemd units
log "Configuring systemd services..." "${YELLOW}"
for unit in permctl.service permctl.timer; do
    if [ -f "contrib/systemd/$unit" ]; then
        install -m 644 "contrib/systemd/$unit" "$SYSTEMD_DIR/$unit" || {
            log "Failed to install $unit" "${RED}"
            exit 1
        }
    else
        log "Systemd unit $unit not found, skipping..." "${YELLOW}"
    fi
done

# Reload systemd and enable services
log "Configuring systemd..." "${YELLOW}"
systemctl daemon-reload
systemctl enable permctl.timer
systemctl start permctl.timer

# Configure sudoers
log "Configuring sudoers..." "${YELLOW}"
SUDOERS_FILE="$SUDOERS_DIR/permctl"
echo "# This file is managed by permctl - do not edit manually" > "$SUDOERS_FILE"
chmod 440 "$SUDOERS_FILE"
chown root:root "$SUDOERS_FILE"

# Verify installation
log "Verifying installation..." "${YELLOW}"
if "$INSTALL_DIR/permctl" verify; then
    log "Installation completed successfully!" "${GREEN}"
    echo
    log "Next steps:" "${GREEN}"
    echo "1. Review configuration at $CONFIG_DIR/config.yaml"
    echo "2. Add allowed commands to the configuration"
    echo "3. Run 'man permctl' for usage information"
    echo "4. Start using permctl with 'permctl --help'"
else
    log "Installation verification failed!" "${RED}"
    log "Please check the error messages above and the log file" "${RED}"
    exit 1
fi

# Create backup of original config if it exists
if [ -f "$CONFIG_DIR/config.yaml" ]; then
    cp "$CONFIG_DIR/config.yaml" "$CONFIG_DIR/config.yaml.bak"
    log "Created backup of existing configuration at $CONFIG_DIR/config.yaml.bak" "${YELLOW}"
fi

# Set secure permissions for database
chmod 600 "$DATA_DIR"/*.db 2>/dev/null || true

log "Installation process completed" "${GREEN}"