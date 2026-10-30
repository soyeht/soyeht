//! The engine's own picture of what the pane shows.

use super::{reply_with, view::ViewCtx};
use crate::{Error, Result};
use std::{
    rc::{Rc, Weak},
    sync::mpsc::Sender,
};
use webview2_com::{
    CapturePreviewCompletedHandler,
    Microsoft::Web::WebView2::Win32::COREWEBVIEW2_CAPTURE_PREVIEW_IMAGE_FORMAT_PNG,
};
use windows::Win32::{
    Foundation::HGLOBAL,
    System::Com::{
        IStream, STATFLAG_NONAME, STATSTG, STREAM_SEEK_SET,
        StructuredStorage::CreateStreamOnHGlobal,
    },
};

/// The most a picture of a page may weigh as a PNG, in bytes.
const MAX_PNG_BYTES: u64 = 64 * 1024 * 1024;

/// Takes the picture and sends it, as the PNG the engine made and the scale it was drawn at, to
/// `reply` when the engine has made it.
pub(super) fn capture(view: &Rc<ViewCtx>, reply: Sender<Result<(Vec<u8>, u32)>>) {
    // SAFETY: a stream on memory of its own, which it frees with its last reference.
    let Ok(stream) = (unsafe { CreateStreamOnHGlobal(HGLOBAL::default(), true) }) else {
        return reply_with(&reply, Err(Error::Page("the page could not be captured")));
    };
    let scale = view.device_scale();
    let read = stream.clone();
    let answered = reply.clone();
    // A page that is not on screen is shown off screen for the length of its picture.
    let revealed = view.reveal();
    let weak = Rc::downgrade(view);
    let handler = CapturePreviewCompletedHandler::create(Box::new(move |result| {
        if revealed && let Some(view) = Weak::upgrade(&weak) {
            view.conceal();
        }
        let png = result
            .map_err(|_| Error::Page("the page could not be captured"))
            .and_then(|()| read_stream(&read))
            .map(|png| (png, scale));
        reply_with(&answered, png);
        Ok(())
    }));
    // SAFETY: the stream and the handler are the call's own; the web view is alive.
    let started = unsafe {
        view.webview.CapturePreview(
            COREWEBVIEW2_CAPTURE_PREVIEW_IMAGE_FORMAT_PNG,
            &stream,
            &handler,
        )
    };
    if started.is_err() {
        if revealed {
            view.conceal();
        }
        reply_with(&reply, Err(Error::Page("the page could not be captured")));
    }
}

/// Everything the engine wrote to `stream`.
fn read_stream(stream: &IStream) -> Result<Vec<u8>> {
    let failed = || Error::Page("the picture could not be read");
    let mut stat = STATSTG::default();
    // SAFETY: the stream is alive and the structure is ours.
    unsafe {
        stream
            .Stat(&raw mut stat, STATFLAG_NONAME)
            .map_err(|_| failed())?;
        stream
            .Seek(0, STREAM_SEEK_SET, None)
            .map_err(|_| failed())?;
    }
    if stat.cbSize == 0 || stat.cbSize > MAX_PNG_BYTES {
        return Err(failed());
    }
    let length = usize::try_from(stat.cbSize).map_err(|_| failed())?;
    let mut bytes = vec![0_u8; length];
    let mut read = 0_u32;
    // SAFETY: the buffer is `length` bytes, which is what the call may write; `read` is ours.
    let result = unsafe {
        stream.Read(
            bytes.as_mut_ptr().cast(),
            u32::try_from(length).map_err(|_| failed())?,
            Some(&raw mut read),
        )
    };
    if result.is_err() || usize::try_from(read).ok() != Some(length) {
        return Err(failed());
    }
    Ok(bytes)
}
