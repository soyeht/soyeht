//! The project panel's tree as a value (`EditorProjectTree` on the Mac; its key rules are in
//! `contracts/vectors/editor.json`, which both clients pass): the folders the engine has listed,
//! which of them are open, and the selected entry. It holds no I/O.

use soyeht_protocol::{ProjectEntryKind, ProjectTreeListResult};
use std::collections::{BTreeSet, HashMap};

/// The empty path names the workspace root.
pub const ROOT: &str = "";

#[derive(Debug, Clone, PartialEq)]
pub struct Node {
    pub name: String,
    pub path: String,
    pub is_directory: bool,
    pub ignored: bool,
}

/// An entry as the panel draws it.
#[derive(Debug, Clone, PartialEq)]
pub struct Row {
    pub node: Node,
    pub depth: usize,
    pub expanded: bool,
}

/// Keys the tree answers the way Zed's project panel does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Key {
    Up,
    Down,
    Left,
    Right,
    First,
    Last,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ProjectTree {
    children: HashMap<String, Vec<Node>>,
    expanded: BTreeSet<String>,
    pub selection: Option<String>,
    show_ignored: bool,
}

impl Default for ProjectTree {
    fn default() -> Self {
        Self {
            children: HashMap::new(),
            expanded: BTreeSet::from([ROOT.to_owned()]),
            selection: None,
            show_ignored: false,
        }
    }
}

pub fn parent_of(path: &str) -> &str {
    path.rsplit_once('/').map_or(ROOT, |(parent, _)| parent)
}

impl ProjectTree {
    pub fn show_ignored(&self) -> bool {
        self.show_ignored
    }

    /// The rows in display order: each open, listed folder's entries follow it, one level deeper.
    pub fn rows(&self) -> Vec<Row> {
        let mut rows = Vec::new();
        self.visit(ROOT, 0, &mut rows);
        rows
    }

    fn visit(&self, directory: &str, depth: usize, rows: &mut Vec<Row>) {
        for node in self.children.get(directory).into_iter().flatten() {
            let open = node.is_directory && self.expanded.contains(&node.path);
            rows.push(Row {
                node: node.clone(),
                depth,
                expanded: open,
            });
            if open {
                self.visit(&node.path, depth + 1, rows);
            }
        }
    }

    /// A folder is reachable when it is the root or every folder above it is open.
    fn is_reachable(&self, directory: &str) -> bool {
        let mut current = directory;
        while current != ROOT {
            if !self.expanded.contains(current) {
                return false;
            }
            current = parent_of(current);
        }
        true
    }

    /// Whether the listing of `directory` is of use: it is the root or a folder that is open on
    /// screen.
    pub fn wants(&self, directory: &str) -> bool {
        directory == ROOT || (self.expanded.contains(directory) && self.is_reachable(directory))
    }

    /// Folders whose listing is on screen; the client watches exactly these.
    pub fn watched_directories(&self) -> Vec<String> {
        self.expanded
            .iter()
            .filter(|directory| {
                self.children.contains_key(*directory) && self.is_reachable(directory)
            })
            .cloned()
            .collect()
    }

    /// Open folders on screen that still need a listing; a nested folder waits until its parent's
    /// listing shows it.
    pub fn directories_to_load(&self) -> Vec<String> {
        self.expanded
            .iter()
            .filter(|directory| {
                !self.children.contains_key(*directory)
                    && self.is_reachable(directory)
                    && (directory.as_str() == ROOT
                        || self
                            .node_at(directory)
                            .is_some_and(|node| node.is_directory))
            })
            .cloned()
            .collect()
    }

    pub fn node_at(&self, path: &str) -> Option<&Node> {
        self.children
            .get(parent_of(path))?
            .iter()
            .find(|node| node.path == path)
    }

    /// Stores one folder listing. Entries that disappeared take their open descendants and the
    /// selection with them.
    pub fn apply(&mut self, listing: &ProjectTreeListResult) {
        let nodes: Vec<Node> = listing
            .entries
            .iter()
            .map(|entry| Node {
                name: entry.name.clone(),
                path: entry.path.clone(),
                is_directory: entry.kind == ProjectEntryKind::Directory,
                ignored: entry.ignored,
            })
            .collect();
        let gone: Vec<String> = self
            .children
            .get(&listing.path)
            .into_iter()
            .flatten()
            .filter(|old| !nodes.iter().any(|node| node.path == old.path))
            .map(|old| old.path.clone())
            .collect();
        for path in gone {
            self.forget(&path);
        }
        self.children.insert(listing.path.clone(), nodes);
    }

