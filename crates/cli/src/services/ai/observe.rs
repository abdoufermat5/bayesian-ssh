//! observe(host, what): fixed commands parsed into structured JSON.
//!
//! The agent never writes shell for these: `command()` returns a fixed string per fact and
//! `parse()` turns its output into JSON. Parsers are pure (no I/O) so they are tested on
//! fixtures. Remote text is untrusted: the caller redacts and labels it.
//!
//!

use anyhow::{anyhow, bail, Result};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

/// Entries returned at most for list-like facts, so one call can't flood the model.
pub const MAX_ENTRIES: usize = 500;

/// Facts the agent can ask for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum What {
    Services,
    FailedUnits,
    Disk,
    Memory,
    Ports,
    Processes,
    Containers,
    Packages,
}

impl What {
    pub const ALL: [What; 8] = [
        What::Services,
        What::FailedUnits,
        What::Disk,
        What::Memory,
        What::Ports,
        What::Processes,
        What::Containers,
        What::Packages,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            What::Services => "services",
            What::FailedUnits => "failed_units",
            What::Disk => "disk",
            What::Memory => "memory",
            What::Ports => "ports",
            What::Processes => "processes",
            What::Containers => "containers",
            What::Packages => "packages",
        }
    }
}

/// The fixed command for a fact. Read-only; `LC_ALL=C` keeps the output parseable.
pub fn command(what: What) -> &'static str {
    match what {
        What::Services => {
            "LC_ALL=C systemctl list-units --type=service --all --no-pager --output=json 2>/dev/null \
             || LC_ALL=C systemctl list-units --type=service --all --no-pager --plain --no-legend"
        }
        What::FailedUnits => {
            "LC_ALL=C systemctl list-units --state=failed --all --no-pager --output=json 2>/dev/null \
             || LC_ALL=C systemctl list-units --state=failed --all --no-pager --plain --no-legend"
        }
        What::Disk => "LC_ALL=C df -P -k",
        What::Memory => "LC_ALL=C free -b",
        What::Ports => "LC_ALL=C ss -Htlnp",
        What::Processes => {
            "LC_ALL=C ps -eo pid=,ppid=,user:32=,pcpu=,pmem=,etime=,comm=,args= --sort=-pcpu | head -n 50"
        }
        What::Containers => {
            "docker ps -a --format '{{json .}}' 2>/dev/null || podman ps -a --format '{{json .}}'"
        }
        What::Packages => {
            "LC_ALL=C dpkg-query -W -f='${Package}\\t${Version}\\n' 2>/dev/null \
             || LC_ALL=C rpm -qa --qf '%{NAME}\\t%{VERSION}-%{RELEASE}\\n'"
        }
    }
}

/// Parse the stdout of `command(what)`.
pub fn parse(what: What, stdout: &str) -> Result<Value> {
    match what {
        What::Services | What::FailedUnits => parse_units(stdout),
        What::Disk => parse_disk(stdout),
        What::Memory => parse_memory(stdout),
        What::Ports => parse_ports(stdout),
        What::Processes => parse_processes(stdout),
        What::Containers => parse_containers(stdout),
        What::Packages => parse_packages(stdout),
    }
}

fn capped(mut items: Vec<Value>) -> (Vec<Value>, bool) {
    let truncated = items.len() > MAX_ENTRIES;
    items.truncate(MAX_ENTRIES);
    (items, truncated)
}

fn num(s: &str) -> Option<u64> {
    s.parse().ok()
}

/// `systemctl list-units --output=json` or the plain text fallback.
pub fn parse_units(stdout: &str) -> Result<Value> {
    let trimmed = stdout.trim();
    let mut units = Vec::new();
    if trimmed.starts_with('[') {
        let rows: Vec<Value> =
            serde_json::from_str(trimmed).map_err(|e| anyhow!("systemctl json: {e}"))?;
        for r in rows {
            let s = |k: &str| r.get(k).and_then(Value::as_str).unwrap_or("").to_string();
            units.push(json!({
                "unit": s("unit"), "load": s("load"), "active": s("active"),
                "sub": s("sub"), "description": s("description"),
            }));
        }
    } else {
        for line in trimmed.lines() {
            // UNIT LOAD ACTIVE SUB DESCRIPTION...; a leading bullet marks failed units.
            let line = line
                .trim_start()
                .trim_start_matches(['●', '*', '×'])
                .trim_start();
            let mut it = line.split_whitespace();
            let (Some(unit), Some(load), Some(active), Some(sub)) =
                (it.next(), it.next(), it.next(), it.next())
            else {
                continue;
            };
            let description = it.collect::<Vec<_>>().join(" ");
            units.push(json!({
                "unit": unit, "load": load, "active": active, "sub": sub, "description": description,
            }));
        }
    }
    let total = units.len();
    let (units, truncated) = capped(units);
    Ok(json!({ "units": units, "total": total, "truncated": truncated }))
}

