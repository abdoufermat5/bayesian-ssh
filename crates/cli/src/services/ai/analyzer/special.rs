//! Programs whose risk needs more than the rule table: they look inside scripts
//! (`sed`, `awk`), walk expressions (`find`), or have option grammars of their own.

use crate::services::ai::types::RiskTag;

use super::rules::{Arg, Class};

pub type SpecialFn = fn(&str, &[Arg]) -> Class;

pub fn lookup(prog: &str) -> Option<SpecialFn> {
    Some(match prog {
        "sed" => sed,
        "awk" | "gawk" | "mawk" | "nawk" => awk,
        "find" => find,
        "curl" => curl,
        "wget" => wget,
        "rsync" => rsync,
        "tar" | "bsdtar" => tar,
        "date" => date,
        "tee" => tee,
        "top" => top,
        "service" => service,
        "ifconfig" => ifconfig,
        "kill" => kill,
        "rm" | "rmdir" | "unlink" | "shred" | "srm" | "dd" | "wipefs" | "lvremove" | "vgremove"
        | "pvremove" | "lvreduce" | "vgreduce" | "pkill" | "killall" | "skill" | "reboot"
        | "shutdown" | "halt" | "poweroff" | "init" | "telinit" | "resize2fs" | "lvresize"
        | "tune2fs" | "cryptsetup" => destructive,
        "cp" | "mv" | "touch" | "mkdir" | "mkfifo" | "mknod" | "install" | "ln" | "chmod"
        | "chown" | "chgrp" | "truncate" | "rename" | "patch" | "mktemp" | "split" | "csplit"
        | "fallocate" | "logger" | "lvcreate" | "vgcreate" | "pvcreate" | "lvextend"
        | "vgextend" | "xfs_growfs" | "losetup" | "e2label" | "sponge" | "ssh-copy-id" | "scp"
        | "sftp" => write,
        "ping" | "ping6" | "traceroute" | "traceroute6" | "tracepath" | "mtr" | "dig"
        | "nslookup" | "host" | "whois" | "nmap" | "iperf" | "iperf3" | "fping" | "hping3"
        | "arping" | "ssh-keyscan" => network,
        "nc" | "ncat" | "netcat" | "socat" | "telnet" | "ftp" | "lftp" | "tftp" | "ssh" => exfil,
        _ => return None,
    })
}

fn is_prefix_of(name: &str, full: &str) -> bool {
    name.len() >= 3 && full.starts_with(name)
}

fn positional_count(args: &[Arg]) -> usize {
    args.iter().filter(|a| !a.text.starts_with('-')).count()
}

// ---------------------------------------------------------------- simple tag sets

fn destructive(prog: &str, _: &[Arg]) -> Class {
    Class::new(prog, &[RiskTag::Destructive])
}

fn write(prog: &str, _: &[Arg]) -> Class {
    match prog {
        "scp" | "sftp" | "ssh-copy-id" => Class::new(
            prog,
            &[RiskTag::Write, RiskTag::Network, RiskTag::ExfilRisk],
        ),
        _ => Class::new(prog, &[RiskTag::Write]),
    }
}

fn network(prog: &str, _: &[Arg]) -> Class {
    Class::new(prog, &[RiskTag::Network])
}

fn exfil(prog: &str, _: &[Arg]) -> Class {
    Class::new(prog, &[RiskTag::Network, RiskTag::ExfilRisk])
}

// ---------------------------------------------------------------- small ones

fn kill(prog: &str, args: &[Arg]) -> Class {
    if args.iter().any(|a| {
        matches!(
            a.text.as_str(),
            "-l" | "-L" | "--list" | "--table" | "-0" | "-s0"
        )
    }) {
        return Class::new("kill -l/-0", &[RiskTag::Read]);
    }
    Class::new(prog, &[RiskTag::Destructive])
}

