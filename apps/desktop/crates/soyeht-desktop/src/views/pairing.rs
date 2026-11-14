//! Pairing a phone (ADR 0164): the place of the rail that lists the paired devices, and the sheet
//! that shows the QR code of a window and the requests that come in on it, each with the field the
//! person types the phone's six digits in. What it says is `soyeht_client::pairing::copy`; what it
//! decides is `soyeht_client::pairing`. Colors are the palette's and sizes are tokens.

use super::{
    cards::{Role, button},
    places::{
        entry_mark, entry_words, field_line, make_pill, place_card, place_heading, place_page,
        scrolling_list, well_card,
    },
    prelude::*,
    sheet,
    trust_prompt::sheet_extent,
    ui::{caption, chrome_text, empty_state, pressable},
};
use crate::{
    pairing_ui as ui,
    theme::{Palette, desktop, radius, space, type_scale},
};
use gpui::{Bounds, canvas, fill, point, size};
use soyeht_client::{
    Connection,
    pairing::{Matrix, Offer, Phase, QUIET_ZONE, Request, copy},
    timestamp,
};
use soyeht_protocol::{
    Device,
    client_policy::{CANCEL, COPIED_CODE, TRY_AGAIN},
};

/// How light a color is, from 0 to 255.
fn luminance(color: u32) -> f32 {
    let channel = |shift: u32| f32::from(((color >> shift) & 0xFF) as u8);
    0.2126 * channel(16) + 0.7152 * channel(8) + 0.0722 * channel(0)
}

/// The colors of a QR code: the darker and the lighter of the palette's ink and canvas, so that
/// the modules are dark on light in every theme and no color is made for it.
fn qr_colors(palette: &Palette) -> (u32, u32) {
    if luminance(palette.ink) <= luminance(palette.canvas) {
        (palette.ink, palette.canvas)
    } else {
        (palette.canvas, palette.ink)
    }
}

/// The square of `desktop::PAIRING_QR_SIDE` that draws `matrix` with its quiet zone. Each module is
/// a whole number of pixels, so the edges stay sharp, and the square holds the code in its middle.
fn qr_square(matrix: &Matrix, palette: &Palette) -> impl IntoElement {
    let (dark, light) = qr_colors(palette);
    let matrix = matrix.clone();
    canvas(
        |_, _, _| (),
        move |bounds, (), window, _| {
            let side = desktop::PAIRING_QR_SIDE;
            let cells = (matrix.side() + 2 * QUIET_ZONE) as f32;
            let module = (side / cells).floor().max(1.0);
            let inset = ((side - module * cells) / 2.0).max(0.0) + module * QUIET_ZONE as f32;
            window.paint_quad(fill(bounds, rgb(light)));
            for (x, y) in matrix.dark_modules() {
                let at = bounds.origin
                    + point(px(inset + module * x as f32), px(inset + module * y as f32));
                let cell = Bounds::new(at, size(px(module), px(module)));
                window.paint_quad(fill(cell, rgb(dark)));
            }
        },
    )
    .size(px(desktop::PAIRING_QR_SIDE))
    .flex_none()
    .rounded(px(radius::CONTROL))
}

/// A line of text that warns.
fn warning(palette: &Palette, text: impl Into<SharedString>) -> Div {
    div()
        .text_size(px(type_scale::CAPTION))
        .text_color(rgb(palette.warning()))
        .child(text.into())
}

