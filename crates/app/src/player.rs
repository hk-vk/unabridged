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
        fn ba_player_rate(player: *mut c_void) -> f32;
        fn ba_player_set_rate(player: *mut c_void, rate: f32);
        fn ba_player_volume(player: *mut c_void) -> f32;
        fn ba_player_set_volume(player: *mut c_void, volume: f32);
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
        pub fn rate(&self) -> f32 {
            unsafe { ba_player_rate(self.0.as_ptr()) }
        }
        pub fn set_rate(&self, rate: f32) {
            unsafe { ba_player_set_rate(self.0.as_ptr(), rate.clamp(0.5, 2.0)) }
        }
        pub fn volume(&self) -> f32 {
            unsafe { ba_player_volume(self.0.as_ptr()) }
        }
        pub fn set_volume(&self, volume: f32) {
            unsafe { ba_player_set_volume(self.0.as_ptr(), volume.clamp(0.0, 1.0)) }
        }
    }
    impl Drop for Player {
        fn drop(&mut self) {
            unsafe { ba_player_close(self.0.as_ptr()) }
        }
    }
}
#[cfg(any(target_os = "macos", target_os = "windows"))]
pub use platform::Player;

#[cfg(target_os = "windows")]
mod platform {
    use std::{path::Path, sync::mpsc, time::Duration};
    use windows::{
        Foundation::{TimeSpan, TypedEventHandler},
        Media::{
            Core::MediaSource,
            Playback::{MediaPlaybackState, MediaPlayer, MediaPlayerFailedEventArgs},
        },
        Storage::StorageFile,
        Win32::{
            Foundation::RPC_E_CHANGED_MODE,
            System::WinRT::{
                RO_INIT_MULTITHREADED, RO_INIT_SINGLETHREADED, RO_INIT_TYPE, RoInitialize,
                RoUninitialize,
            },
        },
        core::HSTRING,
    };

    struct Apartment(bool);
    impl Apartment {
        fn new(mode: RO_INIT_TYPE) -> windows::core::Result<Self> {
            match unsafe { RoInitialize(mode) } {
                Ok(()) => Ok(Self(true)),
                Err(error) if error.code() == RPC_E_CHANGED_MODE => Ok(Self(false)),
                Err(error) => Err(error),
            }
        }
    }
    impl Drop for Apartment {
        fn drop(&mut self) {
            if self.0 {
                unsafe { RoUninitialize() };
            }
        }
    }
    thread_local! {
        static APARTMENT: windows::core::Result<Apartment> = Apartment::new(RO_INIT_SINGLETHREADED);
    }
    pub struct Player(MediaPlayer, std::marker::PhantomData<std::rc::Rc<()>>);

    impl Player {
        pub fn open(path: &Path) -> anyhow::Result<Self> {
            APARTMENT.with(|apartment| apartment.as_ref().map(|_| ()).map_err(Clone::clone))?;
            let path = dunce::canonicalize(path)?;
            // Create the agile player without a UI dispatcher so opening cannot re-enter GPUI.
            let player = std::thread::spawn(move || Self::open_media(&path))
                .join()
                .map_err(|_| anyhow::anyhow!("Audio opening thread panicked"))??;
            Ok(Self(player, std::marker::PhantomData))
        }

