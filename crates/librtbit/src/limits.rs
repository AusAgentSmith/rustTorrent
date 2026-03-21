use arc_swap::ArcSwapOption;
use governor::DefaultDirectRateLimiter as RateLimiter;
use governor::Quota;
use parking_lot::RwLock;
use serde::Deserialize;
use serde::Serialize;
use std::num::NonZeroU32;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::Arc;

#[derive(Default, Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "swagger", derive(utoipa::ToSchema))]
pub struct LimitsConfig {
    #[cfg_attr(feature = "swagger", schema(value_type = Option<u32>))]
    pub upload_bps: Option<NonZeroU32>,
    #[cfg_attr(feature = "swagger", schema(value_type = Option<u32>))]
    pub download_bps: Option<NonZeroU32>,
    pub peer_limit: Option<usize>,
    pub concurrent_init_limit: Option<usize>,
}

/// Per-torrent rate limit configuration.
/// Values of 0 or None mean "no per-torrent limit" (fall back to global).
#[derive(Default, Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "swagger", derive(utoipa::ToSchema))]
pub struct TorrentLimitsConfig {
    /// Download rate limit in bytes per second. 0 or null = unlimited (use global).
    pub download_rate: Option<u32>,
    /// Upload rate limit in bytes per second. 0 or null = unlimited (use global).
    pub upload_rate: Option<u32>,
}

impl TorrentLimitsConfig {
    pub fn download_bps(&self) -> Option<NonZeroU32> {
        self.download_rate.and_then(NonZeroU32::new)
    }
    pub fn upload_bps(&self) -> Option<NonZeroU32> {
        self.upload_rate.and_then(NonZeroU32::new)
    }
}

/// Alternative speed limits configuration ("turtle mode").
#[derive(Default, Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "swagger", derive(utoipa::ToSchema))]
pub struct AltSpeedConfig {
    /// Alternative download speed limit in bytes per second.
    #[cfg_attr(feature = "swagger", schema(value_type = Option<u32>))]
    pub alt_speed_down: Option<NonZeroU32>,
    /// Alternative upload speed limit in bytes per second.
    #[cfg_attr(feature = "swagger", schema(value_type = Option<u32>))]
    pub alt_speed_up: Option<NonZeroU32>,
}

/// Alternative speed schedule configuration.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "swagger", derive(utoipa::ToSchema))]
pub struct AltSpeedSchedule {
    /// Whether the schedule is enabled.
    pub enabled: bool,
    /// Start time in minutes from midnight (e.g. 480 = 8:00 AM).
    pub start_minutes: u32,
    /// End time in minutes from midnight (e.g. 1020 = 5:00 PM).
    pub end_minutes: u32,
    /// Bitmask for days of week: 1=Mon, 2=Tue, 4=Wed, 8=Thu, 16=Fri, 32=Sat, 64=Sun, 127=all.
    pub days: u8,
}

impl Default for AltSpeedSchedule {
    fn default() -> Self {
        Self {
            enabled: false,
            start_minutes: 480,  // 8:00 AM
            end_minutes: 1020,   // 5:00 PM
            days: 127,           // all days
        }
    }
}

impl AltSpeedSchedule {
    /// Check if the current local time falls within the schedule.
    pub fn is_active_now(&self) -> bool {
        if !self.enabled {
            return false;
        }
        let now = std::time::SystemTime::now();
        let since_epoch = now
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default();
        let secs = since_epoch.as_secs();

        // Calculate local time using the system's UTC offset.
        // We compute a rough local offset by comparing timestamps.
        let local_secs = secs as i64 + local_utc_offset_secs();

        let day_secs = local_secs.rem_euclid(86400) as u32;
        let minutes_from_midnight = day_secs / 60;

        // Day of week: 0=Thu (epoch was Thursday), so adjust to 0=Mon
        let days_since_epoch = (local_secs.div_euclid(86400)) as i64;
        // 1970-01-01 was Thursday (day 3 in 0=Mon numbering)
        let day_of_week = ((days_since_epoch + 3) % 7) as u8; // 0=Mon .. 6=Sun
        let day_bit = 1u8 << day_of_week;

        if self.days & day_bit == 0 {
            return false;
        }

        if self.start_minutes <= self.end_minutes {
            // Normal range (e.g., 8:00 to 17:00)
            minutes_from_midnight >= self.start_minutes
                && minutes_from_midnight < self.end_minutes
        } else {
            // Wrapping range (e.g., 22:00 to 6:00)
            minutes_from_midnight >= self.start_minutes
                || minutes_from_midnight < self.end_minutes
        }
    }
}

