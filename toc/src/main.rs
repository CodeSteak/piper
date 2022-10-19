use anyhow::Context;
use clap::{Parser, Subcommand};
use common::{EncryptedWriter, TarHash, TarPassword};
use config::Config;
use std::{
    fmt::Display,
    fs::Permissions,
    io::{Read, Write},
    os::unix::prelude::PermissionsExt,
    path::{Path, PathBuf},
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

    match &cli.subcmd {
        Some(Commands::Send { files }) => {
            send(&cli, files)?;
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
            let mut input = get_read_stream(&input.clone().unwrap_or_else(|| PathBuf::from("-")))?;
            let mut output =
                get_write_stream(&output.clone().unwrap_or_else(|| PathBuf::from("-")))?;

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
            let mut input = get_read_stream(&input.clone().unwrap_or_else(|| PathBuf::from("-")))?;
            let mut output =
                get_write_stream(&output.clone().unwrap_or_else(|| PathBuf::from("-")))?;

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

fn send(cli: &Cli, files: &[PathBuf]) -> anyhow::Result<()> {
    let mut files_out = vec![];
    for file in files {
        collect_files(file, &mut files_out)?;
    }
    const TAR_HEADER_SIZE: usize = 512;
    let total_size = files_out
        .iter()
        .map(|(_, s, _)| *s + TAR_HEADER_SIZE)
        .sum::<usize>();

    let base = if files.len() == 1 {
        if files[0].is_dir() {
            Some(files[0].to_path_buf())
        } else if files[0].is_file() {
            Some(files[0].parent().unwrap().to_path_buf())
        } else {
            None
        }
    } else {
        None
    };

    let code = cli.code.clone().unwrap_or_else(|| TarUrl {
        code: TarPassword::generate(),
        host: None,
        protocol: None,
    });

    if cli.verbose > 0 {
        for (path, size, _) in &files_out {
            println!("{} ({})", path.display(), size);
        }
        println!("Total size: {}", total_size);
        println!("base: {:?}", base);
    }

    let host = code
        .host
        .as_ref()
        .or(cli.host.as_ref())
        .ok_or_else(|| anyhow::anyhow!("No host specified."))?;

    let protocol = code
        .protocol
        .or(cli.protocol)
        .unwrap_or(config::Protocol::Https);

    let token = cli
        .token
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("No token specified."))?;

    let agent = ureq::agent();

    let code_hash = TarHash::from_tarid(&code.code, host);

    let url = format!("{}://{}/raw/{}/", protocol, host, code_hash);

    if cli.verbose > 0 {
        println!("Downloading from {}", url);
    }

    let (writer, reader) = common::create_pipe();
    let mut writer = EncryptedWriter::new(writer, code.code.to_string().as_bytes());

    std::thread::scope(|s| {
        let handle_a = s.spawn(|| {
            let _response = agent
                .post(&url)
                .set("Authorization", &format!("Bearer {}", token))
                .send(reader)
                .context("Failed to send request.")?;
            Ok::<(), anyhow::Error>(())
        });

        println!("\n\n{protocol}://{host}/{}/\n\n", code.code);

        let mut progress = ProgressBar::new(total_size as u64);

        let mut tar = tar::Builder::new(&mut writer);
        for (src_path, size, is_dir) in files_out {
            let mut header = tar::Header::new_gnu();

            let mut p = if let Some(base) = &base {
                src_path.strip_prefix(&base).unwrap()
            } else {
                &src_path
            }
            .display()
            .to_string();
            if p.is_empty() {
                continue;
            }

            if is_dir {
                p += "/";
            }

            if cli.verbose > 0 {
                println!("Adding {} ({})", p, size);
            }

            if p.len() > 100 {
                p = p[..50].to_string() + &p[p.len() - 50..];
                eprint!("Warning: Path {} is too long. Triming.", p);
            }

            header.set_path(p)?;

            progress.update(TAR_HEADER_SIZE as _, src_path.display());
            if is_dir {
                header.set_size(0);
                header.set_cksum();
                tar.append(&header, std::io::empty())?;
            } else {
                let file = std::fs::File::open(&src_path)?;
                let mode = file.metadata()?.permissions().mode();
                let time = file.metadata()?.modified()?;
                header.set_size(size as u64);
                header.set_mode(mode);
                header.set_mtime(time.duration_since(std::time::UNIX_EPOCH)?.as_secs());
                header.set_cksum();
                tar.append(&header, progress.reader(src_path.display(), file))?;
            }
        }
        tar.finish()?;

        println!("\n\n{protocol}://{host}/{}/\n\n", code.code);
        drop(tar);
        drop(writer);
        handle_a.join().unwrap()?;
        Ok::<(), anyhow::Error>(())
    })
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

        if content_length == 0 {
            progress.total += 512;
            progress.total += file.header().size().unwrap_or(0);

            if cli.verbose > 0 {
                println!("New Total Size: {}", progress.total);
            }
        }

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

fn collect_files(root: &Path, out: &mut Vec<(PathBuf, usize, bool)>) -> anyhow::Result<()> {
    if root.is_dir() {
        out.push((root.to_path_buf(), 0, true));
        for entry in std::fs::read_dir(root)? {
            let entry = entry?;
            let path = entry.path();
            collect_files(&path, out)?;
        }
        Ok(())
    } else if root.is_file() {
        let len = std::fs::metadata(root)?.len() as usize;
        out.push((root.to_path_buf(), len, false));
        Ok(())
    } else {
        Err(anyhow::anyhow!("Invalid path: {}", root.display()))
    }
}

const DELETE_LINE: &str = "\x1B[2K\r";

struct ProgressBar {
    last_update: std::time::Instant,
    current: u64,
    last_progress: u64,
    total: u64,
}

struct ProgressReader<'a, D, R> {
    bar: &'a mut ProgressBar,
    display: D,
    inner: R,
}

impl<'a, D: Display, R: Read> Read for ProgressReader<'a, D, R> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let n = self.inner.read(buf)?;
        self.bar.update(n as u64, &self.display);
        Ok(n)
    }
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

    fn reader<D: Display, R: Read>(&mut self, display: D, inner: R) -> ProgressReader<D, R> {
        ProgressReader {
            bar: self,
            display,
            inner,
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
