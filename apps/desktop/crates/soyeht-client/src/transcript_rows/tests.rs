use super::*;

const ROW: f32 = 100.0;
const GAP: f32 = 10.0;
const WIDTH: f32 = 600.0;

fn keys(count: usize) -> Vec<(String, u64)> {
    (0..count)
        .map(|at| (format!("row-{at}"), at as u64))
        .collect()
}

/// The frame a transcript of `keys` was built in: each row built at `built` high and each spacer
/// at its own height, all `width` wide.
fn laid_out_at(
    rows: &mut Rows,
    keys: &[(String, u64)],
    plan: &[Option<f32>],
    (width, built): (f32, f32),
) {
    rows.set_children(
        keys.iter()
            .zip(plan)
            .map(|((key, fingerprint), spacer)| Child::Row {
                key: key.clone(),
                fingerprint: *fingerprint,
                built: spacer.is_none(),
            })
            .collect(),
    );
    let sizes: Vec<(f32, f32)> = plan
        .iter()
        .map(|spacer| (width, spacer.unwrap_or(built)))
        .collect();
    rows.absorb(&sizes);
}

fn laid_out(rows: &mut Rows, keys: &[(String, u64)], plan: &[Option<f32>]) {
    laid_out_at(rows, keys, plan, (WIDTH, ROW));
}

fn at_bottom() -> Viewport {
    Viewport {
        height: 400.0,
        from_bottom: 0.0,
    }
}

/// A transcript that has been laid out once, whole, as the first frame of a pane does.
fn measured(count: usize) -> (Rows, Vec<(String, u64)>) {
    let keys = keys(count);
    let mut rows = Rows::default();
    let first = rows.plan(&keys, at_bottom(), GAP, 1);
    assert!(first.iter().all(Option::is_none), "nothing is known yet");
    laid_out(&mut rows, &keys, &first);
    (rows, keys)
}

fn built(plan: &[Option<f32>]) -> Vec<usize> {
    (0..plan.len()).filter(|&at| plan[at].is_none()).collect()
}

#[test]
fn the_first_frame_builds_everything_so_that_it_can_be_measured() {
    let keys = keys(200);
    let mut rows = Rows::default();
    let plan = rows.plan(&keys, at_bottom(), GAP, 1);
    assert_eq!(built(&plan).len(), 200);
}

#[test]
fn a_long_transcript_builds_the_rows_near_the_viewport_and_example_the_rest() {
    let (mut rows, keys) = measured(200);
    let plan = rows.plan(&keys, at_bottom(), GAP, 1);
    let built = built(&plan);
    // The viewport is four rows high at 110 per row (with the gap); half a viewport of margin
    // above it, so about six rows, and never fewer than the newest ones.
    assert!(built.len() < 20, "{built:?}");
    assert_eq!(*built.last().unwrap(), 199);
    assert!(built.iter().all(|&at| at >= 180), "{built:?}");
    assert!(plan[..180].iter().all(|spacer| *spacer == Some(ROW)));
}

#[test]
fn the_rows_built_follow_the_scroll_position() {
    let (mut rows, keys) = measured(200);
    // 110 per row: scrolled up 100 rows from the end.
    let viewport = Viewport {
        height: 400.0,
        from_bottom: 11_000.0,
    };
    let plan = rows.plan(&keys, viewport, GAP, 1);
    let built = built(&plan);
    assert!(
        built.iter().any(|&at| (95..=100).contains(&at)),
        "{built:?}"
    );
    assert!(!built.contains(&20));
    // The newest are built wherever the person is reading.
    assert!(built.contains(&199) && built.contains(&192));
}

#[test]
fn a_short_transcript_is_built_whole() {
    let (mut rows, keys) = measured(SHORT);
    let plan = rows.plan(&keys, at_bottom(), GAP, 1);
    assert_eq!(built(&plan).len(), SHORT);
}

#[test]
fn a_row_whose_content_changed_is_built_again() {
    let (mut rows, mut keys) = measured(200);
    keys[10].1 += 1;
    let plan = rows.plan(&keys, at_bottom(), GAP, 1);
    assert!(built(&plan).contains(&10));
    assert_eq!(plan[11], Some(ROW));
}

#[test]
fn a_new_row_is_built_and_the_ones_before_it_stay_spaced() {
    let (mut rows, mut keys) = measured(200);
    keys.push(("row-200".into(), 200));
    let plan = rows.plan(&keys, at_bottom(), GAP, 1);
    assert!(built(&plan).contains(&200));
    assert_eq!(plan[0], Some(ROW));
}