/// `df -P -k`.
pub fn parse_disk(stdout: &str) -> Result<Value> {
    let mut filesystems = Vec::new();
    for line in stdout.lines().skip(1) {
        let f: Vec<&str> = line.split_whitespace().collect();
        if f.len() < 6 {
            continue;
        }
        let (Some(total), Some(used), Some(avail)) = (num(f[1]), num(f[2]), num(f[3])) else {
            continue;
        };
        filesystems.push(json!({
            "filesystem": f[0],
            "total_bytes": total * 1024,
            "used_bytes": used * 1024,
            "available_bytes": avail * 1024,
            "use_percent": f[4].trim_end_matches('%').parse::<u64>().ok(),
            "mount": f[5..].join(" "),
        }));
    }
    if filesystems.is_empty() {
        bail!("no filesystems in df output");
    }
    Ok(json!({ "filesystems": filesystems }))
}

/// `free -b`.
pub fn parse_memory(stdout: &str) -> Result<Value> {
    let mut mem = None;
    let mut swap = None;
    for line in stdout.lines() {
        let f: Vec<&str> = line.split_whitespace().collect();
        match f.first().copied() {
            Some("Mem:") if f.len() >= 4 => {
                mem = Some(json!({
                    "total_bytes": num(f[1]), "used_bytes": num(f[2]), "free_bytes": num(f[3]),
                    "shared_bytes": f.get(4).and_then(|s| num(s)),
                    "buff_cache_bytes": f.get(5).and_then(|s| num(s)),
                    "available_bytes": f.get(6).and_then(|s| num(s)),
                }));
            }
            Some("Swap:") if f.len() >= 4 => {
                swap = Some(json!({
                    "total_bytes": num(f[1]), "used_bytes": num(f[2]), "free_bytes": num(f[3]),
                }));
            }
            _ => {}
        }
    }
    let mem = mem.ok_or_else(|| anyhow!("no Mem: line in free output"))?;
    Ok(json!({ "memory": mem, "swap": swap }))
}

/// `ss -Htlnp`: State Recv-Q Send-Q Local Peer [Process].
pub fn parse_ports(stdout: &str) -> Result<Value> {
    let mut listeners = Vec::new();
    for line in stdout.lines() {
        let f: Vec<&str> = line.split_whitespace().collect();
        if f.len() < 5 {
            continue;
        }
        let local = f[3];
        let (addr, port) = local.rsplit_once(':').unwrap_or((local, ""));
        let rest = f[5..].join(" ");
        listeners.push(json!({
            "state": f[0],
            "address": addr.trim_start_matches('[').trim_end_matches(']'),
            "port": port.parse::<u16>().ok(),
            "processes": parse_ss_users(&rest),
        }));
    }
    let (listeners, truncated) = capped(listeners);
    Ok(json!({ "listeners": listeners, "truncated": truncated }))
}

/// `users:(("nginx",pid=812,fd=6),("nginx",pid=813,fd=6))` -> [{name, pid}].
fn parse_ss_users(s: &str) -> Vec<Value> {
    let mut out = Vec::new();
    let Some(start) = s.find("users:(") else {
        return out;
    };
    for part in s[start..].split("((").skip(1).flat_map(|p| p.split("),(")) {
        let name = part.split('"').nth(1).unwrap_or("");
        let pid = part
            .split("pid=")
            .nth(1)
            .and_then(|p| p.split([',', ')']).next())
            .and_then(|p| p.parse::<u32>().ok());
        if !name.is_empty() {
            out.push(json!({ "name": name, "pid": pid }));
        }
    }
    out
}

