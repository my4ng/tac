use clap::{command, crate_authors, crate_description, crate_version, Arg, ArgAction};
use tac_k::reverse_file;

use std::io::{BufWriter, IsTerminal, StdoutLock, Write};
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

const HELP_TEMPLATE: &str = "\
{name} ({version}) {author-with-newline}{about-with-newline}
{usage-heading} {usage}

{all-args}";

fn main() {
    #[allow(non_upper_case_globals)]
    let matches = command!()
        .name("tac")
        .about(crate_description!())
        .author(crate_authors!("\n"))
        .version(crate_version!())
        .help_template(HELP_TEMPLATE)
        .arg(
            Arg::new("force_flush")
                .long("line-buffered")
                .action(ArgAction::SetTrue)
                .help("Always flush output after each line"),
        )
        .arg(
            Arg::new("files")
                .value_name("FILE")
                .num_args(..)
                .help("Files to be reversed.\nRead from stdin if it is `-` or not specified."),
        )
        .get_matches();

    let force_flush = matches.get_flag("force_flush");
    let files = matches
        .get_many::<String>("files")
        .map_or_else(|| vec!["-"], |iter| iter.map(AsRef::as_ref).collect());

    let stdout = std::io::stdout().lock();
    let mut writer = if force_flush || stdout.is_terminal() {
        Writer::StdOut(stdout)
    } else {
        Writer::Buffered(BufWriter::new(stdout))
    };

    for file in files {
        if let Err(e) = reverse_file(&mut writer, file) {
            if e.kind() != std::io::ErrorKind::BrokenPipe {
                eprintln!("{}: {:?}", file, e);
                std::process::exit(-1);
            }
        }
    }
}
