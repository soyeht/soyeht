use super::*;

fn typed(text: &str) -> TextInput {
    let mut input = TextInput::default();
    input.insert(text);
    input
}

/// Types `text` the way the platform delivers it: one key at a time.
fn type_keys(input: &mut TextInput, text: &str) {
    for character in text.chars() {
        input.replace_in_range(None, &character.to_string());
    }
}

fn moves(input: &mut TextInput, motion: Motion) {
    input.move_by(motion, false);
}

fn grows(input: &mut TextInput, motion: Motion) {
    input.move_by(motion, true);
}

#[test]
fn typing_inserts_at_the_caret() {
    let mut input = typed("helo");
    moves(&mut input, Motion::Left);
    input.insert("l");
    assert_eq!(input.text(), "hello");
    assert_eq!(input.caret(), 4);
}

#[test]
fn backspace_and_delete_remove_whole_characters() {
    let mut input = typed("a\u{e9}\u{1f44b}");
    input.backspace();
    assert_eq!(input.text(), "a\u{e9}");
    moves(&mut input, Motion::Left);
    input.delete();
    assert_eq!(input.text(), "a");
    input.backspace();
    input.backspace();
    assert_eq!(input.text(), "");
    assert_eq!(input.caret(), 0);
}

#[test]
fn a_letter_with_its_accents_and_a_flag_are_one_step_for_the_caret() {
    // "e" and a combining acute, then the flag of Brazil (two regional indicators).
    let mut input = typed("e\u{301}\u{1f1e7}\u{1f1f7}");
    moves(&mut input, Motion::Left);
    assert_eq!(input.caret(), 3);
    input.backspace();
    assert_eq!(input.text(), "\u{1f1e7}\u{1f1f7}");
}

#[test]
fn the_caret_stays_inside_the_text() {
    let mut input = typed("ab");
    moves(&mut input, Motion::Right);
    assert_eq!(input.caret(), 2);
    moves(&mut input, Motion::LineStart);
    moves(&mut input, Motion::Left);
    assert_eq!(input.caret(), 0);
}

#[test]
fn home_and_end_follow_the_current_line_and_control_home_the_whole_text() {
    let mut input = typed("one\ntwo\nthree");
    moves(&mut input, Motion::Left);
    moves(&mut input, Motion::LineStart);
    assert_eq!(input.caret(), 8);
    moves(&mut input, Motion::LineEnd);
    assert_eq!(input.caret(), 13);
    moves(&mut input, Motion::DocumentStart);
    assert_eq!(input.caret(), 0);
    moves(&mut input, Motion::LineEnd);
    assert_eq!(input.caret(), 3);
    moves(&mut input, Motion::DocumentEnd);
    assert_eq!(input.caret(), 13);
}

#[test]
fn what_is_sent_is_trimmed_and_blank_text_is_not_sent() {
    let input = typed("  oi \n");
    assert_eq!(input.to_send().as_deref(), Some("oi"));
    assert_eq!(typed(" \n").to_send(), None);
    let mut cleared = input;
    cleared.clear();
    assert!(cleared.is_blank());
    assert_eq!(cleared.caret(), 0);
}

#[test]
fn offsets_convert_between_utf16_and_bytes() {
    // "a", "c with cedilla" (2 bytes, 1 unit), "tilde a" (2 bytes), "o", an emoji (4 bytes, 2 units)
    let input = typed("a\u{e7}\u{e3}o\u{1f44b}");
    assert_eq!(input.bytes_of(&(1..3)), 1..5);
    assert_eq!(input.units_of(&(5..10)), 3..6);
    assert_eq!(input.selection_utf16(), (6..6, false));
    assert_eq!(input.bytes_of(&(0..99)), 0..10);
}

