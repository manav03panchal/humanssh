//! Status bar component showing system telemetry.
//!
//! Displays CPU, memory, network stats and terminal info in a footer bar,
//! similar to MobaXTerm's remote monitoring bar.

use crate::config::status_bar as config;
use crate::theme::terminal_colors;
use gpui::{div, px, App, IntoElement, ParentElement, SharedString, Styled};
use gpui_component::button::{Button, ButtonVariants};
use gpui_component::Sizable;
use parking_lot::RwLock;
use std::sync::Arc;
use std::time::Instant;
use sysinfo::{Networks, System};

/// Collected system statistics.
#[derive(Clone, Debug)]
pub struct SystemStats {
    // System info
    /// Hostname.
    pub hostname: SharedString,
    /// OS name and version.
    pub os_info: SharedString,
    /// System uptime in seconds.
    pub uptime_secs: u64,
    /// Load average (1 min).
    pub load_avg: f64,

    // Resource usage
    /// CPU usage percentage (0-100).
    pub cpu_percent: f32,
    /// Number of CPU cores.
    pub cpu_cores: usize,
    /// Used memory in bytes.
    pub memory_used: u64,
    /// Total memory in bytes.
    pub memory_total: u64,
    /// Available memory in bytes.
    pub memory_available: u64,
    /// Swap used in bytes.
    pub swap_used: u64,
    /// Swap total in bytes.
    pub swap_total: u64,

    // Network
    /// Network bytes received per second.
    pub network_rx_per_sec: u64,
    /// Network bytes transmitted per second.
    pub network_tx_per_sec: u64,
    /// Total bytes received this session.
    pub network_rx_total: u64,
    /// Total bytes transmitted this session.
    pub network_tx_total: u64,

    // Process info
    /// Number of running processes.
    pub process_count: usize,

    // Terminal info
    /// Current shell name.
    pub shell: SharedString,
    /// Current working directory.
    pub cwd: SharedString,
    /// Current foreground process name.
    pub process: SharedString,
}

impl Default for SystemStats {
    fn default() -> Self {
        Self {
            hostname: "localhost".into(),
            os_info: "—".into(),
            uptime_secs: 0,
            load_avg: 0.0,
            cpu_percent: 0.0,
            cpu_cores: 1,
            memory_used: 0,
            memory_total: 0,
            memory_available: 0,
            swap_used: 0,
            swap_total: 0,
            network_rx_per_sec: 0,
            network_tx_per_sec: 0,
            network_rx_total: 0,
            network_tx_total: 0,
            process_count: 0,
            shell: "—".into(),
            cwd: "~".into(),
            process: "—".into(),
        }
    }
}

/// Collector for system statistics with caching.
pub struct SystemStatsCollector {
    system: System,
    networks: Networks,
    last_update: Instant,
    last_network_rx: u64,
    last_network_tx: u64,
    cached_stats: SystemStats,
}

impl SystemStatsCollector {
    /// Create a new stats collector.
    pub fn new() -> Self {
        let mut system = System::new_all();
        system.refresh_all();

        let networks = Networks::new_with_refreshed_list();

        // Get static system info
        let hostname = System::host_name().unwrap_or_else(|| "localhost".to_string());
        let os_name = System::name().unwrap_or_else(|| "Unknown".to_string());
        let os_version = System::os_version().unwrap_or_default();
        let os_info = if os_version.is_empty() {
            os_name
        } else {
            format!("{} {}", os_name, os_version)
        };

        let stats = SystemStats {
            hostname: hostname.into(),
            os_info: os_info.into(),
            cpu_cores: system.cpus().len(),
            ..Default::default()
        };

        Self {
            system,
            networks,
            last_update: Instant::now(),
            last_network_rx: 0,
            last_network_tx: 0,
            cached_stats: stats,
        }
    }

