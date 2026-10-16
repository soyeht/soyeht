//! The project trees of the workspaces: what each lists, what it asks the engine for, and the
//! folders the engine is told to watch. The tree itself (`tree`) holds no I/O; this is where its
//! needs become requests.

use super::{
    commands::EditorCommand,
    tree::{Key, ProjectTree, Row},
};
use crate::{Command, Effect};
use soyeht_protocol::ProjectTreeListResult;
use std::collections::HashMap;

struct Tree {
    tree: ProjectTree,
    /// The folders asked for and not answered, each with the view it was asked for: an answer
    /// that was asked for before ignored files were shown or hidden describes the other view.
    loading: HashMap<String, u64>,
    view: u64,
    /// The folders the engine was last told to watch on this connection.
    watched: Option<Vec<String>>,
}

#[derive(Default)]
pub struct ProjectTrees {
    trees: HashMap<String, Tree>,
}

fn send(command: EditorCommand) -> Effect {
    Effect::Send(Command::Editor(command))
}

impl Tree {
    fn new() -> Self {
        Self {
            tree: ProjectTree::default(),
            loading: HashMap::new(),
            view: 0,
            watched: None,
        }
    }

    /// Asks for every open folder that has no listing, and when none is on its way, tells the
    /// engine which folders to watch.
    fn sync(&mut self, workspace_id: &str) -> Vec<Effect> {
        let show_ignored = self.tree.show_ignored();
        let mut effects = Vec::new();
        for path in self.tree.directories_to_load() {
            if self.loading.insert(path.clone(), self.view) != Some(self.view) {
                effects.push(send(EditorCommand::List {
                    workspace_id: workspace_id.to_owned(),
                    path,
                    show_ignored,
                }));
            }
        }
        let watched = self.tree.watched_directories();
        if self.loading.is_empty() && self.watched.as_ref() != Some(&watched) {
            self.watched = Some(watched.clone());
            effects.push(send(EditorCommand::Watch {
                workspace_id: workspace_id.to_owned(),
                paths: watched,
            }));
        }
        effects
    }
}

impl ProjectTrees {
    fn tree(&mut self, workspace_id: &str) -> &mut Tree {
        self.trees
            .entry(workspace_id.to_owned())
            .or_insert_with(Tree::new)
    }

    /// The rows the panel draws.
    pub fn rows(&self, workspace_id: &str) -> Vec<Row> {
        self.trees
            .get(workspace_id)
            .map_or_else(Vec::new, |known| known.tree.rows())
    }

    pub fn selection(&self, workspace_id: &str) -> Option<&str> {
        self.trees.get(workspace_id)?.tree.selection.as_deref()
    }

    pub fn show_ignored(&self, workspace_id: &str) -> bool {
        self.trees
            .get(workspace_id)
            .is_some_and(|known| known.tree.show_ignored())
    }

    /// Asks for what the tree on screen still lacks.
    pub fn sync(&mut self, workspace_id: &str) -> Vec<Effect> {
        self.tree(workspace_id).sync(workspace_id)
    }

    pub fn select(&mut self, workspace_id: &str, path: &str) {
        self.tree(workspace_id).tree.selection = Some(path.to_owned());
    }

    /// Opens or closes a folder, and selects it.
    pub fn toggle(&mut self, workspace_id: &str, path: &str) -> Vec<Effect> {
        let known = self.tree(workspace_id);
        known.tree.selection = Some(path.to_owned());
        known.tree.toggle(path);
        known.sync(workspace_id)
    }

    pub fn key(&mut self, workspace_id: &str, key: Key) -> Vec<Effect> {
        let known = self.tree(workspace_id);
        known.tree.handle(key);
        known.sync(workspace_id)
    }

    pub fn toggle_ignored(&mut self, workspace_id: &str) -> Vec<Effect> {
        let known = self.tree(workspace_id);
        let show = !known.tree.show_ignored();
        known.tree.set_show_ignored(show);
        known.view += 1;
        known.watched = None;
        known.sync(workspace_id)
    }

    /// Opens the folders above `path` and selects it.
    pub fn reveal(&mut self, workspace_id: &str, path: &str) -> Vec<Effect> {
        let known = self.tree(workspace_id);
        known.tree.reveal(path);
        known.sync(workspace_id)
    }

    /// The engine listed a folder.
    pub fn listed(&mut self, listing: &ProjectTreeListResult) -> Vec<Effect> {
        let known = self.tree(&listing.workspace_id);
        // An answer of another view, or for a folder that is not open any more, is not used.
        let asked = known.loading.remove(&listing.path);
        if asked.is_none_or(|view| view == known.view) && known.tree.wants(&listing.path) {
            known.tree.apply(listing);
        }
        known.sync(&listing.workspace_id)
    }

    /// A folder could not be listed. One that vanished before it was listed simply closes.
    pub fn list_failed(&mut self, workspace_id: &str, path: &str) -> Vec<Effect> {
        let known = self.tree(workspace_id);
        known.loading.remove(path);
        known.tree.collapse(path);
        known.sync(workspace_id)
    }

    /// The engine reported folders changed on disk: the ones on screen are listed again.
    pub fn changed(&mut self, workspace_id: &str, paths: &[String]) -> Vec<Effect> {
        let known = self.tree(workspace_id);
        let show_ignored = known.tree.show_ignored();
        let watched = known.tree.watched_directories();
        let mut effects = Vec::new();
        for path in paths.iter().filter(|path| watched.contains(path)) {
            if known.loading.insert(path.clone(), known.view) != Some(known.view) {
                effects.push(send(EditorCommand::List {
                    workspace_id: workspace_id.to_owned(),
                    path: path.clone(),
                    show_ignored,
                }));
            }
        }
        effects
    }

