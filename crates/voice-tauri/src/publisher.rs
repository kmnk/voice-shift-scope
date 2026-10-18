//! 分析の結果を Tauri の channel で画面に送る `AnalysisPublisher`。

use std::sync::Mutex;

use tauri::ipc::Channel;
use voice_domain::AnalysisPair;
use voice_domain::ports::AnalysisPublisher;

use crate::dto::PairDto;

/// 開始のたびに画面から受けた channel に差し替える。
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
