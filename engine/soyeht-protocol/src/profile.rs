// Generated from contracts/profiles.json. Do not edit.
/// Where a profile keeps its data below the user's data base directory (`/` between
/// components), for each system.
#[derive(Debug, PartialEq, Eq)]
pub struct DataDirectory {
    pub macos: &'static str,
    pub windows: &'static str,
    pub linux: &'static str,
}

impl DataDirectory {
    /// The subpath for the system this program was built for.
    pub const fn native(&self) -> &'static str {
        if cfg!(target_os = "macos") {
            self.macos
        } else if cfg!(windows) {
            self.windows
        } else {
            self.linux
        }
    }
}

/// The values that tell two installed copies of Soyeht apart on one machine.
#[derive(Debug, PartialEq, Eq)]
pub struct Profile {
    pub name: &'static str,
    pub data_directory: DataDirectory,
    pub tcp_port: u16,
    pub macos_app_id: &'static str,
    pub linux_app_id: &'static str,
    pub engine_launchd_label: &'static str,
    pub ptyd_launchd_label: &'static str,
}

pub static DEV: Profile = Profile {
    name: "dev",
    data_directory: DataDirectory {
        macos: "Soyeht.next/dev",
        windows: "Soyeht.next/dev",
        linux: "soyeht-next/dev",
    },
    tcp_port: 8291,
    macos_app_id: "com.soyeht.next.dev.mac",
    linux_app_id: "com.soyeht.next.dev",
    engine_launchd_label: "com.soyeht.next.dev.engine",
    ptyd_launchd_label: "com.soyeht.next.dev.ptyd",
};
pub static PROD: Profile = Profile {
    name: "prod",
    data_directory: DataDirectory {
        macos: "Soyeht.next",
        windows: "Soyeht.next",
        linux: "soyeht-next",
    },
    tcp_port: 8290,
    macos_app_id: "com.soyeht.next.mac",
    linux_app_id: "com.soyeht.next",
    engine_launchd_label: "com.soyeht.next.engine",
    ptyd_launchd_label: "com.soyeht.next.ptyd",
};

pub static PROFILES: [&Profile; 2] = [&DEV, &PROD];
/// The environment variable that names the profile of a running program.
pub const SELECTION_ENV: &str = "SOYEHT_PROFILE";
/// The profile a build defaults to when `SELECTION_ENV` is unset: the value of this variable
/// when the program was compiled.
pub const BUILD_PROFILE: Option<&str> = option_env!("SOYEHT_BUILD_PROFILE");
pub const ENGINE_SOCKET: &str = "engine.sock";
pub const PTYD_SOCKET: &str = "ptyd.sock";
