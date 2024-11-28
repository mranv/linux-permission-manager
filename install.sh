#!/usr/bin/env bash

#===================================================================================
# Linux Permission Manager Installation Script
# Version: 1.0.0
# Author: Anubhav Gain
# Description: Production-grade installer for Linux Permission Manager
#===================================================================================

#---------------------------
# Environment Configuration
#---------------------------
set -euo pipefail
IFS=$'\n\t'

#---------------------------
# Directory Configuration
#---------------------------
declare -r INSTALL_DIR="/usr/sbin"
declare -r CONFIG_DIR="/etc/permctl"
declare -r DATA_DIR="/var/lib/permctl"
declare -r LOG_DIR="/var/log/permctl"
declare -r SUDOERS_DIR="/etc/sudoers.d"
declare -r SYSTEMD_DIR="/etc/systemd/system"
declare -r MAN_DIR="/usr/share/man/man1"

#---------------------------
# Visual Formatting
#---------------------------
declare -r RED='\033[0;31m'
declare -r GREEN='\033[0;32m'
declare -r YELLOW='\033[1;33m'
declare -r BLUE='\033[0;34m'
declare -r BOLD='\033[1m'
declare -r NC='\033[0m'

#---------------------------
# Package Management
#---------------------------

declare -A package_managers=(
    ["apt"]="apt-get -y install"
    ["dnf"]="dnf -y install"
    ["yum"]="yum -y install"
    ["pacman"]="pacman -S --noconfirm"
    ["zypper"]="zypper -n install"
)

declare -A dependencies=(
    ["apt"]="build-essential pkg-config libsqlite3-dev curl"
    ["dnf"]="gcc pkg-config sqlite-devel curl"
    ["yum"]="gcc pkg-config sqlite-devel curl"
    ["pacman"]="base-devel pkg-config sqlite curl"
    ["zypper"]="gcc pkg-config sqlite3-devel curl"
)

#---------------------------
# Utility Functions
#---------------------------
log() {
    local timestamp
    timestamp=$(date '+%Y-%m-%d %H:%M:%S')
    echo -e "${2:-}${timestamp} - $1${NC}" | tee -a "${LOG_DIR}/install.log"
}

print_banner() {
    echo -e "${BLUE}${BOLD}"
    echo "╔═══════════════════════════════════════════════════════════════╗"
    echo "║             Linux Permission Manager Installer                 ║"
    echo "╚═══════════════════════════════════════════════════════════════╝"
    echo -e "${NC}"
}

check_root() {
    if [[ $EUID -ne 0 ]]; then
        log "This script must be run as root" "${RED}"
        exit 1
    fi
}

detect_package_manager() {
    local pkg_manager
    for pkg_manager in "${!package_managers[@]}"; do
        if command -v "$pkg_manager" >/dev/null 2>&1; then
            echo "$pkg_manager"
            return 0
        fi
    done
    return 1
}

#---------------------------
# Installation Functions
#---------------------------
install_dependencies() {
    local pkg_manager
    pkg_manager=$(detect_package_manager)
    
    if [[ -z "$pkg_manager" ]]; then
        log "No supported package manager found" "${RED}"
        exit 1
    fi
    
    log "Using package manager: $pkg_manager" "${BLUE}"
    
    # Update package cache
    case $pkg_manager in
        apt)
            apt-get update
            ;;
        dnf|yum)
            $pkg_manager makecache
            ;;
        pacman)
            pacman -Sy
            if ! pacman -S --noconfirm base-devel pkg-config sqlite curl; then
                log "Failed to install dependencies using pacman" "${RED}"
                exit 1
            fi
            ;;
        zypper)
            zypper refresh
            ;;
    esac
    
    # Install dependencies for non-pacman systems
    if [[ "$pkg_manager" != "pacman" ]]; then
        if ! ${package_managers[$pkg_manager]} ${dependencies[$pkg_manager]}; then
            log "Failed to install dependencies" "${RED}"
            exit 1
        fi
    fi
    
    # Install Rust if not present
    if ! command -v cargo >/dev/null 2>&1; then
        log "Installing Rust..." "${YELLOW}"
        curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
        # shellcheck source=/dev/null
        source "$HOME/.cargo/env"
    fi
}

create_directories() {
    local -a dirs=("$CONFIG_DIR" "$DATA_DIR" "$LOG_DIR" "$SUDOERS_DIR")
    local dir
    
    for dir in "${dirs[@]}"; do
        if [[ ! -d "$dir" ]]; then
            mkdir -p "$dir"
            chown root:root "$dir"
            chmod 755 "$dir"
            log "Created directory: $dir" "${YELLOW}"
        fi
    done
}

build_and_install() {
    log "Building application..." "${BLUE}"
    if ! cargo build --release; then
        log "Build failed" "${RED}"
        exit 1
    fi
    
    install -m 755 target/release/permctl "$INSTALL_DIR/permctl"
    log "Binary installed successfully" "${GREEN}"
}

configure_system() {
    # Initialize configuration
    if [[ ! -f "$CONFIG_DIR/config.yaml" ]]; then
        "$INSTALL_DIR/permctl" init --force
        chmod 600 "$CONFIG_DIR/config.yaml"
        log "Configuration initialized" "${GREEN}"
    fi
    
    # Setup systemd
    if [[ -d "contrib/systemd" ]]; then
        for unit in permctl.service permctl.timer; do
            if [[ -f "contrib/systemd/$unit" ]]; then
                install -m 644 "contrib/systemd/$unit" "$SYSTEMD_DIR/$unit"
            fi
        done
        systemctl daemon-reload
        systemctl enable --now permctl.timer
        log "Systemd services configured" "${GREEN}"
    fi
    
    # Setup sudoers
    echo "# Managed by permctl - DO NOT EDIT" > "$SUDOERS_DIR/permctl"
    chmod 440 "$SUDOERS_DIR/permctl"
    log "Sudoers configuration complete" "${GREEN}"
}

verify_installation() {
    if "$INSTALL_DIR/permctl" verify; then
        log "Installation verified successfully" "${GREEN}"
        return 0
    else
        log "Installation verification failed" "${RED}"
        return 1
    fi
}

print_completion() {
    echo
    echo -e "${GREEN}${BOLD}Installation completed successfully!${NC}"
    echo
    echo "Next steps:"
    echo "1. Edit your configuration: $CONFIG_DIR/config.yaml"
    echo "2. Review permissions: sudo permctl verify"
    echo "3. Grant your first permission: sudo permctl grant"
    echo
    echo "For more information, visit: https://github.com/mranv/linux-permission-manager"
    echo
}

#---------------------------
# Main Installation Process
#---------------------------
main() {
    print_banner
    check_root
    
    # Create log directory first
    mkdir -p "$LOG_DIR"
    
    log "Starting installation..." "${BLUE}"
    
    create_directories
    install_dependencies
    build_and_install
    configure_system
    
    if verify_installation; then
        print_completion
    else
        log "Installation failed" "${RED}"
        exit 1
    fi
}

# Execute main installation
main "$@"