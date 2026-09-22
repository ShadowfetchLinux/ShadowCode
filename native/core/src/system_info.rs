//! Read-only host facts from fixed OS interfaces. No shell, display connection,
//! screenshots, arbitrary paths, or device identifiers are needed.
use serde_json::{json, Value};
use std::{fs, io::Read, path::Path};

fn read_attribute(path: &Path) -> Option<String> {
    let mut text = String::new();
    fs::File::open(path)
        .ok()?
        .take(4097)
        .read_to_string(&mut text)
        .ok()?;
    (text.len() <= 4096).then(|| text.trim().to_owned())
}

fn connector_name(name: &str) -> bool {
    name.strip_prefix("card")
        .and_then(|tail| tail.split_once('-'))
        .is_some_and(|(card, connector)| {
            !card.is_empty() && card.bytes().all(|c| c.is_ascii_digit()) && !connector.is_empty()
        })
}

fn displays(root: &Path) -> Value {
    let mut connectors = Vec::new();
    let mut complete = true;
    if let Ok(entries) = fs::read_dir(root) {
        for (index, entry) in entries.enumerate() {
            if index >= 256 {
                complete = false;
                break;
            }
            let Ok(entry) = entry else {
                complete = false;
                continue;
            };
            let name = entry.file_name().to_string_lossy().into_owned();
            if !connector_name(&name) {
                continue;
            }
            let connected = match read_attribute(&entry.path().join("status")).as_deref() {
                Some("connected") => Some(true),
                Some("disconnected") => Some(false),
                _ => None,
            };
            let enabled = match read_attribute(&entry.path().join("enabled")).as_deref() {
                Some("enabled") => Some(true),
                Some("disabled") => Some(false),
                _ => None,
            };
            complete &= connected.is_some();
            connectors.push(json!({"connector":name,"connected":connected,"enabled":enabled}));
        }
    } else {
        complete = false;
    }
    connectors.sort_by(|a, b| a["connector"].as_str().cmp(&b["connector"].as_str()));
    // Empty/unavailable sysfs is not evidence of zero attached monitors.
    complete &= !connectors.is_empty();
    let connected = connectors.iter().filter(|c| c["connected"] == true).count();
    let enabled = connectors
        .iter()
        .filter(|c| c["connected"] == true && c["enabled"] == true)
        .count();
    let enabled_known = complete
        && connectors
            .iter()
            .filter(|c| c["connected"] == true)
            .all(|c| c["enabled"].is_boolean());
    json!({
        "source":"/sys/class/drm", "complete":complete,
        "connected_count":complete.then_some(connected),
        "enabled_count":enabled_known.then_some(enabled),
        "connectors":connectors,
        "note":"Counts are kernel display outputs on the machine running ShadowCode. They do not identify mirrored/logical desktops, screen contents, current resolution, or remote client monitors. Null counts mean detection is incomplete or unavailable; do not interpret them as zero."
    })
}

pub fn inspect() -> Value {
    json!({
        "os":std::env::consts::OS,
        "architecture":std::env::consts::ARCH,
        "observed_at":crate::now(),
        "read_only":true,
        "displays":displays(Path::new("/sys/class/drm")),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn connector(root: &Path, name: &str, status: &str, enabled: Option<&str>) {
        let dir = root.join(name);
        fs::create_dir(&dir).unwrap();
        fs::write(dir.join("status"), status).unwrap();
        if let Some(enabled) = enabled {
            fs::write(dir.join("enabled"), enabled).unwrap();
        }
    }

    #[test]
    fn counts_connected_and_enabled_outputs_separately() {
        let root = tempfile::tempdir().unwrap();
        connector(root.path(), "card1-DP-2", "connected\n", Some("enabled\n"));
        connector(root.path(), "card0-HDMI-A-1", "connected", Some("disabled"));
        connector(root.path(), "card0-DP-1", "disconnected", Some("disabled"));
        fs::create_dir(root.path().join("card0")).unwrap();
        fs::create_dir(root.path().join("renderD128")).unwrap();
        let info = displays(root.path());
        assert_eq!(info["connected_count"], 2);
        assert_eq!(info["enabled_count"], 1);
        assert_eq!(info["connectors"].as_array().unwrap().len(), 3);
        assert_eq!(info["connectors"][0]["connector"], "card0-DP-1");
        fs::write(root.path().join("card1-DP-2/status"), "disconnected").unwrap();
        assert_eq!(
            displays(root.path())["connected_count"],
            1,
            "Do not cache hardware state"
        );
    }

    #[test]
    fn unavailable_or_unknown_detection_is_not_zero_monitors() {
        let root = tempfile::tempdir().unwrap();
        for dir in [root.path().to_path_buf(), root.path().join("missing")] {
            let info = displays(&dir);
            assert_eq!(info["complete"], false);
            assert!(info["connected_count"].is_null());
            assert!(info["enabled_count"].is_null());
        }
        connector(root.path(), "card0-DP-1", "unknown", Some("disabled"));
        assert!(displays(root.path())["connected_count"].is_null());
        fs::write(root.path().join("card0-DP-1/status"), "disconnected").unwrap();
        assert_eq!(displays(root.path())["connected_count"], 0);
        connector(root.path(), "card0-HDMI-A-1", "connected", None);
        let info = displays(root.path());
        assert_eq!(info["connected_count"], 1);
        assert!(info["enabled_count"].is_null());
    }
}