#[test]
fn a_dead_key_composes_in_place_and_commits_without_extra_text() {
    let mut input = typed("a");
    // The platform marks "~" while the dead key waits, then replaces it with the letter.
    input.replace_and_mark(None, "~", None);
    assert_eq!(input.text(), "a~");
    assert_eq!(input.marked_utf16(), Some(1..2));
    assert!(input.is_composing());
    input.replace_in_range(None, "\u{e3}");
    assert_eq!(input.text(), "a\u{e3}");
    assert!(!input.is_composing());
    input.replace_in_range(None, "o");
    assert_eq!(input.text(), "a\u{e3}o");
    assert_eq!(input.caret(), 4);
}

#[test]
fn typing_replaces_an_explicit_utf16_range() {
    let mut input = typed("ao");
    input.replace_in_range(Some(1..2), "\u{e7}\u{e3}o");
    assert_eq!(input.text(), "a\u{e7}\u{e3}o");
    input.unmark();
    assert_eq!(input.marked_utf16(), None);
}

#[test]
fn the_caret_inside_composing_text_follows_the_selection() {
    let mut input = TextInput::default();
    input.replace_and_mark(None, "\u{3053}\u{3093}", Some(1..1));
    assert_eq!(input.caret(), 3);
}

#[test]
fn shift_and_a_motion_grow_the_selection_and_a_plain_arrow_collapses_it() {
    let mut input = typed("hello world");
    moves(&mut input, Motion::DocumentStart);
    grows(&mut input, Motion::Right);
    grows(&mut input, Motion::Right);
    assert_eq!(input.selection(), 0..2);
    assert_eq!(input.selected_text(), Some("he"));
    grows(&mut input, Motion::WordRight);
    assert_eq!(input.selected_text(), Some("hello "));
    // A plain arrow collapses to the side it points to, without moving past it.
    moves(&mut input, Motion::Left);
    assert!(!input.has_selection());
    assert_eq!(input.caret(), 0);
    grows(&mut input, Motion::LineEnd);
    moves(&mut input, Motion::Right);
    assert_eq!(input.caret(), 11);
}

#[test]
fn a_selection_made_backwards_reports_its_caret_at_the_start() {
    let mut input = typed("abcd");
    grows(&mut input, Motion::Left);
    grows(&mut input, Motion::Left);
    assert_eq!(input.selection(), 2..4);
    assert_eq!(input.caret(), 2);
    assert_eq!(input.selection_utf16(), (2..4, true));
}

#[test]
fn typing_over_a_selection_replaces_it() {
    let mut input = typed("hello world");
    input.select_word(1);
    assert_eq!(input.selected_text(), Some("hello"));
    input.replace_in_range(None, "bye");
    assert_eq!(input.text(), "bye world");
    assert_eq!(input.caret(), 3);
}

#[test]
fn backspace_and_delete_remove_a_selection_instead_of_a_character() {
    let mut input = typed("hello world");
    input.select_word(7);
    input.backspace();
    assert_eq!(input.text(), "hello ");
    input.select_all();
    input.delete();
    assert_eq!(input.text(), "");
}

#[test]
fn select_all_and_cut_take_everything_and_hand_it_back() {
    let mut input = typed("one\ntwo");
    input.select_all();
    assert_eq!(input.selection(), 0..7);
    assert_eq!(input.cut().as_deref(), Some("one\ntwo"));
    assert_eq!(input.text(), "");
    assert_eq!(input.cut(), None);
}

#[test]
fn a_click_places_the_caret_and_a_drag_extends_from_it() {
    let mut input = typed("hello world");
    input.place(6, false);
    assert_eq!(input.caret(), 6);
    input.place(9, true);
    assert_eq!(input.selected_text(), Some("wor"));
    input.place(2, true);
    assert_eq!(input.selection(), 2..6);
    // A point past the end, or inside a character, still lands on a boundary.
    input.place(99, false);
    assert_eq!(input.caret(), 11);
    let mut accented = typed("\u{e9}a");
    accented.place(1, false);
    assert_eq!(accented.caret(), 0);
}

