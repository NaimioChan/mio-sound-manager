// ReSlip P0 probe — can a Tauri 2 window drag files into a real DAW on Windows?
//
// Deliberately minimal: one window, a static HTML page, no dev server, no npm.
// The only thing under test is `plugin:drag|start_drag`, which CrabNebula's
// `drag` crate implements on Windows via OleInitialize + IShellItemArray
// (BHID_DataObject) + DoDragDrop. A DAW accepts that only if the data object
// exposes a proper CF_HDROP; compiling is not evidence of that.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use serde::Serialize;
use std::path::{Path, PathBuf};

#[derive(Serialize)]
struct Sample {
    path: String,
    name: String,
    bytes: u64,
    format: String,
}

/// Absolute paths of the bundled test files, with anything after the crate root
/// stripped from the JS side. Exists so the page never invents a path.
#[tauri::command]
fn samples() -> Vec<Sample> {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("probe crate lives inside probes/tauri-drag-p0")
        .join("samples");

    let mut out = Vec::new();
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return out;
    };
    for entry in entries.flatten() {
        let path: PathBuf = entry.path();
        if !path.is_file() {
            continue;
        }
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let ext = path
            .extension()
            .map(|e| e.to_string_lossy().to_uppercase())
            .unwrap_or_default();
        out.push(Sample {
            path: path.to_string_lossy().into_owned(),
            bytes: entry.metadata().map(|m| m.len()).unwrap_or(0),
            name,
            format: ext,
        });
    }
    out.sort_by(|a, b| a.name.cmp(&b.name));
    out
}

/// Written next to the exe so the operator can hand us the drag result without
/// screenshots. Every attempt appends one line.
#[tauri::command]
fn log_line(line: String) {
    use std::io::Write;
    let mut path = std::env::current_exe().unwrap_or_else(|_| PathBuf::from("."));
    path.pop();
    path.push("p0-drag.log");
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
    {
        let _ = writeln!(f, "{line}");
    }
}

fn main() {
    tauri::Builder::default()
        .plugin(tauri_plugin_drag::init())
        .invoke_handler(tauri::generate_handler![samples, log_line])
        .run(tauri::generate_context!())
        .expect("error while running P0 probe window");
}

#[allow(dead_code)]
fn _assert_path_exists(p: &Path) -> bool {
    p.exists()
}
