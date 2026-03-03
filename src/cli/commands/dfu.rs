use anyhow::Result;

/// Execute the `dfu` command — flash firmware via DFU.
pub async fn execute(firmware: &str) -> Result<()> {
    let _ = firmware;
    anyhow::bail!("'dfu' command is not yet implemented (Phase 6)");
}

/// Execute the `dfu-enter` command — reboot device into DFU bootloader.
pub async fn execute_enter(device: Option<&str>) -> Result<()> {
    let _ = device;
    anyhow::bail!("'dfu-enter' command is not yet implemented (Phase 6)");
}
