//! `sidenote setup herdr`: give sidenote its own name and icon in the herdr app, and its command
//! palette entries (Note: continue the latest, new, pick one).

use anyhow::{Context, Result, bail};
use serde_json::{Value, json};
use std::fs;
use std::path::{Path, PathBuf};

const HERDR_ICON: &str = include_str!("../assets/herdr-icon.svg");
const ICON_PATH: &str = "my-icons/sidenote.svg";
const MOD_JSON: &str = include_str!("../assets/herdr-mod.json");
const MARKER: &str = ".installed-by";

fn herdr_config_dir() -> PathBuf {
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            let home = std::env::var_os("HOME")
                .map(PathBuf::from)
                .unwrap_or_default();
            home.join(".config")
        });
    base.join("herdr-app")
}

pub fn herdr() -> Result<()> {
    let dir = herdr_config_dir();
    if !dir.is_dir() {
        bail!(
            "herdr app config not found at {} (is the herdr app installed?)",
            dir.display()
        );
    }

    let icon = dir.join(ICON_PATH);
    let icon_changed = fs::read_to_string(&icon).ok().as_deref() != Some(HERDR_ICON);
    if icon_changed {
        fs::create_dir_all(icon.parent().unwrap())?;
        write_atomic(&icon, HERDR_ICON)?;
    }

    // Add our entry to programs.json, keeping everything else in it as is.
    let programs = dir.join("programs.json");
    let mut config: Value = match fs::read_to_string(&programs) {
        Ok(text) if !text.trim().is_empty() => serde_json::from_str(&text).with_context(|| {
            format!(
                "{} is not valid JSON; fix it and run this again",
                programs.display()
            )
        })?,
        _ => json!({}),
    };
    let Some(root) = config.as_object_mut() else {
        bail!("{} should contain a JSON object", programs.display());
    };
    let table = root.entry("programs").or_insert_with(|| json!({}));
    let Some(table) = table.as_object_mut() else {
        bail!("\"programs\" in {} should be an object", programs.display());
    };
    let entry_added = !table.contains_key("sidenote");
    if entry_added {
        table.insert(
            "sidenote".into(),
            json!({
                "name": "Sidenote",
                "icon": ICON_PATH,
                "tint": true,
                "tile": "#0e7490",
                "color": "#ffffff",
                "match": ["notes"],
            }),
        );
        write_atomic(&programs, &(serde_json::to_string_pretty(&config)? + "\n"))?;
    }

    match (icon_changed, entry_added) {
        (false, false) => println!("herdr is already set up for sidenote"),
        _ => {
            if icon_changed {
                println!("wrote {}", icon.display());
            }
            if entry_added {
                println!("added \"sidenote\" to {}", programs.display());
            }
            println!(
                "herdr picks this up live: sidenote panes now show as Sidenote with their own icon"
            );
        }
    }
    if icon_changed && !entry_added {
        println!("(the existing \"sidenote\" entry in programs.json was left as is)");
    }
    palette(&dir.join("mods/sidenote"))
}

/// The palette entries, as a mod herdr loads live. A folder someone linked or made is left alone
/// (a copy from herdr-mods' install.sh is replaced).
fn palette(mod_dir: &Path) -> Result<()> {
    let stamp = format!("sidenote {}", env!("CARGO_PKG_VERSION"));
    if fs::symlink_metadata(mod_dir).is_ok_and(|m| m.file_type().is_symlink()) {
        if mod_dir.exists() {
            println!(
                "palette entries: {} is a link; left as is",
                mod_dir.display()
            );
            return Ok(());
        }
        fs::remove_file(mod_dir)?;
    }
    if mod_dir.exists() {
        let marker = fs::read_to_string(mod_dir.join(MARKER)).unwrap_or_default();
        if !marker.starts_with("sidenote") && !mod_dir.join(".herdr-mods").exists() {
            println!(
                "palette entries: {} wasn't installed by sidenote; left as is",
                mod_dir.display()
            );
            return Ok(());
        }
        if marker == stamp
            && fs::read_to_string(mod_dir.join("mod.json")).ok().as_deref() == Some(MOD_JSON)
        {
            println!("palette entries are up to date");
            return Ok(());
        }
    }
    fs::create_dir_all(mod_dir)?;
    write_atomic(&mod_dir.join("mod.json"), MOD_JSON)?;
    write_atomic(&mod_dir.join(MARKER), &stamp)?;
    let _ = fs::remove_file(mod_dir.join(".herdr-mods"));
    println!("added the palette entries: Note: continue the latest, Note: new, Note: pick one");
    Ok(())
}

fn write_atomic(path: &Path, content: &str) -> Result<()> {
    let tmp = path.with_extension("sidenote-tmp");
    fs::write(&tmp, content).with_context(|| format!("cannot write {}", tmp.display()))?;
    fs::rename(&tmp, path)?;
    Ok(())
}

/// `sidenote update`: the installer's updater, or cargo for a cargo install.
pub fn update() -> Result<()> {
    let exe = std::env::current_exe().context("cannot tell where sidenote is")?;
    let updater = exe.with_file_name("sidenote-update");
    let status = if updater.is_file() {
        std::process::Command::new(&updater).status()?
    } else if exe.components().any(|c| c.as_os_str() == ".cargo") {
        println!("cargo install --locked --force sidenote");
        std::process::Command::new("cargo")
            .args(["install", "--locked", "--force", "sidenote"])
            .status()?
    } else {
        bail!(
            "sidenote wasn't installed by its installer or cargo; reinstall with:\n  curl -fsSL https://gbsn.dev/sidenote | sh"
        );
    };
    if !status.success() {
        bail!("the update failed");
    }
    Ok(())
}
