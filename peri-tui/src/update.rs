//! Update mechanism: curl remote install.sh | bash.
//!
//! Delegates all update logic (download, checksum, extract, symlink)
//! to the remote install script.

use anyhow::{Context, Result};
use std::process::Stdio;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;

const SCRIPT_URL: &str = "https://raw.githubusercontent.com/konghayao/peri/main/scripts/install.sh";

/// Run the update flow. Returns Ok(new_tag) on success.
///
/// Streams the remote install script's stdout/stderr to the terminal.
pub async fn run_update() -> Result<String> {
    println!("Peri update");
    println!("  Running remote install script...");

    let mut child = Command::new("bash")
        .arg("-c")
        .arg(format!("curl -fsSL {SCRIPT_URL} | bash"))
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .context("Failed to spawn update process. Is bash/curl available?")?;

    // 流式输出 stdout
    if let Some(stdout) = child.stdout.take() {
        let reader = BufReader::new(stdout);
        let mut lines = reader.lines();
        while let Some(line) = lines.next_line().await? {
            println!("{line}");
        }
    }

    // 流式输出 stderr
    if let Some(stderr) = child.stderr.take() {
        let reader = BufReader::new(stderr);
        let mut lines = reader.lines();
        while let Some(line) = lines.next_line().await? {
            eprintln!("{line}");
        }
    }

    let status = child.wait().await?;
    if !status.success() {
        anyhow::bail!("Update script exited with status {}", status);
    }

    // 从 install_dir 读取安装后的版本号
    let version_file = dirs_next::home_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("."))
        .join(".peri")
        .join("current-version.txt");
    let tag = std::fs::read_to_string(&version_file)
        .ok()
        .map(|s| s.trim().to_string())
        .unwrap_or_else(|| "unknown".to_string());

    Ok(tag)
}
