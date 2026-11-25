use craftui_core::{CraftError, Result};
use std::path::Path;
use std::process::Command;

/// Add a `crates.io` dependency via `cargo add` and record it in `craftui.toml` if present.
pub fn run_add_crate(
    crate_spec: &str,
    features: &[String],
    project_dir: &Path,
    skip_cargo: bool,
) -> Result<()> {
    let trimmed = crate_spec.trim();
    if trimmed.is_empty() {
        return Err(CraftError::Platform("crate name must not be empty".into()));
    }

    let (name, version) = split_crate_spec(trimmed);

    if !skip_cargo {
        let mut cmd = Command::new("cargo");
        cmd.arg("add").arg(trimmed).current_dir(project_dir);
        if !features.is_empty() {
            cmd.arg("--features").arg(features.join(","));
        }
        let status = cmd.status().map_err(|e| {
            CraftError::Platform(format!("failed to invoke `cargo add {trimmed}`: {e}"))
        })?;
        if !status.success() {
            return Err(CraftError::Platform(format!(
                "`cargo add {trimmed}` exited with status {status}"
            )));
        }
    }

    let manifest_path = project_dir.join("craftui.toml");
    if manifest_path.exists() {
        record_dependency_in_manifest(&manifest_path, name, version.unwrap_or("*"))?;
        println!("Recorded `{name}` in {}", manifest_path.display());
    }

    println!("Added `{trimmed}`.");
    println!(
        "Tip: bridge async results from `{name}` into UI signals with `craftui::reactive::ResourceState<T>`."
    );
    Ok(())
}

pub(crate) fn split_crate_spec(spec: &str) -> (&str, Option<&str>) {
    match spec.split_once('@') {
        Some((name, ver)) if !ver.trim().is_empty() => (name.trim(), Some(ver.trim())),
        _ => (spec.trim(), None),
    }
}

pub(crate) fn record_dependency_in_manifest(
    manifest_path: &Path,
    crate_name: &str,
    version: &str,
) -> Result<()> {
    let raw = std::fs::read_to_string(manifest_path).map_err(|e| {
        CraftError::Platform(format!("failed reading {}: {e}", manifest_path.display()))
    })?;

    let entry = format!("{crate_name} = \"{version}\"");
    let mut lines: Vec<String> = raw.lines().map(ToString::to_string).collect();
    let mut in_deps = false;
    let mut deps_header_idx = None;
    let mut replaced = false;

    for (idx, line) in lines.iter_mut().enumerate() {
        let stripped = line.split('#').next().unwrap_or("").trim();
        if stripped.starts_with('[') && stripped.ends_with(']') {
            in_deps = stripped[1..stripped.len() - 1]
                .trim()
                .eq_ignore_ascii_case("dependencies");
            if in_deps {
                deps_header_idx = Some(idx);
            }
            continue;
        }
        if in_deps
            && let Some((k, _)) = stripped.split_once('=')
            && k.trim().trim_matches(|c| c == '"' || c == '\'') == crate_name
        {
            *line = entry.clone();
            replaced = true;
            break;
        }
    }

    if !replaced {
        if let Some(hdr) = deps_header_idx {
            lines.insert(hdr + 1, entry);
        } else {
            if !lines.is_empty() && !lines.last().is_some_and(String::is_empty) {
                lines.push(String::new());
            }
            lines.push("[dependencies]".to_string());
            lines.push(entry);
        }
    }

    let mut out = lines.join("\n");
    out.push('\n');
    std::fs::write(manifest_path, out).map_err(|e| {
        CraftError::Platform(format!("failed writing {}: {e}", manifest_path.display()))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::manifest::AppManifest;

    #[test]
    fn record_dependency_appends_and_updates_manifest() {
        let dir = std::env::temp_dir().join(format!(
            "craftui-add-test-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let manifest_path = dir.join("craftui.toml");
        std::fs::write(&manifest_path, "[app]\nname = \"Demo\"\n").unwrap();

        run_add_crate("reqwest@0.12", &[], &dir, true).unwrap();
        run_add_crate("reqwest@0.13", &[], &dir, true).unwrap();

        let parsed = AppManifest::load_file(&manifest_path).unwrap();
        assert_eq!(
            parsed.dependencies.get("reqwest").map(String::as_str),
            Some("0.13")
        );

        let _ = std::fs::remove_dir_all(&dir);
    }
}
