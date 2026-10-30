"""The profiles contract (`contracts/profiles.json`): what a profile may hold, and the Rust, Swift
and Python forms generated from it (ADR 0112)."""

import json
import re

from generated_header import header_text, rustfmt

PROFILES_SOURCE = "contracts/profiles.json"
PROFILE_COMPONENTS = ("engine", "ptyd")
# The files the programs write their log to, in the `logs` folder of the data directory.
PROFILE_LOGS = ("engine", "ptyd", "desktop")


PROFILE_NAMES = ("dev", "prod")
PROFILE_SYSTEMS = ("macos", "windows", "linux")
PROFILE_APP_SYSTEMS = ("macos", "windows", "linux")
PROFILE_NAME_KEYS = ("macApp", "window", "windowsFolder")
# `displayNames.windowsFolder` is the folder below `%LOCALAPPDATA%\Programs` the Windows installer
# uses. It is a name for the machine as much as for people: the login tasks hold the absolute path of
# each program in it (contract 13), so it must never change once a release is out.
WINDOWS_FOLDER_REJECTED = re.compile(r'[<>:"/\\|?*\x00-\x1f]|[ .]$|^[ .]')
# The names Windows keeps for devices: a folder cannot be called any of them, with or without an
# extension, in any case (`con`, `Aux.d`, `COM1`).
WINDOWS_DEVICE_NAMES = frozenset(
    {"CON", "PRN", "AUX", "NUL", "CONIN$", "CONOUT$"}
    | {f"{device}{number}" for device in ("COM", "LPT") for number in "123456789\u00b9\u00b2\u00b3"}
)
PROFILE_FIELDS = {"dataDirectory", "tcpPort", "displayNames", "appIds", "launchdLabels", "systemdUnits", "windowsTasks"}
# The owner's legacy app is not this repository (AGENTS.md, "Legacy app"). Every machine
# identifier of a profile carries this marker, so no value can name the legacy app's.
LEGACY_MARKER = "next"


def _text(value, what):
    if not isinstance(value, str) or not value or not value.isascii() or not value.isprintable():
        raise ValueError(f"{what} must be printable ASCII text")
    return value


def _keys(value, expected, what):
    if not isinstance(value, dict) or set(value) != set(expected):
        raise ValueError(f"{what} must have exactly the keys {', '.join(expected)}")
    return value


def _next_marked(value, what):
    if LEGACY_MARKER not in value.lower():
        raise ValueError(f'{what} "{value}" must contain "{LEGACY_MARKER}"')


def _validate_data_directory(directories, name):
    _keys(directories, PROFILE_SYSTEMS, f"{name} dataDirectory")
    for system, path in directories.items():
        what = f"{name} data directory for {system}"
        _text(path, what)
        parts = path.split("/")
        if "\\" in path or ":" in path or path.endswith("/") or path.startswith("/"):
            raise ValueError(f"{what} must be relative and use / between components")
        if any(part in {"", ".", ".."} for part in parts):
            raise ValueError(f"{what} has an unsafe component")
        _next_marked(parts[0], what)


