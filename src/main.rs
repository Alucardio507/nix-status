//! nixos-status: show how fresh your NixOS channel(s) are, using
//! the public NixOS Prometheus instance.
//!
//! Flags: --json, --no-color, -h/--help, -V/--version
//! Env:   NO_COLOR is respected.

use serde_json::{Value, json};
use std::collections::{HashMap, HashSet};
use std::io::{self, IsTerminal};
use std::process::Command;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const PROMETHEUS_API: &str = "https://prometheus.nixos.org/api/v1/query";
const REQUEST_TIMEOUT: Duration = Duration::from_secs(15);
const DAY: f64 = 86_400.0;
const FRESH_DAYS: f64 = 2.0;
const AGING_DAYS: f64 = 5.0;
const LABEL_WIDTH: usize = 14;

// ───────────────────────────── data types ─────────────────────────────

#[derive(Debug, Clone)]
struct Channel {
    name: String,
    revision: String,
    status: String,
    /// Unix timestamp of the last channel update, if known.
    updated: Option<f64>,
}

impl Channel {
    fn age(&self, now: f64) -> Option<f64> {
        self.updated.map(|ts| now - ts)
    }
    fn freshness(&self, now: f64) -> Freshness {
        Freshness::from_age(self.updated.filter(|ts| *ts > 0.0).map(|ts| now - ts))
    }
    fn status_order(&self) -> u8 {
        match self.status.as_str() {
            "stable" => 0,
            "rolling" => 1,
            "beta" => 2,
            _ => 9,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Freshness {
    Fresh,
    Aging,
    Stale,
}

impl Freshness {
    fn from_age(age_secs: Option<f64>) -> Self {
        match age_secs {
            Some(a) if a < FRESH_DAYS * DAY => Self::Fresh,
            Some(a) if a < AGING_DAYS * DAY => Self::Aging,
            _ => Self::Stale,
        }
    }
    /// Plain ANSI color code (no bold).
    fn color(self) -> &'static str {
        match self {
            Self::Fresh => "32",
            Self::Aging => "33",
            Self::Stale => "31",
        }
    }
}

#[derive(Debug, Default)]
struct LocalInfo {
    /// (label, channel name) pairs from `nix-channel --list`.
    channels: Vec<(String, String)>,
    revision: Option<String>,
    version: Option<String>,
}

#[derive(Debug, Default)]
struct Options {
    json: bool,
    color: bool,
}

// ───────────────────────────── styling ─────────────────────────────

#[derive(Clone, Copy)]
struct Style {
    enabled: bool,
}

impl Style {
    fn paint(self, code: &str, text: &str) -> String {
        if self.enabled && !code.is_empty() {
            format!("\x1b[{code}m{text}\x1b[0m")
        } else {
            text.to_owned()
        }
    }
}

/// A line that tracks its plain text (for width) and styled text (for output)
/// together, so the two can never drift apart.
#[derive(Default)]
struct Line {
    plain: String,
    styled: String,
}

impl Line {
    fn push(&mut self, style: Style, code: &str, text: &str) -> &mut Self {
        self.plain.push_str(text);
        self.styled.push_str(&style.paint(code, text));
        self
    }
}

fn kv(style: Style, label: &str, value: &str, value_code: &str) -> Line {
    let mut line = Line::default();
    line.push(style, "2", &format!("{label:<LABEL_WIDTH$}"))
        .push(style, value_code, value);
    line
}

fn box_lines(lines: &[Line]) -> String {
    // NOTE: chars().count() is fine for the glyphs used here; use the
    // `unicode-width` crate if you ever print CJK or emoji.
    let width = lines
        .iter()
        .map(|l| l.plain.chars().count())
        .max()
        .unwrap_or(0);
    let mut out = Vec::with_capacity(lines.len() + 2);
    out.push(format!("╭{}╮", "─".repeat(width + 2)));
    for l in lines {
        let pad = width.saturating_sub(l.plain.chars().count());
        out.push(format!("│ {}{} │", l.styled, " ".repeat(pad)));
    }
    out.push(format!("╰{}╯", "─".repeat(width + 2)));
    out.join("\n")
}

// ───────────────────────────── helpers ─────────────────────────────

fn age_string(seconds: f64) -> String {
    let total = seconds.max(0.0) as u64;
    let (days, hours, minutes) = (
        total / 86_400,
        (total % 86_400) / 3_600,
        (total % 3_600) / 60,
    );
    if days > 0 {
        format!("{days}d {hours}h")
    } else if hours > 0 {
        format!("{hours}h {minutes}m")
    } else {
        format!("{minutes}m")
    }
}

fn short(rev: &str, n: usize) -> String {
    rev.chars().take(n).collect()
}

fn now_seconds() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs_f64()
}

fn run(command: &str, args: &[&str]) -> Option<String> {
    let output = Command::new(command).args(args).output().ok()?;
    output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).into_owned())
}

