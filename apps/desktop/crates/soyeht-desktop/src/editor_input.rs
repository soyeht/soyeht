//! What the person does in an editor pane: keys, the mouse, the wheel, the tabs and the project
//! panel. Each turns into a change of the replica (`soyeht_client::editor`), which answers with
//! what to send the engine; this only carries it out.

use crate::{
    editor_ui::EditorUi,
    editor_view::{Frame, columns_in, rows},
    terminal_view::CellSize,
    theme::editor,
    workbench::Workbench,
};
use gpui::{
    Bounds, ClickEvent, ClipboardItem, Context, KeyDownEvent, MouseButton, MouseDownEvent,
    MouseMoveEvent, MouseUpEvent, Pixels, Point, ScrollWheelEvent, Window, px,
};
use soyeht_client::{
    Effect,
    editor::{
        keys::{Action, Chord, action},
        selection::Motion,
        text::{cells_before, column_at_cell, line_cells},
        tree::Key as TreeKey,
    },
};
use soyeht_protocol::{FileFindQuery, PaneMode, TextPosition, TextRange};

impl Workbench {
    /// The editor pane that has the keyboard, when the focused pane is one.
    pub(crate) fn focused_editor(&self) -> Option<String> {
        self.state
            .focused_pane()
            .filter(|pane| pane.mode == PaneMode::Editor)
            .map(|pane| pane.pane_id.clone())
    }

    /// Carries out what the replica asks for.
    pub(crate) fn run_effects(&mut self, effects: Vec<Effect>) {
        for effect in effects {
            self.run_effect(effect);
        }
    }

    /// Whether the find bar of the pane is open and has the keys.
    fn finding(&self, pane_id: &str) -> bool {
        self.panes
            .get(pane_id)
            .is_some_and(|ui| ui.editor.find.open && ui.editor.find.focused)
    }

    fn workspace_of(&self, pane_id: &str) -> Option<String> {
        self.state
            .pane(pane_id)
            .map(|pane| pane.workspace_id.clone())
    }

    /// The lines in view, copied out of the replica for painting; the scroll follows the caret
    /// when it moved, and the replica is told which lines are in view so that it reads the page
    /// that holds them in time.
    pub(crate) fn editor_frame(
        &mut self,
        pane_id: &str,
        bounds: Bounds<Pixels>,
        cell: CellSize,
        _: &mut Context<Self>,
    ) -> Option<Frame> {
        let (cell_width, line) = (f32::from(cell.width), f32::from(cell.height));
        let in_view = (f32::from(bounds.size.height) / line).ceil() as u32 + 1;
        let columns = columns_in(
            bounds.size.width - px(editor::GUTTER_WIDTH + 2.0 * editor::TEXT_INSET),
            cell,
        );
        let file = self.state.editors.file_of_pane_mut(pane_id)?;
        let wants_reveal = file.take_reveal();
        let head = file.selection.head.clone();
        let lines = file.lines;
        let head_cell = file
            .page
            .as_ref()
            .map(|page| cells_before(page.text(head.line), head.column));
        let ui = &mut self.panes.of(pane_id).editor;
        ui.bounds = Some(bounds);
        ui.rows = in_view;
        ui.columns = u32::try_from(columns).unwrap_or(u32::MAX);
        ui.cell = (cell_width, line);
        if let (true, Some(at)) = (wants_reveal, head_cell) {
            ui.reveal(head.line, at);
        }
        ui.clamp(lines, ui.longest);
        let (top, left) = (ui.top, ui.left);
        let first_line = ui.first_line();
        let find_open = ui.find.open;
        let requests = self.state.editors.show(pane_id, first_line, in_view);
        self.run_effects(requests);

        let file = self.state.editors.file_of_pane(pane_id)?;
        let page = file.page.as_ref()?;
        let first_cell = (left / cell_width) as usize;
        let matches: &[TextRange] = if find_open {
            file.search
                .result
                .as_ref()
                .map_or(&[], |result| result.visible.as_slice())
        } else {
            &[]
        };
        let shown = rows(
            page,
            &file.selection,
            matches,
            (first_line, in_view, lines),
            (first_cell, columns),
        );
        let longest = (first_line..first_line.saturating_add(in_view))
            .filter_map(|line| page.line(line))
            .map(|line| line_cells(&line.text))
            .max()
            .unwrap_or(0);
        let caret = (page.holds(head.line) && head.line >= first_line)
            .then(|| {
                (
                    usize::try_from(head.line - first_line).unwrap_or(usize::MAX),
                    head_cell.unwrap_or(0),
                )
            })
            .filter(|(row, _)| *row < shown.len());
        let frame = Frame {
            cell,
            rows: shown,
            caret,
            scroll: (top, f32::from(bounds.size.height)),
            content: EditorUi::content_height(lines),
            first_cell,
            offset: (left, top - first_line as f32 * line),
        };
        self.panes.of(pane_id).editor.longest = longest;
        Some(frame)
    }

