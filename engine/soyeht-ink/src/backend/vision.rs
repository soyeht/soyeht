//! Apple Vision reads the image on the Mac (ADR 0158). It runs on the machine: Vision sends
//! nothing anywhere, and the language data is part of the system, so there is nothing to install.
//!
//! The bindings are the `objc2` family (MIT or Apache-2.0), called in process: no helper program
//! starts, and the engine needs no entitlement or permission for it. An Objective-C object lives
//! only inside one call, in an autorelease pool, so nothing of a request outlives it.

use crate::{Bitmap, Candidate, Error, Piece, Recognizer, Result, candidates::reading_order};
use objc2::{
    AnyThread,
    rc::{Retained, autoreleasepool},
    runtime::AnyObject,
};
use objc2_core_foundation::CFData;
use objc2_core_graphics::{
    CGBitmapInfo, CGColorRenderingIntent, CGColorSpace, CGDataProvider, CGImage,
};
use objc2_foundation::{NSArray, NSDictionary, NSString};
use objc2_vision::{
    VNImageRequestHandler, VNRecognizeTextRequest, VNRecognizedTextObservation, VNRequest,
    VNRequestTextRecognitionLevel,
};
use soyeht_protocol::InkUnsupportedReason;

const ID: &str = "apple-vision";

pub(crate) fn system() -> Result<Box<dyn Recognizer>> {
    Ok(Box::new(Vision::default()))
}

#[derive(Default)]
struct Vision {
    /// What the system reads, asked once: it does not change while the engine runs.
    languages: Option<Vec<String>>,
}

impl Recognizer for Vision {
    fn id(&self) -> &'static str {
        ID
    }

    fn languages(&mut self) -> Result<Vec<String>> {
        if let Some(known) = &self.languages {
            return Ok(known.clone());
        }
        let asked = autoreleasepool(|_| {
            let request = accurate_request();
            // SAFETY: a plain query of the request's class state; it takes no pointers and
            // returns the error as a value.
            unsafe { request.supportedRecognitionLanguagesAndReturnError() }
                .map(|tags| tags.iter().map(|tag| tag.to_string()).collect::<Vec<_>>())
        })
        // An old system without the accurate level has no Vision that reads text.
        .map_err(|_| Error::Unsupported(InkUnsupportedReason::NoOsSupport))?;
        self.languages = Some(asked.clone());
        Ok(asked)
    }

    fn read(
        &mut self,
        image: &Bitmap,
        languages: &[String],
        most: usize,
    ) -> Result<Vec<Candidate>> {
        autoreleasepool(|_| {
            let picture = picture(image)?;
            let request = accurate_request();
            let tags = languages
                .iter()
                .map(|tag| NSString::from_str(tag))
                .collect::<Vec<_>>();
            request.setRecognitionLanguages(&NSArray::from_retained_slice(&tags));
            request.setUsesLanguageCorrection(true);
            // SAFETY: `NSDictionary::new` is an empty dictionary, which has no keys or values
            // of any type to get wrong; the handler copies the image reference it is given.
            let handler = unsafe {
                VNImageRequestHandler::initWithCGImage_options(
                    VNImageRequestHandler::alloc(),
                    &picture,
                    &NSDictionary::<NSString, AnyObject>::new(),
                )
            };
            let as_request: Retained<VNRequest> =
                Retained::into_super(Retained::into_super(request.clone()));
            handler
                .performRequests_error(&NSArray::from_retained_slice(&[as_request]))
                .map_err(|_| Error::Failed("The text recognition request failed"))?;
            let found = request
                .results()
                .map(|list| list.to_vec())
                .unwrap_or_default();
            Ok(candidates_of(&found, most))
        })
    }
}

fn accurate_request() -> Retained<VNRecognizeTextRequest> {
    let request = VNRecognizeTextRequest::new();
    request.setRecognitionLevel(VNRequestTextRecognitionLevel::Accurate);
    request
}

/// The image as Core Graphics holds it: the same eight-bit gray, top row first.
fn picture(image: &Bitmap) -> Result<objc2_core_foundation::CFRetained<CGImage>> {
    let failed = || Error::Failed("The image could not be built");
    let space = CGColorSpace::new_device_gray().ok_or_else(failed)?;
    let provider = CGDataProvider::with_cf_data(Some(&CFData::from_bytes(&image.pixels)))
        .ok_or_else(failed)?;
    // SAFETY: `decode` is null, which the function allows; every other argument is a value
    // or a reference that lives through the call, and the provider owns a copy of the pixels.
    unsafe {
        CGImage::new(
            image.width,
            image.height,
            8,
            8,
            image.width,
            Some(&space),
            CGBitmapInfo::empty(),
            Some(&provider),
            std::ptr::null(),
            false,
            CGColorRenderingIntent::RenderingIntentDefault,
        )
    }
    .ok_or_else(failed)
}

/// One observation is one line or one word: its alternatives are the candidates. Several
/// are put in reading order and joined into one candidate.
fn candidates_of(found: &[Retained<VNRecognizedTextObservation>], most: usize) -> Vec<Candidate> {
    if let [only] = found {
        return only
            .topCandidates(most)
            .iter()
            .map(|text| Candidate {
                text: text.string().to_string(),
                score: Some(f64::from(text.confidence())),
            })
            .collect();
    }
    let pieces = found
        .iter()
        .filter_map(|observation| {
            let top = observation.topCandidates(1).firstObject()?;
            // SAFETY: a plain getter of the observation's box, a value with no pointers.
            let frame = unsafe { observation.boundingBox() };
            Some(Piece {
                text: top.string().to_string(),
                score: Some(f64::from(top.confidence())),
                left: frame.origin.x,
                // Vision's origin is the bottom left corner; the pieces are read from the top.
                top: 1.0 - frame.origin.y - frame.size.height,
                height: frame.size.height,
            })
        })
        .collect();
    reading_order(pieces).into_iter().collect()
}

#[cfg(test)]
mod tests;
