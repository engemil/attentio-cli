#!/bin/bash

# udev_rules_attentio.sh - Script to set up udev rules in Linux (e.g. Ubuntu)
# This script creates a udev rule to allow non-root users.
# It also adds the current user to the 'plugdev' group, which is necessary for USB device access.
#
# HOW-TO use:
# 1. Configure permissions: sudo chmod +x ./udev_rules_attentio.sh
# 2. Execute: sudo ./udev_rules_attentio.sh
#
# Requirements: Must be run with sudo privileges.

# Check if script is run with sudo
if [ "$EUID" -ne 0 ]; then
    echo "Error: This script must be run as root (use sudo)."
    exit 1
fi

# Step 1: Create udev rules file
# Source: https://www.st.com/resource/en/technical_note/tn1235-overview-of-stlink-derivatives-stmicroelectronics.pdf
# If you share your linux system with other users, or just don't like the idea of giving write permissions to everyone, you can change the MODE to 0660
# and change the GROUP to a specific group that you want to allow access to the ST-LINK devices. 
# For example, you can create a group called 'stlink' and change the GROUP to 'stlink'.
# Note: The SYMLINK+="stlinkv2_1_%n" creates a symlink in /dev with the name stlinkv2_1_<bus>_<device> for easy access.

UDEV_RULES_FILE="/etc/udev/rules.d/99-attentio.rules"
echo "Creating udev rules in $UDEV_RULES_FILE..."

echo ""

echo "Content of $UDEV_RULES_FILE:"

echo ""

cat << EOF | tee $UDEV_RULES_FILE
# AttentioLight-1 - EngEmil Bootloader (DFU Mode)
SUBSYSTEM=="usb", ATTRS{idVendor}=="0483", ATTRS{idProduct}=="df11", MODE="0666", ENV{ID_MM_DEVICE_IGNORE}="1"
SUBSYSTEM=="tty", ATTRS{idVendor}=="0483", ATTRS{idProduct}=="df11", MODE="0666", SYMLINK+="attentio-%s{serial}"

# AttentioLight-1 - Test
SUBSYSTEM=="usb", ATTRS{idVendor}=="0483", ATTRS{idProduct}=="0001", MODE="0666", ENV{ID_MM_DEVICE_IGNORE}="1"
SUBSYSTEM=="tty", ATTRS{idVendor}=="0483", ATTRS{idProduct}=="0001", MODE="0666", SYMLINK+="attentio-%s{serial}"

EOF

echo ""

# Step 2: Set permissions for udev rules file
chmod 644 $UDEV_RULES_FILE

# Step 3: Reload udev rules
echo "Reloading udev rules..."
udevadm control --reload-rules
udevadm trigger

# Step 4: Add user to plugdev and dialout group
if [ -n "$SUDO_USER" ]; then
    echo "Adding user $SUDO_USER to plugdev and dialout group..."
    usermod -aG plugdev "$SUDO_USER"
    usermod -a -G dialout "$SUDO_USER"
else
    echo "Warning: Could not determine user (SUDO_USER not set). Skipping group addition."
fi

echo ""
echo "Script COMPLETED successfully!"
echo ""
echo "Please log out and log back in to apply the group changes."

# Exit the script successfully
exit 0