    // Keys.

    /// A key pressed while an editor pane has the keyboard. False when it means nothing to the
    /// editor.
    pub(crate) fn editor_key(
        &mut self,
        pane_id: &str,
        event: &KeyDownEvent,
        cx: &mut Context<Self>,
    ) -> bool {
        let keystroke = &event.keystroke;
        let chord = Chord {
            key: &keystroke.key,
            text: keystroke.key_char.as_deref(),
            control: keystroke.modifiers.control,
            alt: keystroke.modifiers.alt,
            shift: keystroke.modifiers.shift,
            platform: keystroke.modifiers.platform,
        };
        let tree_has_it = self
            .panes
            .get(pane_id)
            .is_some_and(|ui| ui.editor.tree_focused);
        if tree_has_it && self.editor_tree_key(pane_id, &chord, cx) {
            return true;
        }
        let Some(act) = action(&chord) else {
            return false;
        };
        let finding = self.finding(pane_id);
        if finding {
            return self.find_key(pane_id, act, chord.shift, cx);
        }
        self.editor_action(pane_id, act, cx);
        true
    }

    pub(crate) fn editor_action(&mut self, pane_id: &str, act: Action, cx: &mut Context<Self>) {
        let editors = &mut self.state.editors;
        let effects = match act {
            Action::Type(text) => editors.insert(pane_id, &text),
            Action::Backspace => editors.delete(pane_id, true),
            Action::Delete => editors.delete(pane_id, false),
            Action::DeleteWord { backward } => editors.delete_word(pane_id, backward),
            Action::Move { motion, extend } => editors.motion(pane_id, motion, extend),
            Action::Page { up, extend } => {
                let rows = self
                    .panes
                    .get(pane_id)
                    .map_or(1, |ui| ui.editor.rows.saturating_sub(2).max(1));
                let motion = if up {
                    Motion::PageUp(rows)
                } else {
                    Motion::PageDown(rows)
                };
                editors.motion(pane_id, motion, extend)
            }
            Action::SelectAll => editors.select_all(pane_id),
            Action::Copy => {
                if let Some(text) = editors.copy(pane_id) {
                    cx.write_to_clipboard(ClipboardItem::new_string(text));
                }
                Vec::new()
            }
            Action::Cut => {
                let (text, effects) = editors.cut(pane_id);
                if let Some(text) = text {
                    cx.write_to_clipboard(ClipboardItem::new_string(text));
                }
                effects
            }
            Action::Paste => match cx.read_from_clipboard().and_then(|item| item.text()) {
                Some(text) => self.state.editor_paste(pane_id, &text),
                None => Vec::new(),
            },
            Action::Undo => editors.replay(pane_id, false),
            Action::Redo => editors.replay(pane_id, true),
            Action::Save => editors.save(pane_id),
            Action::Find => {
                self.open_find(pane_id);
                Vec::new()
            }
            Action::FindNext => self.step_find(pane_id, true),
            Action::FindPrevious => self.step_find(pane_id, false),
            Action::Escape => {
                self.close_find(pane_id);
                Vec::new()
            }
            Action::ToggleProjectPanel => {
                self.toggle_project_panel(pane_id, cx);
                Vec::new()
            }
            Action::CloseTab => self.state.editor_close_tab(pane_id),
            Action::NextTab => {
                self.cycle_tab(pane_id, true);
                Vec::new()
            }
            Action::PreviousTab => {
                self.cycle_tab(pane_id, false);
                Vec::new()
            }
        };
        if let Some(ui) = self.panes.get_mut(pane_id) {
            ui.editor.tree_focused = false;
        }
        self.run_effects(effects);
        cx.notify();
    }

    /// Pasted text goes where the keys go: the find bar when it has them, else the text.
    pub(crate) fn editor_paste(&mut self, pane_id: &str, text: &str, cx: &mut Context<Self>) {
        let finding = self.finding(pane_id);
        if finding {
            let line = text.lines().next().unwrap_or_default().to_owned();
            self.panes.of(pane_id).editor.find.query.push_str(&line);
            self.search_changed(pane_id);
        } else {
            let effects = self.state.editor_paste(pane_id, text);
            self.run_effects(effects);
        }
        cx.notify();
    }

