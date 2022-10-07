use anyhow::Context;
use clap::{Parser, Subcommand};
use common::{TarHash, TarPassword};
use config::Config;
use std::{
    fmt::Display,
    fs::Permissions,
    io::{Read, Write},
    os::unix::prelude::PermissionsExt,
    path::PathBuf,
    str::FromStr,
};

mod config;

#[derive(Debug, Parser)]
struct Cli {
    #[arg(short, long, action = clap::ArgAction::Count)]
    verbose: u8,

    #[arg(short, long, value_name = "FILE")]
    config: Option<PathBuf>,

    #[arg(short = 'H', long, value_name = "HOST")]
    host: Option<String>,
    #[arg(short, long, value_parser = procotol_parser)]
    protocol: Option<config::Protocol>,
    #[arg(short, long, value_name = "TOKEN")]
    token: Option<String>,

    #[arg(long, value_name = "FILE")]
    history_file: Option<PathBuf>,

    #[arg(short, long, value_name = "FILE")]
    destination: Option<PathBuf>,

    #[arg(short, long)]
    overwrite: bool,

    #[arg(short, long)]
    no_history_file: bool,

    #[clap(subcommand)]
    subcmd: Option<Commands>,

    #[arg(value_parser = tar_password_parser)]
    code: Option<TarUrl>,
}

#[derive(Debug, Subcommand)]
enum Commands {
    /// does testing things
    Send {
        /// lists test values
        files: Vec<PathBuf>,
    },
    Login,
    Encrypt {
        #[arg(long)]
        input: Option<PathBuf>,
        #[arg(long)]
        output: Option<PathBuf>,
    },
    Decrypt {
        #[arg(long)]
        input: Option<PathBuf>,
        #[arg(long)]
        output: Option<PathBuf>,
    },
}

#[derive(Debug, Clone)]
struct TarUrl {
    protocol: Option<config::Protocol>,
    host: Option<String>,
    code: TarPassword,
}

fn procotol_parser(p: &str) -> Result<config::Protocol, String> {
    match p.to_ascii_lowercase().as_str() {
        "https" => Ok(config::Protocol::Https),
        "http" => Ok(config::Protocol::Http),
        "wss" => Ok(config::Protocol::Https),
        "ws" => Ok(config::Protocol::Http),
        _ => Err(format!("Unknown protocol: {}", p)),
    }
}

fn tar_password_parser(input: &str) -> Result<TarUrl, String> {
    let input = input.trim();

    let (input, protocol) = if input.contains("://") {
        let (p, rest) = input.split_once("://").unwrap();

        let protocol = procotol_parser(p)?;
        (rest, Some(protocol))
    } else {
        (input, None)
    };

    let (input, host) = if input.contains('/') {
        let (h, rest) = input.split_once('/').unwrap();
        if !h.is_empty() && !rest.is_empty() {
            (rest, Some(h.to_string()))
        } else {
            (input, None)
        }
    } else {
        (input, None)
    };

    let code = TarPassword::from_str(input.trim_end_matches('/'))
        .map_err(|_| format!("Invalid code: {}.", input))?;

    Ok(TarUrl {
        protocol,
        host,
        code,
    })
}

fn main() -> anyhow::Result<()> {
    let mut cli = Cli::parse();
    let config = config::Config::load(&cli.config)?;

    cli.host = cli.host.or_else(|| config.host.clone());
    cli.token = cli.token.or_else(|| config.token.clone());
    cli.protocol = cli.protocol.or(config.protocol);
    cli.history_file = cli.history_file.or_else(|| config.history_file.clone());

    if cli.no_history_file {
        cli.history_file = None;
    }

    match cli.subcmd {
        Some(Commands::Send { files }) => {
            for f in files {
                println!("TODO: Sending file: {}", f.display());
            }
        }
        Some(Commands::Login) => {
            let file = Config {
                host: cli.host,
                token: cli.token,
                protocol: cli.protocol,
                history_file: if cli.no_history_file {
                    None
                } else {
                    cli.history_file
                },
            }
            .save(&cli.config)?;
            println!("Saved config to {}", file.display());
        }
        Some(Commands::Decrypt { input, output }) => {
            let code = cli
                .code
                .ok_or_else(|| anyhow::anyhow!("No code provided."))?;
            let mut input = get_read_stream(&input.unwrap_or_else(|| PathBuf::from("-")))?;
            let mut output = get_write_stream(&output.unwrap_or_else(|| PathBuf::from("-")))?;

            let mut reader =
                common::EncryptedReader::new(&mut input, code.code.to_string().as_bytes());
            std::io::copy(&mut reader, &mut output)?;
        }
        Some(Commands::Encrypt { input, output }) => {
            let code = cli.code.map(|c| c.code).unwrap_or_else(|| {
                let pwd = TarPassword::generate();
                eprintln!("Generated code: {}", pwd);
                pwd
            });
            let mut input = get_read_stream(&input.unwrap_or_else(|| PathBuf::from("-")))?;
            let mut output = get_write_stream(&output.unwrap_or_else(|| PathBuf::from("-")))?;

            let mut writer = common::EncryptedWriter::new(&mut output, code.to_string().as_bytes());
            std::io::copy(&mut input, &mut writer)?;
        }
        None if cli.code.is_some() => {
            receive(&cli)?;
        }
        None => {
            println!("No action specified. See --help for usage.");
            std::process::exit(1);
        }
    }
    Ok(())
}

