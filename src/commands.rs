// SPDX-FileCopyrightText: 2026 Greg Wuller
// SPDX-License-Identifier: MIT

use std::collections::HashMap;
use std::path::PathBuf;

use gpui::{actions, App, KeyBinding, Keystroke};
use serde::Deserialize;

actions!(
    snd_review,
    [
        Open,
        About,
        Quit,
        TransportHome,
        TransportPrevious,
        TransportStart,
        TransportPlayPause,
        TransportStop,
        TransportNext,
        TransportEnd,
        TransportLoop,
        ViewFitAll,
        ViewFrame,
        ViewZoomIn,
        ViewZoomOut,
    ]
);

const DEFAULT_KEYMAP: &str = include_str!("../assets/keymap.json");

const KNOWN_COMMANDS: &[&str] = &[
    "file.open",
    "file.quit",
    "help.about",
    "view.fit_all",
    "view.frame",
    "view.zoom_in",
    "view.zoom_out",
    "transport.home",
    "transport.previous",
    "transport.start",
    "transport.play_pause",
    "transport.stop",
    "transport.next",
    "transport.end",
    "transport.loop",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Platform {
    Macos,
    Linux,
    Windows,
}

#[derive(Debug, Clone, Default, Deserialize, PartialEq, Eq)]
struct KeymapFile {
    #[serde(default)]
    bindings: HashMap<String, String>,
    #[serde(default)]
    macos: HashMap<String, String>,
    #[serde(default)]
    linux: HashMap<String, String>,
    #[serde(default)]
    windows: HashMap<String, String>,
}

pub fn install_keybindings(cx: &mut App) {
    let bindings = load_keybindings();
    cx.bind_keys(bindings);
}

fn load_keybindings() -> Vec<KeyBinding> {
    resolved_bindings()
        .into_iter()
        .filter_map(|(keystrokes, command_id)| binding_for(&command_id, &keystrokes))
        .collect()
}

fn resolved_bindings() -> HashMap<String, String> {
    let platform = current_platform();
    let mut map = match parse_and_flatten(DEFAULT_KEYMAP, platform) {
        Ok(map) => map,
        Err(err) => {
            eprintln!("snd-review: failed to parse default keymap: {err}");
            HashMap::new()
        }
    };
    match read_user_keymap() {
        None => {}
        Some(Err(err)) => {
            eprintln!("snd-review: failed to read user keymap: {err}");
        }
        Some(Ok(text)) => match parse_and_flatten(&text, platform) {
            Ok(user) => merge_bindings(&mut map, user),
            Err(err) => {
                eprintln!("snd-review: failed to parse user keymap, using defaults: {err}");
            }
        },
    }
    map
}

fn parse_and_flatten(json: &str, platform: Platform) -> Result<HashMap<String, String>, String> {
    let file: KeymapFile =
        serde_json::from_str(json).map_err(|err| format!("invalid keymap JSON: {err}"))?;
    Ok(flatten_keymap(file, platform))
}

fn flatten_keymap(file: KeymapFile, platform: Platform) -> HashMap<String, String> {
    let mut map = file.bindings;
    let overlay = match platform {
        Platform::Macos => file.macos,
        Platform::Linux => file.linux,
        Platform::Windows => file.windows,
    };
    merge_bindings(&mut map, overlay);
    map
}

fn merge_bindings(base: &mut HashMap<String, String>, overlay: HashMap<String, String>) {
    base.extend(overlay);
}

fn current_platform() -> Platform {
    if cfg!(target_os = "macos") {
        Platform::Macos
    } else if cfg!(target_os = "windows") {
        Platform::Windows
    } else {
        Platform::Linux
    }
}

fn read_user_keymap() -> Option<Result<String, String>> {
    let path = user_keymap_path()?;
    if !path.is_file() {
        return None;
    }
    Some(std::fs::read_to_string(&path).map_err(|err| format!("{}: {err}", path.display())))
}

fn user_keymap_path() -> Option<PathBuf> {
    #[cfg(target_os = "macos")]
    {
        let home = std::env::var_os("HOME")?;
        Some(
            PathBuf::from(home)
                .join("Library")
                .join("Application Support")
                .join("snd-review")
                .join("keymap.json"),
        )
    }
    #[cfg(target_os = "windows")]
    {
        let appdata = std::env::var_os("APPDATA")?;
        Some(
            PathBuf::from(appdata)
                .join("snd-review")
                .join("keymap.json"),
        )
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        let dir = std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .or_else(|| {
                let home = std::env::var_os("HOME")?;
                Some(PathBuf::from(home).join(".config"))
            })?;
        Some(dir.join("snd-review").join("keymap.json"))
    }
}

fn is_known_command(command_id: &str) -> bool {
    KNOWN_COMMANDS.contains(&command_id)
}

fn valid_keystrokes(spec: &str) -> bool {
    let mut parts = spec.split_whitespace().peekable();
    if parts.peek().is_none() {
        return false;
    }
    parts.all(|part| Keystroke::parse(part).is_ok())
}

fn binding_for(command_id: &str, keystrokes: &str) -> Option<KeyBinding> {
    if !is_known_command(command_id) {
        eprintln!("snd-review: unknown command {command_id:?} bound to {keystrokes:?}");
        return None;
    }
    if !valid_keystrokes(keystrokes) {
        eprintln!("snd-review: invalid keystroke {keystrokes:?} for {command_id}");
        return None;
    }
    Some(match command_id {
        "file.open" => KeyBinding::new(keystrokes, Open, None),
        "file.quit" => KeyBinding::new(keystrokes, Quit, None),
        "help.about" => KeyBinding::new(keystrokes, About, None),
        "view.fit_all" => KeyBinding::new(keystrokes, ViewFitAll, None),
        "view.frame" => KeyBinding::new(keystrokes, ViewFrame, None),
        "view.zoom_in" => KeyBinding::new(keystrokes, ViewZoomIn, None),
        "view.zoom_out" => KeyBinding::new(keystrokes, ViewZoomOut, None),
        "transport.home" => KeyBinding::new(keystrokes, TransportHome, None),
        "transport.previous" => KeyBinding::new(keystrokes, TransportPrevious, None),
        "transport.start" => KeyBinding::new(keystrokes, TransportStart, None),
        "transport.play_pause" => KeyBinding::new(keystrokes, TransportPlayPause, None),
        "transport.stop" => KeyBinding::new(keystrokes, TransportStop, None),
        "transport.next" => KeyBinding::new(keystrokes, TransportNext, None),
        "transport.end" => KeyBinding::new(keystrokes, TransportEnd, None),
        "transport.loop" => KeyBinding::new(keystrokes, TransportLoop, None),
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_keymap_parses_and_command_ids_resolve() {
        for platform in [Platform::Macos, Platform::Linux, Platform::Windows] {
            let map = parse_and_flatten(DEFAULT_KEYMAP, platform).expect("default keymap parses");
            assert!(!map.is_empty());
            for (keystrokes, command_id) in &map {
                assert!(
                    is_known_command(command_id),
                    "unknown command {command_id:?} on {keystrokes:?}"
                );
                assert!(
                    binding_for(command_id, keystrokes).is_some(),
                    "failed to bind {command_id:?} to {keystrokes:?}"
                );
            }
        }
    }

    #[test]
    fn default_keymap_binds_requested_view_and_transport_keys() {
        let map = parse_and_flatten(DEFAULT_KEYMAP, Platform::Macos).unwrap();
        assert_eq!(map.get("a").map(String::as_str), Some("view.fit_all"));
        assert_eq!(map.get("f").map(String::as_str), Some("view.frame"));
        assert_eq!(
            map.get("secondary-=").map(String::as_str),
            Some("view.zoom_in")
        );
        assert_eq!(
            map.get("secondary-+").map(String::as_str),
            Some("view.zoom_in")
        );
        assert_eq!(
            map.get("secondary-shift-=").map(String::as_str),
            Some("view.zoom_in")
        );
        assert_eq!(
            map.get("secondary--").map(String::as_str),
            Some("view.zoom_out")
        );
        assert_eq!(
            map.get("secondary-0").map(String::as_str),
            Some("view.fit_all")
        );
        assert_eq!(
            map.get("space").map(String::as_str),
            Some("transport.play_pause")
        );
        assert_eq!(map.get("cmd-o").map(String::as_str), Some("file.open"));
        assert!(map.get("ctrl-o").is_none());
    }

    #[test]
    fn platform_overlay_replaces_shared_keys() {
        let json = r#"{
            "bindings": { "o": "file.open" },
            "linux": { "o": "file.quit" }
        }"#;
        let linux = parse_and_flatten(json, Platform::Linux).unwrap();
        let macos = parse_and_flatten(json, Platform::Macos).unwrap();
        assert_eq!(linux.get("o").map(String::as_str), Some("file.quit"));
        assert_eq!(macos.get("o").map(String::as_str), Some("file.open"));
    }

    #[test]
    fn merge_overlay_wins_per_keystroke() {
        let mut base = parse_and_flatten(DEFAULT_KEYMAP, Platform::Linux).unwrap();
        let user = parse_and_flatten(
            r#"{ "bindings": { "a": "view.frame", "g": "view.fit_all" } }"#,
            Platform::Linux,
        )
        .unwrap();
        merge_bindings(&mut base, user);
        assert_eq!(base.get("a").map(String::as_str), Some("view.frame"));
        assert_eq!(base.get("g").map(String::as_str), Some("view.fit_all"));
        assert_eq!(
            base.get("space").map(String::as_str),
            Some("transport.play_pause")
        );
    }

    #[test]
    fn unknown_command_ids_are_skipped() {
        assert!(!is_known_command("not.a.command"));
        assert!(binding_for("not.a.command", "a").is_none());
    }

    #[test]
    fn invalid_json_is_an_error() {
        let err = parse_and_flatten("{not json", Platform::Macos).unwrap_err();
        assert!(err.contains("invalid keymap JSON"));
    }
}
