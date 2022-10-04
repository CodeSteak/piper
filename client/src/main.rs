use clap::{Parser, Subcommand};
use std::path::PathBuf;

#[derive(Debug, Parser)] 
struct Cli {
    #[arg(short, long, action = clap::ArgAction::Count)]
    verbose: u8,

    #[arg(short, long, value_name = "FILE")]
    config: Option<PathBuf>,

    #[arg(short, long, value_name = "HOST")]
    host : Option<String>,
    #[arg(short, long, value_name = "TOKEN")]
    token : Option<String>,

    #[clap(subcommand)]
    subcmd: Option<Commands>,
}

#[derive(Debug, Subcommand)]
enum Commands {
    /// does testing things
    Send {
        /// lists test values
        files : Vec<PathBuf>,
    }
}


fn main() {
    let cli = Cli::parse();
    
    
}