fn tee(prog: &str, args: &[Arg]) -> Class {
    let files: Vec<&Arg> = args
        .iter()
        .filter(|a| !a.text.starts_with('-') && a.text != "/dev/null")
        .collect();
    if files.is_empty() {
        Class::new("tee (stdout only)", &[RiskTag::Read])
    } else {
        Class::new(prog, &[RiskTag::Write])
    }
}

fn top(prog: &str, args: &[Arg]) -> Class {
    if args
        .iter()
        .any(|a| a.text.starts_with('-') && !a.text.starts_with("--") && a.text.contains('b'))
    {
        Class::new("top -b", &[RiskTag::Read])
    } else {
        Class::unknown(prog, "top without -b is interactive")
    }
}

fn service(prog: &str, args: &[Arg]) -> Class {
    if args.len() == 1 && args[0].text == "--status-all" {
        return Class::new("service --status-all", &[RiskTag::Read]);
    }
    if args.iter().any(|a| a.text.starts_with('-')) || args.len() != 2 {
        return Class::unknown(prog, "service: unrecognized arguments");
    }
    match args[1].text.as_str() {
        "status" => Class::new("service status", &[RiskTag::Read]),
        "start" | "stop" | "restart" | "reload" | "force-reload" | "try-restart"
        | "condrestart" | "reload-or-restart" => {
            Class::new("service change", &[RiskTag::ServiceChange])
        }
        other => Class::unknown(prog, format!("service: custom action '{other}'")),
    }
}

fn ifconfig(prog: &str, args: &[Arg]) -> Class {
    let flags_ok = args
        .iter()
        .filter(|a| a.text.starts_with('-'))
        .all(|a| matches!(a.text.as_str(), "-a" | "-s" | "-v"));
    if flags_ok && positional_count(args) <= 1 {
        Class::new("ifconfig (show)", &[RiskTag::Read])
    } else {
        Class::new(prog, &[RiskTag::ServiceChange])
    }
}

fn date(prog: &str, args: &[Arg]) -> Class {
    let mut i = 0;
    while i < args.len() {
        let t = args[i].text.as_str();
        if matches!(t, "-d" | "-r" | "-f" | "--date" | "--reference" | "--file") {
            i += 2;
            continue;
        }
        if t == "-s"
            || t.starts_with("--set")
            || (t.starts_with('-') && !t.starts_with("--") && t[1..].contains('s'))
        {
            return Class::new("date (set)", &[RiskTag::ServiceChange]);
        }
        if !t.starts_with('-') && !t.starts_with('+') {
            return Class::new("date (set)", &[RiskTag::ServiceChange]);
        }
        i += 1;
    }
    Class::new(prog, &[RiskTag::Read])
}

// ---------------------------------------------------------------- find

/// Predicates that take one value; a dynamic value there can't be an option.
const FIND_VALUE_PREDICATES: &[&str] = &[
    "-name",
    "-iname",
    "-path",
    "-ipath",
    "-wholename",
    "-iwholename",
    "-regex",
    "-iregex",
    "-lname",
    "-ilname",
    "-type",
    "-xtype",
    "-user",
    "-group",
    "-uid",
    "-gid",
    "-perm",
    "-size",
    "-mtime",
    "-atime",
    "-ctime",
    "-mmin",
    "-amin",
    "-cmin",
    "-newer",
    "-anewer",
    "-cnewer",
    "-maxdepth",
    "-mindepth",
    "-samefile",
    "-inum",
    "-links",
    "-fstype",
    "-context",
    "-printf",
    "-used",
    "-regextype",
    "-newerat",
    "-newerbt",
    "-newerct",
    "-newermt",
    "-newerac",
    "-newerma",
    "-newermm",
    "-newermc",
    "-newerm",
    "-neweraa",
    "-newerab",
];

const FIND_EXEC: &[&str] = &["-exec", "-execdir", "-ok", "-okdir"];

