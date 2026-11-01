//! LoomCode 演示后端:用脚本化场景驱动界面,不连接真实 Claude。

pub mod player;
pub mod scenarios;
pub mod script;
pub mod workspace;

use player::Player;
use loom_core::*;

/// 演示后端。`speed` 为时间倍率:1.0 为正常速度,0.0 为瞬时(测试用)。
#[derive(Debug, Clone)]
pub struct DemoBackend {
    pub speed: f32,
}

impl Default for DemoBackend {
    fn default() -> Self {
        Self { speed: 1.0 }
    }
}

impl AgentBackend for DemoBackend {
    fn name(&self) -> &'static str {
        "demo"
    }

    fn start(&self, config: BackendConfig) -> Result<BackendHandle, BackendError> {
        let (handle, cmd_rx, ev_tx) = channel_pair();
        let speed = self.speed;
        std::thread::Builder::new()
            .name("loom-demo-backend".into())
            .spawn(move || {
                let mut player = Player::new(cmd_rx, ev_tx, speed, config);
                player.run();
            })
            .map_err(|e| BackendError::Spawn(e.to_string()))?;
        Ok(handle)
    }
}