    /// Opens a folder and says whether it still has to be listed.
    pub fn expand(&mut self, directory: &str) -> bool {
        self.expanded.insert(directory.to_owned());
        !self.children.contains_key(directory)
    }

    /// Closes a folder. Nested folders stay open for when it opens again, but their listings go:
    /// nothing watches a hidden folder, so they would be stale.
    pub fn collapse(&mut self, directory: &str) {
        if directory == ROOT {
            return;
        }
        self.expanded.remove(directory);
        let prefix = format!("{directory}/");
        self.children
            .retain(|path, _| path != directory && !path.starts_with(&prefix));
    }

    /// Opens a closed folder (and says whether it needs a listing), closes an open one.
    pub fn toggle(&mut self, directory: &str) -> bool {
        if directory != ROOT && self.expanded.contains(directory) {
            self.collapse(directory);
            return false;
        }
        self.expand(directory)
    }

    /// Switching ignored files forgets every listing; the open folders are listed again.
    pub fn set_show_ignored(&mut self, show: bool) {
        if show != self.show_ignored {
            self.show_ignored = show;
            self.children.clear();
        }
    }

    /// Forgets every listing, as after a new connection: the open folders are listed again.
    pub fn forget_listings(&mut self) {
        self.children.clear();
    }

    /// Opens every folder above `path` and selects it.
    pub fn reveal(&mut self, path: &str) {
        let mut directory = parent_of(path);
        while directory != ROOT {
            self.expanded.insert(directory.to_owned());
            directory = parent_of(directory);
        }
        self.selection = Some(path.to_owned());
    }

    /// Moves the selection or opens and closes folders. Returns a folder that must be listed
    /// before the move can show its entries.
    pub fn handle(&mut self, key: Key) -> Option<String> {
        let rows = self.rows();
        if rows.is_empty() {
            return None;
        }
        let index = self
            .selection
            .as_ref()
            .and_then(|path| rows.iter().position(|row| &row.node.path == path));
        let select = |tree: &mut Self, at: usize| {
            tree.selection = rows.get(at).map(|row| row.node.path.clone());
        };
        match key {
            Key::First => select(self, 0),
            Key::Last => select(self, rows.len() - 1),
            Key::Up => select(self, index.unwrap_or(rows.len()).saturating_sub(1)),
            Key::Down => select(self, index.map_or(0, |at| (at + 1).min(rows.len() - 1))),
            Key::Right => {
                let row = rows.get(index?)?;
                if !row.node.is_directory {
                    return None;
                }
                if !row.expanded {
                    return self.expand(&row.node.path).then(|| row.node.path.clone());
                }
                if let Some(next) = rows.get(index? + 1).filter(|next| next.depth > row.depth) {
                    self.selection = Some(next.node.path.clone());
                }
            }
            Key::Left => {
                let row = rows.get(index?)?;
                if row.expanded {
                    self.collapse(&row.node.path);
                } else {
                    let parent = parent_of(&row.node.path);
                    if parent != ROOT {
                        self.selection = Some(parent.to_owned());
                    }
                }
            }
        }
        None
    }