fn find(prog: &str, args: &[Arg]) -> Class {
    let mut tags = vec![RiskTag::Read];
    let mut rule = "find";
    let mut in_paths = true;
    let mut i = 0;
    while i < args.len() {
        let a = &args[i];
        let t = a.text.as_str();
        if in_paths && (t.starts_with('-') || t == "(" || t == "!") {
            in_paths = false;
        }
        if in_paths {
            i += 1;
            continue;
        }
        if FIND_EXEC.contains(&t) {
            // The command is analyzed on its own; skip to the terminator.
            while i < args.len() && args[i].text != ";" && args[i].text != "+" {
                i += 1;
            }
            i += 1;
            continue;
        }
        match t {
            "-delete" => {
                tags.push(RiskTag::Destructive);
                rule = "find -delete";
            }
            "-fprint" | "-fprint0" | "-fprintf" | "-fls" => {
                tags.push(RiskTag::Write);
                rule = "find -fprint";
                i += 2;
                continue;
            }
            _ => {}
        }
        if FIND_VALUE_PREDICATES.contains(&t) {
            i += 2;
            continue;
        }
        if a.flaggy {
            return Class::unknown(
                prog,
                "find: an argument is only known at run time and could be -delete or -exec",
            );
        }
        i += 1;
    }
    Class::new(rule, &tags)
}

/// Commands run by `find -exec/-execdir/-ok/-okdir`, with `{}` as a placeholder.
pub fn find_exec_commands(args: &[Arg]) -> Result<Vec<(String, Vec<Arg>)>, String> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < args.len() {
        let t = args[i].text.as_str();
        if FIND_EXEC.contains(&t) {
            let opt = t.to_string();
            let mut inner = Vec::new();
            i += 1;
            let mut closed = false;
            while i < args.len() {
                let w = &args[i];
                if w.text == ";" || w.text == "+" {
                    closed = true;
                    break;
                }
                // find substitutes whole paths (`./name`), so `{}` can't turn into an option.
                if w.text == "{}" {
                    inner.push(Arg {
                        text: w.text.clone(),
                        dynamic: true,
                        flaggy: false,
                        glob: false,
                    });
                } else {
                    inner.push(w.clone());
                }
                i += 1;
            }
            if !closed {
                return Err(format!("find {opt} without a terminating ';' or '+'"));
            }
            if inner.is_empty() {
                return Err(format!("find {opt} without a command"));
            }
            out.push((opt, inner));
        }
        i += 1;
    }
    Ok(out)
}

// ---------------------------------------------------------------- sed

