//! Preferences: which panel is open and the small choices inside them. The panels show what the
//! engine holds (harnesses, settings, usage, skills) and what the person chose here (the theme).

use crate::theme::{ThemeName, desktop, space};
use crate::{
    composer_field::{ComposerEvent, ComposerField, new_field},
    open_menu::Menu,
    workbench::Workbench,
};
use gpui::{Context, Entity};
use soyeht_client::{Command, local_preferences::LocalPreferences};
use soyeht_protocol::UsageProvider;
use std::path::Path;

/// A panel of Preferences, in the order of the sidebar.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Section {
    Setup,
    Appearance,
    Harnesses,
    Usage,
    Skills,
}

impl Section {
    pub const ALL: [Self; 5] = [
        Self::Setup,
        Self::Appearance,
        Self::Harnesses,
        Self::Usage,
        Self::Skills,
    ];

    pub fn title(self) -> &'static str {
        match self {
            Self::Setup => "Setup",
            Self::Appearance => "Appearance",
            Self::Harnesses => "Harnesses",
            Self::Usage => "Usage",
            Self::Skills => "Skills",
        }
    }
}

/// The fields of the form that makes a skill.
pub struct SkillForm {
    pub name: Entity<ComposerField>,
    pub description: Entity<ComposerField>,
    pub instructions: Entity<ComposerField>,
}

#[derive(Default)]
pub struct Prefs {
    pub section: Option<Section>,
    /// The provider whose detail the Usage panel shows.
    pub usage_provider: Option<UsageProvider>,
    /// The harness whose status explanation is open in the Harnesses panel.
    pub status_open: Option<String>,
    /// The harness whose list of permission modes is open. It is not a menu of the window: the
    /// panel stays open under it.
    pub mode_menu: Option<String>,
    /// The width of the open panel's page, as the last frame laid it out.
    pub detail_width: f32,
    /// The form that makes a skill, while it is open.
    pub skill_form: Option<SkillForm>,
    /// The skill whose deletion waits for a second press.
    pub skill_to_delete: Option<String>,
}

/// How wide the sheet of Preferences is in a window of `window` points: as wide as the Mac's,
/// and no wider than the window less its margins. Returns the sheet's width and height.
pub fn sheet_extent(window: (f32, f32)) -> (f32, f32) {
    let fit = |wanted: f32, room: f32| wanted.min(room - 2.0 * desktop::PREFERENCES_INSET);
    (
        fit(desktop::PREFERENCES_WIDTH, window.0),
        fit(desktop::PREFERENCES_HEIGHT, window.1),
    )
}

/// The width of a panel's page in a sheet `sheet` points wide.
pub fn page_width(sheet: f32) -> f32 {
    sheet - 3.0 * space::X10 - desktop::PREFERENCES_SIDEBAR_WIDTH
}

/// Whether a harness row has to put its chips under its name: the page is too narrow for the
/// name, the chips and the status side by side.
pub fn rows_wrap(page: f32) -> bool {
    page < desktop::HARNESS_ROW_WIDE_MINIMUM
}

/// The theme `saved` holds; the default when none was chosen or this build no longer has it.
pub(crate) fn saved_theme(saved: &LocalPreferences) -> ThemeName {
    saved
        .theme
        .as_deref()
        .and_then(ThemeName::from_id)
        .unwrap_or(ThemeName::DEFAULT)
}

/// Writes `theme` as the choice of the next launch, keeping the other choices in the file.
fn remember_theme(directory: &Path, theme: ThemeName) -> std::io::Result<()> {
    let mut saved = LocalPreferences::load(directory);
    saved.theme = Some(theme.id().to_owned());
    saved.save(directory)
}

impl Prefs {
    pub fn section(&self) -> Section {
        self.section.unwrap_or(Section::Harnesses)
    }
}

