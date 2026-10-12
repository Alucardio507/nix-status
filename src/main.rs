use reqwest::blocking::Client;
use serde_json::Value;
use std::collections::{HashMap, HashSet};
use std::io::{self, IsTerminal};
use std::process::Command;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const PROMETHEUS_API: &str = "https://prometheus.nixos.org/api/v1/query";
const REQUEST_TIMEOUT: Duration = Duration::from_secs(15);

#[derive(Debug, Clone)]
struct Channel {
    name: String,
    revision: String,
    status: String,
}

fn ansi_enabled() -> bool {
    io::stdout().is_terminal()
}

fn paint(enabled: bool, code: &str, text: &str) -> String {
    if enabled {
        format!("\x1b[{code}m{text}\x1b[0m")
    } else {
        text.to_owned()
    }
}

fn run(command: &str, args: &[&str]) -> String {
    match Command::new(command).args(args).output() {
        Ok(output) if output.status.success() => {
            String::from_utf8_lossy(&output.stdout).into_owned()
        }
        _ => String::new(),
    }
}

/// Query the public Prometheus API and return its `data.result` array.
fn query(client: &Client, metric: &str) -> Result<Vec<Value>, String> {
    // `metric` is an internal constant (e.g. "channel_revision"), so it does
    // not need URL escaping. Using reqwest keeps HTTPS and response handling in
    // Rust rather than shelling out to curl.
    let url = format!("{PROMETHEUS_API}?query={metric}");
    let response = client
        .get(&url)
        .send()
        .map_err(|error| format!("network request failed for {metric}: {error}"))?;
    let status = response.status();
    let body = response
        .text()
        .map_err(|error| format!("could not read Prometheus response for {metric}: {error}"))?;

    if !status.is_success() {
        let detail = body.trim();
        return Err(if detail.is_empty() {
            format!("Prometheus returned HTTP {status} for {metric}")
        } else {
            format!("Prometheus returned HTTP {status} for {metric}: {detail}")
        });
    }

    let payload: Value = serde_json::from_str(&body)
        .map_err(|error| format!("invalid JSON from Prometheus for {metric}: {error}"))?;

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

fn label(row: &Value, name: &str) -> Option<String> {
    row.get("metric")?.get(name)?.as_str().map(str::to_owned)
}

fn age(seconds: f64) -> String {
    let total = seconds.max(0.0) as u64;
    let days = total / 86_400;
    let hours = (total % 86_400) / 3_600;
    let minutes = (total % 3_600) / 60;

    if days > 0 {
        format!("{days}d {hours}h")
    } else if hours > 0 {
        format!("{hours}h {minutes}m")
    } else {
        format!("{minutes}m")
    }
}

fn age_color(now: f64, timestamp: Option<f64>) -> &'static str {
    match timestamp {
        Some(ts) if ts > 0.0 && now - ts < 2.0 * 86_400.0 => "1;32",
        Some(ts) if ts > 0.0 && now - ts < 5.0 * 86_400.0 => "1;33",
        _ => "1;31",
    }
}

fn box_lines(lines: &[(String, String)]) -> String {
    let width = lines
        .iter()
        .map(|(plain, _)| plain.chars().count())
        .max()
        .unwrap_or(0);
    let mut output = Vec::with_capacity(lines.len() + 2);
    output.push(format!("╭{}╮", "─".repeat(width + 2)));
    for (plain, styled) in lines {
        let padding = width.saturating_sub(plain.chars().count());
        output.push(format!("│ {styled}{} │", " ".repeat(padding)));
    }
    output.push(format!("╰{}╯", "─".repeat(width + 2)));
    output.join("\n")
}

fn local_info() -> (Vec<(String, String)>, Option<String>, Option<String>) {
    let mut channels = Vec::new();
    for line in run("nix-channel", &["--list"]).lines() {
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() == 2 {
            let channel_name = parts[1]
                .trim_end_matches('/')
                .rsplit('/')
                .next()
                .unwrap_or(parts[1])
                .to_owned();
            channels.push((parts[0].to_owned(), channel_name));
        }
    }

    let version_json = run("nixos-version", &["--json"]);
    let parsed = serde_json::from_str::<Value>(&version_json).ok();
    let revision = parsed
        .as_ref()
        .and_then(|value| value.get("nixpkgsRevision"))
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .map(str::to_owned);
    let version = parsed
        .as_ref()
        .and_then(|value| value.get("nixosVersion"))
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .map(str::to_owned);

    (channels, revision, version)
}

fn push_line(
    lines: &mut Vec<(String, String)>,
    plain: impl Into<String>,
    styled: impl Into<String>,
) {
    lines.push((plain.into(), styled.into()));
}

fn push_kv(
    lines: &mut Vec<(String, String)>,
    ansi: bool,
    label: &str,
    value: &str,
    value_color: &str,
) {
    let padded_label = format!("{label:<14}");
    let plain = format!("{padded_label}{value}");
    let styled = format!(
        "{}{}",
        paint(ansi, "2", &padded_label),
        paint(ansi, value_color, value)
    );
    push_line(lines, plain, styled);
}

fn now_seconds() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs_f64()
}

