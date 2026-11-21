use std::time::Instant;

use crate::platform::playback::{Phase, PlaybackStatus};

const PROJECTION_MS: f64 = 550.0;
const SEEK_PREVIEW_MS: u128 = 400;
const SYNC_TOLERANCE_MS: u64 = 260;

/// Display time between decoder samples. The worker still owns playback and seeking.
pub(super) struct Playhead {
    anchor: Instant,
    correction_ms: f64,
    seek_preview: Option<(Instant, i64)>,
}

impl Playhead {
    pub fn new(now: Instant) -> Self {
        Self {
            anchor: now,
            correction_ms: 0.0,
            seek_preview: None,
        }
    }

    pub fn sample(
        &mut self,
        previous: PlaybackStatus,
        next: PlaybackStatus,
        rate: f32,
        now: Instant,
    ) {
        if previous.phase == next.phase && previous.elapsed_ms == next.elapsed_ms {
            return;
        }
        let seeking = self.seek_preview.is_some();
        if let Some((start, target)) = self.seek_preview
            && matches!(next.phase, Phase::Playing | Phase::Paused)
            && now.duration_since(start).as_millis() < SEEK_PREVIEW_MS
            && next.elapsed_ms.abs_diff(target) > SYNC_TOLERANCE_MS
        {
            self.anchor = now;
            self.correction_ms = 0.0;
            return;
        }
        let shown = self.position(previous, rate, now);
        self.anchor = now;
        self.seek_preview = None;
        self.correction_ms = if !seeking
            && previous.phase == Phase::Playing
            && next.phase == Phase::Playing
            && next.elapsed_ms >= previous.elapsed_ms
            && shown.abs_diff(next.elapsed_ms) <= SYNC_TOLERANCE_MS
        {
            // Blend small timestamp differences without moving the marker backwards.
            (shown - next.elapsed_ms) as f64
        } else {
            0.0
        };
    }

    pub fn seek(&mut self, elapsed_ms: i64, now: Instant) {
        self.seek_preview = Some((now, elapsed_ms));
    }

    pub fn change_rate(&mut self, status: PlaybackStatus, old_rate: f32, now: Instant) {
        if self
            .seek_preview
            .is_none_or(|(start, _)| now.duration_since(start).as_millis() >= SEEK_PREVIEW_MS)
        {
            self.correction_ms = (self.position(status, old_rate, now) - status.elapsed_ms) as f64;
            self.anchor = now;
        }
    }

    pub fn position(&self, status: PlaybackStatus, rate: f32, now: Instant) -> i64 {
        let elapsed = if let Some((start, target)) = self.seek_preview
            && now.duration_since(start).as_millis() < SEEK_PREVIEW_MS
        {
            target
        } else if status.phase == Phase::Playing {
            let age = (now.duration_since(self.anchor).as_secs_f64() * 1000.0).min(PROJECTION_MS);
            let offset = age * f64::from(rate) + self.correction_ms * (1.0 - age / PROJECTION_MS);
            status
                .elapsed_ms
                .saturating_add(offset.max(self.correction_ms).round() as i64)
        } else {
            status.elapsed_ms
        };
        elapsed.clamp(
            0,
            if status.duration_ms > 0 {
                status.duration_ms
            } else {
                i64::MAX
            },
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn interpolates_at_playback_rate_and_stops_projecting_stale_samples() {
        let now = Instant::now();
        let clock = Playhead::new(now);
        let status = PlaybackStatus {
            elapsed_ms: 1_000,
            duration_ms: 3_000,
            ..PlaybackStatus::new(Phase::Playing)
        };
        assert_eq!(
            clock.position(status, 2.0, now + Duration::from_millis(16)),
            1_032
        );
        assert_eq!(
            clock.position(status, 2.0, now + Duration::from_secs(8)),
            2_100
        );
        assert_eq!(
            clock.position(
                PlaybackStatus {
                    duration_ms: 1_600,
                    ..status
                },
                2.0,
                now + Duration::from_secs(1)
            ),
            1_600
        );
        assert_eq!(
            clock.position(
                PlaybackStatus {
                    phase: Phase::Paused,
                    ..status
                },
                2.0,
                now + Duration::from_secs(1)
            ),
            1_000
        );
    }

    #[test]
    fn reconciles_small_clock_errors_and_snaps_to_a_confirmed_seek() {
        let now = Instant::now();
        let mut clock = Playhead::new(now);
        let status = PlaybackStatus {
            elapsed_ms: 1_000,
            duration_ms: 4_000,
            ..PlaybackStatus::new(Phase::Playing)
        };
        let sample_time = now + Duration::from_millis(250);
        let sample = PlaybackStatus {
            elapsed_ms: 1_200,
            ..status
        };
        clock.sample(status, sample, 1.0, sample_time);
        assert_eq!(clock.position(sample, 1.0, sample_time), 1_250);
        assert!(clock.position(sample, 1.0, sample_time + Duration::from_millis(16)) > 1_250);

        clock.seek(500, sample_time);
        let stale = PlaybackStatus {
            elapsed_ms: 1_220,
            ..sample
        };
        clock.sample(sample, stale, 1.0, sample_time + Duration::from_millis(20));
        assert_eq!(
            clock.position(stale, 1.0, sample_time + Duration::from_millis(20)),
            500
        );
        let confirmed = PlaybackStatus {
            elapsed_ms: 510,
            ..sample
        };
        let confirmed_at = sample_time + Duration::from_millis(50);
        clock.sample(stale, confirmed, 1.0, confirmed_at);
        assert_eq!(clock.position(confirmed, 1.0, confirmed_at), 510);
    }
}