/// Get the local UTC offset in seconds. Uses nix::libc on unix, hardcodes to 0 on other platforms.
fn local_utc_offset_secs() -> i64 {
    #[cfg(unix)]
    {
        // SAFETY: calling libc::time and libc::localtime_r is safe with a valid pointer.
        unsafe {
            let t = nix::libc::time(std::ptr::null_mut());
            let mut tm: nix::libc::tm = std::mem::zeroed();
            nix::libc::localtime_r(&t, &mut tm);
            tm.tm_gmtoff as i64
        }
    }
    #[cfg(not(unix))]
    {
        0
    }
}

/// Response for alt speed status.
#[derive(Serialize, Deserialize, Clone, Debug)]
#[cfg_attr(feature = "swagger", derive(utoipa::ToSchema))]
pub struct AltSpeedStatus {
    /// Whether alt speed mode is currently active.
    pub enabled: bool,
    /// The current alt speed config.
    pub config: AltSpeedConfig,
}

/// Request to toggle alt speed mode.
#[derive(Serialize, Deserialize, Clone, Debug)]
#[cfg_attr(feature = "swagger", derive(utoipa::ToSchema))]
pub struct AltSpeedToggle {
    pub enabled: bool,
}

/// Manages alternative speed limits state at the session level.
pub struct AltSpeedState {
    /// Whether alt speed mode is currently enabled (manually or by schedule).
    enabled: AtomicBool,
    /// The alt speed limits to apply when enabled.
    config: RwLock<AltSpeedConfig>,
    /// The schedule for automatic toggling.
    schedule: RwLock<AltSpeedSchedule>,
    /// The normal (non-alt) speed limits, saved when alt mode activates.
    saved_normal_upload: AtomicU32,
    saved_normal_download: AtomicU32,
}

impl AltSpeedState {
    pub fn new(config: AltSpeedConfig, schedule: AltSpeedSchedule) -> Self {
        Self {
            enabled: AtomicBool::new(false),
            config: RwLock::new(config),
            schedule: RwLock::new(schedule),
            saved_normal_upload: AtomicU32::new(0),
            saved_normal_download: AtomicU32::new(0),
        }
    }

    pub fn is_enabled(&self) -> bool {
        self.enabled.load(Ordering::Relaxed)
    }

    pub fn config(&self) -> AltSpeedConfig {
        self.config.read().clone()
    }

    pub fn schedule(&self) -> AltSpeedSchedule {
        self.schedule.read().clone()
    }

    pub fn set_schedule(&self, schedule: AltSpeedSchedule) {
        *self.schedule.write() = schedule;
    }

    pub fn set_config(&self, config: AltSpeedConfig) {
        *self.config.write() = config;
    }

    /// Enable alt speed mode, saving the current global limits and applying alt limits.
    pub fn enable(&self, global_limits: &Limits) {
        if self.enabled.swap(true, Ordering::Relaxed) {
            return; // already enabled
        }
        // Save current normal limits
        let cur_up = global_limits.get_upload_bps().map(|v| v.get()).unwrap_or(0);
        let cur_down = global_limits.get_download_bps().map(|v| v.get()).unwrap_or(0);
        self.saved_normal_upload.store(cur_up, Ordering::Relaxed);
        self.saved_normal_download.store(cur_down, Ordering::Relaxed);

        // Apply alt limits
        let cfg = self.config.read();
        global_limits.set_upload_bps(cfg.alt_speed_up);
        global_limits.set_download_bps(cfg.alt_speed_down);
    }