    fn cycle_tab(&mut self, pane_id: &str, forward: bool) {
        if let Some(workspace) = self.workspace_of(pane_id) {
            self.state.editor_cycle(&workspace, forward);
            self.announce_focus();
            self.reveal = self.state.focused_pane().map(|pane| pane.pane_id.clone());
        }
    }

    // The find bar.

    pub(crate) fn open_find(&mut self, pane_id: &str) {
        let seed = self
            .state
            .editors
            .copy(pane_id)
            .filter(|text| !text.contains('\n'));
        let find = &mut self.panes.of(pane_id).editor.find;
        find.open = true;
        find.focused = true;
        if let Some(text) = seed {
            find.query = text;
        }
        self.search_changed(pane_id);
    }

    pub(crate) fn close_find(&mut self, pane_id: &str) {
        let ui = &mut self.panes.of(pane_id).editor;
        ui.find.open = false;
        ui.find.focused = false;
        if let Some(file) = self.state.editors.file_of_pane_mut(pane_id) {
            file.stop_search();
        }
    }

    fn query(&self, pane_id: &str) -> Option<FileFindQuery> {
        let find = &self.panes.get(pane_id)?.editor.find;
        (!find.query.is_empty()).then(|| FileFindQuery {
            text: find.query.clone(),
            case_sensitive: find.case_sensitive,
            whole_word: find.whole_word,
            regex: find.regex,
        })
    }

    /// The query changed: the first match from where the selection starts.
    pub(crate) fn search_changed(&mut self, pane_id: &str) {
        let Some(query) = self.query(pane_id) else {
            if let Some(file) = self.state.editors.file_of_pane_mut(pane_id) {
                file.stop_search();
            }
            return;
        };
        let effects = self
            .state
            .editors
            .file_of_pane_mut(pane_id)
            .map(|file| {
                let from = file.selection.range().start;
                file.find(query, from, true, true)
            })
            .unwrap_or_default();
        self.run_effects(effects);
    }

    /// The next match, from the button of the bar.
    pub(crate) fn find_next(&mut self, pane_id: &str, cx: &mut Context<Self>) {
        let effects = self.step_find(pane_id, true);
        self.run_effects(effects);
        cx.notify();
    }

    /// The previous match, from the button of the bar.
    pub(crate) fn find_previous(&mut self, pane_id: &str, cx: &mut Context<Self>) {
        let effects = self.step_find(pane_id, false);
        self.run_effects(effects);
        cx.notify();
    }

    /// Closes the bar, from its button.
    pub(crate) fn find_close(&mut self, pane_id: &str, cx: &mut Context<Self>) {
        self.close_find(pane_id);
        cx.notify();
    }

    /// A click in the field gives it the keys.
    pub(crate) fn find_focus(&mut self, pane_id: &str, cx: &mut Context<Self>) {
        self.panes.of(pane_id).editor.find.focused = true;
        cx.notify();
    }

    /// The next (or previous) match after the selection, wrapping at the end of the file.
    pub(crate) fn step_find(&mut self, pane_id: &str, forward: bool) -> Vec<Effect> {
        let Some(query) = self.query(pane_id) else {
            self.open_find(pane_id);
            return Vec::new();
        };
        self.state
            .editors
            .file_of_pane_mut(pane_id)
            .map(|file| {
                let range = file.selection.range();
                let from = if forward { range.end } else { range.start };
                file.find(query, from, forward, true)
            })
            .unwrap_or_default()
    }

    fn find_key(
        &mut self,
        pane_id: &str,
        act: Action,
        shift: bool,
        cx: &mut Context<Self>,
    ) -> bool {
        match act {
            Action::Type(text) if text == "\n" => {
                let effects = self.step_find(pane_id, !shift);
                self.run_effects(effects);
            }
            Action::Type(text) if text != "\t" => {
                self.panes.of(pane_id).editor.find.query.push_str(&text);
                self.search_changed(pane_id);
            }
            Action::Backspace => {
                self.panes.of(pane_id).editor.find.query.pop();
                self.search_changed(pane_id);
            }
            Action::Escape => self.close_find(pane_id),
            Action::Paste => {
                if let Some(text) = cx.read_from_clipboard().and_then(|item| item.text()) {
                    self.editor_paste(pane_id, &text, cx);
                }
            }
            Action::FindNext | Action::FindPrevious | Action::Find => {
                let forward = act != Action::FindPrevious;
                let effects = self.step_find(pane_id, forward);
                self.run_effects(effects);
            }
            Action::Save | Action::CloseTab | Action::NextTab | Action::PreviousTab => {
                self.editor_action(pane_id, act, cx);
            }
            _ => {}
        }
        cx.notify();
        true
    }

