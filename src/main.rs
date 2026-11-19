fn main() -> anyhow::Result<()> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();
    veloce::app::run(std::env::args_os().skip(1).collect())
}
