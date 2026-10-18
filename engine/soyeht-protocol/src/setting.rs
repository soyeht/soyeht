// Generated from contracts/protocol.json (sha256 a83e5a091c82349a4173728fab5308dc11fe83521977008e26f2fb81c3b50038). Do not edit.

use serde_json::Value;

/// Why the engine refuses a `settings.get` or `settings.set`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettingError {
    /// The key is not one of `x-settings`.
    Unknown,
    /// The value has the wrong type or is out of bounds.
    Invalid,
}

pub const TERMINAL_DEFAULT_COLS: &str = "terminal.defaultCols";
pub const TERMINAL_DEFAULT_ROWS: &str = "terminal.defaultRows";
pub const RECORD_DEFAULT: &str = "record.default";
pub const HARNESS_PREFERRED_ID: &str = "harness.preferredId";
pub const HARNESS_DEFAULT_MODE_PREFIX: &str = "harness.defaultMode.";

/// Every setting that stands alone.
pub const KEYS: [&str; 4] = [
    TERMINAL_DEFAULT_COLS,
    TERMINAL_DEFAULT_ROWS,
    RECORD_DEFAULT,
    HARNESS_PREFERRED_ID,
];

/// The key of the `harness.defaultMode.<harnessId>` setting for `harness_id`.
pub fn harness_default_mode_key(harness_id: &str) -> String {
    format!("{HARNESS_DEFAULT_MODE_PREFIX}{harness_id}")
}

/// The `harnessId` of a `harness.defaultMode.<harnessId>` key, when `key` is one.
pub fn harness_default_mode_key_parameter(key: &str) -> Option<&str> {
    key.strip_prefix(HARNESS_DEFAULT_MODE_PREFIX)
        .filter(|harness_id| (1..=128).contains(&harness_id.len()))
}

/// The value a setting has until someone changes it.
pub fn default_value(key: &str) -> Result<Value, SettingError> {
    Ok(match key {
        TERMINAL_DEFAULT_COLS => Value::from(80),
        TERMINAL_DEFAULT_ROWS => Value::from(24),
        RECORD_DEFAULT => Value::Bool(true),
        HARNESS_PREFERRED_ID => Value::String("".into()),
        key if harness_default_mode_key_parameter(key).is_some() => Value::String("".into()),
        _ => return Err(SettingError::Unknown),
    })
}

/// Whether `value` may be stored under `key`.
pub fn validate(key: &str, value: &Value) -> Result<(), SettingError> {
    let accepted = match key {
        TERMINAL_DEFAULT_COLS => value.as_i64().is_some_and(|n| (1..=1000).contains(&n)),
        TERMINAL_DEFAULT_ROWS => value.as_i64().is_some_and(|n| (1..=1000).contains(&n)),
        RECORD_DEFAULT => value.is_boolean(),
        HARNESS_PREFERRED_ID => value
            .as_str()
            .is_some_and(|text| text.len() <= 128 && !text.contains('\0')),
        key if harness_default_mode_key_parameter(key).is_some() => value
            .as_str()
            .is_some_and(|text| text.len() <= 128 && !text.contains('\0')),
        _ => return Err(SettingError::Unknown),
    };
    if accepted {
        Ok(())
    } else {
        Err(SettingError::Invalid)
    }
}