/// A button that does `act` when it is pressed and `enabled`.
fn action(
    cx: &mut Context<Workbench>,
    palette: &Palette,
    key: SharedString,
    (role, label): (Role, &'static str),
    (enabled, act): (
        bool,
        impl Fn(&mut Workbench, &mut Context<Workbench>) + 'static,
    ),
) -> gpui::Stateful<Div> {
    pressable(button(key, role, label, palette), enabled, cx, act)
}

/// The key of a control that belongs to the request or device `of`.
fn key_of(kind: &'static str, of: &str) -> SharedString {
    SharedString::from(format!("{kind}-{of}"))
}

/// A fingerprint shown by its ends, as the Mac's list shows it.
fn compact(fingerprint: &str) -> String {
    match (fingerprint.get(..8), fingerprint.len().checked_sub(8)) {
        (Some(head), Some(from)) if fingerprint.len() > 17 => {
            format!(
                "{head}\u{2026}{}",
                fingerprint.get(from..).unwrap_or_default()
            )
        }
        _ => fingerprint.to_owned(),
    }
}

/// The place of the rail that holds the phones: what the Mac's "iPhone or iPad" place holds, the
/// button that pairs one, the requests that wait for the person while the sheet is closed, and
/// the devices already paired.
pub(super) fn phone_place(
    wb: &mut Workbench,
    cx: &mut Context<Workbench>,
    palette: &'static Palette,
) -> AnyElement {
    let connected = wb.state.connection == Connection::Connected;
    let pair = pressable(
        make_pill("pair-phone", palette, copy::PAIR_BUTTON),
        connected,
        cx,
        ui::open,
    );
    let waiting: Vec<Request> = wb.state.pairing.requests().to_vec();
    let devices: Vec<Device> = wb.state.pairing.devices().to_vec();
    let waiting = waiting.iter().filter(|request| !request.closed);
    let mut rows: Vec<AnyElement> = waiting
        .map(|request| entry_row(Entry::Waiting(request), palette, cx))
        .collect();
    rows.extend(
        devices
            .iter()
            .map(|device| entry_row(Entry::Paired(device), palette, cx)),
    );
    let list = if rows.is_empty() {
        empty_state(palette, Some(Icon::Smartphone), copy::NO_DEVICES, None).into_any_element()
    } else {
        scrolling_list("phone-list", space::X8, rows)
    };
    let page = place_page()
        .child(place_heading(
            palette,
            copy::PHONE_TITLE,
            copy::PHONE_SUBTITLE,
            pair,
        ))
        .children(revoke_question(wb, palette, cx))
        .child(list);
    place_card(palette, Icon::Smartphone, copy::PHONE_TITLE, page)
}

/// A line of the list: a request that waits for the person, or a device that is paired.
enum Entry<'a> {
    /// It came in while the sheet was closed and can be denied; allowing it is the sheet's, where
    /// the code is typed.
    Waiting(&'a Request),
    /// Its name, the ends of its key, whether it is paired, and the way to revoke.
    Paired(&'a Device),
}

fn entry_row(
    entry: Entry<'_>,
    palette: &'static Palette,
    cx: &mut Context<Workbench>,
) -> AnyElement {
    let (name, note, status, button) = match entry {
        Entry::Waiting(request) => (
            request.device_name.clone(),
            request.comparison.clone().unwrap_or_default(),
            (copy::WAITING_FOR_APPROVAL, palette.warning()),
            Some(deny_button(cx, palette, request, "deny-waiting")),
        ),
        Entry::Paired(device) => {
            let id = device.device_id.clone();
            let revoke = (!device.revoked).then(|| {
                action(
                    cx,
                    palette,
                    key_of("revoke", &id),
                    (Role::Destructive, copy::REVOKE),
                    (true, move |this, cx| ui::ask_to_revoke(this, &id, cx)),
                )
            });
            let status = if device.revoked {
                (copy::REVOKED, palette.danger())
            } else {
                (copy::PAIRED, palette.success())
            };
            (
                device.name.clone(),
                compact(&device.fingerprint),
                status,
                revoke,
            )
        }
    };
    row()
        .gap(px(space::X12))
        .px(px(space::X12))
        .py(px(space::X10))
        .rounded(px(radius::CONTROL))
        .bg(rgb(palette.well))
        .items_center()
        .child(entry_mark(palette, Icon::Smartphone))
        .child(entry_words(palette, &name, &note))
        .child(
            div()
                .text_size(px(type_scale::SMALL))
                .text_color(rgb(status.1))
                .font_weight(FontWeight::SEMIBOLD)
                .child(status.0),
        )
        .children(button)
        .into_any_element()
}

/// The button that denies `request`; `kind` keeps it apart from the other denial buttons.
fn deny_button(
    cx: &mut Context<Workbench>,
    palette: &Palette,
    request: &Request,
    kind: &'static str,
) -> gpui::Stateful<Div> {
    let id = request.request_id.clone();
    action(
        cx,
        palette,
        key_of(kind, &id),
        (Role::Secondary, copy::DENY),
        (!request.deciding, move |this, cx| {
            ui::decide(this, &id, false, cx)
        }),
    )
}

/// The confirmation before a device is revoked, and why the last revocation failed.
fn revoke_question(
    wb: &Workbench,
    palette: &'static Palette,
    cx: &mut Context<Workbench>,
) -> Option<AnyElement> {
    let failure = wb.state.pairing.revoke_failure().map(str::to_owned);
    let asked = wb
        .state
        .pairing
        .revoking()
        .map(|device| device.name.clone());
    if asked.is_none() && failure.is_none() {
        return None;
    }
    let mut card = well_card(palette);
    if let Some(name) = asked {
        let buttons = row()
            .gap(px(space::X8))
            .child(action(
                cx,
                palette,
                "revoke-cancel".into(),
                (Role::Secondary, CANCEL),
                (true, |this, cx| ui::cancel_revoke(this, cx)),
            ))
            .child(action(
                cx,
                palette,
                "revoke-confirm".into(),
                (Role::Destructive, copy::REVOKE),
                (true, |this, cx| ui::confirm_revoke(this, cx)),
            ));
        card = card
            .child(
                chrome_text(FontWeight::BOLD, type_scale::SUBHEADING)
                    .child(format!("{} {name}", copy::REVOKE_QUESTION)),
            )
            .child(caption(palette).child(copy::REVOKE_NOTE))
            .child(buttons);
    }
    Some(
        card.children(failure.map(|text| warning(palette, text)))
            .into_any_element(),
    )
}

/// The sheet over the window while the person pairs a phone, or `None` when it is closed.
pub(super) fn pairing_sheet(
    wb: &mut Workbench,
    cx: &mut Context<Workbench>,
    palette: &'static Palette,
) -> Option<AnyElement> {
    if !wb.state.pairing.shown() {
        return None;
    }
    let extent = sheet_extent((wb.window.width, wb.window.height));
    let phase = wb.state.pairing.phase().clone();
    let remaining = wb.state.pairing.remaining(timestamp::now());
    let requests: Vec<Request> = wb.state.pairing.requests().to_vec();
    let offers_code = wb.state.pairing.offers_code();
    let subtitle = match &phase {
        Phase::Showing(_) => copy::SCAN_INSTRUCTION,
        Phase::Starting { .. } => copy::STARTING,
        Phase::Refused(_) => copy::CANNOT_PAIR_YET,
        Phase::Expired => copy::EXPIRED,
        Phase::Interrupted => copy::INTERRUPTED,
        Phase::Closed => "",
    };
    let close = close_button("pairing-close", palette)
        .on_click(cx.listener(|this, _: &ClickEvent, _, cx| ui::close(this, cx)));
    let done = sheet::button(palette, "pairing-done".into(), false)
        .cursor_pointer()
        .on_click(cx.listener(|this, _: &ClickEvent, _, cx| ui::close(this, cx)))
        .child(copy::CLOSE);
    let mut card = sheet::panel(palette, "pairing", extent)
        .child(sheet::header(palette, copy::PAIR_BUTTON, subtitle, close))
        .children(window_body(wb, &phase, remaining, palette, cx));
    for request in &requests {
        card = card.child(request_card(wb, request, offers_code, palette, cx));
    }
    let card = card.child(row().justify_end().child(done));
    Some(
        sheet::centered_layer()
            .id("pairing-overlay")
            .occlude()
            .child(card)
            .into_any_element(),
    )
}

/// What the sheet shows of the window: the code, why there is none, or that it ran out.
fn window_body(
    wb: &Workbench,
    phase: &Phase,
    remaining: Option<u64>,
    palette: &'static Palette,
    cx: &mut Context<Workbench>,
) -> Option<AnyElement> {
    let connected = wb.state.connection == Connection::Connected;
    let again = |cx: &mut Context<Workbench>| {
        div().child(action(
            cx,
            palette,
            "pairing-again".into(),
            (Role::Primary, TRY_AGAIN),
            (connected, |this, cx| ui::open(this, cx)),
        ))
    };
    match phase {
        Phase::Showing(offer) => {
            let copied = wb.pairing_ui.link_copied();
            Some(offer_body(offer, (palette, copied), remaining, cx))
        }
        Phase::Refused(refusal) => Some(
            column()
                .gap(px(space::X10))
                .child(warning(palette, refusal.message.clone()))
                .child(again(cx))
                .into_any_element(),
        ),
        Phase::Expired => Some(again(cx).into_any_element()),
        Phase::Starting { .. } | Phase::Interrupted | Phase::Closed => None,
    }
}

/// The QR code, the time left and the way to copy the link.
fn offer_body(
    offer: &Offer,
    (palette, copied): (&'static Palette, bool),
    remaining: Option<u64>,
    cx: &mut Context<Workbench>,
) -> AnyElement {
    let link = offer.link.clone();
    let label = if copied { COPIED_CODE } else { copy::COPY_LINK };
    let copy_link = action(
        cx,
        palette,
        "pairing-copy-link".into(),
        (Role::Secondary, label),
        (true, move |this, cx| ui::copy_link(this, link.clone(), cx)),
    );
    let time_left = remaining.map(|seconds| caption(palette).child(copy::time_left(seconds)));
    column()
        .items_center()
        .gap(px(space::X10))
        .children(
            offer
                .matrix
                .as_ref()
                .map(|matrix| qr_square(matrix, palette)),
        )
        .children(
            offer
                .problem
                .clone()
                .map(|problem| warning(palette, problem)),
        )
        .child(
            row()
                .gap(px(space::X12))
                .items_center()
                .children(time_left)
                .child(copy_link),
        )
        .into_any_element()
}

/// A request on the sheet: who asks, the code both screens show, and what the person does.
fn request_card(
    wb: &mut Workbench,
    request: &Request,
    offers_code: bool,
    palette: &'static Palette,
    cx: &mut Context<Workbench>,
) -> AnyElement {
    let id = request.request_id.clone();
    let note = request.note.clone().map(|note| warning(palette, note));
    let code = request.comparison.as_ref().map(|code| {
        div()
            .mono()
            .font_weight(FontWeight::SEMIBOLD)
            .text_size(px(type_scale::HEADING))
            .child(SharedString::from(code.clone()))
    });
    let mut card = well_card(palette)
        .child(
            chrome_text(FontWeight::BOLD, type_scale::SUBHEADING)
                .child(copy::allow_question(&request.device_name)),
        )
        .children(code);
    if request.comparison.is_none() {
        card = card.child(warning(palette, copy::NO_COMPARISON));
    }
    if request.closed {
        let dismiss_id = id.clone();
        let dismiss = action(
            cx,
            palette,
            key_of("dismiss", &id),
            (Role::Secondary, copy::DISMISS),
            (true, move |this, cx| ui::dismiss(this, &dismiss_id, cx)),
        );
        return card.children(note).child(dismiss).into_any_element();
    }
    let asks_code = offers_code && request.comparison.is_some();
    if request.comparison.is_some() {
        card = card.child(caption(palette).child(copy::COMPARE));
    }
    if asks_code {
        let field = wb.pairing_ui.code_field(&id, palette, cx);
        field.update(cx, |field, _| field.style(palette, copy::CODE_PLACEHOLDER));
        card = card
            .child(caption(palette).child(copy::TYPE_THE_CODE))
            .child(field_line(field, palette));
    }
    let allow_id = id.clone();
    let buttons = row()
        .gap(px(space::X8))
        .child(deny_button(cx, palette, request, "deny"))
        .child(action(
            cx,
            palette,
            key_of("allow", &id),
            (Role::Primary, copy::ALLOW),
            (
                request.comparison.is_some() && !request.deciding,
                move |this, cx| ui::decide(this, &allow_id, true, cx),
            ),
        ));
    card = card.children(note).child(buttons);
    if asks_code {
        card = card
            .child(caption(palette).child(copy::CODE_WARNING))
            .child(caption(palette).child(copy::PHONE_SHOWS_NO_CODE));
    }
    card.into_any_element()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::ThemeName;

    #[test]
    fn a_qr_code_is_dark_on_light_in_every_theme() {
        for name in ThemeName::ALL {
            let palette = name.palette();
            let (dark, light) = qr_colors(palette);
            assert!(
                luminance(light) - luminance(dark) > 60.0,
                "{name:?} is too flat to scan"
            );
            assert!([palette.ink, palette.canvas].contains(&dark));
            assert!([palette.ink, palette.canvas].contains(&light));
        }
    }

    #[test]
    fn a_fingerprint_is_shown_by_its_ends() {
        assert_eq!(
            compact("0123456789abcdef0123456789abcdef"),
            "01234567\u{2026}89abcdef"
        );
        assert_eq!(compact("0123456789abcdef0"), "0123456789abcdef0");
        assert_eq!(compact("abc"), "abc");
    }
}
