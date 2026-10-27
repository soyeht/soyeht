use super::*;

fn parse(text: &str) -> Arc<[Block]> {
    super::super::parse(text, false).into()
}

fn flat_list(items: usize) -> String {
    (0..items)
        .map(|at| format!("- item number {at}\n"))
        .collect()
}

#[test]
fn a_short_reply_is_shown_whole_and_is_not_copied() {
    let blocks = parse("A paragraph.\n\n- one\n- two\n");
    let shown = shown(&blocks, 1);
    assert!(!shown.more);
    assert!(Arc::ptr_eq(&shown.blocks, &blocks));
}

#[test]
fn a_flat_list_weighs_one_unit_per_item() {
    assert_eq!(weight_of(&parse(&flat_list(30))), 30);
}

#[test]
fn a_long_reply_is_cut_to_the_page_whatever_its_length() {
    for items in [PAGE_WEIGHT + 1, 1_000, 20_000] {
        let blocks = parse(&flat_list(items));
        let page = shown(&blocks, 1);
        assert!(page.more);
        assert_eq!(weight_of(&page.blocks), PAGE_WEIGHT, "{items} items");
    }
}

#[test]
fn each_page_adds_the_next_page_and_the_last_one_shows_all() {
    let blocks = parse(&flat_list(PAGE_WEIGHT * 2 + 5));
    assert_eq!(weight_of(&shown(&blocks, 1).blocks), PAGE_WEIGHT);
    assert_eq!(weight_of(&shown(&blocks, 2).blocks), PAGE_WEIGHT * 2);
    let all = shown(&blocks, 3);
    assert!(!all.more);
    assert!(Arc::ptr_eq(&all.blocks, &blocks));
}

#[test]
fn the_cut_falls_inside_a_list_and_keeps_its_numbering() {
    let text: String = (5..5 + PAGE_WEIGHT * 2)
        .map(|at| format!("{at}. step {at}\n"))
        .collect();
    let page = shown(&parse(&text), 1);
    let [Block::List { start, items }] = &page.blocks[..] else {
        panic!("one list expected: {:?}", page.blocks);
    };
    assert_eq!(*start, Some(5));
    assert_eq!(items.len(), PAGE_WEIGHT);
}

#[test]
fn headings_and_lists_before_the_cut_are_kept_in_order() {
    let text = format!("# Title\n\nIntro.\n\n{}", flat_list(PAGE_WEIGHT * 3));
    let page = shown(&parse(&text), 1);
    assert!(matches!(page.blocks[0], Block::Heading { .. }));
    assert!(matches!(page.blocks[1], Block::Paragraph(_)));
    assert_eq!(weight_of(&page.blocks), PAGE_WEIGHT);
}

#[test]
fn a_huge_paragraph_and_a_huge_code_block_are_cut_to_the_page_too() {
    let words = "word ".repeat(100_000);
    let code = format!("```\n{}```\n", "let x = 1;\n".repeat(50_000));
    for text in [words, code] {
        let page = shown(&parse(&text), 1);
        assert!(page.more);
        assert!(weight_of(&page.blocks) <= PAGE_WEIGHT);
        assert!(weight_of(&page.blocks) >= PAGE_WEIGHT - 1);
    }
}

#[test]
fn a_cut_never_splits_a_character() {
    let text = "ã".repeat(PAGE_WEIGHT * BYTES_PER_UNIT);
    let page = shown(&parse(&text), 1);
    let [Block::Paragraph(runs)] = &page.blocks[..] else {
        panic!("one paragraph expected");
    };
    assert!(runs[0].text.chars().all(|letter| letter == 'ã'));
}

#[test]
fn a_cut_inside_a_quote_or_a_table_keeps_the_shape() {
    let quote: String = (0..PAGE_WEIGHT * 2)
        .map(|at| format!("> line {at}\n>\n"))
        .collect();
    let page = shown(&parse(&quote), 1);
    assert!(matches!(&page.blocks[..], [Block::Quote(_)]));
    assert!(weight_of(&page.blocks) <= PAGE_WEIGHT);

    let mut table = String::from("| a | b |\n|---|---|\n");
    table.push_str(&"| x | y |\n".repeat(PAGE_WEIGHT * 2));
    let page = shown(&parse(&table), 1);
    let [Block::Table { rows, .. }] = &page.blocks[..] else {
        panic!("one table expected");
    };
    assert!(rows.len() < PAGE_WEIGHT && !rows.is_empty());
    assert!(weight_of(&page.blocks) <= PAGE_WEIGHT);
}

#[test]
fn nested_lists_are_cut_by_their_total_weight() {
    let text: String = (0..60)
        .map(|at| format!("- parent {at}\n  - child a\n  - child b\n  - child c\n"))
        .collect();
    let blocks = parse(&text);
    assert_eq!(weight_of(&blocks), 60 * 4);
    let page = shown(&blocks, 1);
    assert_eq!(weight_of(&page.blocks), PAGE_WEIGHT);
}

