//! `AnalysisPublisher`, which sends analysis results to the screen over a Tauri channel.

use std::sync::Mutex;

use tauri::ipc::Channel;
use voice_domain::AnalysisPair;
use voice_domain::ports::AnalysisPublisher;

use crate::dto::PairDto;

/// Replaced with the channel received from the screen on each start.
#[derive(Default)]
pub struct ChannelPublisher {
    channel: Mutex<Option<Channel<PairDto>>>,
}

impl ChannelPublisher {
    pub fn set(&self, channel: Channel<PairDto>) {
        *self.channel.lock().expect("channel") = Some(channel);
    }
}

impl AnalysisPublisher for ChannelPublisher {
    fn publish(&self, pair: &AnalysisPair) {
        if let Some(ch) = self.channel.lock().expect("channel").as_ref() {
            let _ = ch.send(PairDto::from(pair));
        }
    }
}
