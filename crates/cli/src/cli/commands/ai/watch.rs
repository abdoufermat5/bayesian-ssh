//! `bssh ai watch`.
//!
//! Follows every running `bssh ai mcp` over its control socket: a header per run, then one
//! line per event as it happens (see `services::ai::feed`). Approval requests show as a
//! compact card; on a terminal `a` approves and `d` denies through the same control-socket
//! path as `bssh ai approve`. New runs that appear while watching are picked up.
//!
//! No new protocol: `Subscribe` already streams `ServerMsg::Event` and `NewRequest`.

use crate::cli::parser_ai::AiArgsWatch;
use crate::config::AppConfig;
use anyhow::Result;

pub async fn execute(args: AiArgsWatch, config: AppConfig) -> Result<()> {
    #[cfg(unix)]
    {
        unix_impl::run(args, config).await
    }
    #[cfg(not(unix))]
    {
        let _ = (args, config);
        anyhow::bail!("bssh ai watch needs Unix sockets and is not available on this platform")
    }
}

#[cfg(unix)]
mod unix_impl {
    use super::*;
    use crate::cli::commands::ai::approve::render_request;
    use crate::services::ai::ai_dir;
    use crate::services::ai::control_client::{list_sockets, ControlClient, RunHello};
    use crate::services::ai::feed::{
        event_line, request_lines, run_header, FeedContext, Folder, RunHeader,
    };
    use crate::services::ai::ipc::{ClientMsg, ServerMsg};
    use crate::services::ai::types::{AiEvent, ApprovalOutcome, ApprovalRequest, Tier};
    use crate::services::ai::ui::{self, Style, Tone};
    use chrono::Utc;
    use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
    use std::collections::{HashMap, HashSet};
    use std::io::{IsTerminal, Write};
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;
    use std::time::{Duration, Instant};
    use tokio::sync::mpsc;

    /// Everything the main loop reacts to.
    pub(super) enum Msg {
        Joined {
            hello: RunHello,
            cmd: mpsc::UnboundedSender<ClientMsg>,
        },
        /// Connected but not wanted (`--run`), or not reachable.
        Skipped {
            socket: PathBuf,
            retry: bool,
        },
        Server {
            run_id: String,
            msg: Box<ServerMsg>,
        },
        Gone {
            socket: PathBuf,
            run_id: String,
        },
        Key(KeyEvent),
    }

    struct Pending {
        run_id: String,
        request: ApprovalRequest,
    }

    struct Run {
        hello: RunHello,
        cmd: mpsc::UnboundedSender<ClientMsg>,
    }

    /// Keeps the terminal in raw mode; restores it on drop and on panic.
    struct RawGuard;

    impl RawGuard {
        fn new() -> Result<Self> {
            crossterm::terminal::enable_raw_mode()?;
            let prev = std::panic::take_hook();
            std::panic::set_hook(Box::new(move |info| {
                let _ = crossterm::terminal::disable_raw_mode();
                prev(info);
            }));
            Ok(RawGuard)
        }
    }

    impl Drop for RawGuard {
        fn drop(&mut self) {
            let _ = crossterm::terminal::disable_raw_mode();
        }
    }

    /// Writes lines, keeping the two-line footer under them on a terminal.
    struct Out {
        raw: bool,
        footer: bool,
        footer_drawn: bool,
    }

    impl Out {
        fn eol(&self) -> &'static str {
            if self.raw {
                "\r\n"
            } else {
                "\n"
            }
        }

        fn clear_footer(&mut self, w: &mut impl Write) {
            if self.footer_drawn {
                let _ = write!(w, "\r\x1b[2K\x1b[1A\x1b[2K\r");
                self.footer_drawn = false;
            }
        }

        fn emit(&mut self, lines: &[String], footer: &str) {
            let mut w = std::io::stdout().lock();
            self.clear_footer(&mut w);
            for l in lines {
                let _ = write!(w, "{}{}", l.replace('\n', self.eol()), self.eol());
            }
            if self.footer {
                let _ = write!(w, "{footer}");
                self.footer_drawn = true;
            }
            let _ = w.flush();
        }

