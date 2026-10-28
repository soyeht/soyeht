// Generated from contracts/legacy.json. Do not edit.
/// Bundle identifiers; a longer name that starts with one is the legacy app's too.
pub const BUNDLE_IDS: [&str; 2] = ["com.soyeht.mac", "com.soyeht.mac.dev"];
/// The folder the legacy programs are installed in.
pub const APPLICATION_FOLDER: &str = "/Applications";
/// Folder names of the legacy programs; anything inside one is the legacy app's.
pub const APPLICATIONS: [&str; 2] = ["Soyeht.app", "Soyeht Dev.app"];
/// The folder that holds application data, below which the legacy app keeps its own.
pub const DATA_BASE: &str = "Application Support";
/// The legacy app's data folders, below `DATA_BASE`.
pub const DATA_FOLDERS: [&str; 2] = ["Soyeht", "Soyeht Dev"];
