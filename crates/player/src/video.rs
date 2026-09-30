//! Music video, played by mpv itself.
//!
//! The picture is one more track of the audio file mpv is already playing (`video-add`), so picture
//! and sound run off one clock: a seek, a pause or a tempo change moves both, and there is nothing
//! to keep in step. mpv draws it through its render API into a GL surface the app owns. Nothing
//! here knows what that surface is (a GtkGLArea on Linux, `src-tauri/src/nativevideo.rs`): the app
//! hands in a GL loader and a wake-up, and calls [`VideoRenderer::render`] on its GL thread.
//!
//! Videos are keyed by the audio URL mpv was handed, never by position in mpv's playlist, so the
//! gapless-next track's video can be registered minutes early and attaches itself the moment that
//! file becomes the one playing.

use std::collections::VecDeque;
use std::ffi::c_void;
use std::sync::atomic::Ordering;
use std::sync::Arc;

use libmpv2::render::{OpenGLInitParams, RenderContext, RenderParam, RenderParamApiType};

use crate::{quoted, Decks, Error, Player};

/// The playing track and the gapless-next one are all that matter; the rest covers a skip landing
/// while a resolve is in flight.
const MAX_VIDEOS: usize = 4;

#[derive(Default)]
pub(crate) struct Videos {
    /// `(audio URL exactly as mpv was handed it, video URL)`, newest last.
    by_audio: VecDeque<(String, String)>,
    /// The file each deck has open, as of its last `FileLoaded`.
    loaded: [Option<String>; 2],
}

impl Videos {
    /// Remember `video` for `audio`, replacing an older entry for the same file.
    fn insert(&mut self, audio: &str, video: &str) {
        self.by_audio.retain(|(a, _)| a != audio);
        if self.by_audio.len() >= MAX_VIDEOS {
            self.by_audio.pop_front();
        }
        self.by_audio.push_back((audio.to_owned(), video.to_owned()));
    }

    fn video_for(&self, deck: usize) -> Option<String> {
        let path = self.loaded[deck].as_deref()?;
        self.by_audio.iter().rev().find(|(a, _)| a == path).map(|(_, v)| v.clone())
    }
}

/// The display connection the app's GL context runs on. mpv needs it for zero-copy hardware
/// decoding (VA-API); without it that falls back to decoding on the CPU, which still plays.
pub enum GlDisplay {
    X11(*const c_void),
    Wayland(*const c_void),
}

impl Player {
    /// `video_url` is the picture for the audio file mpv was (or will be) handed as `audio_url`.
    /// Attached now if that file is the one playing, otherwise when it starts.
    pub fn set_video_for(&self, audio_url: &str, video_url: &str) {
        let deck = self.decks.active.load(Ordering::SeqCst);
        let now = {
            let mut v = self.decks.videos.lock().unwrap();
            v.insert(audio_url, video_url);
            v.loaded[deck].as_deref() == Some(audio_url)
        };
        if now {
            add_video(&self.decks, deck, audio_url.to_owned(), video_url.to_owned());
        }
    }

    /// The file mpv is playing, exactly as it was handed it.
    pub fn current_path(&self) -> Option<String> {
        self.mpv().get_property::<String>("path").ok()
    }

    /// Whether anyone can see the picture. Off, mpv decodes no video at all; back on, it picks the
    /// track up again at the current position, already in step.
    pub fn set_video_visible(&self, on: bool) {
        if self.decks.video_visible.swap(on, Ordering::SeqCst) != on {
            apply_vid(&self.decks);
        }
    }

