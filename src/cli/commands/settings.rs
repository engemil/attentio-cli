use anyhow::Result;

use crate::cli::SettingsAction;

/// Execute the `settings` command — read/write device settings and presets.
pub async fn execute(
    action: &SettingsAction,
    device: Option<&str>,
    json: bool,
) -> Result<()> {
    let _ = (action, device, json);
    anyhow::bail!("'settings' command is not yet implemented (Phase 5)");
}