#[test]
fn a_double_click_selects_a_word_and_a_triple_click_a_line() {
    let mut input = typed("one two\nthree");
    input.select_word(5);
    assert_eq!(input.selected_text(), Some("two"));
    input.select_line(9);
    assert_eq!(input.selected_text(), Some("three"));
    input.select_line(2);
    assert_eq!(input.selected_text(), Some("one two"));
}

#[test]
fn words_are_moved_and_deleted_whole() {
    let mut input = typed("one two  three");
    moves(&mut input, Motion::WordLeft);
    assert_eq!(input.caret(), 9);
    moves(&mut input, Motion::WordLeft);
    assert_eq!(input.caret(), 4);
    moves(&mut input, Motion::WordRight);
    assert_eq!(input.caret(), 9);
    input.delete_word(true);
    assert_eq!(input.text(), "one three");
    input.delete_word(false);
    assert_eq!(input.text(), "one ");
    input.delete_word(false);
    assert_eq!(input.text(), "one ");
}

#[test]
fn a_word_motion_crosses_a_line_break_in_one_stop() {
    let mut input = typed("one\ntwo");
    moves(&mut input, Motion::DocumentStart);
    moves(&mut input, Motion::WordRight);
    assert_eq!(input.caret(), 3);
    moves(&mut input, Motion::WordRight);
    assert_eq!(input.caret(), 4);
    moves(&mut input, Motion::WordLeft);
    assert_eq!(input.caret(), 3);
}

#[test]
fn up_and_down_keep_their_column_across_lines_of_different_lengths() {
    let mut input = typed("abcdef\nab\nabcdef");
    // Column 5 of the last line.
    input.place(15, false);
    moves(&mut input, Motion::Up);
    assert_eq!(input.caret(), 9, "the short line ends");
    moves(&mut input, Motion::Up);
    assert_eq!(input.caret(), 5, "the column is remembered");
    moves(&mut input, Motion::Down);
    moves(&mut input, Motion::Down);
    assert_eq!(input.caret(), 15);
}

#[test]
fn up_from_the_first_line_goes_to_the_start_and_down_from_the_last_to_the_end() {
    let mut input = typed("ab\ncd");
    input.place(1, false);
    moves(&mut input, Motion::Up);
    assert_eq!(input.caret(), 0);
    input.place(4, false);
    moves(&mut input, Motion::Down);
    assert_eq!(input.caret(), 5);
}

#[test]
fn shift_up_selects_to_the_same_column_above() {
    let mut input = typed("abc\ndef");
    grows(&mut input, Motion::Up);
    assert_eq!(input.selected_text(), Some("\ndef"));
}

#[test]
fn a_wide_character_counts_two_cells_when_moving_between_lines() {
    let mut input = typed("\u{4f60}\u{597d}\nabcd");
    input.place(11, false);
    moves(&mut input, Motion::Up);
    // After the two wide characters, which take four cells.
    assert_eq!(input.caret(), 6);
}

#[test]
fn pasted_line_breaks_are_one_newline() {
    let input = typed("a\r\nb\rc");
    assert_eq!(input.text(), "a\nb\nc");
}

#[test]
fn typing_one_word_is_one_undo_step_and_a_new_word_after_a_space_is_the_next() {
    let mut input = TextInput::default();
    type_keys(&mut input, "hello world");
    assert!(input.undo());
    assert_eq!(input.text(), "hello ");
    assert!(input.undo());
    assert_eq!(input.text(), "");
    assert!(!input.undo());
    assert!(input.redo());
    assert_eq!(input.text(), "hello ");
    assert!(input.redo());
    assert_eq!(input.text(), "hello world");
    assert!(!input.redo());
    assert_eq!(input.caret(), 11);
}

#[test]
fn moving_the_caret_ends_a_typing_step() {
    let mut input = TextInput::default();
    type_keys(&mut input, "ab");
    moves(&mut input, Motion::Left);
    type_keys(&mut input, "cd");
    assert_eq!(input.text(), "acdb");
    input.undo();
    assert_eq!(input.text(), "ab");
    assert_eq!(input.caret(), 1);
}

