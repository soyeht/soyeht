// A Windows-subsystem program: no console window of its own (see `soyeht_service::host`).
#![cfg_attr(windows, windows_subsystem = "windows")]

fn main() {
    std::process::exit(soyeht_service::host::run(std::env::args_os().skip(1)));
}