fn label(row: &Value, name: &str) -> Option<String> {
    row.get("metric")?.get(name)?.as_str().map(str::to_owned)
}

// ───────────────────────────── network ─────────────────────────────

/// Query the public Prometheus API and return its `data.result` array.
fn query(agent: &ureq::Agent, metric: &str) -> Result<Vec<Value>, String> {
    let mut response = agent
        .get(PROMETHEUS_API)
        .query("query", metric) // let ureq do the escaping
        .call()
        .map_err(|e| format!("network request failed for {metric}: {e}"))?;
    let status = response.status();
    let body = response
        .body_mut()
        .read_to_string()
        .map_err(|e| format!("could not read Prometheus response for {metric}: {e}"))?;

    if !status.is_success() {
        let detail = body.trim();
        return Err(if detail.is_empty() {
            format!("Prometheus returned HTTP {status} for {metric}")
        } else {
            format!("Prometheus returned HTTP {status} for {metric}: {detail}")
        });
    }

    let payload: Value = serde_json::from_str(&body)
        .map_err(|e| format!("invalid JSON from Prometheus for {metric}: {e}"))?;

    if payload.get("status").and_then(Value::as_str) != Some("success") {
        let detail = payload
            .get("error")
            .and_then(Value::as_str)
            .unwrap_or("unknown Prometheus API error");
        return Err(format!("Prometheus query failed for {metric}: {detail}"));
    }

    payload
        .pointer("/data/result")
        .and_then(Value::as_array)
        .cloned()
        .ok_or_else(|| format!("malformed Prometheus response for {metric}: missing data.result"))
}

/// Fetch both metrics in parallel and merge them into one list of channels.
fn fetch_channels(agent: &ureq::Agent) -> Result<Vec<Channel>, String> {
    let (revision_rows, update_rows) = std::thread::scope(|s| {
        let revs = s.spawn(|| query(agent, "channel_revision"));
        let upds = s.spawn(|| query(agent, "channel_update_time"));
        (
            revs.join()
                .unwrap_or_else(|_| Err("worker thread panicked".into())),
            upds.join()
                .unwrap_or_else(|_| Err("worker thread panicked".into())),
        )
    });
    let (revision_rows, update_rows) = (revision_rows?, update_rows?);

    // Prometheus encodes sample values as strings: ["<ts>", "<value>"].
    let updates: HashMap<String, f64> = update_rows
        .iter()
        .filter_map(|row| {
            let name = label(row, "channel")?;
            let ts = row.pointer("/value/1")?.as_str()?.parse::<f64>().ok()?;
            Some((name, ts))
        })
        .collect();

    let mut channels: Vec<Channel> = revision_rows
        .iter()
        .filter_map(|row| {
            let name = label(row, "channel")?;
            Some(Channel {
                updated: updates.get(&name).copied(),
                revision: label(row, "revision").unwrap_or_else(|| "?".into()),
                status: label(row, "status").unwrap_or_else(|| "?".into()),
                name,
            })
        })
        .collect();

    if channels.is_empty() {
        return Err("No channel data returned.".into());
    }
    channels.sort_by(|a, b| (a.status_order(), &a.name).cmp(&(b.status_order(), &b.name)));
    channels.dedup_by(|a, b| a.name == b.name);
    Ok(channels)
}

// ───────────────────────────── local system ─────────────────────────────