    /// A new connection: the engine's watches are gone, so every tree is listed again.
    pub fn reconnect(&mut self) -> Vec<Effect> {
        let mut effects = Vec::new();
        for (workspace_id, known) in &mut self.trees {
            known.loading.clear();
            known.watched = None;
            known.tree.forget_listings();
            effects.extend(known.sync(workspace_id));
        }
        effects
    }

    pub fn forget(&mut self, workspace_id: &str) {
        self.trees.remove(workspace_id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::listing;

    fn commands(effects: Vec<Effect>) -> Vec<EditorCommand> {
        effects
            .into_iter()
            .filter_map(|effect| match effect {
                Effect::Send(Command::Editor(command)) => Some(command),
                _ => None,
            })
            .collect()
    }

    fn list(path: &str, show_ignored: bool) -> EditorCommand {
        EditorCommand::List {
            workspace_id: "w".into(),
            path: path.into(),
            show_ignored,
        }
    }

    #[test]
    fn a_tree_asks_for_the_root_once_and_then_watches_what_it_lists() {
        let mut trees = ProjectTrees::default();
        assert_eq!(commands(trees.sync("w")), [list("", false)]);
        assert!(commands(trees.sync("w")).is_empty());
        let after = commands(trees.listed(&listing("", &[("src", true), ("a.rs", false)])));
        assert_eq!(
            after,
            [EditorCommand::Watch {
                workspace_id: "w".into(),
                paths: vec![String::new()]
            }]
        );
        assert_eq!(trees.rows("w").len(), 2);
    }

    #[test]
    fn opening_a_folder_lists_it_and_the_watch_follows_the_listing() {
        let mut trees = ProjectTrees::default();
        trees.sync("w");
        trees.listed(&listing("", &[("src", true)]));
        assert_eq!(commands(trees.toggle("w", "src")), [list("src", false)]);
        assert_eq!(trees.selection("w"), Some("src"));
        let after = commands(trees.listed(&listing("src", &[("lib.rs", false)])));
        assert_eq!(
            after,
            [EditorCommand::Watch {
                workspace_id: "w".into(),
                paths: vec![String::new(), "src".into()]
            }]
        );
        // Closing it again leaves one folder to watch.
        let closed = commands(trees.toggle("w", "src"));
        assert_eq!(
            closed,
            [EditorCommand::Watch {
                workspace_id: "w".into(),
                paths: vec![String::new()]
            }]
        );
    }

    #[test]
    fn a_change_on_disk_lists_only_the_folders_on_screen() {
        let mut trees = ProjectTrees::default();
        trees.sync("w");
        trees.listed(&listing("", &[("src", true)]));
        let paths = ["".to_owned(), "elsewhere".to_owned()];
        assert_eq!(commands(trees.changed("w", &paths)), [list("", false)]);
        // Not again while that listing is on its way.
        assert!(commands(trees.changed("w", &paths)).is_empty());
    }

    #[test]
    fn showing_ignored_files_lists_the_tree_again() {
        let mut trees = ProjectTrees::default();
        trees.sync("w");
        // The toggle forgets what was asked; the new view asks again.
        assert_eq!(commands(trees.toggle_ignored("w")), [list("", true)]);
        assert!(trees.show_ignored("w"));
    }

    #[test]
    fn a_new_connection_lists_every_tree_again() {
        let mut trees = ProjectTrees::default();
        trees.sync("w");
        trees.listed(&listing("", &[("a.rs", false)]));
        assert_eq!(commands(trees.reconnect()), [list("", false)]);
    }

    #[test]
    fn a_listing_of_a_folder_that_closed_is_not_used() {
        let mut trees = ProjectTrees::default();
        trees.sync("w");
        trees.listed(&listing("", &[("src", true), ("a.rs", false)]));
        trees.toggle("w", "src");
        trees.toggle("w", "src");
        trees.listed(&listing("src", &[("lib.rs", false)]));
        assert_eq!(trees.rows("w").len(), 2);
    }

    #[test]
    fn a_listing_asked_for_before_ignored_files_were_shown_is_not_used() {
        let mut trees = ProjectTrees::default();
        trees.sync("w");
        trees.listed(&listing("", &[("src", true)]));
        // `src` is asked for in the old view; then the view changes and the root is asked again.
        assert_eq!(commands(trees.toggle("w", "src")), [list("src", false)]);
        assert_eq!(commands(trees.toggle_ignored("w")), [list("", true)]);
        // The answer for `src` is of the old view and is dropped.
        trees.listed(&listing("src", &[("old.rs", false)]));
        // When the root is listed, `src` is asked for again: nothing of the old answer is held.
        let after = commands(trees.listed(&listing("", &[("src", true)])));
        assert_eq!(after, [list("src", true)]);
        assert!(trees.rows("w").iter().all(|row| row.node.name != "old.rs"));
    }

    #[test]
    fn a_folder_that_cannot_be_listed_closes() {
        let mut trees = ProjectTrees::default();
        trees.sync("w");
        trees.listed(&listing("", &[("gone", true)]));
        trees.toggle("w", "gone");
        trees.list_failed("w", "gone");
        assert!(trees.rows("w").iter().all(|row| !row.expanded));
    }
}
