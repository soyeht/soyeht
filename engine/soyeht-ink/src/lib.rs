//! Reads handwriting (ADR 0158, `docs/contracts/14-handwriting-recognition.md`).
//!
//! A client sends the strokes of a pen; the engine draws them as an image ([`raster`]) and the
//! recognizer of the system reads the image: Apple Vision on the Mac, and on a system that has
//! none, a typed refusal. Everything but the last step is the same code on every system, and the
//! last step has the same public shape on every system ([`system`]).
//!
//! Nothing here logs, stores or keeps the strokes or the text. A recognizer is borrowed for the
//! length of one request and holds nothing of it.
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used, clippy::panic))]

mod backend;
pub mod candidates;
mod error;
pub mod raster;
mod writing;

pub use candidates::{Candidate, Piece};
pub use error::{Error, Result};
pub use raster::Bitmap;
pub use writing::{Dot, Writing};

use soyeht_protocol::InkRecognizeResult;

/// A way to read an image of handwriting. One value serves one request at a time.
pub trait Recognizer: Send {
    /// Names the recognizer in the answer (`apple-vision`).
    fn id(&self) -> &'static str;

    /// The language tags it reads, as it names them.
    fn languages(&mut self) -> Result<Vec<String>>;

    /// The candidates for the text in `image`, the likeliest first, when it reads `languages`
    /// (tags from [`Recognizer::languages`]). Nothing legible is an empty list, not an error.
    fn read(&mut self, image: &Bitmap, languages: &[String], most: usize)
    -> Result<Vec<Candidate>>;
}

/// The recognizer of this system, or why there is none. The same on every system.
pub fn system() -> Result<Box<dyn Recognizer>> {
    backend::system()
}

/// Reads `writing` with `recognizer`: resolve the languages, draw, recognize, tidy the answer.
/// It refuses to run on a thread that holds the engine's Core lock (ADR 0079): drawing and
/// recognizing take milliseconds to seconds.
pub fn recognize(recognizer: &mut dyn Recognizer, writing: &Writing) -> Result<InkRecognizeResult> {
    if let Some(holder) = soyeht_process::forbidden() {
        return Err(Error::UnderLock(holder));
    }
    let languages = candidates::languages(&writing.languages, &recognizer.languages()?)?;
    let image = raster::render(writing);
    let read = recognizer.read(&image, &languages, writing.max_candidates)?;
    Ok(InkRecognizeResult {
        candidates: candidates::tidy(read, writing.max_candidates),
        recognizer: recognizer.id().to_owned(),
        language: languages[0].clone(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use soyeht_protocol::{InkPoint, InkRecognizeParams};

    /// Reads whatever it is told to, and notes what it was given.
    struct Scripted {
        offered: Vec<String>,
        says: Vec<Candidate>,
        seen: Vec<(usize, usize, Vec<String>, usize)>,
    }

    impl Recognizer for Scripted {
        fn id(&self) -> &'static str {
            "scripted"
        }

        fn languages(&mut self) -> Result<Vec<String>> {
            Ok(self.offered.clone())
        }

        fn read(
            &mut self,
            image: &Bitmap,
            languages: &[String],
            most: usize,
        ) -> Result<Vec<Candidate>> {
            self.seen
                .push((image.width, image.height, languages.to_vec(), most));
            Ok(self.says.clone())
        }
    }

    fn scripted(says: &[&str]) -> Scripted {
        Scripted {
            offered: vec!["en-US".into(), "pt-BR".into()],
            says: says
                .iter()
                .map(|text| Candidate {
                    text: (*text).into(),
                    score: Some(0.5),
                })
                .collect(),
            seen: Vec::new(),
        }
    }

    fn writing(languages: &[&str], most: Option<u32>) -> Writing {
        Writing::from_params(&InkRecognizeParams {
            strokes: vec![vec![
                InkPoint {
                    x: 0.0,
                    y: 0.0,
                    t: 0,
                },
                InkPoint {
                    x: 4.0,
                    y: 9.0,
                    t: 20,
                },
            ]],
            languages: languages.iter().map(|tag| (*tag).to_owned()).collect(),
            max_candidates: most,
        })
        .unwrap()
    }

    #[test]
    fn a_request_is_drawn_read_in_the_resolved_languages_and_answered_tidily() {
        let mut recognizer = scripted(&[" ola ", "ola", "ala", "oca"]);
        let answer = recognize(&mut recognizer, &writing(&["pt-PT", "en"], Some(2))).unwrap();
        assert_eq!(answer.recognizer, "scripted");
        assert_eq!(answer.language, "pt-BR");
        assert_eq!(
            answer
                .candidates
                .iter()
                .map(|c| c.text.as_str())
                .collect::<Vec<_>>(),
            ["ola", "ala"]
        );
        let (width, height, languages, most) = recognizer.seen[0].clone();
        assert!(width > 0 && height > 0);
        assert_eq!(languages, ["pt-BR", "en-US"]);
        assert_eq!(most, 2);
    }

    #[test]
    fn nothing_legible_is_an_answer_with_no_candidates() {
        let mut recognizer = scripted(&[]);
        let answer = recognize(&mut recognizer, &writing(&["en"], None)).unwrap();
        assert!(answer.candidates.is_empty());
    }

    #[test]
    fn a_language_nobody_reads_never_reaches_the_image() {
        let mut recognizer = scripted(&["x"]);
        assert_eq!(
            recognize(&mut recognizer, &writing(&["sv"], None)),
            Err(Error::LanguageUnavailable)
        );
        assert!(recognizer.seen.is_empty());
    }

    #[test]
    fn a_thread_that_holds_the_core_lock_is_refused_before_any_work() {
        let mut recognizer = scripted(&["x"]);
        let _held = soyeht_process::forbid("Core");
        assert_eq!(
            recognize(&mut recognizer, &writing(&["en"], None)),
            Err(Error::UnderLock("Core"))
        );
        assert!(recognizer.seen.is_empty());
    }

    #[test]
    fn a_failing_recognizer_fails_the_request_and_says_where() {
        struct Broken;
        impl Recognizer for Broken {
            fn id(&self) -> &'static str {
                "broken"
            }
            fn languages(&mut self) -> Result<Vec<String>> {
                Ok(vec!["en".into()])
            }
            fn read(&mut self, _: &Bitmap, _: &[String], _: usize) -> Result<Vec<Candidate>> {
                Err(Error::Failed("The scripted recognizer broke"))
            }
        }
        assert_eq!(
            recognize(&mut Broken, &writing(&["en"], None)),
            Err(Error::Failed("The scripted recognizer broke"))
        );
    }
}
