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
                    if let Some(speed) = this.prefs.speed {
                        player.set_rate(speed);
                    }
                    if let Some(volume) = this.prefs.volume {
                        player.set_volume(volume);
                    }
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

    pub(crate) fn set_sleep(&mut self, choice: &'static str, cx: &mut Context<Self>) {
        let now = std::time::Instant::now();
        self.sleep_choice = choice;
        self.sleep = match choice {
            "15 minutes" => SleepTimer::Deadline(now + std::time::Duration::from_secs(15 * 60)),
            "30 minutes" => SleepTimer::Deadline(now + std::time::Duration::from_secs(30 * 60)),
            "45 minutes" => SleepTimer::Deadline(now + std::time::Duration::from_secs(45 * 60)),
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
        self.prefs.volume = Some(volume);
        self.save_prefs(cx);
    }

    pub(crate) fn set_speed(&mut self, speed: f32, cx: &mut Context<Self>) {
        if let Some(active) = &self.player {
            active.player.set_rate(speed);
            cx.notify();
        }
        self.prefs.speed = Some(speed);
        self.save_prefs(cx);
    }

    /// Writes speed and volume for the next launch.
    pub(crate) fn save_prefs(&mut self, cx: &mut Context<Self>) {
        let prefs = self.prefs.clone();
        cx.background_spawn(async move {
            let _ = backend::save_playback(&prefs);
        })
        .detach();
    }

    pub(crate) fn cycle_speed(&mut self, cx: &mut Context<Self>) {
        if let Some(active) = &self.player {
            let speed = active.player.rate();
            let next = [0.75, 1.0, 1.25, 1.5, 1.75, 2.0]
                .into_iter()
                .find(|next| *next > speed + 0.01)
                .unwrap_or(0.75);
            self.set_speed(next, cx);
        }
    }
}

impl ActivePlayer {
    /// The current chapter's timeline, or the whole book's.
    pub(crate) fn timeline(&self) -> Timeline {
        Timeline::chapter(
            &self.chapters,
            self.player.position(),
            self.player.duration(),
        )
    }
}

impl Shell {
    /// Keeps the read-along transcript on the chapter that is playing.
    pub(crate) fn ensure_read_along(&mut self, _cx: &mut Context<Self>) {
        let Some(active) = &self.player else {
            return;
        };
        let timeline = active.timeline();
        // Without chapter data, read along in ten-minute stretches.
        let (start, end) = if timeline.chapter.is_some() {
            (timeline.start, timeline.end)
        } else {
            let start = (timeline.position / 600.).floor() * 600.;
            (start, (start + 600.).min(timeline.duration.max(start + 1.)))
        };
        let start_ms = (start * 1000.) as u64;
        let downloaded = self.read_along.as_ref().is_some_and(|read| {
            read.downloading
                && read
                    .job
                    .lock()
                    .is_ok_and(|job| job.phase == transcript::Phase::Checking)
        });
        if !downloaded
            && self
                .read_along
                .as_ref()
                .is_some_and(|read| read.asin == active.asin && read.start_ms == start_ms)
        {
            return;
        }
        let Some(audio) = backend::downloaded_path(&active.account_key, &active.asin) else {
            return;
        };
        let language = match &self.account {
            Account::Ready(library) => library
                .items
                .iter()
                .find(|item| item.asin == active.asin)
                .and_then(|item| item.product.language.clone()),
            _ => None,
        };
        let locale = transcript::locale_for(language.as_deref());
        let span = |start: f64, end: f64| transcript::Span {
            key: active.account_key.clone(),
            asin: active.asin.to_string(),
            audio: audio.clone(),
            start,
            end,
            locale,
        };
        let next = active
            .chapters
            .iter()
            .find(|chapter| chapter.start_offset_ms as f64 / 1000. >= end - 0.5)
            .map(|chapter| {
                let next_start = chapter.start_offset_ms as f64 / 1000.;
                span(next_start, next_start + chapter.length_ms as f64 / 1000.)
            });
        self.read_along = Some(ReadAlong {
            asin: active.asin.clone(),
            start_ms,
            locale,
            job: transcript::start(span(start, end), next),
            downloading: false,
        });
        self.read_along_follow.reset();
        self.mini_follow.reset();
    }

    /// Downloads the speech model, then transcribes the chapter.
    pub(crate) fn download_speech_model(&mut self, cx: &mut Context<Self>) {
        if let Some(read) = &mut self.read_along {
            transcript::install_model(read.locale, read.job.clone());
            read.downloading = true;
        }
        cx.notify();
    }

    /// Scrolls the transcript so the spoken line sits in the upper third.
    pub(crate) fn follow_read_along(&mut self, reduce_motion: bool) {
        let Some(active) = &self.player else {
            return;
        };
        let position = active.player.position();
        let Some(read) = &self.read_along else {
            return;
        };
        let Some(current) = read
            .job
            .lock()
            .ok()
            .and_then(|job| transcript::current_line(&job.lines, position))
        else {
            return;
        };
        if self.read_along_open && self.page == Page::NowPlaying {
            self.read_along_follow.follow(current, reduce_motion);
        }
        if self.mini_lyrics && self.mini_window.is_some() {
            self.mini_follow.follow(current, reduce_motion);
        }
    }

    /// How far the mini player is between its pill (0) and read-along (1)
    /// shapes, and whether it is still moving.
    pub(crate) fn mini_expansion(&self) -> (f32, bool) {
        let target = if self.mini_lyrics { 1. } else { 0. };
        let Some(at) = self.mini_lyrics_at else {
            return (target, false);
        };
        let duration = if self.mini_lyrics {
            MINI_EXPAND
        } else {
            MINI_COLLAPSE
        };
        let t = at.elapsed().as_secs_f32() / duration.as_secs_f32();
        if t >= 1. {
            return (target, false);
        }
        let from = self.mini_lyrics_from;
        (from + (target - from) * ease_sheet(t), true)
    }

