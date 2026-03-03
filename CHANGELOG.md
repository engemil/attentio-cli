# Changelog

All notable changes to the Attentio CLI (`attentio`) project will be documented in this file.

**Version Format:** MAJOR.MINOR.PATCH
- **MAJOR:** Incompatible API/protocol changes
- **MINOR:** New features (backward compatible)
- **PATCH:** Bug fixes (backward compatible)

[Semantic Versioning](https://semver.org/spec/v2.0.0.html).

Note: Update `Cargo.toml` when publishing new version.

---

## [Development] (2026-03-03)

Added

- **CLI framework** with clap (derive) supporting subcommands: `list`, `send`, `shell`, `monitor`, `led`, `settings`, `dfu`, `dfu-enter`, and `completions`.
- **Global flags**: `--device <serial>`, `--json`, and `--verbose`.
- **Device discovery** module for enumerating and selecting connected AttentioLight-1 devices via serial ports.
- **Device connection** module for async serial communication using tokio-serial.
- **Error handling** with anyhow and thiserror.
- **Tracing/logging** with tracing-subscriber and env-filter support.
- `list` and `send`/`shell` commands implemented (Phase 1-2 complete).
- **Cargo project setup** with dependencies: clap, tokio, tokio-serial, serialport, serde, serde_json, anyhow, thiserror, tracing.
- `.gitignore` for Rust target directory.
- `rustfmt.toml` formatter configuration.
- **README** with full CLI usage reference, implementation status table, setup instructions for Linux/macOS/Windows, and build/install commands.
- **udev rules script** (`scripts/udev_rules_attentio.sh`) for Linux USB device access.
- Future plan items: unit tests for discovery logic and CI/CD pipeline.

Changed

- Expanded README from placeholder to full documentation.
- Updated `docs/PLAN.md` with future items (unit tests, CI/CD).

Fixed

- Fixed typo in udev script filename (`udev_rules_attetio.sh` → `udev_rules_attentio.sh`).
