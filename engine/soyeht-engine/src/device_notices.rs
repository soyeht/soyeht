//! The notice of a device added, and the panes a device made (ADR 0164).
//!
//! Where the engine asks for the person's code it dates each device it pairs and names the node it
//! came from. A viewer (a paired device, or the person at the computer) is told about every device
//! added since it last dismissed the notice, which keeps it until then. A revoked device's panes keep
//! running, so the revoke dialog lists what the device made and offers to close it.
use crate::{
    core::Core,
    errors::{self, Result},
    model,
};
use chrono::{DateTime, SecondsFormat};
use serde_json::{Value, json};
use soyeht_protocol::{
    Device, DeviceNoticeDismissParams, DeviceNoticeDismissResult, DevicePanesListParams, Role,
    method,
};
use soyeht_store::device_notices;

/// The viewer of the person at the computer, as the marks of a dismissed notice name it. A paired
/// device is its own id, which is a UUID and so never this.
const LOCAL_VIEWER: &str = "local";

/// `unix` as the contract writes a time.
pub(crate) fn time_text(unix: i64) -> Result<String> {
    DateTime::from_timestamp(unix, 0)
        .map(|time| time.to_rfc3339_opts(SecondsFormat::Secs, true))
        .ok_or_else(|| errors::internal("A time is out of range"))
}

fn unix_of(text: &str) -> Option<i64> {
    DateTime::parse_from_rfc3339(text)
        .ok()
        .map(|time| time.timestamp())
}

/// Whether `method` is one of this module's.
pub(crate) fn handles(method: &str) -> bool {
    matches!(
        method,
        method::DEVICE_NOTICE_LIST | method::DEVICE_NOTICE_DISMISS | method::DEVICE_PANES_LIST
    )
}

/// The calls of this module.
pub(crate) fn request(
    core: &mut Core,
    connection: &str,
    method: &str,
    params: &Value,
) -> Result<Value> {
    match method {
        method::DEVICE_NOTICE_LIST => notice_list(core, connection),
        method::DEVICE_NOTICE_DISMISS => notice_dismiss(core, connection, method, params),
        method::DEVICE_PANES_LIST => panes_list(core, params),
        _ => Err(errors::internal("Method is not a device notice method")),
    }
}

/// Who is asking: a paired device, or the person at the computer, and for a device when it was
/// added.
fn viewer(core: &Core, connection: &str) -> Result<(String, Option<i64>)> {
    let client = errors::invariant(core.clients.get(connection), "Connection disappeared")?;
    match client
        .actor
        .as_ref()
        .map(|actor| (&actor.role, &actor.device_id))
    {
        Some((Role::HumanLocal, _)) => Ok((LOCAL_VIEWER.to_owned(), None)),
        Some((Role::HumanDevice, Some(device))) => {
            // A device has been told about what was added before it was.
            let own: Option<Device> =
                crate::ports::StorePort::device(&core.store, device).map_err(errors::storage)?;
            let added = own.and_then(|own| own.added_at).and_then(|at| unix_of(&at));
            Ok((device.clone(), added))
        }
        _ => Err(errors::forbidden_role(
            "Only the person or a paired device is told about devices added",
        )),
    }
}

fn notice_list(core: &Core, connection: &str) -> Result<Value> {
    let (viewer, own_added) = viewer(core, connection)?;
    let seen = device_notices::seen(&core.store, &viewer)
        .map_err(errors::storage)?
        .or(own_added)
        .unwrap_or(0);
    let mut added: Vec<(i64, Device)> = device_notices::dated_devices(&core.store)
        .map_err(errors::storage)?
        .into_iter()
        .filter(|device| device.device_id != viewer)
        .filter_map(|device| {
            // A time this engine cannot read is not one it wrote.
            let at = device.added_at.as_deref().and_then(unix_of)?;
            (at > seen).then_some((at, device))
        })
        .collect();
    added.sort_by_key(|(at, _)| *at);
    let items: Vec<Device> = added.into_iter().map(|(_, device)| device).collect();
    Ok(json!({"items": model::to_value(&items)?}))
}

fn notice_dismiss(
    core: &mut Core,
    connection: &str,
    method: &str,
    params: &Value,
) -> Result<Value> {
    let dismiss: DeviceNoticeDismissParams = model::typed_params(params)?;
    let (viewer, _) = viewer(core, connection)?;
    let through = unix_of(&dismiss.through)
        .ok_or_else(|| errors::invalid("`through` is not a time"))?
        .min(core.now_unix());
    let result = model::to_value(&DeviceNoticeDismissResult {
        through: time_text(through)?,
    })?;
    Ok(core
        .mutate(connection, method, params, |tx| {
            device_notices::dismiss_in(tx, &viewer, through)?;
            Ok((result, vec![]))
        })?
        .result)
}

fn panes_list(core: &Core, params: &Value) -> Result<Value> {
    let list: DevicePanesListParams = model::typed_params(params)?;
    let panes =
        device_notices::panes_created_by(&core.store, &list.device_id).map_err(errors::storage)?;
    Ok(json!({"items": model::to_value(&panes)?}))
}
