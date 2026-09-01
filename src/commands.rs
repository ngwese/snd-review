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
        ViewExplorer,
        ViewHistory,
        ViewScript,
        EditUndo,
        EditRedo,
        EditCut,
        EditCopy,
        EditPaste,
        EditDelete,
        EditRemove,
        EditDuplicate,
        EditTrim,
        EditRollLeft,
        EditRollRight,
    ]
);

const DEFAULT_KEYMAP: &str = include_str!("../assets/keymap.json");

/// Keymap actions apply while the waveform App view is focused, not while
/// typing in the Script panel or other text fields.
const APP_KEY_CONTEXT: &str = "App";

/// Fit/frame letter keys apply while the waveform is focused.
const WAVEFORM_KEY_CONTEXT: &str = "Waveform";

/// Fit/frame letter keys also apply while the pointer is over the waveform.
const WAVEFORM_HOVER_KEY_CONTEXT: &str = "WaveformHover";

const KNOWN_COMMANDS: &[&str] = &[
    "file.open",
    "file.quit",
    "help.about",
    "view.fit_all",
    "view.frame",
    "view.zoom_in",
    "view.zoom_out",
    "view.explorer",
    "view.history",
    "view.script",
    "transport.home",
    "transport.previous",
    "transport.start",
    "transport.play_pause",
    "transport.stop",
    "transport.next",
    "transport.end",
    "transport.loop",
    "edit.undo",
    "edit.redo",
    "edit.cut",
    "edit.copy",
    "edit.paste",
    "edit.delete",
    "edit.remove",
    "edit.duplicate",
    "edit.trim",
    "edit.roll_left",
    "edit.roll_right",
];

/// Command IDs that keymap, menus, and Lua `app:command` share.
pub fn known_commands() -> &'static [&'static str] {
    KNOWN_COMMANDS
}

pub fn is_known_command(command_id: &str) -> bool {
    KNOWN_COMMANDS.contains(&command_id)
}

/// Run a command by keymap ID. Lua and the menu/key path share this table.
pub fn dispatch(command_id: &str, cx: &mut App) -> Result<(), String> {
    validate_command_id(command_id)?;
    if let Some(result) = crate::script::try_invoke_command(command_id) {
        return result;
    }
    crate::app::dispatch_command(command_id, cx)
}

pub fn validate_command_id(command_id: &str) -> Result<(), String> {
    if !is_known_command(command_id) {
        return Err(format!("unknown command `{command_id}`"));
    }
    Ok(())
}

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
        .flat_map(|(keystrokes, command_id)| bindings_for(&command_id, &keystrokes))
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