fn local_info() -> LocalInfo {
    let channels = run("nix-channel", &["--list"])
        .unwrap_or_default()
        .lines()
        .filter_map(|line| {
            let mut parts = line.split_whitespace();
            let (label, url) = (parts.next()?, parts.next()?);
            if parts.next().is_some() {
                return None;
            }
            let name = url.trim_end_matches('/').rsplit('/').next().unwrap_or(url);
            Some((label.to_owned(), name.to_owned()))
        })
        .collect();

    let parsed =
        run("nixos-version", &["--json"]).and_then(|s| serde_json::from_str::<Value>(&s).ok());
    let field = |key: &str| {
        parsed
            .as_ref()?
            .get(key)?
            .as_str()
            .filter(|v| !v.is_empty())
            .map(str::to_owned)
    };

    LocalInfo {
        channels,
        revision: field("nixpkgsRevision"),
        version: field("nixosVersion"),
    }
}

// ───────────────────────────── rendering ─────────────────────────────

fn render_summary(
    style: Style,
    now: f64,
    channels: &[Channel],
    local: &LocalInfo,
    names: &[String],
) -> String {
    let by_name: HashMap<&str, &Channel> = channels.iter().map(|c| (c.name.as_str(), c)).collect();
    let labels: HashMap<&str, &str> = local
        .channels
        .iter()
        .map(|(l, n)| (n.as_str(), l.as_str()))
        .collect();

    let mut lines = Vec::new();
    let mut title = Line::default();
    title.push(style, "1", "Your NixOS channel");
    lines.push(title);

    if names.is_empty() {
        let mut l = Line::default();
        l.push(
            style,
            "2",
            "unknown (no nix-channel entries; flake/pinned system?)",
        );
        lines.push(l);
    }

    for name in names {
        lines.push(Line::default());

        let mut head = Line::default();
        head.push(style, "1;36", &format!("▶ {name}"));
        if let Some(l) = labels.get(name.as_str()) {
            head.push(style, "", &format!("  ({l})"));
        }
        lines.push(head);

        let Some(channel) = by_name.get(name.as_str()) else {
            lines.push(kv(style, "Status", "not listed on status page", "33"));
            continue;
        };

        let updated = channel
            .age(now)
            .map(|a| format!("{} ago", age_string(a)))
            .unwrap_or_else(|| "?".into());
        let color = format!("1;{}", channel.freshness(now).color());
        lines.push(kv(style, "Last updated", &updated, &color));
        lines.push(kv(style, "Commit", &short(&channel.revision, 12), "1"));
        lines.push(kv(style, "Status", &channel.status, "1"));

        if let Some(sys) = local.revision.as_deref() {
            if sys == channel.revision {
                lines.push(kv(style, "Your system", "at channel head", "1;32"));
            } else {
                let text = format!("on {} (differs)", short(sys, 12));
                lines.push(kv(style, "Your system", &text, "1;33"));
            }
        }
    }

    lines.push(Line::default());
    if let Some(v) = local.version.as_deref() {
        lines.push(kv(style, "NixOS version", v, "1"));
    }
    lines.push(kv(style, "nixos-status", env!("CARGO_PKG_VERSION"), "1"));
    box_lines(&lines)
}

fn render_table(style: Style, now: f64, channels: &[Channel], used: &HashSet<String>) -> String {
    let mut out = vec![style.paint(
        "1",
        &format!(
            "  {:<26}{:<14}{:<12}STATUS",
            "CHANNEL", "LAST UPDATED", "COMMIT"
        ),
    )];
    for c in channels {
        let in_use = used.contains(&c.name);
        let updated = c.age(now).map(age_string).unwrap_or_else(|| "?".into());
        let marker = if in_use {
            style.paint("1;36", "▶ ")
        } else {
            "  ".into()
        };
        let name = format!("{:<26}", c.name);
        let name = if in_use {
            style.paint("1", &name)
        } else {
            name
        };
        out.push(format!(
            "{marker}{name}{}{:<12}{}",
            style.paint(c.freshness(now).color(), &format!("{updated:<14}")),
            short(&c.revision, 10),
            c.status
        ));
    }
    out.join("\n")
}

