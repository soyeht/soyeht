//! Attaching files to the composer of an agent pane: from the "+" button's file picker, dropped on
//! the pane and pasted. Staging a file (reading it, preparing an image for the agent) happens off
//! the interface's thread; the chips appear when it is done.

use crate::workbench::Workbench;
use gpui::{AppContext, ClipboardEntry, ClipboardItem, Context, PathPromptOptions, Task};
use soyeht_client::attachments::{
    Draft, IMAGE_FALLBACK_NAME, PasteOffer, Rejection, Staged, stage_image, stage_path,
};
use soyeht_client::state::problem::kind;
use std::path::PathBuf;

type Staging = Vec<Result<Draft, Rejection>>;

impl Workbench {
    pub(crate) fn staged(&self, pane_id: &str) -> Option<&Staged> {
        self.panes.get(pane_id)?.staged.as_ref()
    }

    /// The "+" of the composer: the system's file picker.
    pub(crate) fn pick_attachments(&mut self, pane_id: &str, cx: &mut Context<Self>) {
        let picked = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: true,
            prompt: None,
        });
        let pane_id = pane_id.to_owned();
        cx.spawn(async move |this, cx| {
            let outcome = picked.await;
            // Once the window is gone the update fails, and nobody waits for the answer.
            this.update(cx, |this, cx| match outcome {
                Ok(Ok(Some(paths))) => this.attach_paths(&pane_id, paths, cx),
                Ok(Ok(None)) => {}
                Ok(Err(error)) => this.report(
                    kind::ATTACH_PICKER_FAILED,
                    format!("The file picker could not open: {error}"),
                    cx,
                ),
                // ignored: the picker was dropped before it answered: the person is told so.
                Err(_) => this.report(
                    kind::ATTACH_PICKER_CLOSED,
                    "The file picker closed without answering.".to_owned(),
                    cx,
                ),
            })
        })
        .detach();
    }

    /// Files picked or dropped. A file on this machine is linked where it is, as the Mac links
    /// the files of the machine its engine runs on.
    pub(crate) fn attach_paths(
        &mut self,
        pane_id: &str,
        paths: Vec<PathBuf>,
        cx: &mut Context<Self>,
    ) {
        let work = cx.background_spawn(async move {
            paths.iter().map(|path| stage_path(path, true)).collect()
        });
        self.finish_staging(pane_id, work, cx);
    }

    /// Pastes into an agent's composer: copied files and pictures become attachments; anything
    /// else is text.
    pub(crate) fn paste_into_composer(
        &mut self,
        pane_id: &str,
        item: &ClipboardItem,
        cx: &mut Context<Self>,
    ) {
        if self.attach_pasted(pane_id, item, cx) {
            return;
        }
        if let Some(text) = item.text() {
            let field = self.composer_field(pane_id, cx);
            field.update(cx, |field, cx| field.insert(&text, cx));
        }
    }

    /// A paste that carries files or a picture. False leaves it to the text field.
    pub(crate) fn attach_pasted(
        &mut self,
        pane_id: &str,
        item: &ClipboardItem,
        cx: &mut Context<Self>,
    ) -> bool {
        let mut files = Vec::new();
        let (mut image, mut text) = (None, false);
        for entry in item.entries() {
            match entry {
                ClipboardEntry::ExternalPaths(paths) => files.extend(paths.paths().iter().cloned()),
                ClipboardEntry::Image(held) if image.is_none() => image = Some(held.clone()),
                ClipboardEntry::String(held) if !held.text().is_empty() => text = true,
                _ => {}
            }
        }
        let offer = PasteOffer {
            files: files.len(),
            image: image.is_some(),
            text,
        };
        if !offer.offers_attachments() {
            return false;
        }
        match (files.is_empty(), image) {
            (false, _) => self.attach_paths(pane_id, files, cx),
            (true, Some(image)) => {
                let work = cx.background_spawn(async move {
                    vec![
                        stage_image(&image.bytes, image.format.mime_type(), None).ok_or(
                            Rejection::Unreadable {
                                name: IMAGE_FALLBACK_NAME.to_owned(),
                            },
                        ),
                    ]
                });
                self.finish_staging(pane_id, work, cx);
            }
            (true, None) => {}
        }
        true
    }

    pub(crate) fn remove_attachment(&mut self, pane_id: &str, id: &str, cx: &mut Context<Self>) {
        if let Some(staged) = self
            .panes
            .get_mut(pane_id)
            .and_then(|ui| ui.staged.as_mut())
        {
            staged.remove(id);
            cx.notify();
        }
    }

    fn finish_staging(&mut self, pane_id: &str, work: Task<Staging>, cx: &mut Context<Self>) {
        let pane_id = pane_id.to_owned();
        cx.spawn(async move |this, cx| {
            let results = work.await;
            // Once the window is gone the update fails, and nobody waits for the files.
            this.update(cx, |this, cx| {
                this.panes
                    .of(&pane_id)
                    .staged
                    .get_or_insert_default()
                    .add(results);
                cx.notify();
            })
        })
        .detach();
    }

    /// Says something went wrong with a request the person made, in the banner under the top bar.
    pub(crate) fn report(&mut self, kind: &str, message: String, cx: &mut Context<Self>) {
        self.state.report(kind, message);
        cx.notify();
    }
}
