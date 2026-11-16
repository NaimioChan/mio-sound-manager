// Hide the console window in release builds; keep it in debug so `log` output and
// panics are visible while developing.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    reslip_app::run();
}
