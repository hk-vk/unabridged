//! Playback control: the active player, position sync, chapters, sleep timer,
//! speed and volume.
use crate::*;

impl Shell {
    pub(crate) fn load_chapters(&mut self, cx: &mut Context<Self>) {
        let Some(player) = &self.player else { return };
        let asin = player.asin.clone();
        let key = player.account_key.clone();
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_spawn({
                    let asin = asin.clone();
                    async move { backend::chapters(&key, &asin) }
                })
                .await;
            let _ = this.update(cx, |this, cx| {
                if let Some(player) = &mut this.player
                    && player.asin == asin
                {
                    player.chapters_loading = false;
                    if let Ok(info) = result {
                        player.chapters = info
                            .flatten()
                            .into_iter()
                            .filter(|chapter| chapter.chapters.is_empty())
                            .cloned()
                            .collect();
                        player
                            .chapters
                            .sort_by_key(|chapter| chapter.start_offset_ms);
                    }
                    cx.notify();
                }
            });
        })
        .detach();
    }

    pub(crate) fn sync_position(&self, cx: &mut Context<Self>) {
        let Some(active) = &self.player else { return };
        let asin = active.asin.clone();
        let key = active.account_key.clone();
        let millis = (active.player.position().max(0.) * 1000.) as u64;
        let version = backend::register_save(&asin);
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_spawn(
                    async move { backend::save_position(&key, &asin, millis, version) },
                )
                .await;
            if let Err(error) = result {
                let _ = this.update(cx, |this, cx| {
                    this.download_notice =
                        Some(format!("Could not sync listening position: {error}"));
                    cx.notify();
                });
            }
        })
        .detach();
    }

    pub(crate) fn start_player(
        &mut self,
        key: String,
        asin: audible::Asin,
        title: String,
        cx: &mut Context<Self>,
    ) {
        self.start_player_at(key, asin, title, None, cx);
    }

    pub(crate) fn start_player_at(
        &mut self,
        key: String,
        asin: audible::Asin,
        title: String,
        start_at: Option<u64>,
        cx: &mut Context<Self>,
    ) {
        if self.loading_player {
            return;
        }
        if self.player.as_ref().is_some_and(|p| p.asin == asin) {
            if let Some(active) = &mut self.player {
                if let Some(position) = start_at {
                    active.player.seek(position as f64 / 1000.);
                }
                if let Err(error) = active.player.play() {
                    self.download_notice = Some(format!("Could not resume playback: {error}"));
                } else {
                    active.was_playing = true;
                }
            }
            self.open_player(cx);
            cx.notify();
            return;
        }
        let Some(path) = backend::downloaded_path(&key, &asin) else {
            return;
        };
        if let Some(active) = &self.player {
            active.player.pause();
            self.sync_position(cx);
        }
        self.loading_player = true;
        self.download_notice = Some(format!("Loading {title}…"));
        cx.notify();
        cx.spawn(async move |this, cx| {
            let result = if let Some(position) = start_at {
                Ok(Some(position))
            } else {
                cx.background_spawn({
                    let asin = asin.clone();
                    let key = key.clone();
                    async move { backend::position(&key, &asin) }
                })
                .await
            };
            let _ = this.update(cx, |this, cx| {
                this.loading_player = false;
                match result.and_then(|position| {
                    let player = player::Player::open(&path)?;
                    if let Some(millis) = position {
                        player.seek(millis as f64 / 1000.);
                    }
                    player.play()?;
                    Ok(player)
                }) {
                    Ok(player) => {
                        let cancelled_timer = matches!(&this.sleep, SleepTimer::EndOfChapter { asin: timer_asin, .. } if timer_asin != &asin);
                        if cancelled_timer { this.sleep = SleepTimer::Off; }
                        this.player = Some(ActivePlayer {
                            asin,
                            account_key: key,
                            title,
                            player,
                            was_playing: true,
                            chapters: Vec::new(),
                            chapters_loading: true,
                        });
                        this.chapters_open = false;
                        this.load_chapters(cx);
                        this.download_notice = cancelled_timer.then(|| "Chapter sleep timer turned off when you changed books.".into());
                        this.open_player(cx);
                    }
                    Err(error) => {
                        this.download_notice = Some(format!("Could not start playback: {error}"))
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    pub(crate) fn toggle_playback(&mut self, cx: &mut Context<Self>) {
        if let Some(active) = &mut self.player {
            if active.player.is_playing() {
                active.player.pause();
                active.was_playing = false;
                self.sync_position(cx);
            } else if let Err(error) = active.player.play() {
                self.download_notice = Some(format!("Could not resume playback: {error}"));
            } else {
                active.was_playing = true;
            }
            cx.notify();
        }
    }

    /// Moves to the next chapter (`forward`), or back to the start of this
    /// chapter, or to the previous one when already near its start.
    pub(crate) fn step_chapter(&mut self, forward: bool, cx: &mut Context<Self>) {
        let Some(active) = &self.player else { return };
        let duration = active.player.duration();
        let position_ms = (active.player.position().max(0.) * 1000.) as u64;
        let Some((index, chapter)) = current_chapter(&active.chapters, position_ms) else {
            return;
        };
        let target = if forward {
            active.chapters.get(index + 1)
        } else if position_ms.saturating_sub(chapter.start_offset_ms) > 3_000 || index == 0 {
            Some(chapter)
        } else {
            active.chapters.get(index - 1)
        };
        if let Some(target) = target
            && duration > 0.
        {
            let fraction = (target.start_offset_ms as f64 / 1000. / duration) as f32;
            self.seek_fraction(fraction, cx);
        }
    }

    pub(crate) fn skip(&mut self, seconds: f64, cx: &mut Context<Self>) {
        if let Some(active) = &self.player {
            active.player.seek(active.player.position() + seconds);
            self.sync_position(cx);
            cx.notify();
        }
    }

    pub(crate) fn seek_fraction(&mut self, fraction: f32, cx: &mut Context<Self>) {
        let Some(active) = &self.player else { return };
        active
            .player
            .seek(active.player.duration() * f64::from(fraction.clamp(0., 1.)));
        let asin = active.asin.clone();
        self.seek_generation += 1;
        let generation = self.seek_generation;
        cx.notify();
        cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(std::time::Duration::from_millis(700))
                .await;
            let _ = this.update(cx, |this, cx| {
                if this.seek_generation == generation
                    && this.player.as_ref().is_some_and(|p| p.asin == asin)
                {
                    this.sync_position(cx);
                }
            });
        })
        .detach();
    }

    /// Steps the sleep chip: off, 15 min, 30 min, 1 hour, end of chapter, off.
    pub(crate) fn cycle_sleep(&mut self, cx: &mut Context<Self>) {
        let next = match &self.sleep {
            SleepTimer::Off => "15 minutes",
            SleepTimer::Deadline(deadline) => {
                let left = deadline.saturating_duration_since(std::time::Instant::now());
                if left <= std::time::Duration::from_secs(15 * 60) {
                    "30 minutes"
                } else if left <= std::time::Duration::from_secs(30 * 60) {
                    "1 hour"
                } else {
                    "End of chapter"
                }
            }
            SleepTimer::EndOfChapter { .. } => "Off",
        };
        self.set_sleep(next, cx);
    }

    pub(crate) fn set_sleep(&mut self, choice: &str, cx: &mut Context<Self>) {
        let now = std::time::Instant::now();
        self.sleep = match choice {
            "15 minutes" => SleepTimer::Deadline(now + std::time::Duration::from_secs(15 * 60)),
            "30 minutes" => SleepTimer::Deadline(now + std::time::Duration::from_secs(30 * 60)),
            "1 hour" => SleepTimer::Deadline(now + std::time::Duration::from_secs(60 * 60)),
            "End of chapter" => {
                let chapter = self.player.as_ref().and_then(|active| {
                    current_chapter(
                        &active.chapters,
                        (active.player.position().max(0.) * 1000.) as u64,
                    )
                    .map(|(_, chapter)| {
                        (
                            active.asin.clone(),
                            chapter.start_offset_ms.saturating_add(chapter.length_ms),
                        )
                    })
                });
                if let Some((asin, end_ms)) = chapter {
                    SleepTimer::EndOfChapter {
                        asin,
                        end_seconds: end_ms as f64 / 1000.,
                    }
                } else {
                    self.download_notice = Some(
                        "Chapter timing is unavailable. Choose a timed sleep option instead."
                            .into(),
                    );
                    SleepTimer::Off
                }
            }
            _ => SleepTimer::Off,
        };
        cx.notify();
    }

    pub(crate) fn set_volume(&mut self, volume: f32, cx: &mut Context<Self>) {
        if let Some(active) = &self.player {
            active.player.set_volume(volume);
            cx.notify();
        }
    }

    pub(crate) fn cycle_speed(&mut self, cx: &mut Context<Self>) {
        if let Some(active) = &self.player {
            let speed = active.player.rate();
            active.player.set_rate(
                [0.75, 1.0, 1.25, 1.5, 1.75, 2.0]
                    .into_iter()
                    .find(|next| *next > speed + 0.01)
                    .unwrap_or(0.75),
            );
            cx.notify();
        }
    }
}
