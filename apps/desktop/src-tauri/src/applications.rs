use peerspan_core::{ApplicationKind, ApplicationSource, PublishedApplication};
use std::{
    collections::HashSet,
    fs,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};
use uuid::Uuid;

const APPLICATION_NAMESPACE: Uuid = Uuid::from_u128(0x6078c5d9_0408_4aed_a044_28779b688a8b);

pub fn scan_installed_applications() -> Result<Vec<PublishedApplication>, String> {
    let roots = start_menu_roots();
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64;
    let mut applications = Vec::new();
    let mut visited = HashSet::new();
    for root in roots {
        scan_shortcuts(&root, now, 0, &mut visited, &mut applications)?;
    }
    applications.sort_by(|left, right| {
        left.name
            .to_lowercase()
            .cmp(&right.name.to_lowercase())
            .then_with(|| left.launch_target.cmp(&right.launch_target))
    });
    Ok(applications)
}

fn start_menu_roots() -> Vec<PathBuf> {
    let mut roots = Vec::new();
    if let Some(app_data) = std::env::var_os("APPDATA") {
        roots.push(
            PathBuf::from(app_data)
                .join("Microsoft")
                .join("Windows")
                .join("Start Menu")
                .join("Programs"),
        );
    }
    if let Some(program_data) = std::env::var_os("PROGRAMDATA") {
        roots.push(
            PathBuf::from(program_data)
                .join("Microsoft")
                .join("Windows")
                .join("Start Menu")
                .join("Programs"),
        );
    }
    roots
}

fn scan_shortcuts(
    directory: &Path,
    now: u64,
    depth: usize,
    visited: &mut HashSet<String>,
    output: &mut Vec<PublishedApplication>,
) -> Result<(), String> {
    if depth > 16 {
        return Ok(());
    }
    let directory_identity = format!("dir:{}", directory.to_string_lossy().to_lowercase());
    if !visited.insert(directory_identity) {
        return Ok(());
    }
    let entries = match fs::read_dir(directory) {
        Ok(entries) => entries,
        Err(error)
            if matches!(
                error.kind(),
                std::io::ErrorKind::NotFound | std::io::ErrorKind::PermissionDenied
            ) =>
        {
            return Ok(());
        }
        Err(error) => return Err(format!("cannot scan {}: {error}", directory.display())),
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            scan_shortcuts(&path, now, depth + 1, visited, output)?;
            continue;
        }
        if !path
            .extension()
            .is_some_and(|extension| extension.eq_ignore_ascii_case("lnk"))
        {
            continue;
        }
        let normalized = path.to_string_lossy().replace('/', "\\");
        let identity = normalized.to_lowercase();
        if !visited.insert(identity.clone()) {
            continue;
        }
        let Some(name) = path.file_stem().and_then(|name| name.to_str()) else {
            continue;
        };
        let name = name.trim();
        if name.is_empty() || looks_like_maintenance_shortcut(name) {
            continue;
        }
        output.push(PublishedApplication {
            id: Uuid::new_v5(&APPLICATION_NAMESPACE, identity.as_bytes()),
            name: name.to_owned(),
            launch_target: normalized,
            arguments: String::new(),
            working_directory: path
                .parent()
                .map(|parent| parent.to_string_lossy().into_owned()),
            kind: ApplicationKind::Gui,
            source: ApplicationSource::StartMenu,
            enabled: true,
            updated_at_unix_ms: now,
        });
    }
    Ok(())
}

fn looks_like_maintenance_shortcut(name: &str) -> bool {
    let name = name.to_lowercase();
    ["uninstall", "卸载", "readme", "release notes", "帮助文档"]
        .iter()
        .any(|keyword| name.contains(keyword))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shortcut_identity_is_stable_and_maintenance_entries_are_filtered() {
        let directory = std::env::temp_dir().join(format!("peerspan-app-scan-{}", Uuid::new_v4()));
        fs::create_dir_all(&directory).unwrap();
        fs::write(directory.join("Designer.lnk"), b"shortcut").unwrap();
        fs::write(directory.join("Uninstall Designer.lnk"), b"shortcut").unwrap();
        let mut first = Vec::new();
        scan_shortcuts(&directory, 1, 0, &mut HashSet::new(), &mut first).unwrap();
        let mut second = Vec::new();
        scan_shortcuts(&directory, 2, 0, &mut HashSet::new(), &mut second).unwrap();
        assert_eq!(first.len(), 1);
        assert_eq!(first[0].name, "Designer");
        assert_eq!(first[0].id, second[0].id);
        fs::remove_dir_all(directory).unwrap();
    }
}