#[test]
fn a_width_that_changed_builds_everything_until_it_is_measured_again() {
    let (mut rows, keys) = measured(200);
    // The next frame is laid out narrower: every height measured so far is for another width.
    let first = rows.plan(&keys, at_bottom(), GAP, 1);
    laid_out_at(&mut rows, &keys, &first, (WIDTH - 200.0, ROW * 2.0));
    let plan = rows.plan(&keys, at_bottom(), GAP, 1);
    let rebuilt = built(&plan);
    // The rows built in the last frame were measured at the new width and may be spaced again;
    // the ones that were example keep the old width's heights, which no longer count.
    assert!(rebuilt.len() >= 180, "{}", rebuilt.len());
    assert!(rebuilt.contains(&0));
}

#[test]
fn a_changed_window_builds_everything_in_that_frame() {
    let (mut rows, keys) = measured(200);
    let plan = rows.plan(&keys, at_bottom(), GAP, 2);
    assert_eq!(built(&plan).len(), 200);
    // The signature is stable again in the next one.
    laid_out(&mut rows, &keys, &plan);
    let plan = rows.plan(&keys, at_bottom(), GAP, 2);
    assert!(built(&plan).len() < 20);
}

#[test]
fn nothing_is_left_out_before_the_viewport_has_a_size() {
    let (mut rows, keys) = measured(200);
    let plan = rows.plan(
        &keys,
        Viewport {
            height: 0.0,
            from_bottom: 0.0,
        },
        GAP,
        1,
    );
    assert_eq!(built(&plan).len(), 200);
}

#[test]
fn spacers_keep_the_scroll_range_of_the_whole_transcript() {
    let (mut rows, keys) = measured(200);
    let plan = rows.plan(&keys, at_bottom(), GAP, 1);
    let total = |plan: &[Option<f32>]| -> f32 {
        plan.iter().map(|spacer| spacer.unwrap_or(ROW) + GAP).sum()
    };
    assert_eq!(total(&plan), 200.0 * (ROW + GAP));
}

#[test]
fn only_built_rows_update_what_is_known() {
    let (mut rows, keys) = measured(200);
    let plan = rows.plan(&keys, at_bottom(), GAP, 1);
    // What a spacer reports back is its own height; it must not replace the measure.
    laid_out_at(&mut rows, &keys, &plan, (WIDTH, 7.0));
    let next = rows.plan(&keys, at_bottom(), GAP, 1);
    assert_eq!(next[0], Some(ROW));
}

#[test]
fn the_fingerprint_follows_the_content_and_the_state_around_it() {
    assert_eq!(fingerprint(&"a thought", 0), fingerprint(&"a thought", 0));
    assert_ne!(fingerprint(&"a thought", 0), fingerprint(&"a thought", 1));
    assert_ne!(fingerprint(&"a thought", 0), fingerprint(&"another", 0));
}

#[test]
fn rows_of_a_transcript_that_is_gone_are_forgotten() {
    let (mut rows, keys) = measured(200);
    let plan = rows.plan(&keys[100..], at_bottom(), GAP, 1);
    assert_eq!(plan.len(), 100);
    assert!(rows.measured.len() <= 100);
}

#[test]
fn entries_that_draw_no_row_take_no_room() {
    // Every third entry draws nothing (a usage line, a subagent's own message): they are not
    // rows, so they must not push the rows around them away from the viewport.
    let keys = keys(200);
    let mut rows = Rows::default();
    let first = rows.plan(&keys, at_bottom(), GAP, 1);
    let drawn: Vec<(String, u64)> = keys
        .iter()
        .cloned()
        .enumerate()
        .filter(|(at, _)| at % 3 != 2)
        .map(|(_, key)| key)
        .collect();
    rows.set_children(
        drawn
            .iter()
            .map(|(key, fingerprint)| Child::Row {
                key: key.clone(),
                fingerprint: *fingerprint,
                built: true,
            })
            .collect(),
    );
    rows.absorb(&vec![(WIDTH, ROW); drawn.len()]);
    for (at, (key, fingerprint)) in keys.iter().enumerate() {
        if at % 3 == 2 {
            rows.mark_absent(key, *fingerprint);
        }
    }
    assert!(first.iter().all(Option::is_none));
    let plan = rows.plan(&keys, at_bottom(), GAP, 1);
    let built_rows: Vec<usize> = (0..200)
        .filter(|at| plan[*at].is_none() && at % 3 != 2)
        .collect();
    // Four rows fit the viewport plus half of it each side, and the newest eight are kept: the
    // rows kept are the ones nearest the end, not a handful pushed away by the absent ones.
    assert!(built_rows.len() <= 16, "{built_rows:?}");
    assert!(built_rows.contains(&196) && built_rows.contains(&195));
}

