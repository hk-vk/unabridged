//! Small main-thread AVFoundation player for downloaded audio.
#[cfg(target_os = "macos")]
mod platform {
    use std::os::unix::ffi::OsStrExt;
    use std::{
        ffi::{CString, c_char, c_void},
        path::Path,
        ptr::NonNull,
    };

    unsafe extern "C" {
        fn ba_player_open(path: *const c_char) -> *mut c_void;
        fn ba_player_close(player: *mut c_void);
        fn ba_player_play(player: *mut c_void) -> bool;
        fn ba_player_pause(player: *mut c_void);
        fn ba_player_position(player: *mut c_void) -> f64;
        fn ba_player_duration(player: *mut c_void) -> f64;
        fn ba_player_seek(player: *mut c_void, time: f64);
        fn ba_player_is_playing(player: *mut c_void) -> bool;
    }
    pub struct Player(NonNull<c_void>);
    impl Player {
        pub fn open(path: &Path) -> anyhow::Result<Self> {
            let path = CString::new(path.as_os_str().as_bytes())?;
            // The returned object is retained until Drop; all calls occur on the UI thread.
            let ptr = unsafe { ba_player_open(path.as_ptr()) };
            Ok(Self(NonNull::new(ptr).ok_or_else(|| {
                anyhow::anyhow!("macOS could not open this audio file")
            })?))
        }
        pub fn play(&self) -> anyhow::Result<()> {
            anyhow::ensure!(
                unsafe { ba_player_play(self.0.as_ptr()) },
                "macOS could not start audio playback"
            );
            Ok(())
        }
        pub fn pause(&self) {
            unsafe { ba_player_pause(self.0.as_ptr()) }
        }
        pub fn position(&self) -> f64 {
            unsafe { ba_player_position(self.0.as_ptr()) }
        }
        pub fn duration(&self) -> f64 {
            unsafe { ba_player_duration(self.0.as_ptr()) }
        }
        pub fn seek(&self, seconds: f64) {
            unsafe { ba_player_seek(self.0.as_ptr(), seconds.clamp(0., self.duration())) }
        }
        pub fn is_playing(&self) -> bool {
            unsafe { ba_player_is_playing(self.0.as_ptr()) }
        }
    }
    impl Drop for Player {
        fn drop(&mut self) {
            unsafe { ba_player_close(self.0.as_ptr()) }
        }
    }
}
#[cfg(target_os = "macos")]
pub use platform::Player;

#[cfg(not(target_os = "macos"))]
pub struct Player;
#[cfg(not(target_os = "macos"))]
impl Player {
    pub fn open(_: &std::path::Path) -> anyhow::Result<Self> {
        anyhow::bail!("Playback is currently available on macOS only")
    }
    pub fn play(&self) -> anyhow::Result<()> {
        anyhow::bail!("Playback is currently available on macOS only")
    }
    pub fn pause(&self) {}
    pub fn position(&self) -> f64 {
        0.
    }
    pub fn duration(&self) -> f64 {
        0.
    }
    pub fn seek(&self, _: f64) {}
    pub fn is_playing(&self) -> bool {
        false
    }
}

#[cfg(all(test, target_os = "macos"))]
mod tests {
    use super::Player;
    use std::io::Write;

    #[test]
    fn opens_a_generated_m4b_when_ffmpeg_is_available() {
        if std::process::Command::new("ffmpeg")
            .arg("-version")
            .output()
            .is_err()
        {
            return;
        }
        let path =
            std::env::temp_dir().join(format!("better-audible-player-{}.m4b", std::process::id()));
        let status = std::process::Command::new("ffmpeg")
            .args([
                "-loglevel",
                "error",
                "-y",
                "-f",
                "lavfi",
                "-i",
                "anullsrc=r=44100:cl=mono",
                "-t",
                "1",
                "-c:a",
                "aac",
            ])
            .arg(&path)
            .status()
            .unwrap();
        assert!(status.success());
        let player = Player::open(&path).unwrap();
        assert!((player.duration() - 1.).abs() < 0.2);
        player.seek(0.5);
        assert!((player.position() - 0.5).abs() < 0.1);
        drop(player);
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn opens_and_seeks_a_local_pcm_file() {
        let path =
            std::env::temp_dir().join(format!("better-audible-player-{}.wav", std::process::id()));
        let mut file = std::fs::File::create(&path).unwrap();
        let data_size = 16_000u32 * 2;
        file.write_all(b"RIFF").unwrap();
        file.write_all(&(36 + data_size).to_le_bytes()).unwrap();
        file.write_all(b"WAVEfmt ").unwrap();
        file.write_all(&16u32.to_le_bytes()).unwrap();
        file.write_all(&1u16.to_le_bytes()).unwrap();
        file.write_all(&1u16.to_le_bytes()).unwrap();
        file.write_all(&16_000u32.to_le_bytes()).unwrap();
        file.write_all(&32_000u32.to_le_bytes()).unwrap();
        file.write_all(&2u16.to_le_bytes()).unwrap();
        file.write_all(&16u16.to_le_bytes()).unwrap();
        file.write_all(b"data").unwrap();
        file.write_all(&data_size.to_le_bytes()).unwrap();
        file.write_all(&vec![0; data_size as usize]).unwrap();
        drop(file);
        let player = Player::open(&path).unwrap();
        assert!((player.duration() - 1.).abs() < 0.05);
        player.seek(0.5);
        assert!((player.position() - 0.5).abs() < 0.05);
        drop(player);
        std::fs::remove_file(path).unwrap();
    }
}