    // Tabs and the project panel.

    pub(crate) fn toggle_project_panel(&mut self, pane_id: &str, cx: &mut Context<Self>) {
        if let Some(workspace) = self.workspace_of(pane_id)
            && !self.project_panel_hidden.remove(&workspace)
        {
            self.project_panel_hidden.insert(workspace);
        }
        cx.notify();
    }

    pub(crate) fn close_tab(&mut self, pane_id: &str, cx: &mut Context<Self>) {
        let effects = self.state.editor_close_tab(pane_id);
        self.run_effects(effects);
        cx.notify();
    }

    /// A click on a tab goes to it; the second click of a double click keeps a preview.
    pub(crate) fn tab_clicked(
        &mut self,
        pane_id: &str,
        event: &ClickEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let kept = self
            .state
            .editors
            .file_of_pane(pane_id)
            .map(|file| (file.doc.workspace_id.clone(), file.doc.path.clone()));
        if let (true, Some((workspace, path))) = (event.click_count() >= 2, kept) {
            self.keep_tab(&workspace, &path, cx);
        }
        self.focus_pane(pane_id, window, cx);
    }

    /// A tab that is a preview becomes a permanent one.
    pub(crate) fn keep_tab(&mut self, workspace_id: &str, path: &str, cx: &mut Context<Self>) {
        let effects = self.state.editor_open(workspace_id, path, false);
        self.run_effects(effects);
        cx.notify();
    }

    /// Whether `path` is a folder of the project panel of `workspace_id`.
    fn is_folder(&self, workspace_id: &str, path: &str) -> bool {
        self.state
            .editors
            .trees
            .rows(workspace_id)
            .iter()
            .any(|row| row.node.path == path && row.node.is_directory)
    }

    /// A press in the project panel gives it the keys that move through it.
    pub(crate) fn tree_pressed(&mut self, owner: &str, _: &mut Context<Self>) {
        if let Some(ui) = self.panes.get_mut(owner) {
            ui.editor.tree_focused = true;
        }
    }

    /// A click on an entry of the project panel: a folder opens or closes, a file is previewed,
    /// and kept by a second click.
    pub(crate) fn tree_clicked(
        &mut self,
        path: &str,
        event: &ClickEvent,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(workspace) = self.state.selected_workspace_id() else {
            return;
        };
        if let Some(pane_id) = self.focused_editor() {
            self.panes.of(&pane_id).editor.tree_focused = true;
        }
        let effects = if self.is_folder(&workspace, path) {
            self.state.editors.trees.toggle(&workspace, path)
        } else {
            self.state.editors.trees.select(&workspace, path);
            self.state
                .editor_open(&workspace, path, event.click_count() < 2)
        };
        self.run_effects(effects);
        cx.notify();
    }

    pub(crate) fn toggle_ignored(&mut self, workspace_id: &str, cx: &mut Context<Self>) {
        let effects = self.state.editors.trees.toggle_ignored(workspace_id);
        self.run_effects(effects);
        cx.notify();
    }

    /// Opens the folders above the file of the active tab and selects it.
    pub(crate) fn reveal_in_tree(&mut self, _: &str, cx: &mut Context<Self>) {
        let Some(workspace) = self.state.selected_workspace_id() else {
            return;
        };
        let focused = self.state.focused_pane().map(|pane| pane.pane_id.clone());
        let active = self
            .state
            .editors
            .active_pane(&workspace, focused.as_deref(), self.state.panes())
            .and_then(|pane| self.state.editors.file_of_pane(&pane))
            .map(|file| file.doc.path.clone());
        if let Some(path) = active {
            let effects = self.state.editors.trees.reveal(&workspace, &path);
            self.run_effects(effects);
            cx.notify();
        }
    }