    fn forget(&mut self, path: &str) {
        let prefix = format!("{path}/");
        self.expanded
            .retain(|open| open != path && !open.starts_with(&prefix));
        self.children
            .retain(|listed, _| listed != path && !listed.starts_with(&prefix));
        if self
            .selection
            .as_ref()
            .is_some_and(|selected| selected == path || selected.starts_with(&prefix))
        {
            self.selection = None;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::{listing, vector};
    use serde_json::Value;

    fn paths(tree: &ProjectTree) -> Vec<String> {
        tree.rows().into_iter().map(|row| row.node.path).collect()
    }

    fn loaded() -> ProjectTree {
        let mut tree = ProjectTree::default();
        tree.apply(&listing("", &[("src", true), ("README.md", false)]));
        tree.expand("src");
        tree.apply(&listing("src", &[("ui", true), ("lib.rs", false)]));
        tree
    }

    #[test]
    fn the_tree_lists_only_open_folders_and_asks_for_missing_listings() {
        let mut tree = ProjectTree::default();
        assert_eq!(tree.directories_to_load(), [ROOT]);
        tree = loaded();
        assert_eq!(paths(&tree), ["src", "src/ui", "src/lib.rs", "README.md"]);
        assert_eq!(
            tree.rows().iter().map(|row| row.depth).collect::<Vec<_>>(),
            [0, 1, 1, 0]
        );
        assert!(tree.directories_to_load().is_empty());
        assert_eq!(tree.watched_directories(), [ROOT, "src"]);

        assert!(tree.toggle("src/ui"));
        assert_eq!(tree.directories_to_load(), ["src/ui"]);
        tree.apply(&listing("src/ui", &[("view.rs", false)]));
        assert_eq!(
            paths(&tree),
            ["src", "src/ui", "src/ui/view.rs", "src/lib.rs", "README.md"]
        );

        tree.collapse("src");
        assert_eq!(paths(&tree), ["src", "README.md"]);
        // A closed folder is no longer watched, so opening it lists it again and restores the
        // nested folder that was open.
        assert_eq!(tree.watched_directories(), [ROOT]);
        assert!(tree.expand("src"));
        tree.apply(&listing("src", &[("ui", true), ("lib.rs", false)]));
        assert_eq!(tree.directories_to_load(), ["src/ui"]);
        tree.collapse(ROOT);
        assert!(!tree.rows().is_empty());
    }

    #[test]
    fn a_new_listing_drops_vanished_entries_with_their_state() {
        let mut tree = loaded();
        tree.expand("src/ui");
        tree.apply(&listing("src/ui", &[("view.rs", false)]));
        tree.selection = Some("src/ui/view.rs".into());
        tree.apply(&listing("src", &[("lib.rs", false), ("main.rs", false)]));
        assert_eq!(
            paths(&tree),
            ["src", "src/lib.rs", "src/main.rs", "README.md"]
        );
        assert_eq!(tree.selection, None);
        assert_eq!(tree.directories_to_load(), Vec::<String>::new());
        assert!(!tree.watched_directories().contains(&"src/ui".to_owned()));
    }

    #[test]
    fn revealing_a_file_opens_every_folder_above_it_and_selects_it() {
        let mut tree = ProjectTree::default();
        tree.apply(&listing("", &[("src", true)]));
        tree.reveal("src/ui/view.rs");
        assert_eq!(tree.selection.as_deref(), Some("src/ui/view.rs"));
        assert_eq!(tree.directories_to_load(), ["src"]);
        assert_eq!(parent_of("src/ui/view.rs"), "src/ui");
        assert_eq!(parent_of("README.md"), ROOT);
    }

    #[test]
    fn showing_ignored_files_forgets_every_listing() {
        let mut tree = loaded();
        tree.set_show_ignored(true);
        assert!(tree.show_ignored());
        assert!(tree.rows().is_empty());
        assert_eq!(tree.directories_to_load(), [ROOT]);
        tree.forget_listings();
    }

    #[test]
    fn the_keys_move_the_selection_as_the_vector_says() {
        let vector = &vector("editor")["projectTree"];
        let mut tree = ProjectTree::default();
        let list = |tree: &mut ProjectTree, directory: &str| {
            let entries: Vec<(String, bool)> = vector["listings"][directory]
                .as_array()
                .unwrap()
                .iter()
                .map(|entry| {
                    (
                        entry[0].as_str().unwrap().to_owned(),
                        entry[1].as_bool().unwrap(),
                    )
                })
                .collect();
            let refs: Vec<(&str, bool)> = entries
                .iter()
                .map(|(name, folder)| (name.as_str(), *folder))
                .collect();
            tree.apply(&listing(directory, &refs));
        };
        list(&mut tree, "");
        for directory in vector["expanded"].as_array().unwrap() {
            let directory = directory.as_str().unwrap();
            tree.expand(directory);
            list(&mut tree, directory);
        }
        let wanted = |value: &Value| -> Vec<String> {
            value
                .as_array()
                .unwrap()
                .iter()
                .map(|path| path.as_str().unwrap().to_owned())
                .collect()
        };
        assert_eq!(paths(&tree), wanted(&vector["rows"]));
        for step in vector["steps"].as_array().unwrap() {
            let key = match step["key"].as_str().unwrap() {
                "up" => Key::Up,
                "down" => Key::Down,
                "left" => Key::Left,
                "right" => Key::Right,
                "first" => Key::First,
                _ => Key::Last,
            };
            let loads = tree.handle(key);
            assert_eq!(loads.as_deref(), step["loads"].as_str(), "{step}");
            assert_eq!(
                tree.selection.as_deref(),
                step["selection"].as_str(),
                "{step}"
            );
            if step.get("rows").is_some() {
                assert_eq!(paths(&tree), wanted(&step["rows"]), "{step}");
            }
        }
    }
}