pub fn user_config_dir() -> Option<PathBuf> {
    user_keymap_path()?.parent().map(PathBuf::from)
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

fn valid_keystrokes(spec: &str) -> bool {
    let mut parts = spec.split_whitespace().peekable();
    if parts.peek().is_none() {
        return false;
    }
    parts.all(|part| Keystroke::parse(part).is_ok())
}

fn bindings_for(command_id: &str, keystrokes: &str) -> Vec<KeyBinding> {
    let contexts: &[&str] = match command_id {
        "view.fit_all" | "view.frame" => &[WAVEFORM_KEY_CONTEXT, WAVEFORM_HOVER_KEY_CONTEXT],
        _ => &[APP_KEY_CONTEXT],
    };
    contexts
        .iter()
        .filter_map(|context| binding_in(command_id, keystrokes, context))
        .collect()
}

fn binding_for(command_id: &str, keystrokes: &str) -> Option<KeyBinding> {
    bindings_for(command_id, keystrokes).into_iter().next()
}

fn binding_in(command_id: &str, keystrokes: &str, context: &str) -> Option<KeyBinding> {
    if !is_known_command(command_id) {
        eprintln!("snd-review: unknown command {command_id:?} bound to {keystrokes:?}");
        return None;
    }
    if !valid_keystrokes(keystrokes) {
        eprintln!("snd-review: invalid keystroke {keystrokes:?} for {command_id}");
        return None;
    }
    Some(match command_id {
        "file.open" => KeyBinding::new(keystrokes, Open, Some(context)),
        "file.quit" => KeyBinding::new(keystrokes, Quit, Some(context)),
        "help.about" => KeyBinding::new(keystrokes, About, Some(context)),
        "view.fit_all" => KeyBinding::new(keystrokes, ViewFitAll, Some(context)),
        "view.frame" => KeyBinding::new(keystrokes, ViewFrame, Some(context)),
        "view.zoom_in" => KeyBinding::new(keystrokes, ViewZoomIn, Some(context)),
        "view.zoom_out" => KeyBinding::new(keystrokes, ViewZoomOut, Some(context)),
        "view.explorer" => KeyBinding::new(keystrokes, ViewExplorer, Some(context)),
        "view.history" => KeyBinding::new(keystrokes, ViewHistory, Some(context)),
        "view.script" => KeyBinding::new(keystrokes, ViewScript, Some(context)),
        "transport.home" => KeyBinding::new(keystrokes, TransportHome, Some(context)),
        "transport.previous" => KeyBinding::new(keystrokes, TransportPrevious, Some(context)),
        "transport.start" => KeyBinding::new(keystrokes, TransportStart, Some(context)),
        "transport.play_pause" => KeyBinding::new(keystrokes, TransportPlayPause, Some(context)),
        "transport.stop" => KeyBinding::new(keystrokes, TransportStop, Some(context)),
        "transport.next" => KeyBinding::new(keystrokes, TransportNext, Some(context)),
        "transport.end" => KeyBinding::new(keystrokes, TransportEnd, Some(context)),
        "transport.loop" => KeyBinding::new(keystrokes, TransportLoop, Some(context)),
        "edit.undo" => KeyBinding::new(keystrokes, EditUndo, Some(context)),
        "edit.redo" => KeyBinding::new(keystrokes, EditRedo, Some(context)),
        "edit.cut" => KeyBinding::new(keystrokes, EditCut, Some(context)),
        "edit.copy" => KeyBinding::new(keystrokes, EditCopy, Some(context)),
        "edit.paste" => KeyBinding::new(keystrokes, EditPaste, Some(context)),
        "edit.delete" => KeyBinding::new(keystrokes, EditDelete, Some(context)),
        "edit.remove" => KeyBinding::new(keystrokes, EditRemove, Some(context)),
        "edit.duplicate" => KeyBinding::new(keystrokes, EditDuplicate, Some(context)),
        "edit.trim" => KeyBinding::new(keystrokes, EditTrim, Some(context)),
        "edit.roll_left" => KeyBinding::new(keystrokes, EditRollLeft, Some(context)),
        "edit.roll_right" => KeyBinding::new(keystrokes, EditRollRight, Some(context)),
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
        assert_eq!(map.get("cmd-z").map(String::as_str), Some("edit.undo"));
        assert_eq!(map.get("cmd-x").map(String::as_str), Some("edit.cut"));
        assert_eq!(map.get("cmd-c").map(String::as_str), Some("edit.copy"));
        assert_eq!(map.get("cmd-v").map(String::as_str), Some("edit.paste"));
        assert_eq!(
            map.get("backspace").map(String::as_str),
            Some("edit.delete")
        );
        assert_eq!(map.get("delete").map(String::as_str), Some("edit.remove"));
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
    fn keymap_bindings_are_scoped_to_the_app_view() {
        let binding = binding_for("edit.delete", "backspace").expect("backspace");
        assert!(binding.predicate().is_some());
        let binding = binding_for("transport.play_pause", "space").expect("space");
        assert!(binding.predicate().is_some());
    }

    #[test]
    fn fit_and_frame_keys_are_scoped_to_the_waveform() {
        let fit = bindings_for("view.fit_all", "a");
        assert_eq!(fit.len(), 2);
        assert!(fit.iter().all(|binding| binding.predicate().is_some()));
        let frame = bindings_for("view.frame", "f");
        assert_eq!(frame.len(), 2);
        assert!(frame.iter().all(|binding| binding.predicate().is_some()));
    }

    #[test]
    fn dispatch_rejects_unknown_command_ids() {
        assert_eq!(KNOWN_COMMANDS.len(), known_commands().len());
        for id in known_commands() {
            assert!(is_known_command(id));
            assert!(validate_command_id(id).is_ok());
        }
        let err = validate_command_id("not.a.command").unwrap_err();
        assert!(err.contains("unknown command"));
    }

    #[test]
    fn invalid_json_is_an_error() {
        let err = parse_and_flatten("{not json", Platform::Macos).unwrap_err();
        assert!(err.contains("invalid keymap JSON"));
    }
}