def _validate_profile(name, profile):
    _keys(profile, PROFILE_FIELDS, f"{name} profile")
    _validate_data_directory(profile["dataDirectory"], name)
    port = profile["tcpPort"]
    if isinstance(port, bool) or not isinstance(port, int) or not 1 <= port <= 65535:
        raise ValueError(f"{name} TCP port must be between 1 and 65535")
    names = _keys(profile["displayNames"], PROFILE_NAME_KEYS, f"{name} displayNames")
    for key, text in names.items():
        # What the person reads: the Mac app's file name and the window title. Not an identifier,
        # so the legacy marker is not required here (the packaging script checks the app name).
        if not isinstance(text, str) or not text.strip() or not text.isprintable():
            raise ValueError(f"{name} display name {key} must be printable text")
    folder = names["windowsFolder"]
    if WINDOWS_FOLDER_REJECTED.search(folder):
        raise ValueError(f"{name} windowsFolder must be a valid Windows folder name")
    if folder.split(".")[0].rstrip().upper() in WINDOWS_DEVICE_NAMES:
        raise ValueError(f"{name} windowsFolder must not be a Windows device name")
    _next_marked(folder, f"{name} windowsFolder")
    _keys(profile["appIds"], PROFILE_APP_SYSTEMS, f"{name} appIds")
    for system, app_id in profile["appIds"].items():
        _text(app_id, f"{name} app id for {system}")
        if not re.fullmatch(r"[a-z0-9]+(?:\.[a-z0-9]+)+", app_id):
            raise ValueError(f"{name} app id for {system} must be a reverse-DNS name")
        _next_marked(app_id, f"{name} app id for {system}")
    labels = _keys(profile["launchdLabels"], PROFILE_COMPONENTS, f"{name} launchdLabels")
    if len(set(labels.values())) != len(PROFILE_COMPONENTS):
        raise ValueError(f"{name} launchdLabels values must be unique")
    for component, label in labels.items():
        _text(label, f"{name} launchd label")
        if not re.fullmatch(rf"[a-z0-9]+(?:[.-][a-z0-9]+)*\.{component}", label):
            raise ValueError(f"{name} launchd label must end with its component")
        _next_marked(label, f"{name} launchd label")
    if len({label.removesuffix(f".{c}") for c, label in labels.items()}) != 1:
        raise ValueError(f"{name} launchd labels must share one namespace")
    _validate_service_names(
        profile["systemdUnits"], f"{name} systemdUnits", "systemd unit",
        lambda component: rf"[a-z0-9]+(?:-[a-z0-9]+)*-{component}\.service",
    )
    _validate_service_names(
        profile["windowsTasks"], f"{name} windowsTasks", "Windows task",
        lambda component: rf"[A-Za-z0-9]+(?:\.[A-Za-z0-9]+)*\.{component.capitalize()}",
    )


def _validate_service_names(names, what, noun, pattern):
    """The names the service seam registers: a systemd unit and a Windows task per component.

    The characters are limited to what both `systemctl` and `schtasks` take without quoting, so
    no program that reads a name needs to escape it.
    """
    _keys(names, PROFILE_COMPONENTS, what)
    if len(set(names.values())) != len(PROFILE_COMPONENTS):
        raise ValueError(f"{what} values must be unique")
    for component, value in names.items():
        _text(value, f"{what} {component}")
        if not re.fullmatch(pattern(component), value):
            raise ValueError(f"{what} {component} must be a {noun} name for that component")
        _next_marked(value, f"{what} {component}")


def validate_profiles(document):
    if set(document) != {"version", "selection", "sockets", "logs", "profiles"} or document["version"] != 2:
        raise ValueError("Profiles document must have version 2, selection, sockets, logs and profiles")
    selection = _keys(document["selection"], ("environment", "buildEnvironment"), "selection")
    for field, value in selection.items():
        if not re.fullmatch(r"SOYEHT_[A-Z0-9_]+", _text(value, f"selection {field}")):
            raise ValueError(f"selection {field} must be a SOYEHT_ environment variable")
    if len(set(selection.values())) != 2:
        raise ValueError("selection variables must differ")
    sockets = _keys(document["sockets"], PROFILE_COMPONENTS, "sockets")
    if len(set(sockets.values())) != len(PROFILE_COMPONENTS):
        raise ValueError("socket names must be unique")
    for socket_name in sockets.values():
        if not re.fullmatch(r"[A-Za-z0-9._-]+\.sock", _text(socket_name, "socket name")):
            raise ValueError("Socket names must be .sock basenames")
    logs = _keys(document["logs"], PROFILE_LOGS, "logs")
    if len(set(logs.values())) != len(PROFILE_LOGS):
        raise ValueError("log file names must be unique")
    for log_name in logs.values():
        if not re.fullmatch(r"[A-Za-z0-9_-]+\.log", _text(log_name, "log file name")):
            raise ValueError("Log file names must be .log basenames")
    profiles = _keys(document["profiles"], PROFILE_NAMES, "profiles")
    for name, profile in profiles.items():
        _validate_profile(name, profile)
    for field in ("tcpPort", "displayNames", "appIds", "launchdLabels", "dataDirectory", "systemdUnits", "windowsTasks"):
        seen = [json.dumps(profile[field], sort_keys=True) for profile in profiles.values()]
        if len(set(seen)) != len(seen):
            raise ValueError(f"profiles must not share their {field}")
    folders = {profile["displayNames"]["windowsFolder"].casefold() for profile in profiles.values()}
    if len(folders) != len(profiles):
        raise ValueError("profiles must not share a windowsFolder (Windows ignores case)")
    for system in PROFILE_SYSTEMS:
        if len({profile["dataDirectory"][system] for profile in profiles.values()}) != len(profiles):
            raise ValueError(f"profiles must not share a data directory on {system}")


