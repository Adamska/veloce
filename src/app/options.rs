use std::ffi::OsString;

use anyhow::{Result, bail};

use crate::engine::CacheLimits;

pub const USAGE: &str = "\
Veloce - GPU-accelerated media player

Usage: veloce [OPTIONS] [FILE_OR_URL]...

Options:
      --demo           Start the built-in 4K scrubbing demo
      --mute           Start with audio muted
      --paused         Open media without starting playback
      --inspector      Show the codec and performance inspector on launch
      --cache-mb <N>   Memory budget of the frame ring buffer in MiB
  -h, --help           Print this help
  -V, --version        Print the version";

#[derive(Debug, Default, PartialEq, Eq)]
pub struct Options {
    pub locations: Vec<String>,
    pub demo: bool,
    pub mute: bool,
    pub paused: bool,
    pub inspector: bool,
    pub cache_mb: Option<usize>,
    pub show_help: bool,
    pub show_version: bool,
}

impl Options {
    pub fn parse(arguments: Vec<OsString>) -> Result<Self> {
        let mut options = Self::default();
        let mut arguments = arguments
            .into_iter()
            .map(|argument| argument.to_string_lossy().into_owned());
        while let Some(argument) = arguments.next() {
            match argument.as_str() {
                "--demo" => options.demo = true,
                "--mute" => options.mute = true,
                "--paused" => options.paused = true,
                "--inspector" => options.inspector = true,
                "-h" | "--help" => options.show_help = true,
                "-V" | "--version" => options.show_version = true,
                "--cache-mb" => {
                    let value = arguments.next().and_then(|value| value.parse().ok());
                    match value {
                        Some(megabytes) if megabytes >= 64 => options.cache_mb = Some(megabytes),
                        _ => bail!("--cache-mb expects a size of at least 64"),
                    }
                }
                flag if flag.starts_with("--") => bail!("unknown option {flag}"),
                _ => options.locations.push(argument),
            }
        }
        Ok(options)
    }

    pub fn cache_limits(&self) -> CacheLimits {
        let mut limits = CacheLimits::default();
        if let Some(megabytes) = self.cache_mb {
            limits.budget_bytes = megabytes * 1024 * 1024;
        }
        limits
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(arguments: &[&str]) -> Result<Options> {
        Options::parse(arguments.iter().map(OsString::from).collect())
    }

    #[test]
    fn parses_flags_and_locations_in_any_order() {
        let options = parse(&[
            "a.mkv",
            "--mute",
            "--cache-mb",
            "512",
            "https://host/live.m3u8",
        ])
        .expect("arguments are valid");
        assert_eq!(options.locations, ["a.mkv", "https://host/live.m3u8"]);
        assert!(options.mute && !options.demo);
        assert_eq!(options.cache_limits().budget_bytes, 512 * 1024 * 1024);
    }

    #[test]
    fn rejects_unknown_flags_and_bad_budgets() {
        assert!(parse(&["--fast"]).is_err());
        assert!(parse(&["--cache-mb", "abc"]).is_err());
        assert!(parse(&["--cache-mb", "8"]).is_err());
    }
}