/// `ps -eo pid=,ppid=,user=,pcpu=,pmem=,etime=,comm=,args=`.
pub fn parse_processes(stdout: &str) -> Result<Value> {
    let mut procs = Vec::new();
    for line in stdout.lines() {
        let mut rest = line.trim_start();
        let mut head = Vec::with_capacity(7);
        for _ in 0..7 {
            let end = rest.find(char::is_whitespace).unwrap_or(rest.len());
            if end == 0 {
                break;
            }
            head.push(&rest[..end]);
            rest = rest[end..].trim_start();
        }
        if head.len() < 7 {
            continue;
        }
        let Some(pid) = head[0].parse::<u32>().ok() else {
            continue;
        };
        procs.push(json!({
            "pid": pid,
            "ppid": head[1].parse::<u32>().ok(),
            "user": head[2],
            "cpu_percent": head[3].parse::<f64>().ok(),
            "mem_percent": head[4].parse::<f64>().ok(),
            "elapsed": head[5],
            "command": head[6],
            "args": rest,
        }));
    }
    let (procs, truncated) = capped(procs);
    Ok(json!({ "processes": procs, "truncated": truncated }))
}

/// `docker ps --format '{{json .}}'`: one JSON object per line.
pub fn parse_containers(stdout: &str) -> Result<Value> {
    let mut containers = Vec::new();
    for line in stdout.lines().map(str::trim).filter(|l| !l.is_empty()) {
        let Ok(v) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        let s = |k: &str| v.get(k).and_then(Value::as_str).unwrap_or("").to_string();
        containers.push(json!({
            "id": s("ID"), "names": s("Names"), "image": s("Image"),
            "state": s("State"), "status": s("Status"), "ports": s("Ports"),
        }));
    }
    let (containers, truncated) = capped(containers);
    Ok(json!({ "containers": containers, "truncated": truncated }))
}

