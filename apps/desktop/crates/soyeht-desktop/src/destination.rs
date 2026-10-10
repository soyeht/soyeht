//! The places the rail leads to. On the Mac each one is a screen; the desktop client draws the
//! workspace's panes for the ones it has, and says plainly which ones it does not have yet.

use crate::assets::Icon;
use soyeht_protocol::PaneMode;

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

    /// The panes this place shows: `None` for all of them, else only that mode.
    pub fn shows(self, mode: &PaneMode) -> bool {
        match self {
            Self::Agents => true,
            Self::Terminal => *mode == PaneMode::Terminal,
            Self::Editor => *mode == PaneMode::Editor,
            Self::Web => *mode == PaneMode::Web,
            Self::Tasks | Self::Phone => false,
            Self::Texts | Self::Presentations | Self::Screenplays => *mode == PaneMode::Writing,
        }
    }

    /// Whether this place draws panes at all.
    pub fn is_pane_grid(self) -> bool {
        !matches!(self, Self::Tasks | Self::Phone)
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

    #[test]
    fn terminal_shows_only_terminals_and_agents_shows_all() {
        assert!(Destination::Agents.shows(&PaneMode::Agent));
        assert!(Destination::Terminal.shows(&PaneMode::Terminal));
        assert!(!Destination::Terminal.shows(&PaneMode::Agent));
        assert!(!Destination::Tasks.is_pane_grid());
    }
}
