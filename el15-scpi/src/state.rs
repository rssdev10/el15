use std::sync::Arc;
use tokio::sync::Mutex;

use el15_bt::{Device, EL15Status, Mode};

/// Shared device + last-known-status used by every SCPI session.
#[derive(Clone)]
pub struct SharedState {
    inner: Arc<Mutex<Inner>>,
}

struct Inner {
    pub device: Option<Arc<Device>>,
    pub status: EL15Status,
    pub setpoint_cc: f32,
    pub setpoint_cv: f32,
    pub setpoint_cr: f32,
    pub setpoint_cp: f32,
    pub last_mode: Mode,
}

impl Default for SharedState {
    fn default() -> Self {
        Self {
            inner: Arc::new(Mutex::new(Inner {
                device: None,
                status: EL15Status::default(),
                setpoint_cc: 12.0,
                setpoint_cv: 5.0,
                setpoint_cr: 0.5,
                setpoint_cp: 100.0,
                last_mode: Mode::CC,
            })),
        }
    }
}

impl SharedState {
    pub async fn set_device(&self, dev: Option<Arc<Device>>) {
        self.inner.lock().await.device = dev;
    }

    pub async fn update_status(&self, status: EL15Status) {
        let mut g = self.inner.lock().await;
        if let Some(m) = status.mode() {
            g.last_mode = m;
        }
        g.status = status;
    }

    pub async fn snapshot(&self) -> StateSnapshot {
        let g = self.inner.lock().await;
        StateSnapshot {
            device: g.device.clone(),
            status: g.status.clone(),
            setpoint_cc: g.setpoint_cc,
            setpoint_cv: g.setpoint_cv,
            setpoint_cr: g.setpoint_cr,
            setpoint_cp: g.setpoint_cp,
            last_mode: g.last_mode,
        }
    }

    pub async fn set_setpoint(&self, mode: Mode, value: f32) {
        let mut g = self.inner.lock().await;
        match mode {
            Mode::CC => g.setpoint_cc = value,
            Mode::CV => g.setpoint_cv = value,
            Mode::CR => g.setpoint_cr = value,
            Mode::CP => g.setpoint_cp = value,
            _ => {}
        }
    }

    pub async fn set_mode(&self, mode: Mode) {
        self.inner.lock().await.last_mode = mode;
    }
}

pub struct StateSnapshot {
    pub device: Option<Arc<Device>>,
    pub status: EL15Status,
    pub setpoint_cc: f32,
    pub setpoint_cv: f32,
    pub setpoint_cr: f32,
    pub setpoint_cp: f32,
    pub last_mode: Mode,
}