    /// Refresh stats if enough time has passed since last refresh.
    /// Returns the current stats (cached or freshly computed).
    pub fn refresh(&mut self) -> SystemStats {
        let elapsed = self.last_update.elapsed();

        // Only refresh if enough time has passed
        if elapsed < config::REFRESH_INTERVAL {
            return self.cached_stats.clone();
        }

        // Refresh CPU and memory
        self.system.refresh_cpu_all();
        self.system.refresh_memory();

        // CPU
        let cpu_percent = self.system.global_cpu_usage();

        // Memory
        let memory_used = self.system.used_memory();
        let memory_total = self.system.total_memory();
        let memory_available = self.system.available_memory();
        let swap_used = self.system.used_swap();
        let swap_total = self.system.total_swap();

        // Uptime
        let uptime_secs = System::uptime();

        // Load average (macOS/Linux)
        let load_avg = System::load_average().one;

        // Process count
        self.system
            .refresh_processes(sysinfo::ProcessesToUpdate::All, true);
        let process_count = self.system.processes().len();

        // Refresh network stats
        self.networks.refresh();

        let mut total_rx: u64 = 0;
        let mut total_tx: u64 = 0;
        for (_name, data) in self.networks.iter() {
            total_rx += data.total_received();
            total_tx += data.total_transmitted();
        }

        // Calculate bytes per second
        let elapsed_secs = elapsed.as_secs_f64();
        let rx_per_sec = if elapsed_secs > 0.0 && total_rx >= self.last_network_rx {
            ((total_rx - self.last_network_rx) as f64 / elapsed_secs) as u64
        } else {
            0
        };
        let tx_per_sec = if elapsed_secs > 0.0 && total_tx >= self.last_network_tx {
            ((total_tx - self.last_network_tx) as f64 / elapsed_secs) as u64
        } else {
            0
        };

        self.last_network_rx = total_rx;
        self.last_network_tx = total_tx;
        self.last_update = Instant::now();

        // Update cached stats
        self.cached_stats.cpu_percent = cpu_percent;
        self.cached_stats.memory_used = memory_used;
        self.cached_stats.memory_total = memory_total;
        self.cached_stats.memory_available = memory_available;
        self.cached_stats.swap_used = swap_used;
        self.cached_stats.swap_total = swap_total;
        self.cached_stats.uptime_secs = uptime_secs;
        self.cached_stats.load_avg = load_avg;
        self.cached_stats.process_count = process_count;
        self.cached_stats.network_rx_per_sec = rx_per_sec;
        self.cached_stats.network_tx_per_sec = tx_per_sec;
        self.cached_stats.network_rx_total = total_rx;
        self.cached_stats.network_tx_total = total_tx;

        self.cached_stats.clone()
    }

    /// Update terminal-specific info (shell, cwd, process).
    pub fn set_terminal_info(
        &mut self,
        shell: impl Into<SharedString>,
        cwd: impl Into<SharedString>,
        process: impl Into<SharedString>,
    ) {
        self.cached_stats.shell = shell.into();
        self.cached_stats.cwd = cwd.into();
        self.cached_stats.process = process.into();
    }

    /// Get the current cached stats without refreshing.
    pub fn current(&self) -> SystemStats {
        self.cached_stats.clone()
    }
}

impl Default for SystemStatsCollector {
    fn default() -> Self {
        Self::new()
    }
}

/// Global stats collector instance.
static STATS_COLLECTOR: once_cell::sync::Lazy<Arc<RwLock<SystemStatsCollector>>> =
    once_cell::sync::Lazy::new(|| Arc::new(RwLock::new(SystemStatsCollector::new())));

/// Get the global stats collector.
pub fn stats_collector() -> Arc<RwLock<SystemStatsCollector>> {
    STATS_COLLECTOR.clone()
}

/// Format bytes into human-readable string (KB, MB, GB).
fn format_bytes(bytes: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = KB * 1024;
    const GB: u64 = MB * 1024;

    if bytes >= GB {
        format!("{:.1}G", bytes as f64 / GB as f64)
    } else if bytes >= MB {
        format!("{:.1}M", bytes as f64 / MB as f64)
    } else if bytes >= KB {
        format!("{:.0}K", bytes as f64 / KB as f64)
    } else {
        format!("{}B", bytes)
    }
}

