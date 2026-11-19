#!/bin/sh
# Create the demo project (a small app with ~200 dependencies) at ~/.cargo-remap-demo/myapp.
# Needs network access once to fetch crates. Run record.py afterwards.
set -e
D=$HOME/.cargo-remap-demo
rm -rf "$D"; mkdir -p "$D/myapp/src"; cd "$D/myapp"
cat > Cargo.toml <<'T'
[package]
name = "myapp"
version = "0.1.0"
edition = "2024"

[dependencies]
tokio = { version = "1", features = ["full"] }
clap = { version = "4", features = ["derive"] }
serde = { version = "1", features = ["derive"] }
serde_json = "1"
regex = "1"
anyhow = "1"
reqwest = { version = "0.12", default-features = false, features = ["rustls-tls", "json"] }
axum = "0.8"
tracing = "0.1"
tracing-subscriber = "0.3"
chrono = { version = "0.4", features = ["serde"] }
uuid = { version = "1", features = ["v4", "serde"] }
rand = "0.9"
T
cat > src/main.rs <<'T'
use clap::Parser;
use serde::Serialize;

#[derive(Parser)]
struct Args {
    pattern: String,
}

#[derive(Serialize)]
struct Hit<'a> {
    text: &'a str,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args = Args::parse();
    let re = regex::Regex::new(&args.pattern)?;
    let line = "hello world";
    if re.is_match(line) {
        println!("{}", serde_json::to_string(&Hit { text: line })?);
    }
    Ok(())
}

#[allow(dead_code)]
async fn serve() {
    tracing_subscriber::fmt::init();
    let app: axum::Router = axum::Router::new().route("/", axum::routing::get(|| async { uuid::Uuid::new_v4().to_string() }));
    let _ = reqwest::get("http://localhost").await;
    let _ = (chrono::Utc::now(), rand::random::<u8>(), app);
}
T
printf 'target\n' > .gitignore
git init -q && cargo generate-lockfile -q && git add -A && git -c user.name=d -c user.email=d@d commit -qm init