fn render_json(
    now: f64,
    channels: &[Channel],
    used: &HashSet<String>,
    local: &LocalInfo,
) -> String {
    let items: Vec<Value> = channels
        .iter()
        .map(|c| {
            json!({
                "name": c.name,
                "status": c.status,
                "revision": c.revision,
                "updated_unix": c.updated,
                "age_seconds": c.age(now),
                "in_use": used.contains(&c.name),
                "system_at_head": local.revision.as_deref().map(|r| r == c.revision),
            })
        })
        .collect();
    serde_json::to_string_pretty(&json!({
        "nixos_version": local.version,
        "system_revision": local.revision,
        "channels": items,
    }))
    .unwrap_or_default()
}

// ───────────────────────────── main ─────────────────────────────

fn parse_args() -> Result<Options, String> {
    let mut opts = Options {
        json: false,
        color: io::stdout().is_terminal() && std::env::var_os("NO_COLOR").is_none(),
    };
    for arg in std::env::args().skip(1) {
        match arg.as_str() {
            "--json" => opts.json = true,
            "--no-color" => opts.color = false,
            "-V" | "--version" => {
                println!("nixos-status {}", env!("CARGO_PKG_VERSION"));
                std::process::exit(0);
            }
            "-h" | "--help" => {
                println!("Usage: nixos-status [--json] [--no-color] [--version]");
                std::process::exit(0);
            }
            other => return Err(format!("unknown argument: {other} (try --help)")),
        }
    }
    Ok(opts)
}

fn main_result() -> Result<(), String> {
    let opts = parse_args()?;
    let style = Style {
        enabled: opts.color,
    };

    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(REQUEST_TIMEOUT))
        .user_agent(concat!(
            env!("CARGO_PKG_NAME"),
            "/",
            env!("CARGO_PKG_VERSION")
        ))
        // Keep HTTP error responses so we can show Prometheus's error detail.
        .http_status_as_error(false)
        .build()
        .into();

    let channels = fetch_channels(&agent)?;
    let local = local_info();
    let now = now_seconds();

    // Which channels is this machine on?
    let (names, used): (Vec<String>, HashSet<String>) = if !local.channels.is_empty() {
        let names: Vec<String> = local.channels.iter().map(|(_, n)| n.clone()).collect();
        let used = names.iter().cloned().collect();
        (names, used)
    } else {
        // Fallback: guess from the running system's nixpkgs revision.
        let mut names: Vec<String> = channels
            .iter()
            .filter(|c| Some(c.revision.as_str()) == local.revision.as_deref())
            .map(|c| c.name.clone())
            .collect();
        names.sort();
        let used = names.iter().cloned().collect();
        (names, used)
    };

    if opts.json {
        println!("{}", render_json(now, &channels, &used, &local));
    } else {
        println!("{}", render_summary(style, now, &channels, &local, &names));
        println!("{}", render_table(style, now, &channels, &used));
    }
    Ok(())
}

fn main() {
    if let Err(error) = main_result() {
        eprintln!("nixos-status: {error}");
        std::process::exit(1);
    }
}

// ───────────────────────────── tests ─────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_ages() {
        assert_eq!(age_string(59.0), "0m");
        assert_eq!(age_string(3_700.0), "1h 1m");
        assert_eq!(age_string(90_000.0), "1d 1h");
        assert_eq!(age_string(-5.0), "0m");
    }

    #[test]
    fn classifies_freshness() {
        assert_eq!(Freshness::from_age(Some(DAY)), Freshness::Fresh);
        assert_eq!(Freshness::from_age(Some(3.0 * DAY)), Freshness::Aging);
        assert_eq!(Freshness::from_age(Some(9.0 * DAY)), Freshness::Stale);
        assert_eq!(Freshness::from_age(None), Freshness::Stale);
    }

    #[test]
    fn box_width_ignores_ansi() {
        let style = Style { enabled: true };
        let mut a = Line::default();
        a.push(style, "1", "abc");
        let mut b = Line::default();
        b.push(style, "", "abcdef");
        let boxed = box_lines(&[a, b]);
        let widths: Vec<usize> = boxed
            .lines()
            .map(|l| {
                // strip escape sequences for measuring
                let mut n = 0;
                let mut in_esc = false;
                for ch in l.chars() {
                    match (in_esc, ch) {
                        (false, '\x1b') => in_esc = true,
                        (true, 'm') => in_esc = false,
                        (false, _) => n += 1,
                        _ => {}
                    }
                }
                n
            })
            .collect();
        assert!(widths.windows(2).all(|w| w[0] == w[1]));
    }
}
