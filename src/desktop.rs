use serde::{Deserialize, Serialize};
use std::{env, path::Path, process::Command};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Desktop {
    pub name: String,
    pub session: String,
    pub integration: String,
    pub note: String,
}

impl Desktop {
    pub fn detect() -> Self {
        let name = env::var("XDG_CURRENT_DESKTOP").unwrap_or_default();
        let session = env::var("XDG_SESSION_TYPE").unwrap_or_else(|_| {
            if env::var_os("WAYLAND_DISPLAY").is_some() {
                "wayland".into()
            } else {
                "unknown".into()
            }
        });
        Self::from_environment(
            &name,
            &session,
            env::var_os("HYPRLAND_INSTANCE_SIGNATURE").is_some(),
        )
    }
    pub fn from_environment(name: &str, session: &str, hyprland: bool) -> Self {
        let lower = name.to_lowercase();
        let (name, integration, note) = if hyprland || lower.contains("hyprland") {
            (
                "Hyprland",
                "layer-shell",
                "Native background surface; renderer supplies interaction and fullscreen pause",
            )
        } else if lower.contains("kde") || lower.contains("plasma") {
            (
                "KDE Plasma",
                "plasma-shell",
                "Use the extended renderer with KDE integration; generic layer-shell may be hidden by Plasma",
            )
        } else if lower.contains("gnome") || lower.contains("ubuntu") {
            (
                "GNOME",
                "gnome-extension",
                "Requires the selected renderer's matching GNOME Shell extension; Wallweave does not install it automatically",
            )
        } else if ["sway", "niri", "river", "wayfire", "labwc", "cosmic"]
            .iter()
            .any(|s| lower.contains(s))
        {
            (
                name,
                "layer-shell",
                "Requires layer-shell; input and pause behavior depend on compositor and renderer",
            )
        } else if session == "x11" {
            (
                name,
                "x11",
                "Desktop support depends on renderer and the desktop's own background window",
            )
        } else {
            (
                name,
                "unknown",
                "Desktop integration has not been verified; use a windowed preview first",
            )
        };
        Self {
            name: if name.is_empty() {
                "Unknown desktop".into()
            } else {
                name.into()
            },
            session: session.into(),
            integration: integration.into(),
            note: note.into(),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Monitor {
    pub name: String,
    pub width: u32,
    pub height: u32,
    pub scale: f64,
    #[serde(default)]
    pub focused: bool,
    #[serde(default)]
    pub x: Option<i32>,
    #[serde(default)]
    pub y: Option<i32>,
    #[serde(default)]
    pub transform: u32,
    #[serde(default)]
    pub description: String,
}

impl Monitor {
    /// Compositor positions use logical coordinates, so dimensions must too.
    pub fn logical_size(&self) -> (f32, f32) {
        let scale = if self.scale.is_finite() && self.scale > 0.0 {
            self.scale as f32
        } else {
            1.0
        };
        let (width, height) = if self.transform % 2 == 1 {
            (self.height, self.width)
        } else {
            (self.width, self.height)
        };
        (width.max(1) as f32 / scale, height.max(1) as f32 / scale)
    }
}

pub fn preferred_monitor(monitors: &[Monitor]) -> Option<&Monitor> {
    monitors
        .iter()
        .find(|m| m.focused)
        .or_else(|| monitors.first())
}

pub fn monitors() -> Vec<Monitor> {
    if env::var_os("HYPRLAND_INSTANCE_SIGNATURE").is_some()
        && let Ok(out) = Command::new("hyprctl").args(["-j", "monitors"]).output()
        && out.status.success()
        && let Ok(value) = serde_json::from_slice::<serde_json::Value>(&out.stdout)
        && let Some(items) = value.as_array()
    {
        return hyprland_monitors(items);
    }
    // wlr-randr is optional; unlike xrandr it reports native Wayland connectors.
    if let Ok(out) = Command::new("wlr-randr").arg("--json").output()
        && out.status.success()
        && let Ok(items) = serde_json::from_slice::<Vec<serde_json::Value>>(&out.stdout)
    {
        return wlr_monitors(&items);
    }
    // Names remain manually enterable on KDE/GNOME; do not use XWayland names.
    vec![]
}

fn hyprland_monitors(items: &[serde_json::Value]) -> Vec<Monitor> {
    items
        .iter()
        .filter(|m| m.get("disabled").and_then(|v| v.as_bool()) != Some(true))
        .filter_map(|m| {
            Some(Monitor {
                name: m.get("name")?.as_str()?.into(),
                width: m.get("width")?.as_u64()? as u32,
                height: m.get("height")?.as_u64()? as u32,
                scale: m.get("scale").and_then(|v| v.as_f64()).unwrap_or(1.0),
                focused: m.get("focused").and_then(|v| v.as_bool()).unwrap_or(false),
                x: m.get("x").and_then(|v| v.as_i64()).map(|v| v as i32),
                y: m.get("y").and_then(|v| v.as_i64()).map(|v| v as i32),
                transform: m.get("transform").and_then(|v| v.as_u64()).unwrap_or(0) as u32,
                description: m
                    .get("description")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .into(),
            })
        })
        .collect()
}

fn wlr_monitors(items: &[serde_json::Value]) -> Vec<Monitor> {
    items
        .iter()
        .filter(|m| m.get("enabled").and_then(|v| v.as_bool()) != Some(false))
        .filter_map(|m| {
            let mode = m.get("current_mode").or_else(|| {
                m.get("modes")?
                    .as_array()?
                    .iter()
                    .find(|mode| mode.get("current").and_then(|v| v.as_bool()) == Some(true))
            })?;
            Some(Monitor {
                name: m.get("name")?.as_str()?.into(),
                width: mode.get("width")?.as_u64()? as u32,
                height: mode.get("height")?.as_u64()? as u32,
                scale: m.get("scale").and_then(|v| v.as_f64()).unwrap_or(1.0),
                focused: false,
                x: m.pointer("/position/x")
                    .and_then(|v| v.as_i64())
                    .map(|v| v as i32),
                y: m.pointer("/position/y")
                    .and_then(|v| v.as_i64())
                    .map(|v| v as i32),
                transform: match m.get("transform").and_then(|v| v.as_str()).unwrap_or("") {
                    "90" | "flipped-90" => 1,
                    "270" | "flipped-270" => 3,
                    _ => 0,
                },
                description: m
                    .get("description")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .into(),
            })
        })
        .collect()
}

pub fn find_program(name: &str) -> Option<std::path::PathBuf> {
    use std::os::unix::fs::PermissionsExt;
    env::split_paths(&env::var_os("PATH")?)
        .map(|root| root.join(name))
        .find(|p| {
            p.is_file()
                && p.metadata()
                    .is_ok_and(|m| m.permissions().mode() & 0o111 != 0)
        })
}

pub fn wayland_socket_available() -> bool {
    let Some(display) = env::var_os("WAYLAND_DISPLAY") else {
        return false;
    };
    let display = Path::new(&display);
    if display.is_absolute() {
        return display.exists();
    }
    env::var_os("XDG_RUNTIME_DIR").is_some_and(|root| Path::new(&root).join(display).exists())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn hyprland_layout_preserves_negative_origins_focus_scale_and_rotation() {
        let monitors = hyprland_monitors(&[
            json!({"name":"HDMI-A-1","width":1920,"height":1080,"x":-1920,"y":0}),
            json!({"name":"DP-2","width":3840,"height":2160,"x":0,"y":-2560,"scale":1.5,"transform":1,"focused":true}),
            json!({"name":"disabled","width":1920,"height":1080,"disabled":true}),
        ]);
        assert_eq!(monitors.len(), 2);
        assert_eq!(monitors[0].x, Some(-1920));
        assert_eq!(monitors[1].y, Some(-2560));
        assert_eq!(monitors[1].logical_size(), (1440.0, 2560.0));
        assert_eq!(
            preferred_monitor(&monitors).expect("focused monitor").name,
            "DP-2"
        );
        assert_eq!(monitors[0].logical_size(), (1920.0, 1080.0));
    }

    #[test]
    fn wlr_uses_the_current_mode_and_keeps_missing_positions_unknown() {
        let monitors = wlr_monitors(&[
            json!({"name":"DP-1","enabled":true,"scale":2.0,"position":{"x":-1080,"y":0},"transform":"270","modes":[
                {"width":1920,"height":1080,"current":false},
                {"width":3840,"height":2160,"current":true}
            ]}),
            json!({"name":"HDMI-A-1","current_mode":{"width":1920,"height":1080}}),
            json!({"name":"off","enabled":false,"current_mode":{"width":1920,"height":1080}}),
        ]);
        assert_eq!(monitors.len(), 2);
        assert_eq!(monitors[0].logical_size(), (1080.0, 1920.0));
        assert_eq!(monitors[0].x, Some(-1080));
        assert_eq!(monitors[1].x, None);
        assert_eq!(monitors[1].y, None);
    }
}
