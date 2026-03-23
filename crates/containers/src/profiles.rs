use std::path::Path;

use anyhow::{Context, Result};
use serde::Deserialize;

/// A loaded container profile from `docker/profiles/<name>.toml`.
#[derive(Debug, Clone)]
pub struct Profile {
    pub name: String,
    pub image: String,
}

#[derive(Debug, Deserialize)]
struct ProfileFile {
    image: ImageSection,
}

#[derive(Debug, Deserialize)]
struct ImageSection {
    name: String,
}

/// Load all profiles from the given directory (e.g. `docker/profiles/`).
///
/// Each `.toml` file in the directory is a profile. The file stem is the
/// profile name (e.g. `rust.toml` → profile "rust").
pub fn load_profiles(profiles_dir: &Path) -> Result<Vec<Profile>> {
    let mut profiles = Vec::new();

    let entries = std::fs::read_dir(profiles_dir)
        .with_context(|| format!("reading profiles directory {}", profiles_dir.display()))?;

    for entry in entries {
        let entry = entry.context("reading directory entry")?;
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("toml") {
            continue;
        }

        let name = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("")
            .to_string();

        if name.is_empty() {
            continue;
        }

        let content = std::fs::read_to_string(&path)
            .with_context(|| format!("reading profile file {}", path.display()))?;

        let parsed: ProfileFile = toml::from_str(&content)
            .with_context(|| format!("parsing profile file {}", path.display()))?;

        profiles.push(Profile {
            name,
            image: parsed.image.name,
        });
    }

    profiles.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(profiles)
}