fn get_read_stream(path: &PathBuf) -> anyhow::Result<Box<dyn Read>> {
    if path.display().to_string() == "-" {
        Ok(Box::new(std::io::stdin()))
    } else {
        Ok(Box::new(std::fs::File::open(path).context(format!(
            "Failed to open file: {}",
            path.display()
        ))?))
    }
}

fn get_write_stream(path: &PathBuf) -> anyhow::Result<Box<dyn Write>> {
    if path.display().to_string() == "-" {
        Ok(Box::new(std::io::stdout()))
    } else {
        Ok(Box::new(std::fs::File::create(path).context(format!(
            "Failed to create file: {}",
            path.display()
        ))?))
    }
}

fn receive(cli: &Cli) -> anyhow::Result<()> {
    let code = cli.code.clone().unwrap();

    let host = code
        .host
        .as_ref()
        .or(cli.host.as_ref())
        .ok_or_else(|| anyhow::anyhow!("No host specified."))?;
    let protocol = code
        .protocol
        .or(cli.protocol)
        .unwrap_or(config::Protocol::Https);

    let agent = ureq::agent();

    let code_hash = TarHash::from_tarid(&code.code, host);

    let url = format!("{}://{}/raw/{}/", protocol, host, code_hash);
    if cli.verbose > 0 {
        println!("Downloading from {}", url);
    }

    let response = match agent.get(&url).call() {
        Ok(r) => r,
        Err(ureq::Error::Status(404, _)) => {
            println!("Repo not found.");
            std::process::exit(1);
        }
        Err(ureq::Error::Status(code, response)) => {
            println!("Server returned status code: {}", code);
            let s = response.into_string()?;
            println!("{}", s);
            std::process::exit(1);
        }
        Err(e) => {
            return Err(e.into());
        }
    };

    let content_length = response
        .header("Content-Length")
        .and_then(|s| s.parse::<u64>().ok())
        .unwrap_or(0);

    let reader = response.into_reader();
    let reader = common::EncryptedReader::new(reader, code.code.to_string().as_bytes());

    let mut tar = tar::Archive::new(reader);
    let destination = cli
        .destination
        .clone()
        .unwrap_or_else(|| PathBuf::from("."));
    let overwrite = cli.overwrite;

    let mut progress = ProgressBar::new(content_length);

    println!(); // For progress bar
    let mut buf = vec![0; 128 * 1024];
    for entry in tar.entries()? {
        let mut file = entry?;
        let display = file.path()?.display().to_string();
        let file_destination = destination.join(file.path()?);

        progress.update(512, &display);

        if display == "./" || display == "." {
            // Current directory does not need to be created
            continue;
        }

        if file_destination.exists() && !overwrite {
            println!("Skipping because it already exists: {}", display);
            loop {
                let n = file.read(&mut buf)?;
                if n == 0 {
                    break;
                }
                progress.update(n as u64, &display);
            }
            continue;
        }

        let perm = file.header().mode().unwrap_or(0o644);
        if file.header().entry_type().is_dir() {
            std::fs::create_dir_all(&file_destination)?;
            std::fs::set_permissions(&file_destination, Permissions::from_mode(perm))?;
        } else if file.header().entry_type().is_file() {
            let mut new_file = if overwrite {
                std::fs::OpenOptions::new()
                    .write(true)
                    .create(true)
                    .truncate(true)
                    .open(&file_destination)
            } else {
                std::fs::OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .open(&file_destination)
            }
            .with_context(|| format!("Failed to create file {}", file_destination.display()))?;

            loop {
                let n = file.read(&mut buf)?;
                if n == 0 {
                    break;
                }
                new_file.write_all(&buf[..n])?;
                progress.update(n as u64, &display);
            }
        }
    }

    println!("\nDone.");
    Ok(())
}

const DELETE_LINE: &str = "\x1B[2K\r";

struct ProgressBar {
    last_update: std::time::Instant,
    current: u64,
    last_progress: u64,
    total: u64,
}

impl ProgressBar {
    fn new(total: u64) -> Self {
        Self {
            last_update: std::time::Instant::now(),
            current: 0,
            last_progress: 0,
            total,
        }
    }

    fn update<D: Display>(&mut self, progress: u64, message: D) {
        self.current += progress;

        let now = std::time::Instant::now();
        let elapsed = now.duration_since(self.last_update).as_secs_f32();
        if elapsed < 0.5 {
            return;
        }

        let speed = (self.current - self.last_progress) as f32 / (elapsed + 0.0001);
        self.last_progress = self.current;
        self.last_update = now;

        let percent = if self.current < self.total && self.total > 0 {
            (self.current as f64 / self.total as f64) * 100.0
        } else {
            100.0
        };
        let eta = if self.current < self.total && self.total > 0 && speed > 0.0 {
            let remaining = self.total - self.current;
            remaining as f32 / speed
        } else {
            0.0
        };

        let speed = if speed > 1024.0 * 1024.0 {
            format!("{:.2} MB/s", speed / 1024.0 / 1024.0)
        } else if speed > 1024.0 {
            format!("{:.2} KB/s", speed / 1024.0)
        } else {
            format!("{:.2} B/s", speed)
        };

        let eta = if eta > 60.0 * 60.0 {
            format!("{:.2} h", eta / 60.0 / 60.0)
        } else if eta > 60.0 {
            format!("{:.2} m", eta / 60.0)
        } else {
            format!("{:.2} s", eta)
        };

        let bar = (0..((percent / 5.0) as isize))
            .map(|_| "=")
            .collect::<String>();

        print!("{DELETE_LINE}|{bar:20}|  {percent:02.0}%  {speed:10}  eta {eta:9} - {message}");
        let _ = std::io::stdout().flush();
    }
}
