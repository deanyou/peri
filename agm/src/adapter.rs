use crate::error::Result;
use crate::fs_util::{paths_equal, remove_symlink_or_dir};
use crate::resolver::PackageType;
use std::path::{Path, PathBuf};

/// Tool adapter trait
pub trait ToolAdapter: Send + Sync {
    /// Tool identifier name
    fn target_name(&self) -> &str;

    /// Map package type to target directory
    fn map_dir(&self, typ: PackageType, project_root: &Path) -> PathBuf {
        let subdir = match typ {
            PackageType::Skills => "skills",
            PackageType::Agents => "agents",
            PackageType::Mcp => "mcp",
        };
        let dot_dir = format!(".{}", self.target_name());
        project_root.join(dot_dir).join(subdir)
    }

    /// Install: create symlink from store to tool directory
    fn install(&self, store_path: &Path, target_dir: &Path, pkg_name: &str) -> Result<()> {
        std::fs::create_dir_all(target_dir)?;

        let link_path = target_dir.join(pkg_name);

        if link_path.exists() {
            if let Ok(meta) = std::fs::symlink_metadata(&link_path) {
                if meta.file_type().is_symlink() {
                    let existing = std::fs::read_link(&link_path)?;
                    if paths_equal(&existing, store_path) {
                        return Ok(());
                    }
                }
            }
        }

        if link_path.exists() {
            remove_symlink_or_dir(&link_path)?;
        }

        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(store_path, &link_path)?;
        }
        #[cfg(windows)]
        {
            if store_path.is_dir() {
                std::os::windows::fs::symlink_dir(store_path, &link_path)?;
            } else {
                std::os::windows::fs::symlink_file(store_path, &link_path)?;
            }
        }

        Ok(())
    }

    /// Post-install hook
    fn post_install(&self) -> Result<()> {
        Ok(())
    }

    /// Uninstall: remove symlink
    fn uninstall(&self, target_dir: &Path, pkg_name: &str) -> Result<()> {
        let link_path = target_dir.join(pkg_name);
        if link_path.exists() {
            remove_symlink_or_dir(&link_path)?;
        }
        Ok(())
    }
}

// ---- Built-in adapters ----

pub struct ClaudeAdapter;
impl ToolAdapter for ClaudeAdapter {
    fn target_name(&self) -> &str {
        "claude"
    }
}

/// Get adapter by name
pub fn get_adapter(name: &str) -> Option<Box<dyn ToolAdapter>> {
    match name.to_lowercase().as_str() {
        "claude" => Some(Box::new(ClaudeAdapter)),
        _ => None,
    }
}

/// List all built-in adapter names
pub fn list_adapters() -> Vec<&'static str> {
    vec!["claude"]
}

/// Map adapter name to symlink name (add scope prefix on conflict)
pub fn symlink_name(pkg_name: &str, existing_names: &[String]) -> String {
    let base = pkg_name.replace('/', "_").replace('@', "");
    if existing_names.contains(&base) {
        let parts: Vec<&str> = pkg_name.split('/').collect();
        if parts.len() >= 2 {
            let prefix = parts[0].trim_start_matches('@');
            format!("{}_{}", prefix, parts.last().unwrap())
        } else {
            base
        }
    } else {
        base
    }
}
