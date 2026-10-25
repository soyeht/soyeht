//! File references in a reply (`src/main.rs:12`), marked on the text that holds them. The rule is
//! [`crate::file_links`], the Mac's `FilePathLinks`; a reference is marked in code spans too, as
//! the Mac links them, but not in code blocks. A stretch that is already a link (a written one, or
//! a bare web address) is left alone. Whether the file is in the workspace, and so whether the
//! text is drawn as a link, is the window's to say.

use super::{Block, Run, Style, web_links};
use crate::file_links::{self, Dialect};

/// The blocks with the file references in their runs marked (`Style::file`).
pub fn link_references(blocks: Vec<Block>) -> Vec<Block> {
    web_links::map_runs(blocks, &mark_runs)
}

fn mark_runs(runs: Vec<Run>) -> Vec<Run> {
    let mut marked: Vec<Run> = Vec::with_capacity(runs.len());
    for run in runs {
        if run.style.link.is_some() {
            marked.push(run);
            continue;
        }
        let mut rest = 0;
        for found in file_links::find(Dialect::HOST, &run.text) {
            web_links::push(&mut marked, &run, rest..found.range.start, None);
            let reference = found.reference;
            let mark = |style: &mut Style| style.file = Some(reference.clone());
            web_links::push(&mut marked, &run, found.range.clone(), Some(&mark));
            rest = found.range.end;
        }
        web_links::push(&mut marked, &run, rest..run.text.len(), None);
    }
    marked
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        file_links::Reference,
        markdown::{link_blocks, parse},
    };

    fn runs(source: &str) -> Vec<Run> {
        let blocks = link_references(link_blocks(parse(source, false)));
        match blocks.into_iter().next() {
            Some(Block::Paragraph(runs)) => runs,
            other => panic!("a paragraph, not {other:?}"),
        }
    }

    fn reference(path: &str, line: u32) -> Option<Reference> {
        Some(Reference {
            path: path.to_owned(),
            line,
            column: None,
        })
    }

    #[test]
    fn a_reference_keeps_the_style_of_the_words_around_it() {
        let runs = runs("**see src/a.rs:12.** now");
        let texts: Vec<_> = runs.iter().map(|run| run.text.as_str()).collect();
        assert_eq!(texts, ["see ", "src/a.rs:12", ".", " now"]);
        assert!(runs[..3].iter().all(|run| run.style.bold));
        assert_eq!(runs[1].style.file, reference("src/a.rs", 11));
        assert_eq!(runs[0].style.file, None);
        assert_eq!(runs[3].style, Style::default());
    }

    #[test]
    fn a_reference_in_a_code_span_is_marked_and_keeps_the_code_look() {
        let runs = runs("open `src/a.rs:3` please");
        assert_eq!(runs[1].text, "src/a.rs:3");
        assert!(runs[1].style.code);
        assert_eq!(runs[1].style.file, reference("src/a.rs", 2));
    }

    #[test]
    fn written_links_web_addresses_and_code_blocks_are_left_alone() {
        let source = "[t](https://a.test/x.rs:3) https://a.test/b/c.rs:4\n\n```\nsrc/a.rs:3\n```";
        let blocks = link_blocks(parse(source, false));
        assert_eq!(link_references(blocks.clone()), blocks);
    }

    #[test]
    fn lists_quotes_and_tables_are_marked_too() {
        for source in [
            "- src/a.rs:1",
            "> src/a.rs:1",
            "| h |\n|---|\n| src/a.rs:1 |",
        ] {
            let shown = format!("{:?}", link_references(parse(source, false)));
            assert!(shown.contains("file: Some(Reference"), "{source}");
        }
    }
}
