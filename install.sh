#!/usr/bin/env bash

# Script: install.sh
# Description: Production-grade installer for Linux Permission Manager
# Author: Anubhav Gain
# Version: 1.0.0

# Strict mode configuration
set -euo pipefail
IFS=$'\n\t'

# Configuration variables
readonly INSTALL_DIR="/usr/sbin"
readonly CONFIG_DIR="/etc/permctl"
readonly DATA_DIR="/var/lib/permctl"
readonly LOG_DIR="/var/log/permctl"
readonly SUDOERS_DIR="/etc/sudoers.d"
readonly SYSTEMD_DIR="/etc/systemd/system"
readonly MAN_DIR="/usr/share/man/man1"

# Color definitions
readonly RED='\033[0;31m'
readonly GREEN='\033[0;32m'
readonly YELLOW='\033[1;33m'
readonly BLUE='\033[0;34m'
readonly NC='\033[0m'

# Package management variables
declare -A PKG_MANAGERS=(
    ["apt"]="apt-get install -y"
    ["dnf"]="dnf install -y"
    ["yum"]="yum install -y"
    ["pacman"]="pacman -S --noconfirm"
    ["zypper"]="zypper install -y"
)

# Required dependencies for different distributions
declare -A DEPENDENCIES=(
    ["apt"]="build-essential pkg-config libsqlite3-dev curl"
    ["dnf"]="gcc pkg-config sqlite-devel curl"
    ["yum"]="gcc pkg-config sqlite-devel curl"
    ["pacman"]="base-devel pkg-config sqlite curl"
    ["zypper"]="gcc pkg-config sqlite3-devel curl"
)

# Error handling
trap 'error_handler $? $LINENO $BASH_LINENO "$BASH_COMMAND" $(printf "::%s" ${FUNCNAME[@]:-})' ERR

error_handler() {
    local exit_code=$1
    local line_number=$2
    local bash_lineno=$3
    local last_command=$4
    local func_trace=$5

    log "Error occurred in script at line: $line_number" "${RED}"
    log "Last command executed: $last_command" "${RED}"
    log "Exit code: $exit_code" "${RED}"

    # Clean up any temporary files or partial installations
    cleanup
    exit "$exit_code"
}

cleanup() {
    log "Performing cleanup..." "${YELLOW}"
    # Add cleanup tasks here if needed
}

# Logging function with timestamps and log file support
log() {
    local message=$1
    local color=${2:-$NC}
    local timestamp
    timestamp=$(date '+%Y-%m-%d %H:%M:%S')
    
    echo -e "${color}${timestamp} - ${message}${NC}" | tee -a "$LOG_DIR/install.log"
}

# Detect package manager
detect_package_manager() {
    for pkg_mgr in "${!PKG_MANAGERS[@]}"; do
        if command -v "$pkg_mgr" >/dev/null 2>&1; then
            echo "$pkg_mgr"
            return 0
        fi
    done
    log "No supported package manager found" "${RED}"
    exit 1
}

# Install system dependencies
install_dependencies() {
    local pkg_mgr
    pkg_mgr=$(detect_package_manager)
    
    log "Installing system dependencies using $pkg_mgr..." "${BLUE}"
    
    # Update package manager cache
    case $pkg_mgr in
        apt)
            apt-get update
            ;;
        dnf|yum)
            $pkg_mgr makecache
            ;;
        pacman)
            pacman -Sy
            ;;
        zypper)
            zypper refresh
            ;;
    esac

    # Install dependencies
    ${PKG_MANAGERS[$pkg_mgr]} ${DEPENDENCIES[$pkg_mgr]}

    # Install Rust if not present
    if ! command -v cargo >/dev/null 2>&1; then
        log "Installing Rust..." "${BLUE}"
        curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
        source "$HOME/.cargo/env"
    fi
}

# Directory management with proper permissions
create_directory() {
    local dir=$1
    local perms=${2:-755}
    local owner=${3:-root:root}
    
    if [[ ! -d "$dir" ]]; then
        mkdir -p "$dir"
        log "Created directory: $dir" "${YELLOW}"
    fi
    
    chown "$owner" "$dir"
    chmod "$perms" "$dir"
}

# Main installation process
main() {
    # Check for root privileges
    if [[ $EUID -ne 0 ]]; then
        log "This script must be run as root" "${RED}"
        exit 1
    }

    # Create log directory first for logging
    create_directory "$LOG_DIR" "755" "root:root"
    
    log "Starting Linux Permission Manager installation..." "${BLUE}"
    
    # Install dependencies
    install_dependencies

    # Create required directories
    create_directory "$CONFIG_DIR" "755"
    create_directory "$DATA_DIR" "750"
    create_directory "$SUDOERS_DIR" "750"

    # Build and install
    log "Building application..." "${BLUE}"
    if ! cargo build --release; then
        log "Build failed" "${RED}"
        exit 1
    fi

    # Install binary
    install -m 755 target/release/permctl "$INSTALL_DIR/permctl"

    # Initialize configuration
    if [[ ! -f "$CONFIG_DIR/config.yaml" ]]; then
        "$INSTALL_DIR/permctl" init --force
        chmod 600 "$CONFIG_DIR/config.yaml"
    fi

    # Install systemd units
    for unit in permctl.service permctl.timer; do
        if [[ -f "contrib/systemd/$unit" ]]; then
            install -m 644 "contrib/systemd/$unit" "$SYSTEMD_DIR/$unit"
        fi
    done

    # Configure system
    systemctl daemon-reload
    systemctl enable --now permctl.timer

    # Setup sudoers
    echo "# Managed by permctl - DO NOT EDIT" > "$SUDOERS_DIR/permctl"
    chmod 440 "$SUDOERS_DIR/permctl"

    # Verify installation
    if "$INSTALL_DIR/permctl" verify; then
        log "Installation completed successfully!" "${GREEN}"
        print_next_steps
    else
        log "Installation verification failed" "${RED}"
        exit 1
    fi
}

print_next_steps() {
    cat << EOF

${GREEN}Installation completed successfully!${NC}

Next steps:
1. Configure your allowed commands in $CONFIG_DIR/config.yaml
2. Run 'permctl --help' to see available commands
3. Check logs in $LOG_DIR for any issues
4. Visit https://github.com/yourusername/linux-permission-manager for documentation

For support, please file an issue on the GitHub repository.
EOF
}

# Execute main installation process
main "$@"