        fn open_media(path: &Path) -> anyhow::Result<MediaPlayer> {
            let _apartment = Apartment::new(RO_INIT_MULTITHREADED)?;
            let file =
                StorageFile::GetFileFromPathAsync(&HSTRING::from(path.as_os_str()))?.get()?;
            let source = MediaSource::CreateFromStorageFile(&file)?;
            let player = MediaPlayer::new()?;
            player.SetAutoPlay(false)?;
            let (sender, receiver) = mpsc::sync_channel(1);
            let opened_sender = sender.clone();
            let opened = player.MediaOpened(&TypedEventHandler::new(move |_, _| {
                let _ = opened_sender.try_send(Ok(()));
                Ok(())
            }))?;
            let failed = player.MediaFailed(&TypedEventHandler::<
                MediaPlayer,
                MediaPlayerFailedEventArgs,
            >::new(move |_, args| {
                let error = args.as_ref().map_or_else(
                    || anyhow::anyhow!("Could not open this audio file"),
                    |args| {
                        let code = args
                            .ExtendedErrorCode()
                            .unwrap_or(windows::core::HRESULT(0x80004005u32 as i32));
                        anyhow::Error::new(windows::core::Error::from_hresult(code)).context(
                            format!(
                                "Could not open this audio file: {}",
                                args.ErrorMessage().unwrap_or_default()
                            ),
                        )
                    },
                );
                let _ = sender.try_send(Err(error));
                Ok(())
            }))?;
            let result = (|| -> anyhow::Result<()> {
                player.SetSource(&source)?;
                receiver.recv_timeout(Duration::from_secs(10))?
            })();
            let _ = player.RemoveMediaOpened(opened);
            let _ = player.RemoveMediaFailed(failed);
            if let Err(error) = result {
                let _ = player.Close();
                return Err(error);
            }
            Ok(player)
        }
        pub fn play(&self) -> anyhow::Result<()> {
            Ok(self.0.Play()?)
        }
        pub fn pause(&self) {
            let _ = self.0.Pause();
        }
        pub fn position(&self) -> f64 {
            self.0
                .PlaybackSession()
                .and_then(|s| s.Position())
                .map_or(0., |t| t.Duration as f64 / 10_000_000.)
        }
        pub fn duration(&self) -> f64 {
            self.0
                .PlaybackSession()
                .and_then(|s| s.NaturalDuration())
                .map_or(0., |t| t.Duration as f64 / 10_000_000.)
        }
        pub fn seek(&self, seconds: f64) {
            if let Ok(session) = self.0.PlaybackSession() {
                let _ = session.SetPosition(TimeSpan {
                    Duration: (seconds.clamp(0., self.duration()) * 10_000_000.) as i64,
                });
            }
        }
        pub fn is_playing(&self) -> bool {
            self.0
                .PlaybackSession()
                .and_then(|s| s.PlaybackState())
                .is_ok_and(|s| s == MediaPlaybackState::Playing)
        }
        pub fn rate(&self) -> f32 {
            self.0
                .PlaybackSession()
                .and_then(|s| s.PlaybackRate())
                .unwrap_or(1.) as f32
        }
        pub fn set_rate(&self, rate: f32) {
            if let Ok(session) = self.0.PlaybackSession() {
                let _ = session.SetPlaybackRate(rate.clamp(0.5, 2.0) as f64);
            }
        }
        pub fn volume(&self) -> f32 {
            self.0.Volume().unwrap_or(0.) as f32
        }
        pub fn set_volume(&self, volume: f32) {
            let _ = self.0.SetVolume(volume.clamp(0., 1.) as f64);
        }
    }
    impl Drop for Player {
        fn drop(&mut self) {
            let _ = self.0.Close();
        }
    }
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
pub struct Player;
#[cfg(not(any(target_os = "macos", target_os = "windows")))]
impl Player {
    pub fn open(_: &std::path::Path) -> anyhow::Result<Self> {
        anyhow::bail!("Playback is not available on this platform")
    }
    pub fn play(&self) -> anyhow::Result<()> {
        anyhow::bail!("Playback is not available on this platform")
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
    pub fn rate(&self) -> f32 {
        1.0
    }
    pub fn set_rate(&self, _rate: f32) {}
    pub fn volume(&self) -> f32 {
        0.
    }
    pub fn set_volume(&self, _volume: f32) {}
}

#[cfg(all(test, any(target_os = "macos", target_os = "windows")))]
mod tests {
    use super::Player;
    use std::io::Write;

    fn open_test_audio(path: &std::path::Path) -> Option<Player> {
        match Player::open(path) {
            Ok(player) => Some(player),
            Err(error) => {
                #[cfg(target_os = "windows")]
                if error
                    .downcast_ref::<windows::core::Error>()
                    .is_some_and(|error| error.code().0 == 0xC00D36FAu32 as i32)
                {
                    eprintln!(
                        "Skipping playback test: Windows has no usable audio output endpoint (MF_E_CANNOT_CREATE_SINK)."
                    );
                    return None;
                }
                panic!("Could not open test audio: {error:#}");
            }
        }
    }

    #[test]
    fn rejects_a_missing_file() {
        assert!(
            Player::open(&std::env::temp_dir().join("better-audible-missing-file.m4b")).is_err()
        );
    }

    #[test]
    fn rejects_empty_audio() {
        let path =
            std::env::temp_dir().join(format!("better-audible-empty-{}.wav", std::process::id()));
        std::fs::write(&path, []).unwrap();
        assert!(Player::open(&path).is_err());
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn opens_a_generated_m4b_when_ffmpeg_is_available() {
        if std::process::Command::new("ffmpeg")
            .arg("-version")
            .output()
            .is_err()
        {
            eprintln!("Skipping generated m4b playback test: ffmpeg is not installed.");
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
        let Some(player) = open_test_audio(&path) else {
            std::fs::remove_file(path).unwrap();
            return;
        };
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
        let Some(player) = open_test_audio(&path) else {
            std::fs::remove_file(path).unwrap();
            return;
        };
        assert!((player.duration() - 1.).abs() < 0.05);
        player.seek(0.5);
        assert!((player.position() - 0.5).abs() < 0.05);
        player.set_rate(1.25);
        assert!((player.rate() - 1.25).abs() < 0.01);
        player.set_volume(0.4);
        assert!((player.volume() - 0.4).abs() < 0.01);
        drop(player);
        std::fs::remove_file(path).unwrap();
    }
}
