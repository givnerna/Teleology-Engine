//! Native (desktop) audio system.
//!
//! Minimal goals:
//! - Play SFX/music from file paths
//! - Master volume
//! - Stop/pause via handle (optional later)

#[cfg(not(target_arch = "wasm32"))]
mod imp {
    use kira::manager::{AudioManager, AudioManagerSettings};
    use kira::sound::static_sound::StaticSoundData;
    use kira::sound::Region;
    use kira::tween::Tween;
    use std::collections::HashMap;
    use std::path::Path;

    #[derive(Default)]
    pub struct AudioSystem {
        mgr: Option<AudioManager>,
        next_handle: u32,
        // Keep sound data cached by path (simple).
        cache: HashMap<String, StaticSoundData>,
    }

    impl AudioSystem {
        pub fn new() -> Self {
            let mgr = AudioManager::new(AudioManagerSettings::default()).ok();
            Self {
                mgr,
                next_handle: 1,
                cache: HashMap::new(),
            }
        }

        pub fn is_available(&self) -> bool {
            self.mgr.is_some()
        }

        pub fn play_file(&mut self, path: &Path, looping: bool, volume: f32) -> u32 {
            let Some(mgr) = self.mgr.as_mut() else { return 0 };
            let key = path.to_string_lossy().to_string();
            let data = if let Some(d) = self.cache.get(&key).cloned() {
                d
            } else {
                let Ok(d) = StaticSoundData::from_file(path) else { return 0 };
                let d = d
                    .loop_region(if looping {
                        Some(Region::from(..))
                    } else {
                        None
                    })
                    .volume(volume as f64);
                self.cache.insert(key.clone(), d.clone());
                d
            };
            let _ = mgr.play(data);
            let h = self.next_handle.max(1);
            self.next_handle = h.saturating_add(1);
            h
        }

        pub fn set_master_volume(&mut self, volume: f32) {
            let Some(mgr) = self.mgr.as_mut() else { return };
            mgr.main_track()
                .set_volume(volume as f64, Tween::default());
        }
    }
}

#[cfg(target_arch = "wasm32")]
mod imp {
    use std::path::Path;

    #[derive(Default)]
    pub struct AudioSystem;

    impl AudioSystem {
        pub fn new() -> Self {
            Self
        }
        pub fn is_available(&self) -> bool {
            false
        }
        pub fn play_file(&mut self, _path: &Path, _looping: bool, _volume: f32) -> u32 {
            0
        }
        pub fn set_master_volume(&mut self, _volume: f32) {}
    }
}

pub use imp::AudioSystem;

