#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() -> impl std::process::Termination {
    studio_editor::run()
}