    /// Expands the mini player into read along, or folds it back into the
    /// pill. Returns the window height to animate to, and over how long.
    pub(crate) fn toggle_mini_lyrics(
        &mut self,
        cx: &mut Context<Self>,
    ) -> (f32, std::time::Duration) {
        let (now, _) = self.mini_expansion();
        self.mini_lyrics = !self.mini_lyrics;
        self.mini_lyrics_from = now;
        self.mini_lyrics_at = (!cx.reduce_motion()).then(std::time::Instant::now);
        if self.mini_lyrics {
            self.ensure_read_along(cx);
            self.mini_follow.reset();
        }
        cx.notify();
        let (height, duration) = if self.mini_lyrics {
            (mini_player::EXPANDED_HEIGHT, MINI_EXPAND)
        } else {
            (mini_player::SIZE.1, MINI_COLLAPSE)
        };
        let duration = if cx.reduce_motion() {
            std::time::Duration::ZERO
        } else {
            duration
        };
        (height, duration)
    }
}

impl Follow {
    /// Forgets the followed line and scrolls back to the top, for a new
    /// chapter or a view that just appeared.
    pub(crate) fn reset(&mut self) {
        self.followed = None;
        self.previous = None;
        self.glide.set(None);
        self.scroll.set_offset(point(px(0.), px(0.)));
    }

    /// Scrolls so the spoken line sits 30% down the view.
    fn follow(&mut self, current: usize, reduce_motion: bool) {
        if self.followed == Some(current) {
            return;
        }
        let handle = &self.scroll;
        let (Some(item), viewport) = (handle.bounds_for_item(current), handle.bounds()) else {
            return;
        };
        // Child bounds are laid out without the scroll applied, so the offset
        // that puts the line 30% down the viewport doesn't depend on the
        // current one.
        let target = viewport.origin.y - item.origin.y + viewport.size.height * 0.3;
        let target = target.clamp(-handle.max_offset().y, px(0.));
        if reduce_motion {
            handle.set_offset(point(handle.offset().x, target));
        } else {
            // Start from wherever the list is now, so quick line changes
            // redirect the glide instead of restarting it.
            self.glide.set(Some(Glide {
                from: handle.offset().y,
                to: target,
                started: std::time::Instant::now(),
            }));
        }
        self.previous = self.followed;
        self.followed = Some(current);
        self.changed_at = std::time::Instant::now();
    }
}

/// How long the mini player takes to open into read along.
pub(crate) const MINI_EXPAND: std::time::Duration = std::time::Duration::from_millis(460);
/// Folding back is quicker: the user has already decided.
pub(crate) const MINI_COLLAPSE: std::time::Duration = std::time::Duration::from_millis(320);

/// The sheet curve, cubic-bezier(0.32, 0.72, 0, 1): a steep start that settles
/// slowly. The native window resize uses the same control points.
pub(crate) fn ease_sheet(t: f32) -> f32 {
    cubic_bezier(0.32, 0.72, 0., 1., t)
}

/// CSS-style cubic Bézier easing: solves x(s) = t, returns y(s).
fn cubic_bezier(x1: f32, y1: f32, x2: f32, y2: f32, t: f32) -> f32 {
    let t = t.clamp(0., 1.);
    let curve = |a: f32, b: f32, s: f32| {
        let inv = 1. - s;
        3. * inv * inv * s * a + 3. * inv * s * s * b + s * s * s
    };
    // Bisection: x(s) is monotonic for control x in [0, 1].
    let (mut lo, mut hi) = (0f32, 1f32);
    for _ in 0..24 {
        let mid = (lo + hi) / 2.;
        if curve(x1, x2, mid) < t {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    curve(y1, y2, (lo + hi) / 2.)
}

/// How long the transcript takes to glide to the next line.
pub(crate) const GLIDE: std::time::Duration = std::time::Duration::from_millis(520);
/// How long the highlight takes to move to the next line.
pub(crate) const HIGHLIGHT_FADE: std::time::Duration = std::time::Duration::from_millis(260);

/// Ease-in-out cubic: the list is already on screen, so it accelerates
/// gently and settles instead of snapping.
pub(crate) fn ease_in_out_cubic(t: f32) -> f32 {
    let t = t.clamp(0., 1.);
    if t < 0.5 {
        4. * t * t * t
    } else {
        1. - (-2. * t + 2.).powi(3) / 2.
    }
}

/// Linear blend between two colours.
pub(crate) fn mix(from: gpui_kit::Hsla, to: gpui_kit::Hsla, t: f32) -> gpui_kit::Hsla {
    let t = t.clamp(0., 1.);
    gpui_kit::Hsla {
        h: from.h + (to.h - from.h) * t,
        s: from.s + (to.s - from.s) * t,
        l: from.l + (to.l - from.l) * t,
        a: from.a + (to.a - from.a) * t,
    }
}

impl Follow {
    /// Advances the scroll glide; true while it still needs frames.
    pub(crate) fn step_glide(&self) -> bool {
        let handle = &self.scroll;
        let Some(glide) = self.glide.get() else {
            return false;
        };
        let t = glide.started.elapsed().as_secs_f32() / GLIDE.as_secs_f32();
        let eased = ease_in_out_cubic(t);
        let y = glide.from + (glide.to - glide.from) * eased;
        handle.set_offset(point(handle.offset().x, y));
        if t >= 1. {
            self.glide.set(None);
            false
        } else {
            true
        }
    }
}
