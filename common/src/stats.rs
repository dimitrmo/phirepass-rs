use comfy_table::Table;
use get_if_addrs::{IfAddr, get_if_addrs};
use mac_address::get_mac_address;
use netstat2::{AddressFamilyFlags, ProtocolFlags, get_sockets_info};
use os_info;
use serde::{Deserialize, Serialize};
use std::num::NonZeroUsize;
use sysinfo::{ProcessStatus, System, get_current_pid};
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
    pub pid: String,
    pub hostname: String,
    pub host_ip: String,
    pub host_mac: String,
    pub proc_threads: usize,
    pub proc_cpu: f32,
    pub proc_mem_bytes: u64,
    pub host_cpu: f32,
    pub host_mem_used_bytes: u64,
    pub host_mem_total_bytes: u64,
    pub host_uptime_secs: u64,
    pub proc_uptime_secs: u64,
    pub host_load_average: [f64; 3],
    pub host_os_info: String,
    pub host_connections: usize,
    pub host_processes: usize,
}

impl Stats {
    pub fn gather() -> Option<Self> {
        let mut sys = System::new_all();
        sys.refresh_all();

        let pid = get_current_pid().ok()?;
        let proc = sys.process(pid)?;
        let proc_threads = thread_count().map(NonZeroUsize::get).unwrap_or(0);

        let hostname = std::env::var("HOSTNAME")
            .ok()
            .or_else(System::host_name)
            .unwrap_or_else(|| "unknown".into());

        let host_ip = resolve_host_ip();
        let host_os_info = os_info::get();
        let host_load_average = Self::loadavg();
        let host_connections = Self::connections().unwrap_or(0);

        // let host_processes = sys.processes().len();
        let host_processes = sys
            .processes()
            .values()
            .filter(|p| p.status() == ProcessStatus::Run)
            .count();

        let host_mac = Self::mac();

        Some(Self {
            pid: pid.to_string(),
            hostname,
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
        match get_mac_address() {
            Ok(mac) => match mac {
                None => String::from("unknown"),
                Some(addr) => addr.to_string(),
            },
            Err(_) => String::from("unknown"),
        }
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
        let mut table = Table::new();

        table
            .set_header(vec!["Stat name", "value"])
            // process
            .add_row(vec!["Proc pid", &self.pid])
            .add_row(vec!["Proc threads", &self.proc_threads.to_string()])
            .add_row(vec!["Proc CPU", format!("{:.1}%", self.proc_cpu).as_str()])
            .add_row(vec!["Proc RAM", format_mem(self.proc_mem_bytes).as_str()])
            .add_row(vec![
                "Proc uptime",
                format_duration(self.proc_uptime_secs).as_str(),
            ])
            // host
            .add_row(vec!["Host OS", &self.host_os_info])
            .add_row(vec!["Host name", &self.hostname])
            .add_row(vec![
                "Host CPU",
                format!("{:.1}%", self.host_cpu.to_string()).as_str(),
            ])
            .add_row(vec![
                "Host RAM",
                format!(
                    "{} / {}",
                    format_mem(self.host_mem_used_bytes),
                    format_mem(self.host_mem_total_bytes)
                )
                .as_str(),
            ])
            .add_row(vec!["Host connections", &self.host_connections.to_string()])
            .add_row(vec![
                "Host running processes",
                &self.host_processes.to_string(),
            ])
            .add_row(vec![
                "Host load",
                format!(
                    "{} / {} / {}",
                    &self.host_load_average[0],
                    &self.host_load_average[1],
                    &self.host_load_average[2],
                )
                .as_str(),
            ])
            .add_row(vec!["Host IP", &self.host_ip])
            .add_row(vec!["Host MAC", &self.host_mac])
            .add_row(vec![
                "Host uptime",
                &*format_duration(self.host_uptime_secs),
            ]);

        table.to_string()
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
