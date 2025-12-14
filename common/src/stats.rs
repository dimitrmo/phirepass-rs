use get_if_addrs::{IfAddr, get_if_addrs};
use mac_address::get_mac_address;
use netstat2::{AddressFamilyFlags, ProtocolFlags, get_sockets_info};
use os_info;
use serde::{Deserialize, Serialize};
use std::num::NonZeroUsize;
use std::sync::{Mutex, OnceLock};
use sysinfo::{ProcessStatus, ProcessesToUpdate, System, get_current_pid};
use thread_count::thread_count;

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
    // process
    pub proc_id: String,
    pub proc_threads: usize,
    pub proc_cpu: f32,
    pub proc_mem_bytes: u64,
    pub proc_uptime_secs: u64,
    // host
    pub host_name: String,
    pub host_ip: String,
    pub host_mac: String,
    pub host_cpu: f32,
    pub host_mem_used_bytes: u64,
    pub host_mem_total_bytes: u64,
    pub host_uptime_secs: u64,
    pub host_load_average: [f64; 3],
    pub host_os_info: String,
    pub host_connections: usize,
    pub host_processes: usize,
}

static HOST_IP: OnceLock<String> = OnceLock::new();
static HOST_MAC: OnceLock<String> = OnceLock::new();
static HOST_NAME: OnceLock<String> = OnceLock::new();
static HOST_OS_INFO: OnceLock<String> = OnceLock::new();
static SYS_INFO: OnceLock<Mutex<System>> = OnceLock::new();

impl Stats {
    pub fn gather() -> Option<Self> {
        let sys = SYS_INFO.get_or_init(|| Mutex::new(System::new_all()));
        let mut sys = sys.lock().ok()?;

        sys.refresh_memory();
        sys.refresh_cpu_all();
        sys.refresh_processes(ProcessesToUpdate::All, true);

        let pid = get_current_pid().ok()?;
        let proc = sys.process(pid)?;
        let proc_threads = thread_count().map(NonZeroUsize::get).unwrap_or(0);

        let hostname = HOST_NAME
            .get_or_init(|| {
                std::env::var("HOSTNAME")
                    .ok()
                    .or_else(System::host_name)
                    .unwrap_or_else(|| "unknown".into())
            })
            .to_string();

        let host_ip = HOST_IP.get_or_init(|| resolve_host_ip()).to_string();
        let host_os_info = HOST_OS_INFO
            .get_or_init(|| format!("{}", os_info::get()))
            .to_string();

        let host_load_average = Self::loadavg();
        let host_connections = Self::connections().unwrap_or(0);
        let host_mac = Self::mac();

        let host_processes = sys
            .processes()
            .values()
            .filter(|p| p.status() == ProcessStatus::Run)
            .count();

        Some(Self {
            proc_id: pid.to_string(),
            host_name: hostname,
            host_ip,
            host_mac,
            proc_threads,
            proc_cpu: proc.cpu_usage(),
            proc_mem_bytes: proc.memory(),
            proc_uptime_secs: proc.run_time(),
            host_cpu: sys.global_cpu_usage(),
            host_mem_used_bytes: sys.used_memory(),
            host_mem_total_bytes: sys.total_memory(),
            host_uptime_secs: System::uptime(),
            host_load_average,
            host_os_info: format!("{}", host_os_info),
            host_connections,
            host_processes,
        })
    }

    fn mac() -> String {
        HOST_MAC
            .get_or_init(|| match get_mac_address() {
                Ok(mac) => match mac {
                    None => String::from("unknown"),
                    Some(addr) => addr.to_string(),
                },
                Err(_) => String::from("unknown"),
            })
            .clone()
    }

    fn connections() -> anyhow::Result<usize> {
        let af_flags = AddressFamilyFlags::IPV4 | AddressFamilyFlags::IPV6;
        let proto_flags = ProtocolFlags::TCP | ProtocolFlags::UDP;
        let sockets = get_sockets_info(af_flags, proto_flags)?;
        Ok(sockets.len())
    }

    #[cfg(unix)]
    fn loadavg() -> [f64; 3] {
        let mut v = [0f64; 3];
        let n = unsafe { libc::getloadavg(v.as_mut_ptr(), 3) };
        if n == 3 {
            [v[0], v[1], v[2]]
        } else {
            [0f64, 0f64, 0f64]
        }
    }

    #[cfg(not(unix))]
    fn loadavg() -> (f64, f64, f64) {
        (0f64, 0f64, 0f64)
    }

    pub fn log_line(&self) -> String {
        format!(
            "pid={} threads={} cpu={:.1}% mem={} uptime={} | host={} ip={} os={} cpu={:.1}% mem={}/{} procs={} conns={} load={:.2}/{:.2}/{:.2} uptime={}",
            self.proc_id,
            self.proc_threads,
            self.proc_cpu,
            format_mem(self.proc_mem_bytes),
            format_duration(self.proc_uptime_secs),
            self.host_name,
            self.host_ip,
            self.host_os_info,
            self.host_cpu,
            format_mem(self.host_mem_used_bytes),
            format_mem(self.host_mem_total_bytes),
            self.host_processes,
            self.host_connections,
            self.host_load_average[0],
            self.host_load_average[1],
            self.host_load_average[2],
            format_duration(self.host_uptime_secs),
        )
    }

    pub fn encoded(&self) -> anyhow::Result<String> {
        serde_json::to_string_pretty(&self).map_err(|e| e.into())
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