#[test]
fn a_run_of_backspaces_is_one_step_and_so_is_a_run_of_deletes() {
    let mut input = typed("hello");
    for _ in 0..3 {
        input.backspace();
    }
    assert_eq!(input.text(), "he");
    input.undo();
    assert_eq!(input.text(), "hello");
    input.place(0, false);
    input.delete();
    input.delete();
    assert_eq!(input.text(), "llo");
    input.undo();
    assert_eq!(input.text(), "hello");
    assert_eq!(input.caret(), 0);
}

#[test]
fn a_paste_a_cut_and_a_replaced_selection_are_steps_of_their_own() {
    let mut input = TextInput::default();
    type_keys(&mut input, "ab");
    input.insert("PASTED");
    type_keys(&mut input, "cd");
    assert_eq!(input.text(), "abPASTEDcd");
    input.undo();
    assert_eq!(input.text(), "abPASTED");
    input.undo();
    assert_eq!(input.text(), "ab");
    input.redo();
    input.select_word(3);
    input.replace_in_range(None, "x");
    assert_eq!(input.text(), "x");
    input.undo();
    assert_eq!(input.text(), "abPASTED");
    assert_eq!(
        input.selection(),
        0..8,
        "the selection comes back with the text"
    );
}

#[test]
fn undo_restores_the_selection_it_replaced() {
    let mut input = typed("hello world");
    input.select_word(1);
    input.cut();
    assert_eq!(input.text(), " world");
    input.undo();
    assert_eq!(input.text(), "hello world");
    assert_eq!(input.selected_text(), Some("hello"));
}

#[test]
fn a_new_edit_drops_what_could_be_redone() {
    let mut input = TextInput::default();
    type_keys(&mut input, "ab");
    input.undo();
    type_keys(&mut input, "c");
    assert!(!input.redo());
    assert_eq!(input.text(), "c");
}

#[test]
fn a_composition_is_one_undo_step() {
    let mut input = typed("a");
    input.replace_and_mark(None, "~", None);
    input.replace_and_mark(None, "\u{3053}", None);
    input.replace_in_range(None, "\u{e3}");
    assert_eq!(input.text(), "a\u{e3}");
    input.undo();
    assert_eq!(input.text(), "a");
}

#[test]
fn undo_keeps_a_bounded_history() {
    let mut input = TextInput::default();
    for _ in 0..300 {
        input.insert("x");
    }
    let mut undone = 0;
    while input.undo() {
        undone += 1;
    }
    assert_eq!(undone, 200);
    assert_eq!(input.text().len(), 100);
}

#[test]
fn clearing_forgets_the_history() {
    let mut input = typed("sent");
    input.clear();
    assert!(!input.undo());
}

#[test]
fn a_word_and_a_line_are_found_in_text_that_is_not_an_input() {
    let text = "let path = a/b.rs;\nnext";
    assert_eq!(&text[word_range(text, 5)], "path");
    assert_eq!(&text[word_range(text, 9)], "=");
    assert_eq!(&text[line_range(text, 20)], "next");
    assert_eq!(floor_boundary("\u{e9}", 1), 0);
    assert_eq!(floor_boundary("ab", 9), 2);
}

#[test]
fn text_loaded_from_elsewhere_keeps_the_selection_it_has_room_for_and_drops_the_history() {
    let mut input = typed("hello world");
    input.place(8, false);
    input.load("hi\u{e9}");
    // The caret was past the end of the new text; a byte inside a character rounds down.
    assert_eq!(input.text(), "hi\u{e9}");
    assert_eq!(input.caret(), 4);
    assert!(!input.undo());
    input.load("longer text here");
    assert_eq!(input.caret(), 4);
    assert!(!input.has_selection() && !input.is_composing());
}
