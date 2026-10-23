//! How the parts of a text that have a look are drawn: a heading and a bold run in bold, an
//! italic run slanted, a quote in the secondary ink, code on a well, a link in the accent. Every
//! color is a role of the palette; which parts of the source have a look is
//! `soyeht_client::writing::looks`.

use crate::{
    composer_field::Looks,
    theme::{Palette, pane_interior},
};
use gpui::{FontStyle, FontWeight, HighlightStyle, UnderlineStyle, px, rgb};
use soyeht_client::writing::looks::{Look, looks};

/// The look of one kind of part.
fn style(look: Look, palette: &Palette) -> HighlightStyle {
    match look {
        Look::Heading | Look::Strong => HighlightStyle {
            font_weight: Some(FontWeight::BOLD),
            ..HighlightStyle::default()
        },
        Look::Emphasis => HighlightStyle {
            font_style: Some(FontStyle::Italic),
            ..HighlightStyle::default()
        },
        Look::Quote => HighlightStyle {
            color: Some(rgb(palette.secondary).into()),
            ..HighlightStyle::default()
        },
        Look::Code => HighlightStyle {
            background_color: Some(rgb(palette.well).into()),
            ..HighlightStyle::default()
        },
        Look::Link => HighlightStyle {
            color: Some(rgb(palette.accent).into()),
            underline: Some(UnderlineStyle {
                thickness: px(pane_interior::SEPARATOR_WIDTH),
                color: Some(rgb(palette.accent).into()),
                wavy: false,
            }),
            ..HighlightStyle::default()
        },
    }
}

/// The parts of `text` that have a look, each with how it is drawn.
pub(crate) fn highlights(text: &str, palette: &Palette) -> Looks {
    looks(text)
        .into_iter()
        .map(|(range, look)| (range, style(look, palette)))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::ThemeName;

    #[test]
    fn a_heading_and_a_bold_run_are_bold_and_a_link_takes_the_accent() {
        let palette = ThemeName::NeoMilk.palette();
        let text = "# Title\n\nA **word** and [a link](https://x.y)";
        let marks = highlights(text, palette);
        let weight = |at: usize| {
            marks
                .iter()
                .find(|(range, _)| range.contains(&at))
                .and_then(|(_, style)| style.font_weight)
        };
        assert_eq!(weight(2), Some(FontWeight::BOLD));
        assert_eq!(weight(text.find("word").unwrap()), Some(FontWeight::BOLD));
        assert_eq!(weight(text.find("A ").unwrap()), None);
        let link = marks
            .iter()
            .find(|(range, _)| text[range.clone()].starts_with('['))
            .unwrap();
        assert_eq!(link.1.color, Some(rgb(palette.accent).into()));
        assert!(link.1.underline.is_some());
    }

    #[test]
    fn plain_text_has_no_marks() {
        assert!(highlights("plain words", ThemeName::NeoMilk.palette()).is_empty());
    }
}
