use anyhow::Result;

/// Execute the `monitor` command — TUI dashboard with dual CDC view.
pub async fn execute(device: Option<&str>) -> Result<()> {
    let _ = device;
    anyhow::bail!("'monitor' command is not yet implemented (Phase 3)");
}