fn sed(prog: &str, args: &[Arg]) -> Class {
    let mut scripts: Vec<&Arg> = Vec::new();
    let mut script_flag = false;
    let mut after_dd = false;
    let mut i = 0;
    while i < args.len() {
        let a = &args[i];
        let t = a.text.as_str();
        i += 1;
        if after_dd || !t.starts_with('-') || t == "-" {
            if !script_flag && scripts.is_empty() {
                scripts.push(a);
            } else if a.flaggy && !after_dd {
                return Class::unknown(
                    prog,
                    "sed: an argument is only known at run time and could be -i",
                );
            }
            continue;
        }
        if t == "--" {
            after_dd = true;
            continue;
        }
        if let Some(long) = t.strip_prefix("--") {
            let (name, value) = match long.split_once('=') {
                Some((n, v)) => (n, Some(v)),
                None => (long, None),
            };
            let full = format!("--{name}");
            if is_prefix_of(&full, "--in-place") {
                return Class::new("sed -i", &[RiskTag::Write]);
            }
            if is_prefix_of(&full, "--expression") {
                script_flag = true;
                match value {
                    Some(_) => scripts.push(a),
                    None => {
                        if let Some(next) = args.get(i) {
                            scripts.push(next);
                            i += 1;
                        } else {
                            return Class::unknown(prog, "sed: missing script");
                        }
                    }
                }
                continue;
            }
            if matches!(
                full.as_str(),
                "--quiet"
                    | "--silent"
                    | "--regexp-extended"
                    | "--posix"
                    | "--debug"
                    | "--sandbox"
                    | "--follow-symlinks"
                    | "--null-data"
                    | "--separate"
                    | "--unbuffered"
                    | "--binary"
                    | "--version"
                    | "--help"
                    | "--zero-terminated"
            ) {
                continue;
            }
            return Class::unknown(prog, format!("sed: option {full} not analyzed"));
        }
        // short cluster
        let chars: Vec<char> = t[1..].chars().collect();
        for (k, ch) in chars.iter().enumerate() {
            match ch {
                'i' => return Class::new("sed -i", &[RiskTag::Write]),
                'e' => {
                    script_flag = true;
                    if k + 1 < chars.len() {
                        scripts.push(a);
                    } else if let Some(next) = args.get(i) {
                        scripts.push(next);
                        i += 1;
                    } else {
                        return Class::unknown(prog, "sed: missing script");
                    }
                    break;
                }
                'n' | 's' | 'E' | 'r' | 'u' | 'z' | 'b' => {}
                _ => return Class::unknown(prog, format!("sed: option -{ch} not analyzed")),
            }
        }
    }
    if scripts.is_empty() {
        return Class::unknown(prog, "sed: no script found");
    }
    for s in scripts {
        if s.dynamic {
            return Class::unknown(prog, "sed: script is only known at run time");
        }
        // `-e` attached to a cluster (`-es/a/b/`): the script is the rest of the cluster.
        let text = if s.text.starts_with('-') && !s.text.starts_with("--") {
            match s.text[1..].find('e') {
                Some(p) => &s.text[p + 2..],
                None => s.text.as_str(),
            }
        } else if let Some(v) = s.text.strip_prefix("--expression=") {
            v
        } else {
            s.text.as_str()
        };
        if let Some(why) = sed_script_risk(text) {
            return Class::unknown(prog, format!("sed script {why}"));
        }
    }
    Class::new("sed (no -i)", &[RiskTag::Read])
}

/// Walks a sed script and reports commands that run programs or write files.
fn sed_script_risk(script: &str) -> Option<&'static str> {
    let c: Vec<char> = script.chars().collect();
    let n = c.len();
    let mut i = 0;
    let skip_to_eol = |i: &mut usize| {
        while *i < n && c[*i] != '\n' {
            *i += 1;
        }
    };
    let skip_delimited = |i: &mut usize, delim: char| -> bool {
        while *i < n {
            if c[*i] == '\\' {
                *i += 2;
                continue;
            }
            if c[*i] == delim {
                *i += 1;
                return true;
            }
            *i += 1;
        }
        false
    };
    while i < n {
        while i < n && (c[i].is_whitespace() || c[i] == ';') {
            i += 1;
        }
        if i >= n {
            break;
        }
        // addresses
        loop {
            if i < n && c[i].is_ascii_digit() {
                while i < n && c[i].is_ascii_digit() {
                    i += 1;
                }
                if i < n && c[i] == '~' {
                    i += 1;
                    while i < n && c[i].is_ascii_digit() {
                        i += 1;
                    }
                }
            } else if i < n && c[i] == '$' {
                i += 1;
            } else if i < n && (c[i] == '/' || c[i] == '\\') {
                let delim = if c[i] == '\\' {
                    i += 1;
                    *c.get(i)?
                } else {
                    '/'
                };
                i += 1;
                if !skip_delimited(&mut i, delim) {
                    return Some("has an unterminated address");
                }
                while i < n && matches!(c[i], 'I' | 'M') {
                    i += 1;
                }
            } else if i < n && matches!(c[i], '+' | '~') {
                i += 1;
                while i < n && c[i].is_ascii_digit() {
                    i += 1;
                }
            } else {
                break;
            }
            while i < n && c[i] == ' ' {
                i += 1;
            }
            if i < n && c[i] == ',' {
                i += 1;
                while i < n && c[i] == ' ' {
                    i += 1;
                }
                continue;
            }
            break;
        }
        while i < n && (c[i] == ' ' || c[i] == '!') {
            i += 1;
        }
        if i >= n {
            break;
        }
        let cmd = c[i];
        i += 1;
        match cmd {
            '{' | '}' | 'p' | 'd' | 'D' | 'g' | 'G' | 'h' | 'H' | 'n' | 'N' | 'x' | '=' | 'z'
            | 'P' | 'F' => {}
            's' => {
                let delim = *c.get(i)?;
                i += 1;
                if !skip_delimited(&mut i, delim) || !skip_delimited(&mut i, delim) {
                    return Some("has an unterminated s command");
                }
                while i < n && !matches!(c[i], ';' | '\n' | '}' | '#' | ' ') {
                    if matches!(c[i], 'e' | 'w') {
                        return Some("runs a command or writes a file (s///e or s///w)");
                    }
                    i += 1;
                }
            }
            'y' => {
                let delim = *c.get(i)?;
                i += 1;
                if !skip_delimited(&mut i, delim) || !skip_delimited(&mut i, delim) {
                    return Some("has an unterminated y command");
                }
            }
            'e' => return Some("runs a command (e)"),
            'w' | 'W' => return Some("writes a file (w)"),
            'r' | 'R' => skip_to_eol(&mut i),
            'a' | 'i' | 'c' => loop {
                skip_to_eol(&mut i);
                if i > 0 && c.get(i.wrapping_sub(1)) == Some(&'\\') && i < n {
                    i += 1;
                } else {
                    break;
                }
            },
            ':' | 'b' | 't' | 'T' => {
                while i < n && !matches!(c[i], ';' | '\n' | '}') {
                    i += 1;
                }
            }
            'q' | 'Q' | 'l' | 'L' => {
                while i < n && c[i].is_ascii_digit() {
                    i += 1;
                }
            }
            '#' => skip_to_eol(&mut i),
            _ => return Some("uses a command that isn't analyzed"),
        }
    }
    None
}

