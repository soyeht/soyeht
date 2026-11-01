//! The Usage panel of Preferences, as the Mac's: the providers with their percentage down the
//! left, and the detail of the one chosen: its windows with the time to reset, the state when
//! there is no reading, and where the reading came from.

use super::{
    preferences::{page, pill_button},
    ui::{Typeface, chrome_text, column, id, logo, row},
    usage_ring::band_color,
};
use crate::neo;
use crate::{
    assets::{self, Icon},
    theme::{Palette, desktop, elevation, radius, space, type_scale},
    workbench::Workbench,
};
use gpui::{
    AnyElement, ClickEvent, Div, FontWeight, SharedString, div, prelude::*, px, relative, rgb,
};
use soyeht_client::{
    timestamp,
    usage::{self, Band, Presentation},
};
use soyeht_protocol::{UsageProvider, UsageReading, UsageState, UsageWindow};

const SUMMARY: &str =
    "Subscription windows reported by the provider tools signed in on this machine.";

impl Workbench {
    pub(super) fn usage_page(&mut self, palette: &'static Palette, cx: &mut Context<Self>) -> Div {
        let readings = self.state.usage().to_vec();
        let selected = self
            .prefs
            .usage_provider
            .clone()
            .filter(|chosen| readings.iter().any(|reading| &reading.provider == chosen))
            .or_else(|| readings.first().map(|reading| reading.provider.clone()));
        let refresh = selected.clone();
        let actions = row().child(
            pill_button(
                palette,
                "refresh-usage".into(),
                Some(Icon::Refresh),
                "Refresh",
            )
            .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                this.refresh_usage(refresh.clone(), cx)
            })),
        );
        let detail = match readings
            .iter()
            .find(|reading| Some(&reading.provider) == selected.as_ref())
        {
            Some(reading) => balloon(reading, palette),
            None => div()
                .chrome()
                .w(px(desktop::USAGE_DETAIL_WIDTH))
                .font_weight(FontWeight::SEMIBOLD)
                .text_size(px(type_scale::CAPTION))
                .text_color(rgb(palette.secondary))
                .child("No usage reader is available."),
        };
        let list = self.provider_list(&readings, selected.as_ref(), palette, cx);
        page(
            palette,
            "Usage",
            Some(SUMMARY),
            Some(actions),
            row()
                .flex_wrap()
                .items_start()
                .gap(px(space::X16))
                .child(list)
                .child(detail),
        )
    }

    /// The providers in a raised card: logo, name and percentage, the chosen one on the well.
    fn provider_list(
        &mut self,
        readings: &[UsageReading],
        selected: Option<&UsageProvider>,
        palette: &'static Palette,
        cx: &mut Context<Self>,
    ) -> Div {
        let rows: Vec<AnyElement> = readings
            .iter()
            .map(|reading| {
                let shown = Presentation(reading);
                let name = reading.provider.display_name();
                let chosen = selected == Some(&reading.provider);
                let provider = reading.provider.clone();
                row()
                    .id(id("usage-provider", name))
                    .h(px(desktop::USAGE_PROVIDER_ROW_HEIGHT))
                    .px(px(space::X12))
                    .gap(px(space::X10))
                    .rounded(px(radius::COMPACT))
                    .cursor_pointer()
                    .text_color(rgb(if shown.is_stale() {
                        palette.muted
                    } else {
                        palette.ink
                    }))
                    .when(chosen, |item| item.bg(rgb(palette.well)))
                    .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                        this.select_usage_provider(provider.clone(), cx)
                    }))
                    .children(
                        assets::logo_path(name).map(|path| logo(path, space::X18, palette.ink)),
                    )
                    .child(
                        chrome_text(FontWeight::BOLD, type_scale::CAPTION)
                            .flex_1()
                            .child(name),
                    )
                    .child(
                        chrome_text(FontWeight::BOLD, type_scale::CONTROL)
                            .child(shown.percentage()),
                    )
                    .into_any_element()
            })
            .collect();
        neo::raised(palette, radius::CONTROL, elevation::TASK_CARD)
            .w(px(desktop::USAGE_PROVIDER_LIST_WIDTH))
            .flex_none()
            .flex()
            .flex_col()
            .gap(px(space::X8))
            .p(px(space::X8))
            .children(rows)
    }
}