    /// The project panel's keys, as Zed's panel answers them. False for a key it has no use for.
    fn editor_tree_key(&mut self, pane_id: &str, chord: &Chord, cx: &mut Context<Self>) -> bool {
        let Some(workspace) = self.workspace_of(pane_id) else {
            return false;
        };
        if chord.control || chord.platform || chord.alt {
            return false;
        }
        let key = match chord.key {
            "up" => Some(TreeKey::Up),
            "down" => Some(TreeKey::Down),
            "left" => Some(TreeKey::Left),
            "right" => Some(TreeKey::Right),
            "home" => Some(TreeKey::First),
            "end" => Some(TreeKey::Last),
            _ => None,
        };
        let effects = if let Some(key) = key {
            self.state.editors.trees.key(&workspace, key)
        } else if matches!(chord.key, "enter" | "space") {
            let preview = chord.key == "space";
            let Some(path) = self
                .state
                .editors
                .trees
                .selection(&workspace)
                .map(str::to_owned)
            else {
                return true;
            };
            if self.is_folder(&workspace, &path) {
                self.state.editors.trees.toggle(&workspace, &path)
            } else {
                self.state.editor_open(&workspace, &path, preview)
            }
        } else {
            return false;
        };
        self.run_effects(effects);
        cx.notify();
        true
    }

    // The mouse.

    /// The place in the document under a point of the window, when the page holds that line.
    fn editor_position(&self, pane_id: &str, at: Point<Pixels>) -> Option<TextPosition> {
        let ui = &self.panes.get(pane_id)?.editor;
        let bounds = ui.bounds?;
        let page = self.state.editors.file_of_pane(pane_id)?.page.as_ref()?;
        let lines = self.state.editors.file_of_pane(pane_id)?.lines;
        let y = f32::from(at.y - bounds.top()) - editor::VIEWPORT_VERTICAL_INSET / 2.0 + ui.top;
        let line = ((y / editor::LINE_HEIGHT).floor().max(0.0) as u32).min(lines.saturating_sub(1));
        if !page.holds(line) {
            return None;
        }
        let x =
            f32::from(at.x - bounds.left()) - editor::GUTTER_WIDTH - editor::TEXT_INSET + ui.left;
        let cell = (x / ui.cell.0.max(1.0)).round().max(0.0) as usize;
        Some(TextPosition {
            line,
            column: column_at_cell(page.text(line), cell),
        })
    }

    pub(crate) fn editor_mouse_down(
        &mut self,
        pane_id: &str,
        event: &MouseDownEvent,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if event.button != MouseButton::Left {
            return;
        }
        let ui = &mut self.panes.of(pane_id).editor;
        ui.tree_focused = false;
        ui.find.focused = false;
        let Some(at) = self.editor_position(pane_id, event.position) else {
            return;
        };
        let effects = match event.click_count {
            0 | 1 => {
                self.panes.of(pane_id).editor.dragging = true;
                self.state.editors.click(pane_id, at, event.modifiers.shift)
            }
            2 => self.state.editors.select_around(pane_id, &at, false),
            _ => self.state.editors.select_around(pane_id, &at, true),
        };
        self.run_effects(effects);
        cx.notify();
    }

    pub(crate) fn editor_mouse_move(
        &mut self,
        pane_id: &str,
        event: &MouseMoveEvent,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let dragging = self.panes.get(pane_id).is_some_and(|ui| ui.editor.dragging);
        if !dragging || event.pressed_button != Some(MouseButton::Left) {
            if dragging {
                self.panes.of(pane_id).editor.dragging = false;
            }
            return;
        }
        if let Some(at) = self.editor_position(pane_id, event.position) {
            let effects = self.state.editors.click(pane_id, at, true);
            self.run_effects(effects);
            cx.notify();
        }
    }

    pub(crate) fn editor_mouse_up(
        &mut self,
        pane_id: &str,
        _: &MouseUpEvent,
        _: &mut Window,
        _: &mut Context<Self>,
    ) {
        self.panes.of(pane_id).editor.dragging = false;
    }

    /// The wheel scrolls the text; Shift (or a sideways wheel) scrolls it sideways.
    pub(crate) fn editor_scroll(
        &mut self,
        pane_id: &str,
        event: &ScrollWheelEvent,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(file) = self.state.editors.file_of_pane(pane_id) else {
            return;
        };
        let lines = file.lines;
        let delta = event.delta.pixel_delta(px(editor::LINE_HEIGHT));
        let ui = &mut self.panes.of(pane_id).editor;
        let (dx, dy) = (f32::from(delta.x), f32::from(delta.y));
        let (dx, dy) = if event.modifiers.shift {
            (dy, 0.0)
        } else {
            (dx, dy)
        };
        ui.top -= dy;
        ui.left -= dx;
        ui.clamp(lines, ui.longest);
        cx.notify();
    }
}
