use anyhow::Result;

/// Execute the `completions` command — generate shell completions.
pub fn execute(shell: &str) -> Result<()> {
    let _ = shell;
    anyhow::bail!("'completions' command is not yet implemented");
}