        fn finish(&mut self) {
            let mut w = std::io::stdout().lock();
            self.clear_footer(&mut w);
            let _ = w.flush();
        }
    }

    /// `1 agent · 0 waiting` on the left, the keys on the right, under a rule.
    pub(super) fn footer_text(
        style: &Style,
        width: usize,
        agents: usize,
        waiting: usize,
    ) -> String {
        let left = format!(
            "{agents} {} {} {waiting} waiting",
            if agents == 1 { "agent" } else { "agents" },
            style.glyph(ui::Glyph::Dot)
        );
        let right = format!(
            "q quit   r reads on/off   {} last request",
            if style.unicode { "⏎" } else { "enter" }
        );
        let fill = width
            .saturating_sub(ui::display_width(&left) + ui::display_width(&right))
            .max(2);
        let rule = if style.unicode { "─" } else { "-" }.repeat(width);
        format!(
            "{}\r\n{left}{}{}",
            style.paint(Tone::Dim, &rule),
            " ".repeat(fill),
            style.paint(Tone::Dim, &right)
        )
    }

    struct Watcher {
        style: Style,
        ctx: FeedContext,
        folder: Folder,
        out: Out,
        json: bool,
        tty: bool,
        host: Option<String>,
        runs: HashMap<String, Run>,
        pending: Vec<Pending>,
        seen_requests: HashSet<String>,
        first_header: bool,
    }

    impl Watcher {
        fn footer(&self) -> String {
            footer_text(
                &self.style,
                self.ctx.width,
                self.runs.len(),
                self.pending.len(),
            )
        }

        fn print(&mut self, lines: &[String]) {
            let footer = self.footer();
            self.out.emit(lines, &footer);
        }

        fn host_ok(&self, ev: &AiEvent) -> bool {
            let Some(want) = &self.host else { return true };
            match ev {
                AiEvent::Call { call, .. } => call.host.as_deref() == Some(want.as_str()),
                AiEvent::Result { call_id, .. } => {
                    self.ctx.call_host(call_id) == Some(want.as_str())
                }
                _ => true,
            }
        }

        fn joined(&mut self, hello: RunHello, cmd: mpsc::UnboundedSender<ClientMsg>) {
            self.ctx.note_run(&hello.run_id, &hello.client);
            let header = RunHeader {
                client: hello.client.clone(),
                run_id: hello.run_id.clone(),
                policy: hello.policy.clone(),
                on_behalf_of: whoami::username(),
                hosts: Vec::new(),
                started_at: hello.started_at.clone(),
            };
            self.runs.insert(hello.run_id.clone(), Run { hello, cmd });
            if self.json {
                return;
            }
            let mut lines = Vec::new();
            if !self.first_header {
                lines.push(String::new());
            }
            self.first_header = false;
            lines.push(run_header(&self.style, &header, self.ctx.width));
            lines.push(String::new());
            self.print(&lines);
        }

        fn event(&mut self, ev: AiEvent) {
            let now = Utc::now();
            let ok = self.host_ok(&ev);
            // Always feed the context, so later lines know their call even when hidden.
            let line = event_line(&self.style, &ev, &mut self.ctx, now);
            if !ok {
                return;
            }
            if self.json {
                if let Ok(s) = serde_json::to_string(&ev) {
                    println!("{s}");
                }
                return;
            }
            let mut lines = self.folder.push(&self.style, &ev, line);
            if let AiEvent::Approval { request_id, .. } = &ev {
                let had = self.pending.len();
                self.pending.retain(|p| &p.request.request_id != request_id);
                if self.pending.len() != had {
                    lines.extend(self.next_card(now));
                }
            }
            if !lines.is_empty() || matches!(ev, AiEvent::Approval { .. }) {
                self.print(&lines);
            }
        }

        /// The card of the newest request still waiting, if any.
        fn next_card(&mut self, now: chrono::DateTime<Utc>) -> Vec<String> {
            let Some(req) = self.pending.last().map(|p| p.request.clone()) else {
                return Vec::new();
            };
            request_lines(&self.style, &req, &mut self.ctx, now, self.tty)
        }

        fn request(&mut self, run_id: String, request: ApprovalRequest) {
            if !self.seen_requests.insert(request.request_id.clone()) || self.json {
                return;
            }
            let now = Utc::now();
            let mut lines: Vec<String> = self.folder.flush(&self.style).into_iter().collect();
            lines.extend(request_lines(
                &self.style,
                &request,
                &mut self.ctx,
                now,
                self.tty,
            ));
            self.pending.push(Pending { run_id, request });
            self.print(&lines);
        }

        fn note(&mut self, text: &str) {
            let line = format!("          {}", self.style.paint(Tone::Dim, text));
            self.print(&[line]);
        }

        /// Handle a key; `true` quits.
        fn key(&mut self, k: KeyEvent) -> bool {
            if k.kind != KeyEventKind::Press {
                return false;
            }
            match k.code {
                KeyCode::Char('c') if k.modifiers.contains(KeyModifiers::CONTROL) => return true,
                KeyCode::Char('q') | KeyCode::Esc => return true,
                KeyCode::Char('r') => {
                    let folded = self.folder.toggle();
                    let mut lines: Vec<String> =
                        self.folder.flush(&self.style).into_iter().collect();
                    lines.push(format!(
                        "          {}",
                        self.style.paint(
                            Tone::Dim,
                            if folded {
                                "reads: folded after three"
                            } else {
                                "reads: all shown"
                            }
                        )
                    ));
                    self.print(&lines);
                }
                KeyCode::Char('a') => self.decide(true),
                KeyCode::Char('d') => self.decide(false),
                KeyCode::Enter => match self.pending.last() {
                    Some(p) => {
                        let text = match self.runs.get(&p.run_id) {
                            Some(r) => render_request(&r.hello, &p.request),
                            None => "That run has ended.".into(),
                        };
                        let mut lines: Vec<String> = text.lines().map(String::from).collect();
                        lines.push(String::new());
                        lines.push("          a approve    d deny".into());
                        self.print(&lines);
                    }
                    None => self.note("nothing is waiting for you"),
                },
                _ => {}
            }
            false
        }

        fn decide(&mut self, approve: bool) {
            let Some(p) = self.pending.pop() else {
                self.note("nothing is waiting for you");
                return;
            };
            let who = format!("{}@cli", whoami::username());
            let outcome = if approve {
                ApprovalOutcome::Approved {
                    by: who,
                    channel: "cli".into(),
                    grant: None,
                }
            } else {
                ApprovalOutcome::Denied {
                    by: who,
                    reason: None,
                }
            };
            let sent = self.runs.get(&p.run_id).is_some_and(|r| {
                r.cmd
                    .send(ClientMsg::Decide {
                        request_id: p.request.request_id.clone(),
                        outcome,
                    })
                    .is_ok()
            });
            if !sent {
                self.note("that run has ended; nothing to decide");
            } else if approve && p.request.tier == Tier::TwoPerson {
                self.note("this request needs a second, different approver");
            }
            // The approval event prints the outcome; show the next card now if more wait.
            let lines = self.next_card(Utc::now());
            if !lines.is_empty() {
                self.print(&lines);
            }
        }

        fn gone(&mut self, run_id: &str) {
            self.runs.remove(run_id);
            self.pending.retain(|p| p.run_id != run_id);
            if !self.json {
                self.print(&[]);
            }
        }
    }

    /// One run: subscribe, replay what waits, then relay messages and send commands.
    pub(super) async fn follow(
        mut c: ControlClient,
        mut cmd: mpsc::UnboundedReceiver<ClientMsg>,
        tx: mpsc::UnboundedSender<Msg>,
    ) {
        let run_id = c.hello.run_id.clone();
        let socket = c.socket.clone();
        let result: Result<()> = async {
            c.subscribe().await?;
            for request in c.pending().await? {
                let _ = tx.send(Msg::Server {
                    run_id: run_id.clone(),
                    msg: Box::new(ServerMsg::NewRequest { request }),
                });
            }
            loop {
                tokio::select! {
                    m = c.recv() => {
                        let msg = m?;
                        if tx.send(Msg::Server { run_id: run_id.clone(), msg: Box::new(msg) }).is_err() {
                            return Ok(());
                        }
                    }
                    m = cmd.recv() => match m {
                        Some(m) => c.send(&m).await?,
                        None => return Ok(()),
                    },
                }
            }
        }
        .await;
        if let Err(e) = result {
            tracing::debug!("watch: run {run_id}: {e}");
        }
        let _ = tx.send(Msg::Gone { socket, run_id });
    }

    async fn connect(socket: PathBuf, run: Option<String>, tx: mpsc::UnboundedSender<Msg>) {
        match ControlClient::connect(&socket, "bssh-cli").await {
            Ok(c) => {
                if run
                    .as_deref()
                    .is_some_and(|p| !c.hello.run_id.starts_with(p))
                {
                    let _ = tx.send(Msg::Skipped {
                        socket,
                        retry: false,
                    });
                    return;
                }
                let (cmd_tx, cmd_rx) = mpsc::unbounded_channel();
                let _ = tx.send(Msg::Joined {
                    hello: c.hello.clone(),
                    cmd: cmd_tx,
                });
                follow(c, cmd_rx, tx).await;
            }
            Err(e) => {
                let stale = e
                    .chain()
                    .filter_map(|c| c.downcast_ref::<std::io::Error>())
                    .any(|io| {
                        matches!(
                            io.kind(),
                            std::io::ErrorKind::ConnectionRefused | std::io::ErrorKind::NotFound
                        )
                    });
                if stale {
                    let _ = std::fs::remove_file(&socket);
                }
                let _ = tx.send(Msg::Skipped {
                    socket,
                    retry: !stale,
                });
            }
        }
    }

    pub(super) async fn run(args: AiArgsWatch, config: AppConfig) -> Result<()> {
        let dir = ai_dir(&config);
        let tty = !args.json && std::io::stdout().is_terminal() && std::io::stdin().is_terminal();
        let (tx, mut rx) = mpsc::unbounded_channel::<Msg>();

        let _raw = if tty { Some(RawGuard::new()?) } else { None };
        let stop = Arc::new(AtomicBool::new(false));
        if tty {
            let tx = tx.clone();
            let stop = stop.clone();
            std::thread::spawn(move || {
                while !stop.load(Ordering::Relaxed) {
                    if event::poll(Duration::from_millis(100)).unwrap_or(false) {
                        if let Ok(Event::Key(k)) = event::read() {
                            if tx.send(Msg::Key(k)).is_err() {
                                break;
                            }
                        }
                    }
                }
            });
        }

        let mut w = Watcher {
            style: Style::terminal(),
            ctx: FeedContext::default(),
            folder: Folder::default(),
            out: Out {
                raw: tty,
                footer: tty,
                footer_drawn: false,
            },
            json: args.json,
            tty,
            host: args.host.clone(),
            runs: HashMap::new(),
            pending: Vec::new(),
            seen_requests: HashSet::new(),
            first_header: true,
        };
        if !args.json && list_sockets(&dir).is_empty() {
            w.print(&[
                "No agent is running yet. Start one with `bssh ai mcp`; it shows up here.".into(),
                String::new(),
            ]);
        }

        let mut known: HashSet<PathBuf> = HashSet::new();
        let mut tick = tokio::time::interval(Duration::from_secs(1));
        let mut last_event = Instant::now();

        loop {
            tokio::select! {
                _ = tick.tick() => {
                    for sock in list_sockets(&dir) {
                        if known.insert(sock.clone()) {
                            tokio::spawn(connect(sock, args.run.clone(), tx.clone()));
                        }
                    }
                    if last_event.elapsed() > Duration::from_secs(2) {
                        if let Some(l) = w.folder.flush(&w.style) {
                            w.print(&[l]);
                        }
                    }
                }
                _ = tokio::signal::ctrl_c(), if !tty => break,
                msg = rx.recv() => {
                    let Some(msg) = msg else { break };
                    match msg {
                        Msg::Joined { hello, cmd } => w.joined(hello, cmd),
                        Msg::Skipped { socket, retry } => {
                            if retry {
                                known.remove(&socket);
                            }
                        }
                        Msg::Server { run_id, msg } => {
                            last_event = Instant::now();
                            match *msg {
                                ServerMsg::Event { event } => w.event(event),
                                ServerMsg::NewRequest { request } => w.request(run_id, request),
                                ServerMsg::Error { message } if !w.json => {
                                    w.note(&format!("could not decide: {message}"));
                                }
                                _ => {}
                            }
                        }
                        Msg::Gone { socket, run_id } => {
                            known.remove(&socket);
                            w.gone(&run_id);
                        }
                        Msg::Key(k) => {
                            if w.key(k) {
                                break;
                            }
                        }
                    }
                }
            }
        }
        stop.store(true, Ordering::Relaxed);
        w.out.finish();
        Ok(())
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use crate::services::ai::approval::serve_control;
        use crate::services::ai::approval::tests::{broker, request, run_info, wait_pending};
        use crate::services::ai::control_client::socket_path;
        use crate::services::ai::types::ApprovalBroker;
        use tokio::sync::{broadcast, watch};

        #[test]
        fn footer_has_counts_left_and_keys_right() {
            let f = footer_text(&Style::plain(), 78, 1, 0);
            let (rule, status) = f.split_once("\r\n").unwrap();
            assert_eq!(rule.chars().count(), 78);
            assert!(status.starts_with("1 agent · 0 waiting"), "{status}");
            assert!(status.ends_with("q quit   r reads on/off   ⏎ last request"));
            assert_eq!(status.chars().count(), 78);
            assert!(footer_text(&Style::plain(), 78, 2, 1).contains("2 agents · 1 waiting"));
        }

        #[tokio::test]
        async fn follow_relays_events_requests_and_decisions() {
            let (tmp, b) = broker(30, None);
            let dir = tmp.path().join("ai");
            let sock = socket_path(&dir, "run1");
            let (events, _) = broadcast::channel(16);
            let (stop, stop_rx) = watch::channel(false);
            let server = tokio::spawn(serve_control(
                sock.clone(),
                run_info(),
                b.clone(),
                events.clone(),
                stop_rx,
            ));
            for _ in 0..200 {
                if sock.exists() {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
            let (tx, mut rx) = mpsc::unbounded_channel();
            let (cmd_tx, cmd_rx) = mpsc::unbounded_channel();
            let c = ControlClient::connect(&sock, "bssh-cli").await.unwrap();
            tokio::spawn(follow(c, cmd_rx, tx));

            // Give it time to subscribe, then produce an event and a request.
            tokio::time::sleep(Duration::from_millis(300)).await;
            events
                .send(AiEvent::RunEnded {
                    run_id: "run1".into(),
                    reason: "x".into(),
                })
                .unwrap();
            let b2 = b.clone();
            let req = tokio::spawn(async move { b2.request(request("q1", Tier::OutOfBand)).await });
            wait_pending(&b, 1).await;

            let mut got_event = false;
            let mut got_request = false;
            while !(got_event && got_request) {
                let m = tokio::time::timeout(Duration::from_secs(5), rx.recv())
                    .await
                    .unwrap()
                    .unwrap();
                if let Msg::Server { msg, .. } = m {
                    match *msg {
                        ServerMsg::Event { .. } => got_event = true,
                        ServerMsg::NewRequest { request } => {
                            assert_eq!(request.request_id, "q1");
                            got_request = true;
                        }
                        _ => {}
                    }
                }
            }
            cmd_tx
                .send(ClientMsg::Decide {
                    request_id: "q1".into(),
                    outcome: ApprovalOutcome::Denied {
                        by: "ana@cli".into(),
                        reason: None,
                    },
                })
                .unwrap();
            let out = req.await.unwrap().unwrap();
            assert!(matches!(out, ApprovalOutcome::Denied { .. }));
            stop.send(true).unwrap();
            let _ = server.await;
        }
    }
}
