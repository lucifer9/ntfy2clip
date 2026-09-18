use anyhow::Result;
use ntfy2clip::{
    clipboard::Clipboard,
    config::{Config, Mode},
    platform::{self, Backend},
    sync::Coordinator,
    transport::{self, Publisher},
};
use std::{env, time::Duration};
use tokio::{task::JoinSet, time};

#[cfg(target_os = "macos")]
struct ProjectOsLogger(oslog::OsLogger);
#[cfg(target_os = "macos")]
impl log::Log for ProjectOsLogger {
    fn enabled(&self, metadata: &log::Metadata<'_>) -> bool {
        (metadata.target() == "n2c" || metadata.target().starts_with("ntfy2clip"))
            && self.0.enabled(metadata)
    }
    fn log(&self, record: &log::Record<'_>) {
        if self.enabled(record.metadata()) {
            self.0.log(record);
        }
    }
    fn flush(&self) {
        self.0.flush();
    }
}

fn main() {
    if env::args().nth(1).as_deref() == Some("--clipboard-helper") {
        if platform::helper_main().is_err() {
            eprintln!("clipboard helper failed");
            std::process::exit(1);
        }
        return;
    }
    let log_level = if env::var_os("DEV").is_some() {
        log::LevelFilter::Debug
    } else {
        env::var("RUST_LOG")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(log::LevelFilter::Info)
    };
    // Dependency debug logs may contain WebSocket bodies or HTTP credentials.
    // Only project targets are allowed, including when DEV enables debug.
    #[cfg(target_os = "macos")]
    if env::var_os("DEV").is_none() {
        log::set_boxed_logger(Box::new(ProjectOsLogger(
            oslog::OsLogger::new("ntfyclip").level_filter(log_level),
        )))
        .expect("logger initialization");
    }
    if !cfg!(target_os = "macos") || env::var_os("DEV").is_some() {
        pretty_env_logger::formatted_builder()
            .filter_level(log::LevelFilter::Off)
            .filter_module("n2c", log_level)
            .filter_module("ntfy2clip", log_level)
            .init();
    }
    #[cfg(target_os = "macos")]
    rustls::crypto::aws_lc_rs::default_provider()
        .install_default()
        .expect("TLS provider initialization");
    let result = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("runtime initialization")
        .block_on(run());
    if let Err(error) = result {
        log::error!("n2c stopped: {error}");
        std::process::exit(1);
    }
}
async fn run() -> Result<()> {
    let mut config = Config::from_env()?;
    let requested = config.mode;
    let (backend, can_observe) = Backend::detect(config.mode == Mode::Bidirectional).await?;
    if !can_observe {
        config.mode = Mode::Receive;
    }
    log::info!(
        "mode requested={requested:?} effective={:?} readable={can_observe}",
        config.mode
    );
    let publisher = Publisher::new(config.clone())?;
    let mut peer = Coordinator::new(config.clone(), backend)?;
    if config.mode == Mode::Bidirectional {
        peer.observe().await;
    }
    let mut subscription = tokio::spawn(transport::subscribe(config.clone(), peer.receiver()));
    let mut publications = JoinSet::new();
    let mut poll = time::interval(config.poll);
    let mut work = time::interval(Duration::from_millis(10));
    poll.set_missed_tick_behavior(time::MissedTickBehavior::Skip);
    work.set_missed_tick_behavior(time::MissedTickBehavior::Skip);
    let result = loop {
        tokio::select! {
            signal = tokio::signal::ctrl_c() => { break signal.map_err(Into::into); }
            result = &mut subscription => { break match result { Ok(Err(e)) => Err(e), _ => Err(anyhow::anyhow!("subscription worker stopped")) }; }
            Some(result) = publications.join_next(), if !publications.is_empty() => {
                match result { Ok(outcome) => peer.published(outcome), Err(_) => break Err(anyhow::anyhow!("publisher worker stopped")) }
            }
            _ = poll.tick(), if config.mode == Mode::Bidirectional => { peer.observe().await; }
            _ = work.tick() => { peer.write_next().await; }
        }
        if let Some(job) = peer.next_publish() {
            let publisher = publisher.clone();
            publications.spawn(async move { publisher.publish(job).await });
        }
    };
    subscription.abort();
    let _ = subscription.await;
    publications.shutdown().await;
    peer.clipboard_mut().shutdown().await?;
    log::info!("stopped; pending memory-only jobs discarded");
    result
}