fn main_result() -> Result<(), String> {
    let client = Client::builder()
        .timeout(REQUEST_TIMEOUT)
        .user_agent(concat!(
            env!("CARGO_PKG_NAME"),
            "/",
            env!("CARGO_PKG_VERSION")
        ))
        .build()
        .map_err(|error| format!("could not initialize HTTP client: {error}"))?;

    let revision_rows = query(&client, "channel_revision")?;
    let update_rows = query(&client, "channel_update_time")?;

    let mut revisions: HashMap<String, Channel> = HashMap::new();
    for row in &revision_rows {
        let Some(name) = label(row, "channel") else {
            continue;
        };
        let channel = Channel {
            name: name.clone(),
            revision: label(row, "revision").unwrap_or_else(|| "?".to_owned()),
            status: label(row, "status").unwrap_or_else(|| "?".to_owned()),
        };
        revisions.insert(name, channel);
    }
    if revisions.is_empty() {
        return Err("No channel data returned.".to_owned());
    }

    let mut updates: HashMap<String, f64> = HashMap::new();
    for row in &update_rows {
        let Some(name) = label(row, "channel") else {
            continue;
        };
        let timestamp = row
            .pointer("/value/1")
            .and_then(Value::as_str)
            .and_then(|value| value.parse::<f64>().ok())
            .or_else(|| row.pointer("/value/1").and_then(Value::as_f64));
        if let Some(timestamp) = timestamp {
            updates.insert(name, timestamp);
        }
    }

    let (local_channels, system_revision, system_version) = local_info();
    let mut used: HashSet<String> = local_channels
        .iter()
        .map(|(_, name)| name.clone())
        .collect();
    if used.is_empty() {
        used.extend(
            revisions
                .iter()
                .filter(|(_, channel)| Some(channel.revision.as_str()) == system_revision.as_deref())
                .map(|(name, _)| name.clone()),
        );
    }

    let now = now_seconds();
    let names: Vec<String> = if local_channels.is_empty() {
        let mut names: Vec<String> = used.iter().cloned().collect();
        names.sort();
        names
    } else {
        local_channels
            .iter()
            .map(|(_, name)| name.clone())
            .collect()
    };
    let labels: HashMap<String, String> = local_channels
        .iter()
        .map(|(label, name)| (name.clone(), label.clone()))
        .collect();

    let ansi = ansi_enabled();
    let mut lines: Vec<(String, String)> = Vec::new();
    let title = "Your NixOS channel";
    push_line(&mut lines, title, paint(ansi, "1", title));

    if names.is_empty() {
        let text = "unknown (no nix-channel entries; flake/pinned system?)";
        push_line(&mut lines, text, paint(ansi, "2", text));
    }

    for name in &names {
        push_line(&mut lines, "", "");
        let mut head = format!("▶ {name}");
        if let Some(label) = labels.get(name) {
            head.push_str(&format!("  ({label})"));
        }
        let styled_head = format!(
            "{}{}",
            paint(ansi, "1;36", &format!("▶ {name}")),
            labels
                .get(name)
                .map(|label| format!("  ({label})"))
                .unwrap_or_default()
        );
        push_line(&mut lines, head, styled_head);

        let Some(channel) = revisions.get(name) else {
            push_kv(
                &mut lines,
                ansi,
                "Status",
                "not listed on status page",
                "33",
            );
            continue;
        };

        let timestamp = updates.get(name).copied();
        let updated = timestamp
            .map(|ts| format!("{} ago", age(now - ts)))
            .unwrap_or_else(|| "?".to_owned());
        push_kv(
            &mut lines,
            ansi,
            "Last updated",
            &updated,
            age_color(now, timestamp),
        );
        push_kv(
            &mut lines,
            ansi,
            "Commit",
            &channel.revision.chars().take(12).collect::<String>(),
            "1",
        );
        push_kv(&mut lines, ansi, "Status", &channel.status, "1");

        if let Some(system_revision) = system_revision.as_deref() {
            if system_revision == channel.revision {
                push_kv(&mut lines, ansi, "Your system", "at channel head", "1;32");
            } else {
                let description = format!(
                    "on {} (differs)",
                    system_revision.chars().take(12).collect::<String>()
                );
                push_kv(&mut lines, ansi, "Your system", &description, "1;33");
            }
        }
    }

    if let Some(version) = system_version.as_deref() {
        push_line(&mut lines, "", "");
        push_kv(&mut lines, ansi, "NixOS version", version, "1");
    }

    println!("{}", box_lines(&lines));
    println!();

    let mut rows: Vec<&Channel> = revisions.values().collect();
    rows.sort_by_key(|channel| {
        let order = match channel.status.as_str() {
            "stable" => 0,
            "rolling" => 1,
            "beta" => 2,
            _ => 9,
        };
        (order, channel.name.clone())
    });

    println!(
        "{}",
        paint(
            ansi,
            "1",
            &format!(
                "  {:<26}{:<14}{:<12}STATUS",
                "CHANNEL", "LAST UPDATED", "COMMIT"
            )
        )
    );
    for channel in rows {
        let timestamp = updates.get(&channel.name).copied();
        let updated = timestamp
            .map(|ts| age(now - ts))
            .unwrap_or_else(|| "?".to_owned());
        let color = age_color(now, timestamp).replace("1;", "");
        let marker = if used.contains(&channel.name) {
            paint(ansi, "1;36", "▶ ")
        } else {
            "  ".to_owned()
        };
        let padded_name = format!("{:<26}", channel.name);
        let name = if used.contains(&channel.name) {
            paint(ansi, "1", &padded_name)
        } else {
            padded_name
        };
        let commit: String = channel.revision.chars().take(10).collect();
        println!(
            "{}{}{}{:<12}{}",
            marker,
            name,
            paint(ansi, &color, &format!("{updated:<14}")),
            commit,
            channel.status
        );
    }

    Ok(())
}

fn main() {
    if let Err(error) = main_result() {
        eprintln!("nixos-status: {error}");
        std::process::exit(1);
    }
}
