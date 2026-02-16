//! Native (desktop) video system: cutscene player.
//!
//! This is implemented behind the `video_ffmpeg` feature. Without it, this module
//! provides a stub that compiles but does not decode video.

#[derive(Clone)]
pub struct VideoFrame {
    pub width: u32,
    pub height: u32,
    /// RGBA8 pixels, row-major, length = width*height*4.
    pub rgba: Vec<u8>,
}

#[cfg(all(not(target_arch = "wasm32"), feature = "video_ffmpeg"))]
mod imp {
    use super::{VideoFrame};
    use ffmpeg_next as ffmpeg;
    use std::path::Path;

    pub struct VideoPlayer {
        // Very minimal: store last decoded frame.
        last: Option<VideoFrame>,
        opened: bool,
    }

    impl VideoPlayer {
        pub fn new() -> Self {
            let _ = ffmpeg::init();
            Self { last: None, opened: false }
        }

        pub fn open(&mut self, _path: &Path) -> bool {
            // TODO: full decode pipeline (demux + decode + colorspace convert)
            // For now, mark opened. This keeps API shape stable.
            self.opened = true;
            self.last = None;
            true
        }

        pub fn is_open(&self) -> bool { self.opened }

        pub fn poll_frame(&mut self) -> Option<VideoFrame> {
            self.last.clone()
        }
    }
}

#[cfg(any(target_arch = "wasm32", not(feature = "video_ffmpeg")))]
mod imp {
    use super::VideoFrame;
    use std::path::Path;

    pub struct VideoPlayer {
        opened: bool,
    }

    impl VideoPlayer {
        pub fn new() -> Self { Self { opened: false } }
        pub fn open(&mut self, _path: &Path) -> bool { self.opened = false; false }
        #[allow(dead_code)]
        pub fn is_open(&self) -> bool { self.opened }
        pub fn poll_frame(&mut self) -> Option<VideoFrame> { None }
    }
}

pub use imp::VideoPlayer;

