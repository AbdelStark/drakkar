//! The declarative `clap` command tree (RFC-0008 §1, public-API §2.1).
//!
//! Every command from the milestone surface table is present so `--help` and
//! completions are honest about the full surface from v0.1; milestone-gated
//! commands carry a note in their help. Command bodies land in their own issues
//! (#98 run, #99 fit, ...); this crate is the framework they plug into.

use clap::{Args, Parser, Subcommand};

/// The `drakkar` command-line interface.
#[derive(Parser, Debug)]
#[command(
    name = "drakkar",
    version,
    about = "A native LLM inference engine for Apple Silicon.",
    long_about = None,
    propagate_version = true
)]
pub struct Cli {
    /// Flags accepted on every command.
    #[command(flatten)]
    pub globals: GlobalFlags,

    /// The subcommand to run.
    #[command(subcommand)]
    pub command: Command,
}

/// Global flags, accepted before or after the subcommand (public-API §2.2,
/// RFC-0008 CLI6–CLI9).
#[derive(Args, Debug, Clone, Default)]
pub struct GlobalFlags {
    /// Emit a single machine-readable JSON object on stdout.
    #[arg(long, global = true)]
    pub json: bool,

    /// Emit JSON Lines streaming events (streaming commands only).
    #[arg(long, global = true)]
    pub stream_json: bool,

    /// Suppress non-error progress on stderr.
    #[arg(long, short, global = true)]
    pub quiet: bool,

    /// Raise the stderr log level; repeat for more detail (`-vv`).
    #[arg(long, short, global = true, action = clap::ArgAction::Count)]
    pub verbose: u8,

    /// Assume "yes" to interactive confirmations.
    #[arg(long, short = 'y', global = true)]
    pub yes: bool,

    /// Override a `Won't fit` verdict and proceed (exit-4 path).
    #[arg(long, global = true)]
    pub force: bool,
}

/// The command surface (public-API §2.1). Stability and milestone are documented
/// per command; the whole surface is present from v0.1 for honest `--help`.
#[derive(Subcommand, Debug)]
pub enum Command {
    /// Fit-check, acquire, load, then REPL or one-shot generate.
    Run {
        /// The model reference (e.g. `qwen3:8b`, `org/repo`).
        reference: String,
        /// A one-shot prompt; omit for an interactive REPL.
        prompt: Option<String>,
    },
    /// Acquire and prepare a model without running it.
    Pull {
        /// The model reference.
        reference: String,
    },
    /// Print a feasibility report without downloading (FE25).
    Fit {
        /// The model reference.
        reference: String,
        /// Target context length.
        #[arg(long)]
        ctx: Option<u32>,
        /// KV precision in bits (16, 8, or 4).
        #[arg(long)]
        kv_bits: Option<u8>,
        /// Concurrency to plan for.
        #[arg(long)]
        concurrency: Option<u32>,
        /// Simulate a machine profile instead of probing.
        #[arg(long)]
        machine: Option<String>,
    },
    /// List installed models.
    Ls,
    /// Remove a model.
    Rm {
        /// The model reference.
        reference: String,
    },
    /// Garbage-collect blobs unreferenced by any manifest.
    Prune,
    /// Report the environment, GPU, and configuration.
    Doctor {
        /// Check for a newer DRAKKAR release (explicit, on-demand).
        #[arg(long)]
        check_update: bool,
    },
    /// Run the HTTP server in the foreground.
    Serve {
        /// The model to load on start; omit to load on first request.
        reference: Option<String>,
    },
    /// Read or write configuration (CLI10–CLI11).
    Config {
        /// The config action.
        #[command(subcommand)]
        action: ConfigAction,
    },
    /// [v0.2] Show resident models and pool occupancy.
    Ps,
    /// [v0.2] Benchmark a model, optionally writing calibration.
    Bench {
        /// The model reference.
        reference: String,
        /// Write a per-chip calibration store.
        #[arg(long)]
        calibrate: bool,
    },
    /// [v0.2] Quantize a model on device to the store.
    Convert {
        /// The model reference.
        reference: String,
        /// Target bit width.
        #[arg(long)]
        bits: u8,
    },
}

/// `drakkar config` subcommands (CLI10–CLI11).
#[derive(Subcommand, Debug)]
pub enum ConfigAction {
    /// Print a config value.
    Get {
        /// The dotted config key, e.g. `server.port`.
        key: String,
    },
    /// Set a config value (validated, atomic write).
    Set {
        /// The dotted config key.
        key: String,
        /// The new value.
        value: String,
    },
    /// Print the config file path.
    Path,
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::CommandFactory;

    #[test]
    fn command_tree_is_valid() {
        // clap validates the derived tree (no duplicate flags/shorts, etc.).
        Cli::command().debug_assert();
    }

    #[test]
    fn help_lists_the_full_surface() {
        let help = Cli::command().render_long_help().to_string();
        for cmd in [
            "run", "pull", "fit", "ls", "rm", "prune", "doctor", "serve", "config", "ps", "bench",
            "convert",
        ] {
            assert!(help.contains(cmd), "help is missing `{cmd}`");
        }
    }

    #[test]
    fn global_flags_parse() {
        let cli = Cli::try_parse_from([
            "drakkar", "--json", "--quiet", "-vv", "--yes", "--force", "fit", "qwen3:8b",
        ])
        .unwrap();
        assert!(cli.globals.json);
        assert!(cli.globals.quiet);
        assert_eq!(cli.globals.verbose, 2);
        assert!(cli.globals.yes);
        assert!(cli.globals.force);
        assert!(matches!(cli.command, Command::Fit { .. }));
    }

    #[test]
    fn stream_json_flag_parses() {
        let cli =
            Cli::try_parse_from(["drakkar", "run", "qwen3:8b", "hi", "--stream-json"]).unwrap();
        assert!(cli.globals.stream_json);
    }

    #[test]
    fn unknown_flag_is_a_parse_error() {
        // Unknown flags are usage errors (exit 2), never silently ignored.
        assert!(Cli::try_parse_from(["drakkar", "fit", "qwen3:8b", "--nope"]).is_err());
    }

    #[test]
    fn config_subcommands_parse() {
        let cli =
            Cli::try_parse_from(["drakkar", "config", "set", "server.port", "11711"]).unwrap();
        assert!(matches!(
            cli.command,
            Command::Config {
                action: ConfigAction::Set { .. }
            }
        ));
    }
}