/// Every text of the blocks, in order, one per line.
fn texts(blocks: &[Block]) -> Vec<String> {
    let runs = |runs: &[Run]| runs.iter().map(|run| run.text.as_str()).collect::<String>();
    let mut out = Vec::new();
    for block in blocks {
        match block {
            Block::Paragraph(r) | Block::Heading { runs: r, .. } => out.push(runs(r)),
            Block::Code { text, .. } => out.push(text.clone()),
            Block::Quote(inner) => out.extend(texts(inner)),
            Block::List { items, .. } => items.iter().for_each(|item| out.extend(texts(item))),
            Block::Table { header, rows } => {
                out.push(header.iter().map(|cell| runs(cell)).collect());
                rows.iter()
                    .for_each(|row| out.push(row.iter().map(|cell| runs(cell)).collect()));
            }
            Block::Rule => out.push("---".into()),
        }
    }
    out
}

#[test]
fn each_page_continues_the_one_before_without_dropping_or_repeating() {
    let mut table = String::from("| a | b |\n|---|---|\n");
    table.push_str(&"| x | y |\n".repeat(PAGE_WEIGHT * 2));
    let samples = [
        flat_list(PAGE_WEIGHT * 3),
        table,
        format!(
            "# T\n\n{}\n\n{}",
            "ã é ü ".repeat(PAGE_WEIGHT * 100),
            flat_list(40)
        ),
        format!(
            "```\n{}```\n\nafter",
            "let x = 1;\n".repeat(PAGE_WEIGHT * 30)
        ),
        (0..PAGE_WEIGHT * 2)
            .map(|at| format!("> line {at}\n>\n"))
            .collect(),
    ];
    for text in samples {
        let blocks = parse(&text);
        let whole = texts(&blocks);
        let mut before: Vec<String> = Vec::new();
        for pages in 1..=8 {
            let page = texts(&shown(&blocks, pages).blocks);
            // The page before is a prefix of this one, apart from the line a cut shortened.
            let (head, tail) = before.split_at(before.len().saturating_sub(1));
            assert!(
                page.len() >= head.len() && page[..head.len()] == *head,
                "{pages}"
            );
            if let (Some(cut), Some(grown)) = (tail.first(), page.get(head.len())) {
                assert!(grown.starts_with(cut.as_str()), "{pages}");
            }
            assert!(
                page.iter()
                    .zip(&whole)
                    .all(|(a, b)| b.starts_with(a.as_str())),
                "{pages}"
            );
            before = page;
        }
        // The pages together are the whole reply: the last page holds all of its text.
        let last = shown(&blocks, 1_000);
        assert!(!last.more);
        assert_eq!(texts(&last.blocks), whole);
    }
}

#[test]
fn a_reply_that_is_arriving_keeps_its_newest_text_in_view_at_a_bounded_cost() {
    for items in [PAGE_WEIGHT - 1, PAGE_WEIGHT + 1, 300, 1_000, 5_000] {
        let blocks = parse(&flat_list(items));
        let tail = shown_tail(&blocks, 1);
        assert_eq!(tail.more, items > PAGE_WEIGHT);
        assert_eq!(tail.tail, items > PAGE_WEIGHT);
        assert!(weight_of(&tail.blocks) <= PAGE_WEIGHT.max(items.min(PAGE_WEIGHT)));
        // The newest item is the last one, and the shown ones are consecutive.
        let texts = texts(&tail.blocks);
        assert_eq!(texts.last().unwrap(), &format!("item number {}", items - 1));
        assert_eq!(texts.len(), items.min(PAGE_WEIGHT));
    }
}

#[test]
fn a_reply_that_grows_by_a_chunk_costs_the_same_each_time() {
    let mut weights = Vec::new();
    for items in (PAGE_WEIGHT..400).step_by(17) {
        let tail = shown_tail(&parse(&flat_list(items)), 1);
        weights.push(weight_of(&tail.blocks));
    }
    assert!(
        weights.iter().all(|&weight| weight == PAGE_WEIGHT),
        "{weights:?}"
    );
}

#[test]
fn the_tail_of_a_long_list_keeps_its_numbering_and_of_a_table_its_header() {
    let text: String = (1..=PAGE_WEIGHT * 2)
        .map(|at| format!("{at}. step {at}\n"))
        .collect();
    let tail = shown_tail(&parse(&text), 1);
    let [Block::List { start, items }] = &tail.blocks[..] else {
        panic!("one list expected");
    };
    assert_eq!(*start, Some(PAGE_WEIGHT as u64 + 1));
    assert_eq!(items.len(), PAGE_WEIGHT);

    let mut table = String::from("| a | b |\n|---|---|\n");
    table.push_str(&"| x | y |\n".repeat(PAGE_WEIGHT * 2));
    let tail = shown_tail(&parse(&table), 1);
    let [Block::Table { header, rows }] = &tail.blocks[..] else {
        panic!("one table expected");
    };
    assert_eq!(header.len(), 2);
    assert!(!rows.is_empty() && rows.len() < PAGE_WEIGHT * 2);
}

#[test]
fn the_tail_of_a_huge_paragraph_or_code_block_ends_with_its_end() {
    let words: String = (0..100_000).map(|at| format!("w{at} ")).collect();
    let code: String = (0..50_000).map(|at| format!("line {at}\n")).collect();
    for (text, last) in [
        (words, "w99999"),
        (format!("```\n{code}```\n"), "line 49999"),
    ] {
        let tail = shown_tail(&parse(&text), 1);
        assert!(tail.tail && weight_of(&tail.blocks) <= PAGE_WEIGHT + 1);
        assert!(
            texts(&tail.blocks)
                .last()
                .unwrap()
                .trim_end()
                .ends_with(last)
        );
    }
}