def profile_values(document):
    profiles = {}
    for name, profile in document["profiles"].items():
        labels = profile["launchdLabels"]
        profiles[name] = {
            "name": name,
            "data": profile["dataDirectory"],
            "port": profile["tcpPort"],
            "mac_app_name": profile["displayNames"]["macApp"],
            "window_title": profile["displayNames"]["window"],
            "windows_folder": profile["displayNames"]["windowsFolder"],
            "app_ids": profile["appIds"],
            "engine_label": labels["engine"],
            "ptyd_label": labels["ptyd"],
            "label_prefix": labels["engine"].removesuffix(".engine"),
            "engine_unit": profile["systemdUnits"]["engine"],
            "ptyd_unit": profile["systemdUnits"]["ptyd"],
            "engine_task": profile["windowsTasks"]["engine"],
            "ptyd_task": profile["windowsTasks"]["ptyd"],
        }
    return {
        "profiles": profiles,
        "engine_socket": document["sockets"]["engine"],
        "ptyd_socket": document["sockets"]["ptyd"],
        "engine_log": document["logs"]["engine"],
        "ptyd_log": document["logs"]["ptyd"],
        "desktop_log": document["logs"]["desktop"],
        "environment": document["selection"]["environment"],
        "build_environment": document["selection"]["buildEnvironment"],
    }


def _rust_profile(profile):
    data, apps = profile["data"], profile["app_ids"]
    return f'''pub static {profile["name"].upper()}: Profile = Profile {{
    name: {json.dumps(profile["name"])},
    data_directory: DataDirectory {{
        macos: {json.dumps(data["macos"])},
        windows: {json.dumps(data["windows"])},
        linux: {json.dumps(data["linux"])},
    }},
    tcp_port: {profile["port"]},
    mac_app_name: {json.dumps(profile["mac_app_name"], ensure_ascii=False)},
    window_title: {json.dumps(profile["window_title"], ensure_ascii=False)},
    macos_app_id: {json.dumps(apps["macos"])},
    windows_app_id: {json.dumps(apps["windows"])},
    linux_app_id: {json.dumps(apps["linux"])},
    engine_launchd_label: {json.dumps(profile["engine_label"])},
    ptyd_launchd_label: {json.dumps(profile["ptyd_label"])},
    engine_systemd_unit: {json.dumps(profile["engine_unit"])},
    ptyd_systemd_unit: {json.dumps(profile["ptyd_unit"])},
    engine_windows_task: {json.dumps(profile["engine_task"])},
    ptyd_windows_task: {json.dumps(profile["ptyd_task"])},
}};
'''


def _swift_profile(profile):
    return f'''    public static let {profile["name"]} = ProfileValues(
        name: {json.dumps(profile["name"])},
        applicationSupportSubpath: {json.dumps(profile["data"]["macos"])},
        tcpPort: {profile["port"]},
        macAppName: {json.dumps(profile["mac_app_name"], ensure_ascii=False)},
        windowTitle: {json.dumps(profile["window_title"], ensure_ascii=False)},
        appBundleId: {json.dumps(profile["app_ids"]["macos"])},
        engineLaunchdLabel: {json.dumps(profile["engine_label"])},
        ptydLaunchdLabel: {json.dumps(profile["ptyd_label"])}
    )
'''