    /// The renderer, for the app's GL thread. `get_proc_address` resolves GL functions for the
    /// context that thread renders with; `wake` is called from any thread when there is a new frame
    /// to draw (or a new deck to give a context), and must get [`VideoRenderer::render`] called
    /// soon on the GL thread. Creates nothing yet: see [`VideoRenderer::ensure_contexts`].
    pub fn video_renderer(
        &self,
        get_proc_address: fn(&(), &str) -> *mut c_void,
        display: Option<GlDisplay>,
        wake: impl Fn() + Send + Sync + 'static,
    ) -> VideoRenderer {
        let wake: Arc<dyn Fn() + Send + Sync> = Arc::new(wake);
        let w = wake.clone();
        let _ = self.decks.on_new_deck.set(Box::new(move || w()));
        VideoRenderer {
            ctx: [None, None],
            decks: self.decks.clone(),
            get_proc_address,
            display,
            wake,
        }
    }
}

/// `vid` on every deck: on only for the one being heard, and only while someone can see it.
///
/// Under the `videos` lock, like the selection after a `video-add` (see [`add_video`]): the two
/// race, and whichever writes last has to be the one that read the current visibility.
pub(crate) fn apply_vid(decks: &Decks) {
    let _serial = decks.videos.lock().unwrap();
    let active = decks.active.load(Ordering::SeqCst);
    let on = decks.video_visible.load(Ordering::SeqCst);
    for deck in 0..2 {
        if let Some(m) = decks.mpv(deck) {
            let _ = m.set_property("vid", if on && deck == active { "auto" } else { "no" });
        }
    }
}

/// A deck finished opening a file. Reads which one on its own thread: the caller is the event
/// loop, and a synchronous property read there can stall the pump mid-transition.
pub(crate) fn file_loaded(decks: &Arc<Decks>, deck: usize) {
    let Some(mpv) = decks.mpv(deck).cloned() else { return };
    let decks = decks.clone();
    let _ = std::thread::Builder::new().name("mpv-video".into()).spawn(move || {
        let path = mpv.get_property::<String>("path").ok();
        let video = {
            let mut v = decks.videos.lock().unwrap();
            v.loaded[deck] = path.clone();
            (deck == decks.active.load(Ordering::SeqCst)).then(|| v.video_for(deck)).flatten()
        };
        if let (Some(audio), Some(url)) = (path, video) {
            add_video(&decks, deck, audio, url);
        }
    });
}

/// A crossfade made the other deck the one being heard: move the picture over with the sound.
pub(crate) fn deck_swapped(decks: &Arc<Decks>) {
    apply_vid(decks);
    let deck = decks.active.load(Ordering::SeqCst);
    let (audio, video) = {
        let v = decks.videos.lock().unwrap();
        (v.loaded[deck].clone(), v.video_for(deck))
    };
    if let (Some(audio), Some(url)) = (audio, video) {
        add_video(decks, deck, audio, url);
    }
}

/// `video-add`, off the calling thread: it returns only once mpv has opened the file, which is a
/// network round trip (mpv opens it on a thread of its own, so playback never waits on it).
///
/// Always added unselected, and selected afterwards by id if someone can see it by then. Choosing
/// `select` or `auto` up front read the visibility before the round trip: the view asking for the
/// picture during it set `vid=auto` while there was no track to pick, the track then arrived
/// unselected, and setting `auto` again is a no-op in mpv. That was the black box after a track
/// change or an opened link, which only closing and reopening the view (`no` then `auto`) cleared.
///
/// `audio` is the file the picture belongs to, which a failure names.
fn add_video(decks: &Arc<Decks>, deck: usize, audio: String, url: String) {
    let Some(mpv) = decks.mpv(deck).cloned() else { return };
    let decks = decks.clone();
    let _ = std::thread::Builder::new().name("mpv-video-add".into()).spawn(move || {
        if let Err(e) = mpv.command("video-add", &[&quoted(&url), "auto"]) {
            tracing::warn!(deck, error = %e, "video: mpv could not open the picture");
            let _ = decks.tx.send(crate::PlayerEvent::VideoFailed(audio));
            return;
        }
        let _serial = decks.videos.lock().unwrap();
        let shown = decks.video_visible.load(Ordering::SeqCst)
            && decks.active.load(Ordering::SeqCst) == deck;
        let track = shown.then(|| newest_video_track(&mpv)).flatten();
        let _ = match track {
            Some(id) => mpv.set_property("vid", id),
            None => mpv.set_property("vid", "no"),
        };
        tracing::debug!(deck, ?track, "video: attached");
    });
}