/// Format bytes per second into human-readable throughput.
fn format_throughput(bytes_per_sec: u64) -> String {
    format!("{}/s", format_bytes(bytes_per_sec))
}

/// Format uptime into human-readable string.
fn format_uptime(secs: u64) -> String {
    let days = secs / 86400;
    let hours = (secs % 86400) / 3600;
    let mins = (secs % 3600) / 60;

    if days > 0 {
        format!("{}d {}h", days, hours)
    } else if hours > 0 {
        format!("{}h {}m", hours, mins)
    } else {
        format!("{}m", mins)
    }
}

/// Format memory percentage.
fn format_mem_percent(used: u64, total: u64) -> String {
    if total > 0 {
        format!("{:.0}%", (used as f64 / total as f64) * 100.0)
    } else {
        "0%".to_string()
    }
}

/// Render the status bar.
pub fn render_status_bar(stats: &SystemStats, cx: &App) -> impl IntoElement {
    let colors = terminal_colors(cx);
    let bg = colors.title_bar;
    let fg = colors.foreground;
    let muted = colors.muted;
    let border = colors.border;

    // Color coding for CPU based on usage
    let cpu_color = if stats.cpu_percent > 80.0 {
        colors.red
    } else if stats.cpu_percent > 50.0 {
        colors.yellow
    } else {
        colors.green
    };

    // Color coding for memory based on usage
    let mem_percent = if stats.memory_total > 0 {
        (stats.memory_used as f64 / stats.memory_total as f64 * 100.0) as f32
    } else {
        0.0
    };
    let mem_color = if mem_percent > 80.0 {
        colors.red
    } else if mem_percent > 50.0 {
        colors.yellow
    } else {
        colors.green
    };

    // Color coding for load average
    let load_color = if stats.load_avg > stats.cpu_cores as f64 {
        colors.red
    } else if stats.load_avg > (stats.cpu_cores as f64 * 0.7) {
        colors.yellow
    } else {
        colors.green
    };

    // Build detailed tooltips
    let cpu_tooltip = format!(
        "CPU: {:.1}%\nCores: {}\nLoad: {:.2}",
        stats.cpu_percent, stats.cpu_cores, stats.load_avg
    );

    let mem_tooltip = format!(
        "Used: {}\nAvailable: {}\nTotal: {}\nSwap: {}/{}",
        format_bytes(stats.memory_used),
        format_bytes(stats.memory_available),
        format_bytes(stats.memory_total),
        format_bytes(stats.swap_used),
        format_bytes(stats.swap_total)
    );

    let net_tooltip = format!(
        "Download: {}\nUpload: {}\nTotal ↓: {}\nTotal ↑: {}",
        format_throughput(stats.network_rx_per_sec),
        format_throughput(stats.network_tx_per_sec),
        format_bytes(stats.network_rx_total),
        format_bytes(stats.network_tx_total)
    );

    let sys_tooltip = format!(
        "Host: {}\nOS: {}\nUptime: {}\nProcesses: {}",
        stats.hostname,
        stats.os_info,
        format_uptime(stats.uptime_secs),
        stats.process_count
    );

    div()
        .h(px(config::HEIGHT))
        .w_full()
        .bg(bg)
        .border_t_1()
        .border_color(border)
        .flex()
        .items_center()
        .px(px(config::HORIZONTAL_PADDING))
        .gap(px(config::ITEM_GAP))
        .text_xs()
        // Host info with tooltip
        .child(
            Button::new("status-host")
                .xsmall()
                .ghost()
                .label(stats.hostname.to_string())
                .tooltip(sys_tooltip),
        )
        // Separator
        .child(div().text_color(muted).child("│"))
        // CPU with tooltip
        .child(
            div()
                .flex()
                .items_center()
                .gap_1()
                .child(div().text_color(muted).child("CPU"))
                .child(
                    Button::new("status-cpu")
                        .xsmall()
                        .ghost()
                        .label(format!("{:.0}%", stats.cpu_percent))
                        .text_color(cpu_color)
                        .tooltip(cpu_tooltip),
                ),
        )
        // Separator
        .child(div().text_color(muted).child("│"))
        // Memory with tooltip
        .child(
            div()
                .flex()
                .items_center()
                .gap_1()
                .child(div().text_color(muted).child("MEM"))
                .child(
                    Button::new("status-mem")
                        .xsmall()
                        .ghost()
                        .label(format_mem_percent(stats.memory_used, stats.memory_total))
                        .text_color(mem_color)
                        .tooltip(mem_tooltip),
                ),
        )
        // Separator
        .child(div().text_color(muted).child("│"))
        // Load average
        .child(
            div()
                .flex()
                .items_center()
                .gap_1()
                .child(div().text_color(muted).child("LOAD"))
                .child(
                    div()
                        .text_color(load_color)
                        .child(format!("{:.2}", stats.load_avg)),
                ),
        )
        // Separator
        .child(div().text_color(muted).child("│"))
        // Network with tooltip
        .child(
            Button::new("status-net")
                .xsmall()
                .ghost()
                .label(format!(
                    "↓{} ↑{}",
                    format_throughput(stats.network_rx_per_sec),
                    format_throughput(stats.network_tx_per_sec)
                ))
                .tooltip(net_tooltip),
        )
        // Separator
        .child(div().text_color(muted).child("│"))
        // Uptime
        .child(
            div()
                .flex()
                .items_center()
                .gap_1()
                .child(div().text_color(muted).child("UP"))
                .child(div().text_color(fg).child(format_uptime(stats.uptime_secs))),
        )
        // Spacer
        .child(div().flex_1())
        // Process
        .child(
            div()
                .flex()
                .items_center()
                .gap_1()
                .child(div().text_color(muted).child("PROC"))
                .child(div().text_color(fg).child(stats.process.clone())),
        )
        // Separator
        .child(div().text_color(muted).child("│"))
        // CWD (truncated from left if too long)
        .child(
            div()
                .flex()
                .items_center()
                .gap_1()
                .max_w(px(250.0))
                .overflow_hidden()
                .child(
                    div()
                        .text_color(fg)
                        .overflow_hidden()
                        .whitespace_nowrap()
                        .child(truncate_path(&stats.cwd, 40)),
                ),
        )
}