/// `dpkg-query -W -f='${Package}\t${Version}\n'` or the rpm equivalent.
pub fn parse_packages(stdout: &str) -> Result<Value> {
    let mut packages = Vec::new();
    for line in stdout.lines() {
        let Some((name, version)) = line.split_once('\t') else {
            continue;
        };
        if name.is_empty() {
            continue;
        }
        packages.push(json!({ "name": name, "version": version.trim() }));
    }
    let total = packages.len();
    let (packages, truncated) = capped(packages);
    Ok(json!({ "packages": packages, "total": total, "truncated": truncated }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn units_json() {
        let out = r#"[{"unit":"nginx.service","load":"loaded","active":"active","sub":"running","description":"A web server"},
                      {"unit":"cron.service","load":"loaded","active":"failed","sub":"failed","description":"Cron"}]"#;
        let v = parse_units(out).unwrap();
        assert_eq!(v["total"], 2);
        assert_eq!(v["units"][0]["unit"], "nginx.service");
        assert_eq!(v["units"][1]["active"], "failed");
    }

    #[test]
    fn units_plain_with_failed_bullet() {
        let out = "  nginx.service loaded active running A high performance web server\n\
                   ● foo.service  loaded failed failed  Foo daemon\n";
        let v = parse_units(out).unwrap();
        assert_eq!(v["total"], 2);
        assert_eq!(
            v["units"][0]["description"],
            "A high performance web server"
        );
        assert_eq!(v["units"][1]["unit"], "foo.service");
        assert_eq!(v["units"][1]["sub"], "failed");
    }

    #[test]
    fn units_empty_is_ok() {
        let v = parse_units("").unwrap();
        assert_eq!(v["total"], 0);
    }

    #[test]
    fn disk_df() {
        let out = "Filesystem     1024-blocks      Used Available Capacity Mounted on\n\
                   /dev/sda1         10190136   4096000   5553980      43% /\n\
                   tmpfs               816228         0    816228       0% /run/user 1000\n";
        let v = parse_disk(out).unwrap();
        let fs = v["filesystems"].as_array().unwrap();
        assert_eq!(fs.len(), 2);
        assert_eq!(fs[0]["total_bytes"], 10190136u64 * 1024);
        assert_eq!(fs[0]["use_percent"], 43);
        assert_eq!(fs[1]["mount"], "/run/user 1000");
    }

    #[test]
    fn disk_garbage_errors() {
        assert!(parse_disk("df: command not found").is_err());
    }

    #[test]
    fn memory_free() {
        let out = "               total        used        free      shared  buff/cache   available\n\
                   Mem:      8000000000  2000000000  3000000000   100000000  3000000000  5500000000\n\
                   Swap:     2000000000           0  2000000000\n";
        let v = parse_memory(out).unwrap();
        assert_eq!(v["memory"]["total_bytes"], 8_000_000_000u64);
        assert_eq!(v["memory"]["available_bytes"], 5_500_000_000u64);
        assert_eq!(v["swap"]["free_bytes"], 2_000_000_000u64);
        assert!(parse_memory("nope").is_err());
    }

    #[test]
    fn ports_ss() {
        let out = "LISTEN 0 511 0.0.0.0:80 0.0.0.0:* users:((\"nginx\",pid=812,fd=6),(\"nginx\",pid=813,fd=6))\n\
                   LISTEN 0 128 [::]:22 [::]:*\n\
                   LISTEN 0 4096 127.0.0.53%lo:53 0.0.0.0:*\n";
        let v = parse_ports(out).unwrap();
        let l = v["listeners"].as_array().unwrap();
        assert_eq!(l.len(), 3);
        assert_eq!(l[0]["port"], 80);
        assert_eq!(l[0]["processes"][0]["name"], "nginx");
        assert_eq!(l[0]["processes"][1]["pid"], 813);
        assert_eq!(l[1]["address"], "::");
        assert_eq!(l[1]["processes"].as_array().unwrap().len(), 0);
        assert_eq!(l[2]["port"], 53);
    }

    #[test]
    fn processes_ps() {
        let out = "  812     1 root       2.5  1.2    01:02:03 nginx           nginx: master process /usr/sbin/nginx\n\
                   9001   812 www-data   0.1  0.3       10:00 nginx           nginx: worker process\n";
        let v = parse_processes(out).unwrap();
        let p = v["processes"].as_array().unwrap();
        assert_eq!(p.len(), 2);
        assert_eq!(p[0]["pid"], 812);
        assert_eq!(p[0]["cpu_percent"], 2.5);
        assert_eq!(p[0]["command"], "nginx");
        assert_eq!(p[0]["args"], "nginx: master process /usr/sbin/nginx");
        assert_eq!(p[1]["user"], "www-data");
    }

    #[test]
    fn containers_json_lines() {
        let out = "{\"ID\":\"abc123\",\"Names\":\"web\",\"Image\":\"nginx:1.27\",\"State\":\"running\",\"Status\":\"Up 2 hours\",\"Ports\":\"0.0.0.0:80->80/tcp\"}\n\
                   not json\n\
                   {\"ID\":\"def456\",\"Names\":\"db\",\"Image\":\"postgres:16\",\"State\":\"exited\",\"Status\":\"Exited (1)\",\"Ports\":\"\"}\n";
        let v = parse_containers(out).unwrap();
        let c = v["containers"].as_array().unwrap();
        assert_eq!(c.len(), 2);
        assert_eq!(c[0]["image"], "nginx:1.27");
        assert_eq!(c[1]["state"], "exited");
    }

    #[test]
    fn packages_dpkg_and_cap() {
        let out = "bash\t5.2.21-2\ncoreutils\t9.4-3\n";
        let v = parse_packages(out).unwrap();
        assert_eq!(v["total"], 2);
        assert_eq!(v["packages"][1]["version"], "9.4-3");

        let big: String = (0..MAX_ENTRIES + 10)
            .map(|i| format!("pkg{i}\t1.0\n"))
            .collect();
        let v = parse_packages(&big).unwrap();
        assert_eq!(v["packages"].as_array().unwrap().len(), MAX_ENTRIES);
        assert_eq!(v["truncated"], true);
        assert_eq!(v["total"], MAX_ENTRIES + 10);
    }

    #[test]
    fn every_fact_has_a_command_and_a_wire_name() {
        for w in What::ALL {
            assert!(!command(w).is_empty());
            assert_eq!(
                serde_json::to_value(w).unwrap(),
                Value::String(w.as_str().into())
            );
        }
        assert!(command(What::Disk).starts_with("LC_ALL=C"));
    }
}