// ---------------------------------------------------------------- awk

fn awk(prog: &str, args: &[Arg]) -> Class {
    let mut program: Option<&Arg> = None;
    let mut i = 0;
    let mut after_dd = false;
    while i < args.len() {
        let a = &args[i];
        let t = a.text.as_str();
        i += 1;
        if after_dd || !t.starts_with('-') || t == "-" {
            if program.is_none() {
                program = Some(a);
            } else if a.flaggy && !after_dd {
                return Class::unknown(prog, "awk: an argument is only known at run time");
            }
            continue;
        }
        if t == "--" {
            after_dd = true;
            continue;
        }
        if t.starts_with("--") {
            let name = t.split('=').next().unwrap_or(t);
            if matches!(
                name,
                "--version"
                    | "--help"
                    | "--posix"
                    | "--traditional"
                    | "--re-interval"
                    | "--characters-as-bytes"
                    | "--use-lc-numeric"
                    | "--lint"
                    | "--csv"
            ) {
                continue;
            }
            if is_prefix_of(name, "--field-separator") || is_prefix_of(name, "--assign") {
                if !t.contains('=') {
                    i += 1;
                }
                continue;
            }
            return Class::unknown(prog, format!("awk: option {name} not analyzed"));
        }
        for (k, ch) in t[1..].chars().enumerate() {
            match ch {
                'F' | 'v' => {
                    if k + 1 >= t.len() - 1 {
                        i += 1;
                    }
                    break;
                }
                'b' | 'c' | 'n' | 'N' | 'P' | 'r' | 's' | 'S' | 't' | 'u' | 'V' | 'h' => {}
                _ => return Class::unknown(prog, format!("awk: option -{ch} not analyzed")),
            }
        }
    }
    let Some(p) = program else {
        return Class::unknown(prog, "awk: no program found");
    };
    if p.dynamic {
        return Class::unknown(prog, "awk: program is only known at run time");
    }
    if let Some(why) = awk_program_risk(&p.text) {
        return Class::unknown(prog, format!("awk program {why}"));
    }
    Class::new("awk (no system/pipe/redirect)", &[RiskTag::Read])
}

