#!/bin/bash

set -e

# Ensure we're root
if [ "$EUID" -ne 0 ]; then
    echo "Please run as root"
    exit 1
fi

# Configuration
INSTALL_DIR="/usr/sbin"
CONFIG_DIR="/etc/permctl"
DATA_DIR="/var/lib/permctl"
LOG_DIR="/var/log/permctl"
SUDOERS_DIR="/etc/sudoers.d"
SYSTEMD_DIR="/etc/systemd/system"
MAN_DIR="/usr/share/man/man1"

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m'

echo "Installing Linux Permission Manager..."

# Create directories
echo -e "${YELLOW}Creating directories...${NC}"
mkdir -p "$CONFIG_DIR"
mkdir -p "$DATA_DIR"
mkdir -p "$LOG_DIR"
mkdir -p "$SUDOERS_DIR"

# Set directory permissions
chown root:root "$CONFIG_DIR"
chmod 755 "$CONFIG_DIR"
chown root:root "$DATA_DIR"
chmod 755 "$DATA_DIR"
chown root:root "$LOG_DIR"
chmod 755 "$LOG_DIR"

# Build the binary
echo -e "${YELLOW}Building binary...${NC}"
cargo build --release

# Install binary
echo -e "${YELLOW}Installing binary...${NC}"
install -m 755 target/release/permctl "$INSTALL_DIR/permctl"

# Install default config if it doesn't exist
if [ ! -f "$CONFIG_DIR/config.yaml" ]; then
    echo -e "${YELLOW}Installing default configuration...${NC}"
    permctl init
    chmod 644 "$CONFIG_DIR/config.yaml"
fi

# Install man page
echo -e "${YELLOW}Installing man page...${NC}"
install -m 644 docs/permctl.1 "$MAN_DIR/permctl.1"
gzip -f "$MAN_DIR/permctl.1"

# Install systemd service and timer
echo -e "${YELLOW}Installing systemd units...${NC}"
install -m 644 contrib/systemd/permctl.service "$SYSTEMD_DIR/permctl.service"
install -m 644 contrib/systemd/permctl.timer "$SYSTEMD_DIR/permctl.timer"

# Reload systemd
systemctl daemon-reload

# Enable and start timer
systemctl enable permctl.timer
systemctl start permctl.timer

# Create initial sudoers file
echo -e "${YELLOW}Setting up sudoers...${NC}"
touch "$SUDOERS_DIR/permctl"
chmod 440 "$SUDOERS_DIR/permctl"
chown root:root "$SUDOERS_DIR/permctl"

# Verify installation
echo -e "${YELLOW}Verifying installation...${NC}"
if permctl verify; then
    echo -e "${GREEN}Installation completed successfully!${NC}"
    echo
    echo "Next steps:"
    echo "1. Review configuration at $CONFIG_DIR/config.yaml"
    echo "2. Add allowed commands to the configuration"
    echo "3. Run 'man permctl' for usage information"
else
    echo -e "${RED}Installation verification failed!${NC}"
    echo "Please check the error messages above"
    exit 1
fi