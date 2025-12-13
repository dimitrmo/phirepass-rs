use get_if_addrs::{IfAddr, get_if_addrs};
use serde::{Deserialize, Serialize};
use std::num::NonZeroUsize;
use sysinfo::{System, get_current_pid};
use thread_count::thread_count;
use os_info;

pub fn format_mem(bytes: u64) -> String {
    let mut size = bytes as f64;
    let units = ["B", "KiB", "MiB", "GiB", "TiB", "PiB"];
    let mut unit = units[0];

    for next in &units[1..] {
        if size < 1024.0 {
            break;
        }
        size /= 1024.0;
        unit = next;
    }

    format!("{:.1} {}", size, unit)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Stats {
    pub pid: String,
    pub hostname: String,
    pub host_ip: String,
    pub threads: usize,
    pub proc_cpu: f32,
    pub proc_mem_bytes: u64,
    pub host_cpu: f32,
    pub host_mem_used_bytes: u64,
    pub host_mem_total_bytes: u64,
    pub host_uptime_secs: u64,
    pub proc_uptime_secs: u64,
    pub host_os_info: String,
}

impl Stats {
    pub fn gather() -> Option<Self> {
        let mut sys = System::new_all();
        sys.refresh_all();

        let pid = get_current_pid().ok()?;
        let proc = sys.process(pid)?;

        let count = thread_count().map(NonZeroUsize::get).unwrap_or(0);

        let hostname = std::env::var("HOSTNAME")
            .ok()
            .or_else(System::host_name)
            .unwrap_or_else(|| "unknown".into());

        let host_ip = resolve_host_ip();
        let host_os_info = os_info::get();

        Some(Self {
            pid: pid.to_string(),
            hostname,
            host_ip,
            threads: count,
            proc_cpu: proc.cpu_usage(),
            proc_mem_bytes: proc.memory(),
            proc_uptime_secs: proc.run_time(),
            host_cpu: sys.global_cpu_usage(),
            host_mem_used_bytes: sys.used_memory(),
            host_mem_total_bytes: sys.total_memory(),
            host_uptime_secs: System::uptime(),
            host_os_info: format!("{}", host_os_info),
        })
    }

    pub fn log_line(&self) -> String {
        format!(
            "stats: pid={}, host_os={}, host_name={}, host_ip={}, host_uptime={}, proc_uptime={}, proc_cpu={:.1}%, proc_mem={}, host_cpu={:.1}%, host_mem={} / {}, host_threads={}",
            self.pid,
            self.host_os_info,
            self.hostname,
            self.host_ip,
            format_duration(self.host_uptime_secs),
            format_duration(self.proc_uptime_secs),
            self.proc_cpu,
            format_mem(self.proc_mem_bytes),
            self.host_cpu,
            format_mem(self.host_mem_used_bytes),
            format_mem(self.host_mem_total_bytes),
            self.threads,
        )
    }
}

fn format_duration(secs: u64) -> String {
    let hours = secs / 3600;
    let minutes = (secs % 3600) / 60;
    let seconds = secs % 60;
    format!("{hours:02}:{minutes:02}:{seconds:02}")
}

fn resolve_host_ip() -> String {
    let interfaces = get_if_addrs().ok();
    match interfaces {
        Some(interfaces) => {
            for interface in interfaces {
                if interface.is_loopback() {
                    continue;
                }

                return match interface.addr {
                    IfAddr::V4(v4) => v4.ip.to_string(),
                    IfAddr::V6(v6) => v6.ip.to_string(),
                };
            }

            "unknown".into()
        }
        None => "unknown".into(),
    }
}
