use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(author, version, about, long_about = None)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Subcommand)]
pub enum Commands {
    /// Show command history in an interactive viewer
    History,
    /// Show summary statistics about command usage
    Stats,
    /// Show today's stats
    Today,
}
