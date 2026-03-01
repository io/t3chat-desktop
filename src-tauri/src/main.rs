// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

#[cfg(target_os = "macos")]
fn set_macos_process_name() {
    use objc2_foundation::{NSProcessInfo, NSString};

    let process_info = NSProcessInfo::processInfo();
    let process_name = NSString::from_str("T3.chat");
    process_info.setProcessName(&process_name);
}

fn main() {
    #[cfg(target_os = "macos")]
    set_macos_process_name();

    t3chat_lib::run()
}