impl Workbench {
    /// Preferences have just opened: the panels read what the engine holds, as the Mac's do when
    /// they appear.
    pub(crate) fn begin_prefs(&mut self, cx: &mut Context<Self>) {
        self.send(Command::LoadHarnesses { rescan: false });
        let harnesses: Vec<String> = self
            .state
            .harnesses()
            .iter()
            .map(|harness| harness.id.clone())
            .collect();
        self.send(Command::LoadPreferences { harnesses });
        let available: Vec<String> = self
            .state
            .harnesses()
            .iter()
            .filter(|harness| harness.available)
            .map(|harness| harness.id.clone())
            .collect();
        for harness in available {
            self.send(Command::LoadCatalogue { harness });
        }
        self.send(Command::LoadUsage);
        self.send(Command::LoadSkills { workspace_id: None });
        self.send(Command::LoadReadiness);
        cx.notify();
    }

    pub(crate) fn select_section(&mut self, section: Section, cx: &mut Context<Self>) {
        self.prefs.section = Some(section);
        self.prefs.status_open = None;
        self.prefs.mode_menu = None;
        self.prefs.skill_to_delete = None;
        cx.notify();
    }

    /// Applies `theme` at once and keeps it for the next launch, as the Mac does.
    pub(crate) fn select_theme(&mut self, theme: ThemeName, cx: &mut Context<Self>) {
        if self.theme != theme {
            self.theme = theme;
            if let Some(directory) = self.preferences.as_deref()
                && let Err(error) = remember_theme(directory, theme)
            {
                soyeht_platform::log::failure("desktop.theme_not_saved", &error);
            }
        }
        cx.notify();
    }

    pub(crate) fn toggle_status(&mut self, harness: &str, cx: &mut Context<Self>) {
        let open = self.prefs.status_open.as_deref() == Some(harness);
        self.prefs.status_open = (!open).then(|| harness.to_owned());
        cx.notify();
    }

    pub(crate) fn toggle_mode_menu(&mut self, harness: &str, cx: &mut Context<Self>) {
        let open = self.prefs.mode_menu.as_deref() == Some(harness);
        self.prefs.mode_menu = (!open).then(|| harness.to_owned());
        cx.notify();
    }

    pub(crate) fn make_default_harness(&mut self, harness: &str, cx: &mut Context<Self>) {
        self.send(Command::SetPreferredHarness {
            harness: harness.to_owned(),
        });
        self.prefs.status_open = None;
        cx.notify();
    }

    /// The permission mode new panes of `harness` start in; `None` keeps the harness's own.
    pub(crate) fn set_default_mode(
        &mut self,
        harness: &str,
        mode: Option<String>,
        cx: &mut Context<Self>,
    ) {
        self.send(Command::SetDefaultMode {
            harness: harness.to_owned(),
            mode,
        });
        self.prefs.mode_menu = None;
        cx.notify();
    }

    /// Opens Preferences on the usage of `provider`: what a click on its ring does.
    pub(crate) fn open_usage(&mut self, provider: UsageProvider, cx: &mut Context<Self>) {
        self.prefs.section = Some(Section::Usage);
        self.prefs.usage_provider = Some(provider);
        if !self.prefs_open() {
            self.toggle_menu(Menu::Settings, cx);
        }
        cx.notify();
    }

    pub(crate) fn select_usage_provider(
        &mut self,
        provider: UsageProvider,
        cx: &mut Context<Self>,
    ) {
        self.prefs.usage_provider = Some(provider);
        cx.notify();
    }

    pub(crate) fn refresh_usage(
        &mut self,
        provider: Option<UsageProvider>,
        cx: &mut Context<Self>,
    ) {
        self.send(Command::RefreshUsage { provider });
        cx.notify();
    }

    /// Opens the form that makes a skill.
    pub(crate) fn begin_skill(&mut self, cx: &mut Context<Self>) {
        let palette = self.theme.palette();
        let field = |cx: &mut Context<Self>| {
            let field = new_field(palette, cx);
            cx.subscribe(&field, |this, _, event: &ComposerEvent, cx| {
                if *event == ComposerEvent::Dismiss {
                    this.close_menu(cx);
                }
            })
            .detach();
            field
        };
        self.prefs.skill_form = Some(SkillForm {
            name: field(cx),
            description: field(cx),
            instructions: field(cx),
        });
        cx.notify();
    }