/// The detail of a reading: its windows, or why there are none, and its source.
fn balloon(reading: &UsageReading, palette: &Palette) -> Div {
    let shown = Presentation(reading);
    let stale = reading.state == UsageState::Stale;
    let dot = if reading.state == UsageState::Ok {
        palette.success()
    } else {
        palette.muted
    };
    let message = |text: String| {
        row()
            .chrome()
            .min_h(px(space::X18 * 2.0))
            .font_weight(FontWeight::SEMIBOLD)
            .text_size(px(type_scale::SMALL))
            .text_color(rgb(palette.secondary))
            .child(SharedString::from(text))
    };
    let activity = shown.activity_notice();
    let body: Vec<AnyElement> = match reading.state {
        // What the device recorded, with no window: a line, never a percentage.
        UsageState::Ok | UsageState::Stale if activity.is_some() => {
            vec![message(activity.clone().unwrap_or_default()).into_any_element()]
        }
        UsageState::LoginNeeded => {
            vec![message("Sign in in your terminal.".to_owned()).into_any_element()]
        }
        UsageState::NoAccess => {
            vec![message("Allow access in Settings.".to_owned()).into_any_element()]
        }
        _ => match &reading.primary {
            Some(primary) => {
                let mut rows = vec![window_row(primary, stale, palette)];
                rows.extend(
                    reading
                        .secondary
                        .iter()
                        .map(|window| window_row(window, stale, palette)),
                );
                rows.extend(
                    shown
                        .stale_hint()
                        .map(|hint| micro(palette, hint).into_any_element()),
                );
                rows
            }
            None => vec![
                message(match reading.state {
                    UsageState::Ok => "No reading yet.".to_owned(),
                    UsageState::Stale => "Last reading is stale.".to_owned(),
                    _ => shown.unavailable_message(),
                })
                .into_any_element(),
            ],
        },
    };
    let reset = match (&activity, shown.window()) {
        (Some(_), _) => "No plan limit".to_owned(),
        (None, Some(window)) => usage::reset_text(Some(&window.resets_at), timestamp::now()),
        (None, None) => usage::reset_text(None, timestamp::now()),
    };
    neo::raised(palette, desktop::USAGE_BALLOON_RADIUS, elevation::BALLOON)
        .w(px(desktop::USAGE_DETAIL_WIDTH))
        .flex_none()
        .flex()
        .flex_col()
        .gap(px(space::X6))
        .p(px(space::X12))
        .child(
            row()
                .h(px(space::X16))
                .gap(px(space::X6))
                .child(
                    div()
                        .size(px(desktop::USAGE_STATUS_DOT))
                        .flex_none()
                        .rounded_full()
                        .bg(rgb(dot)),
                )
                .child(
                    chrome_text(FontWeight::BOLD, type_scale::CONTROL)
                        .flex_1()
                        .child(reading.provider.display_name()),
                )
                .child(micro(palette, reset)),
        )
        .children(body)
        .child(
            micro(
                palette,
                format!(
                    "Source: {} \u{b7} {}",
                    usage::source_name(&reading.source),
                    usage::age_text(reading, timestamp::now())
                ),
            )
            .truncate(),
        )
}

fn micro(palette: &Palette, text: String) -> Div {
    div()
        .chrome()
        .text_size(px(type_scale::MICRO))
        .text_color(rgb(palette.muted))
        .child(SharedString::from(text))
}

/// One window: its label, the percentage and the reset above a bar in the band's color.
fn window_row(window: &UsageWindow, stale: bool, palette: &Palette) -> AnyElement {
    let used = window.used.clamp(0.0, 1.0);
    let color = band_color(Band::of(used), palette);
    let fill = div()
        .h_full()
        .w(relative(used as f32))
        .rounded_full()
        .bg(rgb(color))
        .when(stale, |fill| {
            fill.opacity(usage::Emphasis::LastKnown.arc_opacity())
        });
    column()
        .gap(px(space::X3))
        .child(
            row()
                .gap(px(space::X4))
                .child(
                    chrome_text(FontWeight::SEMIBOLD, type_scale::SMALL)
                        .flex_1()
                        .child(SharedString::from(window.label.clone())),
                )
                .child(
                    chrome_text(FontWeight::BOLD, type_scale::MICRO)
                        .child(format!("{}%", (used * 100.0).round())),
                )
                .child(micro(
                    palette,
                    usage::reset_text(Some(&window.resets_at), timestamp::now()),
                )),
        )
        .child(
            div()
                .h(px(desktop::USAGE_BAR_HEIGHT))
                .w_full()
                .rounded_full()
                .bg(rgb(palette.well))
                .child(fill),
        )
        .into_any_element()
}