/// The id of the last video track in the file, which is the one `video-add` just appended.
fn newest_video_track(mpv: &libmpv2::Mpv) -> Option<i64> {
    let n = mpv.get_property::<i64>("track-list/count").ok()?;
    (0..n).rev().find_map(|i| {
        let kind = mpv.get_property::<String>(&format!("track-list/{i}/type")).ok()?;
        if kind != "video" {
            return None;
        }
        mpv.get_property::<i64>(&format!("track-list/{i}/id")).ok()
    })
}

/// mpv's render contexts, one per deck, living on the app's GL thread.
pub struct VideoRenderer {
    // Before `decks`: fields drop in order, and mpv requires every render context to be freed
    // before its core is destroyed, which dropping the last `Decks` would do.
    ctx: [Option<RenderContext>; 2],
    decks: Arc<Decks>,
    get_proc_address: fn(&(), &str) -> *mut c_void,
    display: Option<GlDisplay>,
    wake: Arc<dyn Fn() + Send + Sync>,
}

impl VideoRenderer {
    /// Give every deck that exists a render context. On the GL thread, with its context current,
    /// and before any picture is asked for: a deck with no context cannot start its video output.
    pub fn ensure_contexts(&mut self) -> Result<(), Error> {
        for deck in 0..2 {
            if self.ctx[deck].is_some() {
                continue;
            }
            let Some(mpv) = self.decks.mpv(deck).cloned() else { continue };
            let mut params = vec![
                RenderParam::ApiType(RenderParamApiType::OpenGl),
                RenderParam::InitParams(OpenGLInitParams {
                    get_proc_address: self.get_proc_address,
                    ctx: (),
                }),
            ];
            match self.display {
                Some(GlDisplay::X11(p)) => params.push(RenderParam::X11Display(p)),
                Some(GlDisplay::Wayland(p)) => params.push(RenderParam::WaylandDisplay(p)),
                None => {}
            }
            // SAFETY: the handle is live for as long as `self.decks` is, and the field order above
            // frees this context first.
            let handle = unsafe { &mut *mpv.ctx.as_ptr() };
            let mut ctx = RenderContext::new(handle, params)?;
            let wake = self.wake.clone();
            ctx.set_update_callback(move || wake());
            self.ctx[deck] = Some(ctx);
        }
        Ok(())
    }

    /// Draw the playing deck's current frame into `fbo` (`width` x `height` pixels). On the GL
    /// thread, with its context current.
    pub fn render(&mut self, fbo: i32, width: i32, height: i32) -> Result<(), Error> {
        self.ensure_contexts()?;
        let deck = self.decks.active.load(Ordering::SeqCst);
        if let Some(ctx) = &self.ctx[deck] {
            // Flipped: a GL framebuffer's origin is bottom-left, a video frame's top-left.
            ctx.render::<()>(fbo, width, height, true)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A video belongs to the file a deck has open, by exact URL, and a re-resolve replaces it.
    #[test]
    fn a_video_follows_its_audio_file() {
        let mut v = Videos::default();
        v.insert("a1", "v1");
        v.insert("a2", "v2");
        assert_eq!(v.video_for(0), None, "nothing loaded yet");
        v.loaded[0] = Some("a2".into());
        v.loaded[1] = Some("a1".into());
        assert_eq!(v.video_for(0).as_deref(), Some("v2"));
        assert_eq!(v.video_for(1).as_deref(), Some("v1"));
        v.insert("a2", "v2b");
        assert_eq!(v.video_for(0).as_deref(), Some("v2b"));
        assert_eq!(v.by_audio.len(), 2);
        for i in 0..MAX_VIDEOS {
            v.insert(&format!("x{i}"), "x");
        }
        assert_eq!(v.by_audio.len(), MAX_VIDEOS);
        assert_eq!(v.video_for(0), None, "evicted by newer tracks");
    }
}