fn message(id: &str, seq: u32, extra: serde_json::Value) -> soyeht_protocol::TranscriptMessage {
    let mut base = serde_json::json!({
        "localId": id, "generation": 1, "seq": seq.to_string(), "role": "assistant",
        "kind": "message", "text": "", "complete": false
    });
    base.as_object_mut()
        .unwrap()
        .extend(extra.as_object().unwrap().clone());
    serde_json::from_value(base).unwrap()
}

fn tool(id: &str, seq: u32, status: &str) -> soyeht_protocol::TranscriptMessage {
    message(
        id,
        seq,
        serde_json::json!({"kind": "tool", "tool": {
            "toolCallId": id, "title": "Read", "status": status, "locations": []}}),
    )
}

fn subagent_row(status: &str) -> soyeht_protocol::TranscriptMessage {
    message(
        "row",
        5,
        serde_json::json!({"kind": "subagent", "subagent": {
            "subagentId": "s1", "name": "worker", "task": "t", "status": status}}),
    )
}

fn inner(id: &str, seq: u32, text: &str) -> soyeht_protocol::TranscriptMessage {
    message(
        id,
        seq,
        serde_json::json!({"subagentId": "s1", "text": text}),
    )
}

/// The fingerprints of every entry of `view`, in order.
fn prints(view: &AgentView) -> Vec<u64> {
    crate::cards::entries(view.messages())
        .iter()
        .map(|entry| entry_fingerprint(entry, view, 0))
        .collect()
}

#[test]
fn an_entry_whose_text_grew_has_a_new_fingerprint_and_the_others_keep_theirs() {
    let mut view = AgentView::default();
    view.merge(message("a", 1, serde_json::json!({"text": "Hel"})))
        .unwrap();
    view.merge(message("b", 2, serde_json::json!({"text": "other"})))
        .unwrap();
    let before = prints(&view);
    assert_eq!(before, prints(&view), "the same transcript, the same print");
    view.merge(message("a", 1, serde_json::json!({"text": "Hello"})))
        .unwrap();
    let after = prints(&view);
    assert_ne!(before[0], after[0]);
    assert_eq!(before[1], after[1]);
}

#[test]
fn an_entry_that_finished_streaming_or_changed_in_length_less_visibly_has_a_new_fingerprint() {
    let mut view = AgentView::default();
    view.merge(message("a", 1, serde_json::json!({"text": "same"})))
        .unwrap();
    let streaming = prints(&view)[0];
    view.merge(message(
        "a",
        1,
        serde_json::json!({"text": "same", "complete": true}),
    ))
    .unwrap();
    let complete = prints(&view)[0];
    assert_ne!(streaming, complete);
    // Another text of the same length is still another text.
    view.merge(message(
        "a",
        1,
        serde_json::json!({"text": "sane", "complete": true}),
    ))
    .unwrap();
    assert_ne!(complete, prints(&view)[0]);
}

#[test]
fn a_tool_call_whose_status_changed_has_a_new_fingerprint() {
    let mut view = AgentView::default();
    view.merge(tool("t", 1, "pending")).unwrap();
    let pending = prints(&view)[0];
    view.merge(tool("t", 1, "in_progress")).unwrap();
    let running = prints(&view)[0];
    view.merge(tool("t", 1, "completed")).unwrap();
    let done = prints(&view)[0];
    assert!(pending != running && running != done && pending != done);
}

#[test]
fn an_entry_replaced_by_another_message_with_its_id_has_a_new_fingerprint() {
    let mut view = AgentView::default();
    view.merge(message("a", 1, serde_json::json!({"text": "one"})))
        .unwrap();
    let text = prints(&view)[0];
    view.merge(tool("a", 1, "pending")).unwrap();
    assert_ne!(text, prints(&view)[0]);
}

#[test]
fn the_state_around_an_entry_changes_its_fingerprint() {
    let mut view = AgentView::default();
    view.merge(message("a", 1, serde_json::json!({"text": "t"})))
        .unwrap();
    let entries = crate::cards::entries(view.messages());
    assert_ne!(
        entry_fingerprint(&entries[0], &view, 0),
        entry_fingerprint(&entries[0], &view, 1)
    );
}

#[test]
fn a_subagent_row_follows_its_run_and_what_runs_under_it() {
    let mut view = AgentView::default();
    view.merge(subagent_row("running")).unwrap();
    view.merge(inner("i1", 6, "step")).unwrap();
    let start = prints(&view);
    assert_eq!(start.len(), 1, "the activity nests under the row");
    view.merge(inner("i1", 6, "step, longer")).unwrap();
    let grown = prints(&view)[0];
    assert_ne!(start[0], grown);
    view.merge(inner("i2", 7, "next")).unwrap();
    let added = prints(&view)[0];
    assert_ne!(grown, added);
    view.merge(subagent_row("completed")).unwrap();
    assert_ne!(added, prints(&view)[0]);
}
