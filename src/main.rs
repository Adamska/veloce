fn main() -> anyhow::Result<()> {
    veloce::app::run(std::env::args_os().skip(1).collect())
}
