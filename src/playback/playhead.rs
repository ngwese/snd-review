use std::sync::Arc;

use super::provider::PlaybackDataProvider;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlayheadEvent {
    Ok,
    Looped,
    ReachedEnd,
}

pub struct Playhead {
    provider: Arc<dyn PlaybackDataProvider>,
    position: usize,
    in_point: Option<usize>,
    out_point: Option<usize>,
    looping: bool,
}

impl Playhead {
    pub fn new(provider: Arc<dyn PlaybackDataProvider>) -> Self {
        Self {
            provider,
            position: 0,
            in_point: None,
            out_point: None,
            looping: false,
        }
    }

    pub fn provider(&self) -> &Arc<dyn PlaybackDataProvider> {
        &self.provider
    }

    pub fn position(&self) -> usize {
        self.position
    }

    pub fn in_point(&self) -> Option<usize> {
        self.in_point
    }

    pub fn out_point(&self) -> Option<usize> {
        self.out_point
    }

    pub fn looping(&self) -> bool {
        self.looping
    }

    pub fn set_position(&mut self, sample: usize) {
        self.position = self.clamp_sample(sample);
    }

    pub fn set_in(&mut self, sample: usize) {
        self.in_point = Some(self.clamp_sample(sample));
    }

    pub fn set_out(&mut self, sample: usize) {
        self.out_point = Some(self.clamp_sample(sample));
    }

    pub fn clear_in_out(&mut self) {
        self.in_point = None;
        self.out_point = None;
    }

    pub fn set_in_out(&mut self, start: usize, end: usize) {
        let (start, end) = if start <= end {
            (start, end)
        } else {
            (end, start)
        };
        self.in_point = Some(self.clamp_sample(start));
        self.out_point = Some(self.clamp_sample(end));
    }

    pub fn set_looping(&mut self, looping: bool) {
        self.looping = looping;
    }

    pub fn toggle_looping(&mut self) {
        self.looping = !self.looping;
    }

    pub fn playback_start(&self) -> usize {
        self.in_point.unwrap_or(0)
    }

    /// End sample for transport **End** command (region out or buffer end).
    pub fn transport_end(&self) -> usize {
        self.out_point.unwrap_or_else(|| self.max_sample())
    }

    /// End sample for playback stopping / looping (region out only when looping).
    pub fn playback_end(&self) -> usize {
        if self.looping {
            self.transport_end()
        } else {
            self.max_sample()
        }
    }

    pub fn clamp_sample(&self, sample: usize) -> usize {
        sample.min(self.max_sample())
    }

    fn max_sample(&self) -> usize {
        self.provider.frames().saturating_sub(1)
    }

    pub fn advance(&mut self, frames_played: usize) -> PlayheadEvent {
        if frames_played == 0 {
            return PlayheadEvent::Ok;
        }
        let end = self.playback_end();
        let start = self.playback_start();
        let next = self.position.saturating_add(frames_played);
        if next >= end {
            if self.looping && end > start {
                self.position = start;
                return PlayheadEvent::Looped;
            }
            self.position = end;
            return PlayheadEvent::ReachedEnd;
        }
        self.position = next;
        PlayheadEvent::Ok
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audio::DecodedAudio;

    fn playhead(frames: usize) -> Playhead {
        Playhead::new(Arc::new(DecodedAudio {
            sample_rate: 44100,
            channels: vec![vec![0.0; frames]],
            peaks: vec![vec![]],
        }))
    }

    #[test]
    fn loops_inside_in_out() {
        let mut ph = playhead(1000);
        ph.set_in_out(100, 200);
        ph.set_looping(true);
        ph.set_position(195);
        assert_eq!(ph.advance(10), PlayheadEvent::Looped);
        assert_eq!(ph.position(), 100);
    }

    #[test]
    fn stops_at_buffer_end_without_loop_even_with_in_out() {
        let mut ph = playhead(1000);
        ph.set_in_out(100, 200);
        ph.set_position(995);
        assert_eq!(ph.advance(10), PlayheadEvent::ReachedEnd);
        assert_eq!(ph.position(), 999);
    }
}
