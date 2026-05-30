use anyhow::Result;
use clap::Parser;
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        // symphonia logs benign INFO/WARN noise (e.g. "skipped N bytes of
        // junk") per file it decodes for waveforms; quiet it by default
        // while leaving RUST_LOG in control when set.
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| "info,symphonia=error".into()),
        )
        .init();

    let args = iggybilly::cli::Cli::parse();
    iggybilly::cli::run(args).await
}
