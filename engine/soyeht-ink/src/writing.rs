//! The writing of one `ink.recognize` request, checked against the contract's limits.

use crate::error::{Error, Result};
use soyeht_protocol::{InkRecognizeParams, limits};

/// One point of a stroke, in the client's units. The y axis grows downward, as on a screen.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Dot {
    pub x: f64,
    pub y: f64,
}

/// What a request asks, after it passed every limit: strokes that hold at least one finite point
/// each, language tags that are tags, and a candidate count within bounds.
#[derive(Debug, Clone, PartialEq)]
pub struct Writing {
    pub(crate) strokes: Vec<Vec<Dot>>,
    pub(crate) languages: Vec<String>,
    pub(crate) max_candidates: usize,
}

impl Writing {
    /// Checks `params` and keeps the part the recognizers read. The time of each point is
    /// checked and then dropped: the image pipeline of a recognizer does not use it.
    pub fn from_params(params: &InkRecognizeParams) -> Result<Self> {
        if params.strokes.is_empty() {
            return Err(Error::Invalid("The writing has no strokes"));
        }
        if params.strokes.len() > limits::INK_STROKES {
            return Err(Error::LimitExceeded("The writing has too many strokes"));
        }
        let mut total = 0usize;
        let mut strokes = Vec::with_capacity(params.strokes.len());
        for stroke in &params.strokes {
            if stroke.is_empty() {
                return Err(Error::Invalid("A stroke has no points"));
            }
            if stroke.len() > limits::INK_STROKE_POINTS {
                return Err(Error::LimitExceeded("A stroke has too many points"));
            }
            total = total.saturating_add(stroke.len());
            if total > limits::INK_TOTAL_POINTS {
                return Err(Error::LimitExceeded("The writing has too many points"));
            }
            strokes.push(
                stroke
                    .iter()
                    .map(|point| dot(point.x, point.y, point.t))
                    .collect::<Result<Vec<_>>>()?,
            );
        }
        Ok(Self {
            strokes,
            languages: languages(&params.languages)?,
            max_candidates: candidates(params.max_candidates)?,
        })
    }
}

fn dot(x: f64, y: f64, t: u32) -> Result<Dot> {
    if !x.is_finite() || !y.is_finite() {
        return Err(Error::Invalid("A point is not a finite number"));
    }
    let largest = limits::INK_COORDINATE as f64;
    if x.abs() > largest || y.abs() > largest {
        return Err(Error::LimitExceeded("A point is too far from the origin"));
    }
    if t as usize > limits::INK_DURATION_MS {
        return Err(Error::LimitExceeded("The writing lasts too long"));
    }
    Ok(Dot { x, y })
}

fn candidates(asked: Option<u32>) -> Result<usize> {
    match asked {
        None => Ok(limits::INK_CANDIDATES_DEFAULT),
        Some(0) => Err(Error::Invalid("A request asks for at least one candidate")),
        Some(count) if count as usize > limits::INK_CANDIDATES => Err(Error::LimitExceeded(
            "The request asks for too many candidates",
        )),
        Some(count) => Ok(count as usize),
    }
}

fn languages(tags: &[String]) -> Result<Vec<String>> {
    if tags.is_empty() {
        return Err(Error::Invalid("The request names no language"));
    }
    if tags.len() > limits::INK_LANGUAGES {
        return Err(Error::LimitExceeded("The request names too many languages"));
    }
    tags.iter()
        .map(|tag| language_tag(tag).map(|()| tag.clone()))
        .collect()
}

