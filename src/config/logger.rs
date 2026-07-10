use tracing_appender::non_blocking::WorkerGuard;
use tracing_subscriber::{fmt, layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};

/// Init tracing: log ke console + file `logs/combined.log`.
/// Return `WorkerGuard` harus tetap hidup selama aplikasi berjalan (simpan di `main`).
pub fn init() -> WorkerGuard {
    std::fs::create_dir_all("logs").expect("Gagal membuat folder logs");

    let file_appender = tracing_appender::rolling::never("logs", "combined.log");
    let (non_blocking, guard) = tracing_appender::non_blocking(file_appender);

    let env_filter =
        EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));

    tracing_subscriber::registry()
        .with(env_filter)
        .with(fmt::layer().with_target(false))
        .with(fmt::layer().with_writer(non_blocking).with_ansi(false).with_target(false))
        .init();

    guard
}
