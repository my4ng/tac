use tac_k::reverse_file;

use std::io::{BufWriter, IsTerminal, StdoutLock, Write};

fn version() {
    println!("Tack {}", env!("CARGO_PKG_VERSION"));
    println!("Copyright (c) 2024 Michael Yang <admin@my4ng.dev>");
    println!("Copyright (c) 2017 NeoSmart Technologies <https://neosmart.net/>");
    println!("Report bugs at <https://github.com/my4ng/tack>");
}

fn help() {
    version();
    println!();
    println!("Usage: tac [OPTIONS] [FILE1..]");
    println!("Write each FILE to standard output, last line first.");
    println!("Reads from stdin if FILE is - or not specified.");
    println!();
    println!("Options:");
    println!("  -h --help        Print this help text and exit");
    println!("  -v --version     Print version and exit.");
    println!("  --line-buffered  Always flush output after each line.");
}

enum Writer {
    StdOut(StdoutLock<'static>),
    Buffered(BufWriter<StdoutLock<'static>>),
}

impl Write for Writer {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        match self {
            Writer::StdOut(stdout) => stdout.write(buf),
            Writer::Buffered(buffered) => buffered.write(buf),
        }
    }

    fn flush(&mut self) -> std::io::Result<()> {
        match self {
            Writer::StdOut(stdout) => stdout.flush(),
            Writer::Buffered(buffered) => buffered.flush(),
        }
    }
}

fn main() {
    let args = std::env::args();
    // This is intentionally one more than what we might need, in case no arguments were provided
    // and we want to stub a "-" argument in there.
    let mut files = Vec::with_capacity(args.len());
    let mut force_flush = false;
    let mut skip_switches = false;
    for arg in args.skip(1) {
        if !skip_switches && arg.starts_with('-') && arg != "-" {
            match arg.as_str() {
                "-h" | "--help" => {
                    help();
                    std::process::exit(0);
                }
                "-v" | "--version" => {
                    version();
                    std::process::exit(0);
                }
                "--line-buffered" => {
                    force_flush = true;
                }
                "--" => {
                    skip_switches = true;
                    continue;
                }
                _ => {
                    eprintln!("{}: Invalid option!", arg);
                    eprintln!("Try 'tac --help' for more information");
                    std::process::exit(-1);
                }
            }
        } else {
            let file = arg;
            files.push(file)
        }
    }

    // Read from stdin by default
    if files.is_empty() {
        files.push("-".into());
    }

    let stdout = std::io::stdout().lock();
    let mut writer = if force_flush || stdout.is_terminal() {
        Writer::StdOut(stdout)
    } else {
        Writer::Buffered(BufWriter::new(stdout))
    };

    for file in &files {
        if let Err(e) = reverse_file(&mut writer, file) {
            if e.kind() != std::io::ErrorKind::BrokenPipe {
                eprintln!("{}: {:?}", file, e);
                std::process::exit(-1);
            }
        }
    }
}