/// Replaces strings, regex literals and comments so only code remains.
fn awk_code_only(src: &str) -> String {
    let c: Vec<char> = src.chars().collect();
    let mut out = String::with_capacity(src.len());
    let mut i = 0;
    while i < c.len() {
        match c[i] {
            '"' => {
                i += 1;
                while i < c.len() && c[i] != '"' {
                    if c[i] == '\\' {
                        i += 1;
                    }
                    i += 1;
                }
                i += 1;
                out.push_str("\"\"");
            }
            '#' => {
                while i < c.len() && c[i] != '\n' {
                    i += 1;
                }
            }
            '/' => {
                let prev = out.chars().rev().find(|ch| !ch.is_whitespace());
                let regex = match prev {
                    None => true,
                    Some(p) => "(,;{}!~&|=<>?:+-*%^".contains(p),
                };
                if regex {
                    i += 1;
                    while i < c.len() && c[i] != '/' && c[i] != '\n' {
                        if c[i] == '\\' {
                            i += 1;
                        }
                        i += 1;
                    }
                    i += 1;
                    out.push_str("//");
                } else {
                    out.push('/');
                    i += 1;
                }
            }
            ch => {
                out.push(ch);
                i += 1;
            }
        }
    }
    out
}

fn awk_program_risk(src: &str) -> Option<&'static str> {
    let code = awk_code_only(src);
    if code.contains('@') {
        return Some("uses a gawk directive (@include/@load)");
    }
    for word in code.split(|c: char| !(c.is_alphanumeric() || c == '_')) {
        if word == "system" {
            return Some("calls system()");
        }
    }
    let flat = code.replace("||", "  ");
    if flat.contains('|') {
        return Some("pipes to or from a command");
    }
    // `print ... > file` (outside parentheses) writes a file.
    let chars: Vec<char> = code.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        let rest: String = chars[i..].iter().collect();
        let word_start = i == 0 || !(chars[i - 1].is_alphanumeric() || chars[i - 1] == '_');
        let kw = if word_start && rest.starts_with("printf") {
            6
        } else if word_start && rest.starts_with("print") {
            5
        } else {
            0
        };
        if kw > 0
            && !chars
                .get(i + kw)
                .is_some_and(|c| c.is_alphanumeric() || *c == '_')
        {
            let mut depth = 0i32;
            let mut j = i + kw;
            while j < chars.len() {
                match chars[j] {
                    '(' => depth += 1,
                    ')' => depth -= 1,
                    ';' | '}' | '\n' if depth <= 0 => break,
                    '>' if depth <= 0 => return Some("redirects print output to a file"),
                    _ => {}
                }
                j += 1;
            }
            i = j;
        } else {
            i += 1;
        }
    }
    None
}

// ---------------------------------------------------------------- network tools