/// Whether `tag` has the shape of a BCP 47 tag: letters and digits in groups of one to eight
/// separated by hyphens, the first group letters only. Whether anyone reads it is for the
/// recognizer to say.
fn language_tag(tag: &str) -> Result<()> {
    if tag.len() > limits::INK_LANGUAGE_BYTES {
        return Err(Error::LimitExceeded("A language tag is too long"));
    }
    let mut groups = tag.split('-');
    let primary = groups.next().unwrap_or_default();
    let shaped = (2..=8).contains(&primary.len())
        && primary.bytes().all(|byte| byte.is_ascii_alphabetic())
        && groups.all(|group| {
            (1..=8).contains(&group.len()) && group.bytes().all(|byte| byte.is_ascii_alphanumeric())
        });
    if shaped {
        Ok(())
    } else {
        Err(Error::Invalid("A language tag is not a language tag"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use soyeht_protocol::InkPoint;

    fn point(x: f64, y: f64, t: u32) -> InkPoint {
        InkPoint { x, y, t }
    }

    fn params(strokes: Vec<Vec<InkPoint>>) -> InkRecognizeParams {
        InkRecognizeParams {
            strokes,
            languages: vec!["pt-BR".into(), "en-US".into()],
            max_candidates: None,
        }
    }

    fn one_stroke() -> InkRecognizeParams {
        params(vec![vec![point(0.0, 0.0, 0), point(5.0, 5.0, 10)]])
    }

    #[test]
    fn a_request_within_the_limits_is_kept_with_the_default_candidate_count() {
        let writing = Writing::from_params(&one_stroke()).unwrap();
        assert_eq!(writing.strokes.len(), 1);
        assert_eq!(writing.strokes[0][1], Dot { x: 5.0, y: 5.0 });
        assert_eq!(writing.languages, ["pt-BR", "en-US"]);
        assert_eq!(writing.max_candidates, limits::INK_CANDIDATES_DEFAULT);
    }

    #[test]
    fn a_request_at_exactly_each_limit_is_accepted() {
        let stroke = vec![point(1.0, 1.0, 1); limits::INK_STROKE_POINTS];
        let strokes = vec![stroke; limits::INK_TOTAL_POINTS / limits::INK_STROKE_POINTS];
        let mut request = params(strokes);
        request.max_candidates = Some(limits::INK_CANDIDATES as u32);
        assert!(Writing::from_params(&request).is_ok());
        let tight = params(vec![vec![point(1.0, 1.0, 0)]; limits::INK_STROKES]);
        assert!(Writing::from_params(&tight).is_ok());
    }

    #[test]
    fn each_limit_one_over_is_refused_by_name() {
        let over_strokes = params(vec![vec![point(0.0, 0.0, 0)]; limits::INK_STROKES + 1]);
        let over_stroke = params(vec![vec![
            point(0.0, 0.0, 0);
            limits::INK_STROKE_POINTS + 1
        ]]);
        let over_total = params(vec![
            vec![point(0.0, 0.0, 0); limits::INK_STROKE_POINTS];
            limits::INK_TOTAL_POINTS / limits::INK_STROKE_POINTS + 1
        ]);
        let mut over_candidates = one_stroke();
        over_candidates.max_candidates = Some(limits::INK_CANDIDATES as u32 + 1);
        let mut over_languages = one_stroke();
        over_languages.languages = vec!["en".into(); limits::INK_LANGUAGES + 1];
        let mut over_tag = one_stroke();
        over_tag.languages = vec![format!("en-{}", "x".repeat(limits::INK_LANGUAGE_BYTES))];
        let far = params(vec![vec![point(
            limits::INK_COORDINATE as f64 + 1.0,
            0.0,
            0,
        )]]);
        let long = params(vec![vec![point(
            0.0,
            0.0,
            limits::INK_DURATION_MS as u32 + 1,
        )]]);
        for request in [
            over_strokes,
            over_stroke,
            over_total,
            over_candidates,
            over_languages,
            over_tag,
            far,
            long,
        ] {
            let refused = Writing::from_params(&request);
            assert!(
                matches!(refused, Err(Error::LimitExceeded(_))),
                "{refused:?}"
            );
        }
    }

    #[test]
    fn a_malformed_request_is_invalid_not_oversize() {
        let mut no_language = one_stroke();
        no_language.languages.clear();
        let mut bad_tag = one_stroke();
        bad_tag.languages = vec!["not a tag".into()];
        let mut zero = one_stroke();
        zero.max_candidates = Some(0);
        let mut not_finite = one_stroke();
        not_finite.strokes[0][0].x = f64::NAN;
        for request in [
            params(vec![]),
            params(vec![vec![]]),
            no_language,
            bad_tag,
            zero,
            not_finite,
        ] {
            assert!(matches!(
                Writing::from_params(&request),
                Err(Error::Invalid(_))
            ));
        }
    }

    #[test]
    fn language_tags_of_every_shape_a_client_sends_pass() {
        for tag in [
            "en",
            "pt-BR",
            "zh-Hans",
            "yue-Hant-HK",
            "es-419",
            "en-US-x-private",
        ] {
            assert!(language_tag(tag).is_ok(), "{tag}");
        }
        for tag in [
            "", "e", "en_US", "en-", "-en", "1en", "en--US", "pt BR", "açúcar",
        ] {
            assert!(language_tag(tag).is_err(), "{tag}");
        }
    }
}
