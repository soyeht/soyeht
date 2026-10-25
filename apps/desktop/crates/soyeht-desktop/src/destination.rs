//! The places the rail leads to. On the Mac each one is a screen; the desktop client draws the
//! workspace's panes for the ones it has, and says plainly which ones it does not have yet.

use crate::assets::Icon;
use soyeht_protocol::{PaneMode, WritingSection};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Destination {
    /// Every pane of the workspace.
    #[default]
    Agents,
    Editor,
    Web,
    Tasks,
    Terminal,
    Phone,
    Texts,
    Presentations,
    Screenplays,
}

impl Destination {
    /// The rail's order.
    pub const ALL: [Self; 9] = [
        Self::Agents,
        Self::Editor,
        Self::Web,
        Self::Tasks,
        Self::Terminal,
        Self::Phone,
        Self::Texts,
        Self::Presentations,
        Self::Screenplays,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Agents => "Agents",
            Self::Editor => "Editor",
            Self::Web => "Web",
            Self::Tasks => "Tasks",
            Self::Terminal => "Terminal",
            Self::Phone => "iPhone",
            Self::Texts => "Texts",
            Self::Presentations => "Presentations",
            Self::Screenplays => "Screenplays",
        }
    }

    pub fn icon(self) -> Icon {
        match self {
            Self::Agents => Icon::Cpu,
            Self::Editor => Icon::FileText,
            Self::Web => Icon::Globe,
            Self::Tasks => Icon::ListChecks,
            Self::Terminal => Icon::Terminal,
            Self::Phone => Icon::Smartphone,
            Self::Texts => Icon::Feather,
            Self::Presentations => Icon::Copy,
            Self::Screenplays => Icon::Clapperboard,
        }
    }

    /// The section of documents this place lists, for the places that do.
    pub fn section(self) -> Option<WritingSection> {
        match self {
            Self::Texts => Some(WritingSection::Texts),
            Self::Presentations => Some(WritingSection::Presentations),
            Self::Screenplays => Some(WritingSection::Screenplays),
            _ => None,
        }
    }

    /// The place that lists the documents of a section.
    pub fn of_section(section: &WritingSection) -> Self {
        match section {
            WritingSection::Texts => Self::Texts,
            WritingSection::Presentations => Self::Presentations,
            WritingSection::Screenplays => Self::Screenplays,
        }
    }

    /// The panes this place shows: `None` for all of them, else only that mode.
    pub fn shows(self, mode: &PaneMode) -> bool {
        match self {
            Self::Agents => true,
            Self::Terminal => *mode == PaneMode::Terminal,
            Self::Editor => *mode == PaneMode::Editor,
            // The places that draw no panes (`is_pane_grid`) show none.
            _ => false,
        }
    }

    /// Whether this place draws panes at all. The three writing places list the documents of
    /// the workspace, as the Mac's sections do, and open each in a pane that the agents' place
    /// shows. The web place is not one: the desktop client has no web view, so a web pane
    /// the workspace holds shows its notice in the agents' place and the place itself says so.
    pub fn is_pane_grid(self) -> bool {
        !matches!(
            self,
            Self::Tasks
                | Self::Web
                | Self::Phone
                | Self::Texts
                | Self::Presentations
                | Self::Screenplays
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_rail_lists_every_place_once_in_the_mac_order() {
        assert_eq!(Destination::ALL.len(), 9);
        assert_eq!(Destination::ALL[0], Destination::default());
        assert_eq!(Destination::ALL[4], Destination::Terminal);
    }

    /// What each place draws: the panes of its kind, a list, or the one sentence that says the
    /// desktop client does not serve it. A place the client cannot serve has no button: the "+"
    /// menu has no row for its kind either (`views::top_bar`).
    #[test]
    fn a_place_the_desktop_cannot_draw_says_so_and_offers_nothing() {
        let unserved: Vec<Destination> = Destination::ALL
            .into_iter()
            .filter(|place| {
                !place.is_pane_grid() && place.section().is_none() && *place != Destination::Tasks
            })
            .collect();
        assert_eq!(unserved, [Destination::Web, Destination::Phone]);
        for place in &unserved {
            assert!(!place.shows(&PaneMode::Web) && !place.shows(&PaneMode::Agent));
        }
        // The places that do draw panes still show their own kind.
        assert!(Destination::Editor.shows(&PaneMode::Editor));
        assert!(Destination::Terminal.is_pane_grid() && Destination::Agents.is_pane_grid());
    }

    #[test]
    fn terminal_shows_only_terminals_and_agents_shows_all() {
        assert!(Destination::Agents.shows(&PaneMode::Agent));
        assert!(Destination::Terminal.shows(&PaneMode::Terminal));
        assert!(!Destination::Terminal.shows(&PaneMode::Agent));
        assert!(!Destination::Tasks.is_pane_grid());
        assert_eq!(Destination::Texts.section(), Some(WritingSection::Texts));
        assert_eq!(
            Destination::Presentations.section(),
            Some(WritingSection::Presentations)
        );
        for section in [
            WritingSection::Texts,
            WritingSection::Presentations,
            WritingSection::Screenplays,
        ] {
            assert_eq!(
                Destination::of_section(&section).label(),
                soyeht_client::writing::rules::title(&section)
            );
        }
        for place in [
            Destination::Texts,
            Destination::Presentations,
            Destination::Screenplays,
        ] {
            assert!(!place.is_pane_grid() && !place.shows(&PaneMode::Writing));
        }
    }
}