fn curl(prog: &str, args: &[Arg]) -> Class {
    let mut tags = vec![RiskTag::Network];
    let mut rule = "curl";
    let mut i = 0;
    let add = |tags: &mut Vec<RiskTag>, t: RiskTag| {
        if !tags.contains(&t) {
            tags.push(t);
        }
    };
    while i < args.len() {
        let t = args[i].text.as_str();
        i += 1;
        if let Some(long) = t.strip_prefix("--") {
            let (name, value) = match long.split_once('=') {
                Some((n, v)) => (n, Some(v.to_string())),
                None => (long, None),
            };
            let take = |i: &mut usize| {
                value
                    .clone()
                    .or_else(|| args.get(*i).map(|a| a.text.clone()))
            };
            match name {
                "config" => return Class::unknown(prog, "curl --config can hold any option"),
                "request" => {
                    if let Some(m) = take(&mut i) {
                        if !matches!(m.to_ascii_uppercase().as_str(), "GET" | "HEAD" | "OPTIONS") {
                            add(&mut tags, RiskTag::ExfilRisk);
                            rule = "curl (sends data)";
                        }
                    }
                }
                "output" | "dump-header" | "cookie-jar" | "trace" | "trace-ascii" | "stderr"
                | "libcurl" => {
                    if let Some(v) = take(&mut i) {
                        if v != "-" && v != "/dev/null" {
                            add(&mut tags, RiskTag::Write);
                        }
                    }
                }
                "remote-name" | "remote-name-all" | "remote-header-name" | "create-dirs" => {
                    add(&mut tags, RiskTag::Write)
                }
                n if n.starts_with("data")
                    || n.starts_with("form")
                    || n == "upload-file"
                    || n == "json"
                    || n == "post301"
                    || n == "post302"
                    || n == "post303" =>
                {
                    add(&mut tags, RiskTag::ExfilRisk);
                    rule = "curl (sends data)";
                }
                _ => {
                    if is_prefix_of(&format!("--{name}"), "--upload-file") {
                        add(&mut tags, RiskTag::ExfilRisk);
                        rule = "curl (sends data)";
                    }
                }
            }
            continue;
        }
        if t.starts_with('-') && t.len() > 1 {
            let chars: Vec<char> = t[1..].chars().collect();
            for (k, ch) in chars.iter().enumerate() {
                let rest: String = chars[k + 1..].iter().collect();
                let value = if rest.is_empty() {
                    args.get(i).map(|a| a.text.clone())
                } else {
                    Some(rest.clone())
                };
                match ch {
                    'K' => return Class::unknown(prog, "curl -K reads options from a file"),
                    'd' | 'F' | 'T' => {
                        add(&mut tags, RiskTag::ExfilRisk);
                        rule = "curl (sends data)";
                        break;
                    }
                    'X' => {
                        if let Some(m) = value {
                            if !matches!(
                                m.to_ascii_uppercase().as_str(),
                                "GET" | "HEAD" | "OPTIONS"
                            ) {
                                add(&mut tags, RiskTag::ExfilRisk);
                                rule = "curl (sends data)";
                            }
                        }
                        break;
                    }
                    'o' | 'D' | 'c' => {
                        if value.is_some_and(|v| v != "-" && v != "/dev/null") {
                            add(&mut tags, RiskTag::Write);
                        }
                        break;
                    }
                    'O' | 'J' => add(&mut tags, RiskTag::Write),
                    // options that take a value we don't need
                    'H' | 'A' | 'e' | 'u' | 'b' | 'm' | 'w' | 'x' | 'y' | 'Y' | 'z' | 'C' | 'E'
                    | 'r' | 't' | 'U' | 'Q' | 'P' => break,
                    _ => {}
                }
            }
        }
    }
    Class::new(rule, &tags)
}

fn wget(prog: &str, args: &[Arg]) -> Class {
    let mut tags = vec![RiskTag::Network];
    let mut rule = "wget";
    let mut to_stdout = false;
    let mut spider = false;
    let mut i = 0;
    while i < args.len() {
        let t = args[i].text.as_str();
        i += 1;
        if let Some(long) = t.strip_prefix("--") {
            let (name, value) = match long.split_once('=') {
                Some((n, v)) => (n, Some(v)),
                None => (long, None),
            };
            match name {
                "spider" => spider = true,
                "output-document" => {
                    let v = value.or_else(|| args.get(i).map(|a| a.text.as_str()));
                    if v == Some("-") {
                        to_stdout = true;
                    }
                }
                "execute" => return Class::unknown(prog, "wget --execute runs wgetrc commands"),
                "post-data" | "post-file" | "body-data" | "body-file" => {
                    if !tags.contains(&RiskTag::ExfilRisk) {
                        tags.push(RiskTag::ExfilRisk);
                    }
                    rule = "wget (sends data)";
                }
                "method" => {
                    let v = value.or_else(|| args.get(i).map(|a| a.text.as_str()));
                    if v.is_some_and(|m| !matches!(m.to_ascii_uppercase().as_str(), "GET" | "HEAD"))
                    {
                        tags.push(RiskTag::ExfilRisk);
                        rule = "wget (sends data)";
                    }
                }
                _ => {}
            }
            continue;
        }
        if t.starts_with('-') && t.len() > 1 {
            let chars: Vec<char> = t[1..].chars().collect();
            for (k, ch) in chars.iter().enumerate() {
                match ch {
                    'e' => return Class::unknown(prog, "wget -e runs wgetrc commands"),
                    'O' => {
                        let rest: String = chars[k + 1..].iter().collect();
                        let v = if rest.is_empty() {
                            args.get(i).map(|a| a.text.clone())
                        } else {
                            Some(rest)
                        };
                        if v.as_deref() == Some("-") {
                            to_stdout = true;
                        }
                        break;
                    }
                    _ => {}
                }
            }
        }
    }
    if !to_stdout && !spider {
        tags.push(RiskTag::Write);
    }
    Class::new(rule, &tags)
}