    /// Disable alt speed mode, restoring the previously saved global limits.
    pub fn disable(&self, global_limits: &Limits) {
        if !self.enabled.swap(false, Ordering::Relaxed) {
            return; // already disabled
        }
        // Restore saved limits
        let up = NonZeroU32::new(self.saved_normal_upload.load(Ordering::Relaxed));
        let down = NonZeroU32::new(self.saved_normal_download.load(Ordering::Relaxed));
        global_limits.set_upload_bps(up);
        global_limits.set_download_bps(down);
    }

    /// Check the schedule and toggle alt speed mode if needed.
    /// Returns true if the state changed.
    pub fn check_schedule(&self, global_limits: &Limits) -> bool {
        let should_be_active = self.schedule.read().is_active_now();
        let is_active = self.is_enabled();

        if should_be_active && !is_active {
            self.enable(global_limits);
            true
        } else if !should_be_active && is_active {
            self.disable(global_limits);
            true
        } else {
            false
        }
    }
}

struct Limit {
    limiter: ArcSwapOption<RateLimiter>,
    current_bps: AtomicU32,
}

impl Limit {
    fn new_inner(bps: Option<NonZeroU32>) -> Option<Arc<RateLimiter>> {
        let bps = bps?;
        Some(Arc::new(RateLimiter::direct(Quota::per_second(bps))))
    }

    fn new(bps: Option<NonZeroU32>) -> Self {
        Self {
            limiter: ArcSwapOption::new(Self::new_inner(bps)),
            current_bps: AtomicU32::new(bps.map(|v| v.get()).unwrap_or(0)),
        }
    }

    async fn acquire(&self, size: NonZeroU32) -> crate::Result<()> {
        let lim = self.limiter.load().clone();
        if let Some(rl) = lim.as_ref() {
            rl.until_n_ready(size).await?;
        }
        Ok(())
    }

    fn set(&self, limit: Option<NonZeroU32>) {
        let new = Self::new_inner(limit);
        self.limiter.swap(new);
        self.current_bps
            .store(limit.map(|v| v.get()).unwrap_or(0), Ordering::Relaxed);
    }

    fn get(&self) -> Option<NonZeroU32> {
        NonZeroU32::new(self.current_bps.load(Ordering::Relaxed))
    }
}

pub struct Limits {
    down: Limit,
    up: Limit,
}

impl Limits {
    pub fn new(config: LimitsConfig) -> Self {
        Self {
            down: Limit::new(config.download_bps),
            up: Limit::new(config.upload_bps),
        }
    }

    pub async fn prepare_for_upload(&self, len: NonZeroU32) -> crate::Result<()> {
        self.up.acquire(len).await
    }

    pub async fn prepare_for_download(&self, len: NonZeroU32) -> crate::Result<()> {
        self.down.acquire(len).await
    }

    pub fn set_upload_bps(&self, bps: Option<NonZeroU32>) {
        self.up.set(bps);
    }

    pub fn set_download_bps(&self, bps: Option<NonZeroU32>) {
        self.down.set(bps);
    }

    pub fn get_upload_bps(&self) -> Option<NonZeroU32> {
        self.up.get()
    }

    pub fn get_download_bps(&self) -> Option<NonZeroU32> {
        self.down.get()
    }

    pub fn get_config(&self) -> LimitsConfig {
        LimitsConfig {
            upload_bps: self.get_upload_bps(),
            download_bps: self.get_download_bps(),
            peer_limit: None,
            concurrent_init_limit: None,
        }
    }
}