/// Truncate a path from the left, keeping the rightmost chars.
fn truncate_path(path: &str, max_len: usize) -> String {
    if path.len() <= max_len {
        path.to_string()
    } else {
        format!("…{}", &path[path.len() - max_len + 1..])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_bytes() {
        assert_eq!(format_bytes(500), "500B");
        assert_eq!(format_bytes(1024), "1K");
        assert_eq!(format_bytes(1536), "2K");
        assert_eq!(format_bytes(1024 * 1024), "1.0M");
        assert_eq!(format_bytes(1024 * 1024 * 1024), "1.0G");
        assert_eq!(format_bytes(1536 * 1024 * 1024), "1.5G");
    }

    #[test]
    fn test_format_throughput() {
        assert_eq!(format_throughput(0), "0B/s");
        assert_eq!(format_throughput(1024), "1K/s");
        assert_eq!(format_throughput(1024 * 1024), "1.0M/s");
    }

    #[test]
    fn test_format_uptime() {
        assert_eq!(format_uptime(30), "0m");
        assert_eq!(format_uptime(90), "1m");
        assert_eq!(format_uptime(3700), "1h 1m");
        assert_eq!(format_uptime(90000), "1d 1h");
    }

    #[test]
    fn test_truncate_path() {
        assert_eq!(truncate_path("/short", 30), "/short");
        assert_eq!(
            truncate_path("/very/long/path/that/exceeds/the/maximum/length", 20),
            "…/the/maximum/length"
        );
    }

    #[test]
    fn test_system_stats_default() {
        let stats = SystemStats::default();
        assert_eq!(stats.cpu_percent, 0.0);
        assert_eq!(stats.memory_used, 0);
        assert_eq!(stats.memory_total, 0);
    }
}