def render_profiles(document):
    value = profile_values(document)
    header = header_text(PROFILES_SOURCE)
    profiles = list(value["profiles"].values())
    rust = f'''// {header}
/// Where a profile keeps its data below the user's data base directory (`/` between
/// components), for each system.
#[derive(Debug, PartialEq, Eq)]
pub struct DataDirectory {{
    pub macos: &'static str,
    pub windows: &'static str,
    pub linux: &'static str,
}}

impl DataDirectory {{
    /// The subpath for the system this program was built for.
    pub const fn native(&self) -> &'static str {{
        if cfg!(target_os = "macos") {{
            self.macos
        }} else if cfg!(windows) {{
            self.windows
        }} else {{
            self.linux
        }}
    }}
}}

/// The values that tell two installed copies of Soyeht apart on one machine.
#[derive(Debug, PartialEq, Eq)]
pub struct Profile {{
    pub name: &'static str,
    pub data_directory: DataDirectory,
    pub tcp_port: u16,
    /// The Mac app's name, which is also its file name (`<name>.app`).
    pub mac_app_name: &'static str,
    /// The title of the Mac app's main window.
    pub window_title: &'static str,
    pub macos_app_id: &'static str,
    pub windows_app_id: &'static str,
    pub linux_app_id: &'static str,
    pub engine_launchd_label: &'static str,
    pub ptyd_launchd_label: &'static str,
    pub engine_systemd_unit: &'static str,
    pub ptyd_systemd_unit: &'static str,
    pub engine_windows_task: &'static str,
    pub ptyd_windows_task: &'static str,
}}

{"".join(_rust_profile(profile) for profile in profiles)}
pub static PROFILES: [&Profile; {len(profiles)}] = [{", ".join("&" + p["name"].upper() for p in profiles)}];
/// The environment variable that names the profile of a running program.
pub const SELECTION_ENV: &str = {json.dumps(value["environment"])};
/// The profile a build defaults to when `SELECTION_ENV` is unset: the value of this variable
/// when the program was compiled.
pub const BUILD_PROFILE: Option<&str> = option_env!({json.dumps(value["build_environment"])});
pub const ENGINE_SOCKET: &str = {json.dumps(value["engine_socket"])};
pub const PTYD_SOCKET: &str = {json.dumps(value["ptyd_socket"])};
/// The file the engine's output is appended to, in the `logs` folder of the data directory.
pub const ENGINE_LOG: &str = {json.dumps(value["engine_log"])};
/// The file the supervisor's output is appended to, beside [`ENGINE_LOG`].
pub const PTYD_LOG: &str = {json.dumps(value["ptyd_log"])};
/// The file the desktop client writes its log lines to, beside [`ENGINE_LOG`].
pub const DESKTOP_LOG: &str = {json.dumps(value["desktop_log"])};
'''
    swift = f'''// {header}
public struct ProfileValues: Sendable {{
    public let name: String
    public let applicationSupportSubpath: String
    public let tcpPort: UInt16
    public let macAppName: String
    public let windowTitle: String
    public let appBundleId: String
    public let engineLaunchdLabel: String
    public let ptydLaunchdLabel: String

{"".join(_swift_profile(profile) for profile in profiles)}
    public static let all = [{", ".join(p["name"] for p in profiles)}]
    public static let selectionEnvironment = {json.dumps(value["environment"])}
    public static let engineSocketName = {json.dumps(value["engine_socket"])}
    public static let ptydSocketName = {json.dumps(value["ptyd_socket"])}
}}
'''
    python_profiles = {
        profile["name"]: {
            "dataDirectory": profile["data"],
            "tcpPort": profile["port"],
            "displayNames": {
                "macApp": profile["mac_app_name"],
                "window": profile["window_title"],
                "windowsFolder": profile["windows_folder"],
            },
            "appIds": profile["app_ids"],
            "launchdLabels": {"engine": profile["engine_label"], "ptyd": profile["ptyd_label"]},
            "launchdPrefix": profile["label_prefix"],
            "systemdUnits": {"engine": profile["engine_unit"], "ptyd": profile["ptyd_unit"]},
            "windowsTasks": {"engine": profile["engine_task"], "ptyd": profile["ptyd_task"]},
        }
        for profile in profiles
    }
    python = f'''# {header}
PROFILES = {json.dumps(python_profiles, indent=4)}
SELECTION_ENVIRONMENT = {value["environment"]!r}
BUILD_ENVIRONMENT = {value["build_environment"]!r}
ENGINE_SOCKET = {value["engine_socket"]!r}
PTYD_SOCKET = {value["ptyd_socket"]!r}
ENGINE_LOG = {value["engine_log"]!r}
PTYD_LOG = {value["ptyd_log"]!r}
DESKTOP_LOG = {value["desktop_log"]!r}
'''
    return {
        "engine/soyeht-protocol/src/profile.rs": rustfmt(rust),
        "contracts/swift/Sources/SoyehtProtocol/Profile.swift": swift,
        "scripts/generated_profile.py": python,
    }
