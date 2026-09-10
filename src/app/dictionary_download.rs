use super::*;

use crate::spellcheck::SpellCheckLanguage;

impl SmaragdApp {
    /// Kick off downloading `language`'s real dictionary (see
    /// `spellcheck::download_dictionary`) on a background thread — real network
    /// I/O, so it never runs synchronously on the UI thread, mirroring
    /// `spawn_git_operation`. Refuses to start a second download while one is
    /// already in flight rather than queuing or racing it.
    #[cfg(not(target_arch = "wasm32"))]
    pub(super) fn spawn_dictionary_download(
        &mut self,
        ctx: &egui::Context,
        language: SpellCheckLanguage,
    ) {
        if self.pending_dictionary_download.is_some() {
            self.push_error_toast("A dictionary download is already in progress");
            return;
        }
        let (sender, receiver) = std::sync::mpsc::channel();
        let repaint_ctx = ctx.clone();
        std::thread::spawn(move || {
            let result = crate::spellcheck::download_dictionary(language);
            let _ = sender.send(result);
            repaint_ctx.request_repaint();
        });
        self.set_status_message(format!("Downloading {}…", language.label()));
        self.pending_dictionary_download = Some((language, receiver));
    }

    /// No threads on wasm32 (no atomics/shared memory in this build), and
    /// `spellcheck::download_dictionary` already fails fast there anyway (no
    /// `fetch`-based networking wired up yet — see its own wasm32 stub) — so
    /// there's nothing to actually background; just call it synchronously and
    /// report the result immediately instead of spawning a thread that would
    /// panic on this target.
    #[cfg(target_arch = "wasm32")]
    pub(super) fn spawn_dictionary_download(
        &mut self,
        _ctx: &egui::Context,
        language: SpellCheckLanguage,
    ) {
        match crate::spellcheck::download_dictionary(language) {
            Ok(()) => {
                crate::spellcheck::invalidate_dictionary_cache(language);
                self.set_status_message(format!("Downloaded {} dictionary", language.label()));
            }
            Err(err) => {
                self.push_error_toast(format!("Couldn't download {}: {err}", language.label()));
            }
        }
    }

    /// Check whether the in-flight `pending_dictionary_download` (if any) has
    /// finished, and apply its result: on success, drop the language's
    /// memoized dictionary (`spellcheck::invalidate_dictionary_cache`) so the
    /// next spell check picks up the freshly downloaded file instead of
    /// whatever was already cached in memory (the bundled placeholder, most
    /// likely). Called every frame; a no-op whenever nothing is pending or the
    /// background thread hasn't sent its result yet.
    pub(super) fn poll_dictionary_download(&mut self) {
        let Some((_, receiver)) = &self.pending_dictionary_download else {
            return;
        };
        let result = match receiver.try_recv() {
            Ok(result) => result,
            Err(std::sync::mpsc::TryRecvError::Empty) => return,
            Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                let (language, _) = self
                    .pending_dictionary_download
                    .take()
                    .expect("checked above");
                self.push_error_toast(format!(
                    "Downloading {}: background thread panicked",
                    language.label()
                ));
                return;
            }
        };
        let (language, _) = self
            .pending_dictionary_download
            .take()
            .expect("checked above");
        match result {
            Ok(()) => {
                crate::spellcheck::invalidate_dictionary_cache(language);
                self.set_status_message(format!("Downloaded {} dictionary", language.label()));
            }
            Err(err) => {
                self.push_error_toast(format!("Couldn't download {}: {err}", language.label()));
            }
        }
    }
}
