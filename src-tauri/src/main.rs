// Prevent a console window from appearing alongside the GUI on Windows release
// builds. (Debug builds keep it — it is where panics become visible.)
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    kokin_osint_lib::run()
}