    pub(crate) fn cancel_skill(&mut self, cx: &mut Context<Self>) {
        self.prefs.skill_form = None;
        cx.notify();
    }

    /// Makes the skill the form describes; the engine tells when the list changes.
    pub(crate) fn create_skill(&mut self, cx: &mut Context<Self>) {
        let Some(form) = self.prefs.skill_form.as_ref() else {
            return;
        };
        let read = |field: &Entity<ComposerField>| field.read(cx).draft().text().trim().to_owned();
        let (name, description, instructions) = (
            read(&form.name),
            read(&form.description),
            read(&form.instructions),
        );
        if name.is_empty() || description.is_empty() {
            return;
        }
        self.send(Command::CreateSkill {
            name,
            description,
            instructions,
        });
        self.prefs.skill_form = None;
        cx.notify();
    }

    /// Deletes a skill on the second press, as the Mac asks first.
    pub(crate) fn delete_skill(&mut self, name: &str, cx: &mut Context<Self>) {
        if self.prefs.skill_to_delete.as_deref() == Some(name) {
            self.send(Command::DeleteSkill {
                name: name.to_owned(),
            });
            self.prefs.skill_to_delete = None;
        } else {
            self.prefs.skill_to_delete = Some(name.to_owned());
        }
        cx.notify();
    }

    /// Whether Preferences are the open menu.
    pub(crate) fn prefs_open(&self) -> bool {
        self.menus.is_open(&Menu::Settings)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> std::path::PathBuf {
        let path = std::env::temp_dir().join(format!("soyeht-theme-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&path);
        path
    }

    #[test]
    fn the_chosen_theme_is_the_one_the_next_launch_opens_in() {
        let directory = scratch("chosen");
        assert_eq!(
            saved_theme(&LocalPreferences::load(&directory)),
            ThemeName::DEFAULT,
            "a first launch opens in the default"
        );
        for theme in ThemeName::ALL.iter().copied() {
            remember_theme(&directory, theme).unwrap();
            assert_eq!(saved_theme(&LocalPreferences::load(&directory)), theme);
        }
        let _ = std::fs::remove_dir_all(&directory);
    }

    #[test]
    fn a_theme_this_build_does_not_know_falls_back_to_the_default() {
        let removed = LocalPreferences {
            theme: Some("neoRemovedYesterday".into()),
        };
        assert_eq!(saved_theme(&removed), ThemeName::DEFAULT);
        let empty = LocalPreferences {
            theme: Some(String::new()),
        };
        assert_eq!(saved_theme(&empty), ThemeName::DEFAULT);
    }

    #[test]
    fn the_names_are_the_ones_the_mac_persists() {
        assert_eq!(ThemeName::NeoDeepForest.id(), "neoDeepForest");
        for theme in ThemeName::ALL.iter().copied() {
            assert_eq!(ThemeName::from_id(theme.id()), Some(theme));
        }
        assert_eq!(ThemeName::from_id("Neo · Milk"), None);
    }

    #[test]
    fn the_harnesses_open_first() {
        assert_eq!(Prefs::default().section(), Section::Harnesses);
    }

    #[test]
    fn the_sheet_fits_the_smallest_window_and_its_rows_wrap_there() {
        let (width, height) = sheet_extent((640.0, 440.0));
        assert!(width > 0.0 && width <= 640.0 - 2.0 * desktop::PREFERENCES_INSET);
        assert!(height > 0.0 && height <= 440.0 - 2.0 * desktop::PREFERENCES_INSET);
        assert!(page_width(width) > 0.0);
        assert!(
            rows_wrap(page_width(width)),
            "the rows cannot hold side by side here"
        );
        // The window of the audit and the default window.
        assert!(rows_wrap(page_width(sheet_extent((960.0, 640.0)).0)));
        assert!(!rows_wrap(page_width(sheet_extent((1280.0, 800.0)).0)));
    }
}
