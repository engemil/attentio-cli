# Attentio CLI

CLI tool for AttentioLight-1 (AL-1) device management.

(TODO: More info)

## Setup

### Rust Toolchain

```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
```

### System Dependencies

**Ubuntu/Debian:**
```bash
sudo apt install build-essential pkg-config libudev-dev libusb-1.0-0-dev
```

**Fedora/RHEL:**
```bash
sudo dnf install gcc pkgconf-pkg-config systemd-devel libusb1-devel
```

**Arch:**
```bash
sudo pacman -S base-devel pkgconf libusb
```

**Alpine:**
```bash
apk add build-base pkgconf eudev-dev libusb-dev
```

**macOS:**
```bash
brew install libusb
```

**Windows:**
- Install libusb via [vcpkg](https://vcpkg.io/) or [libusb.info](https://libusb.info)
- Install WinUSB driver with [Zadig](https://zadig.akeo.ie/)

### Build

```bash
cargo build --release
```

### udev Rules (Linux, optional)

For non-root USB access, create `/etc/udev/rules.d/99-attentio.rules`:
```
SUBSYSTEM=="usb", ATTRS{idVendor}=="0483", ATTRS{idProduct}=="df11", MODE="0666"
SUBSYSTEM=="tty", ATTRS{idVendor}=="0483", ATTRS{idProduct}=="df11", MODE="0666", SYMLINK+="attentio-%s{serial}"
```

Reload rules:
```bash
sudo udevadm control --reload-rules && sudo udevadm trigger
```

**NB!** Script available for this, `scripts/udev_rules_attetio.sh`.

## License

MIT License, see `LICENSE`-file for details.
