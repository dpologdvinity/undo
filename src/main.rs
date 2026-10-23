use clap::{Parser, Subcommand, ValueEnum};
use std::{path::PathBuf, process::ExitCode};

#[derive(Parser)]
#[command(
    name = "undo",
    version,
    about = "Ctrl-Z for your terminal and your AI agent"
)]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand)]
enum Commands {
    /// Capture the current working tree silently
    Snap {
        #[arg(long, default_value = "manual")]
        trigger: String,
        #[arg(long)]
        cmd: Option<String>,
        #[arg(long)]
        cwd: Option<PathBuf>,
    },
    /// Restore a snapshot, optionally limited to paths after --
    Restore {
        id: String,
        #[arg(last = true)]
        paths: Vec<PathBuf>,
    },
    /// List recent snapshots
    List {
        #[arg(short = 'n', default_value_t = 20)]
        n: usize,
    },
    /// Show what restoring a snapshot would change
    Diff {
        id: String,
        #[arg(long)]
        patch: bool,
    },
    /// Show protection, hooks, errors, and recovery limits
    Status,
    /// Protect a directory outside git
    Protect {
        dir: PathBuf,
        #[arg(long)]
        force: bool,
    },
    /// Stop protecting a non-git directory
    Unprotect { dir: PathBuf },
    /// Print a shell hook or install the Claude Code hook
    Init { target: Target },
    /// Apply snapshot retention
    Prune {
        #[arg(long)]
        root: Option<PathBuf>,
    },
}

#[derive(Clone, ValueEnum)]
enum Target {
    Zsh,
    Bash,
    Fish,
    Claude,
}

fn dispatch(cli: Cli) -> anyhow::Result<()> {
    use undo_cli::{root, snapshot};
    match cli.command {
        Some(Commands::Snap { trigger, cmd, cwd }) => {
            snapshot::cli_snap(trigger, cmd, cwd);
            Ok(())
        }
        Some(Commands::Protect { dir, force }) => {
            undo_cli::git::check_version()?;
            root::protect(&dir, force)?;
            let path = std::fs::canonicalize(dir)?;
            let r = root::require(&path)?;
            snapshot::snap(
                &r,
                &snapshot::Meta {
                    trigger: "protect".into(),
                    cmd: None,
                    cwd: path,
                },
            )?;
            Ok(())
        }
        Some(Commands::Unprotect { dir }) => root::unprotect(&dir),
        Some(Commands::Status) => {
            if let Some(r) = root::find_root(&std::env::current_dir()?) {
                println!(
                    "root: {} ({})",
                    r.path.display(),
                    if r.is_git { "git" } else { "protected" }
                );
            }
            Ok(())
        }
        _ => todo!(),
    }
}

fn main() -> ExitCode {
    match dispatch(Cli::parse()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("undo: {e:#}");
            ExitCode::FAILURE
        }
    }
}