fn rsync(_prog: &str, args: &[Arg]) -> Class {
    let mut tags = vec![RiskTag::Write];
    let mut rule = "rsync";
    let mut dry = false;
    let mut remote = false;
    for a in args {
        let t = a.text.as_str();
        if t.starts_with("--") {
            let name = t.split('=').next().unwrap_or(t);
            if name == "--dry-run" {
                dry = true;
            } else if name.starts_with("--delete") || name == "--remove-source-files" {
                tags.push(RiskTag::Destructive);
                rule = "rsync --delete";
            } else if name == "--rsh" || name == "--daemon" || name == "--rsync-path" {
                remote = true;
            }
        } else if let Some(shorts) = t.strip_prefix('-') {
            if shorts.contains('n') {
                dry = true;
            }
            if shorts.contains('e') {
                remote = true;
            }
        } else if t.starts_with("rsync://")
            || (t.contains(':')
                && !t.starts_with('/')
                && !t.starts_with("./")
                && !t.starts_with('.'))
        {
            remote = true;
        }
    }
    if dry {
        tags.retain(|t| *t != RiskTag::Write && *t != RiskTag::Destructive);
        tags.push(RiskTag::Read);
    }
    if remote {
        tags.push(RiskTag::Network);
        tags.push(RiskTag::ExfilRisk);
    }
    Class::new(rule, &tags)
}

// ---------------------------------------------------------------- tar

fn tar(prog: &str, args: &[Arg]) -> Class {
    let mut write_mode = false;
    let mut read_mode = false;
    fn letters(s: &str, write_mode: &mut bool, read_mode: &mut bool) {
        for ch in s.chars() {
            match ch {
                'c' | 'r' | 'u' | 'x' | 'A' => *write_mode = true,
                't' | 'd' => *read_mode = true,
                _ => {}
            }
        }
    }
    for (idx, a) in args.iter().enumerate() {
        let t = a.text.as_str();
        if let Some(long) = t.strip_prefix("--") {
            let name = long.split('=').next().unwrap_or(long);
            match name {
                "to-command"
                | "checkpoint-action"
                | "use-compress-program"
                | "rsh-command"
                | "info-script"
                | "new-volume-script"
                | "transform-no" => {
                    return Class::unknown(prog, format!("tar --{name} runs a program"));
                }
                "create" | "append" | "update" | "extract" | "get" | "delete" | "concatenate"
                | "catenate" => write_mode = true,
                "list" | "diff" | "compare" => read_mode = true,
                _ => {}
            }
        } else if t.starts_with('-') && t.len() > 1 {
            if t[1..].contains(['I', 'F']) {
                return Class::unknown(prog, "tar -I/-F runs a program");
            }
            letters(&t[1..], &mut write_mode, &mut read_mode);
        } else if idx == 0 && !t.contains('=') && t.chars().all(|c| c.is_ascii_alphabetic()) {
            // old style: `tar xzf file`
            letters(t, &mut write_mode, &mut read_mode);
        }
    }
    if write_mode {
        Class::new("tar (create/extract)", &[RiskTag::Write])
    } else if read_mode {
        Class::new("tar (list/diff)", &[RiskTag::Read])
    } else {
        Class::unknown(prog, "tar: mode not recognized")
    }
}
