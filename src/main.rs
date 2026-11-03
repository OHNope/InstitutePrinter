mod config;
mod lpd;
mod pjl;

use config::{PRINTER_HOST, PRINTER_QUEUE, STUDENT_ID};
use pjl::{ColorMode, Duplex, Language, Settings};
use std::env;
use std::ffi::OsString;
use std::fs::File;
use std::io::Read;
use std::net::{SocketAddr, ToSocketAddrs};
use std::path::{Path, PathBuf};

fn main() {
    let options = match parse_args(env::args_os().skip(1)) {
        Ok(Request::Print(options)) => options,
        Ok(Request::Help) => {
            print!("{}", usage());
            return;
        }
        Err(err) => {
            eprintln!("ERROR: {err}\n");
            eprint!("{}", usage());
            std::process::exit(2);
        }
    };

    if let Err(err) = run(&options) {
        eprintln!("ERROR: {err}");
        std::process::exit(1);
    }
}

fn usage() -> String {
    format!(
        "\
Usage: institute-print [options] FILE...

Sends PDF or PostScript files straight to the institute printer at
lpd://{PRINTER_HOST}/{PRINTER_QUEUE} with LPD user {STUDENT_ID}.
No printer driver is involved: the printer renders the files itself.

Options:
  -n, --copies N      print N copies (default 1)
  -d, --duplex MODE   long, short or off (default: printer setting)
      --mono          print in black and white
      --color         print in color
      --dry-run       show what would be sent without connecting
  -h, --help          show this help
"
    )
}

#[derive(Debug, PartialEq)]
enum Request {
    Help,
    Print(Options),
}

#[derive(Debug, PartialEq)]
struct Options {
    copies: u32,
    settings: Settings,
    dry_run: bool,
    files: Vec<PathBuf>,
}

fn parse_args(args: impl IntoIterator<Item = OsString>) -> Result<Request, String> {
    let mut options = Options {
        copies: 1,
        settings: Settings::default(),
        dry_run: false,
        files: Vec::new(),
    };
    let mut args = args.into_iter();
    let mut only_files = false;

    while let Some(arg) = args.next() {
        if only_files {
            options.files.push(arg.into());
            continue;
        }
        match arg.to_str() {
            Some("-h" | "--help") => return Ok(Request::Help),
            Some("-n" | "--copies") => {
                options.copies = parse_copies(&value(&mut args, "--copies")?)?
            }
            Some("-d" | "--duplex") => {
                options.settings.duplex = Some(parse_duplex(&value(&mut args, "--duplex")?)?)
            }
            Some("--mono") => set_color(&mut options.settings, ColorMode::Mono)?,
            Some("--color") => set_color(&mut options.settings, ColorMode::Color)?,
            Some("--dry-run") => options.dry_run = true,
            Some("--") => only_files = true,
            Some(flag) if flag.starts_with('-') => return Err(format!("unknown option {flag}")),
            _ => options.files.push(arg.into()),
        }
    }

    if options.files.is_empty() {
        return Err("no files given".to_string());
    }
    Ok(Request::Print(options))
}

fn value(args: &mut impl Iterator<Item = OsString>, option: &str) -> Result<String, String> {
    args.next()
        .and_then(|value| value.into_string().ok())
        .ok_or_else(|| format!("{option} needs a value"))
}

fn parse_copies(text: &str) -> Result<u32, String> {
    match text.parse::<u32>() {
        Ok(copies) if copies >= 1 => Ok(copies),
        _ => Err(format!("invalid number of copies: {text}")),
    }
}

fn parse_duplex(text: &str) -> Result<Duplex, String> {
    match text {
        "long" => Ok(Duplex::LongEdge),
        "short" => Ok(Duplex::ShortEdge),
        "off" => Ok(Duplex::Off),
        _ => Err(format!(
            "invalid duplex mode {text}: use long, short or off"
        )),
    }
}

fn set_color(settings: &mut Settings, mode: ColorMode) -> Result<(), String> {
    if settings.color.is_some_and(|current| current != mode) {
        return Err("--mono and --color cannot be combined".to_string());
    }
    settings.color = Some(mode);
    Ok(())
}

fn run(options: &Options) -> Result<(), String> {
    // Check every file before sending anything.
    let documents = options
        .files
        .iter()
        .map(|path| Document::open(path, &options.settings))
        .collect::<Result<Vec<_>, _>>()?;
    let host = lpd::local_hostname();
    let mut number = lpd::first_job_number();

    if options.dry_run {
        for document in &documents {
            let job = document.job(number, &host);
            document.describe(&job, options.copies);
        }
        return Ok(());
    }

    let server = printer_address()?;
    // Each copy is a separate upload of one copy, as the CUPS backend this
    // tool replaces did, so the printer never has to interpret a copy count.
    for document in &documents {
        for copy in 1..=options.copies {
            let job = document.job(number, &host);
            number = (number + 1) % 1000;
            document.send(server, &job)?;
            println!(
                "Sent {} (copy {copy} of {}) to lpd://{PRINTER_HOST}/{PRINTER_QUEUE} as {STUDENT_ID}",
                document.path.display(),
                options.copies
            );
        }
    }
    Ok(())
}

fn printer_address() -> Result<SocketAddr, String> {
    (PRINTER_HOST, lpd::PORT)
        .to_socket_addrs()
        .map_err(|e| format!("cannot resolve {PRINTER_HOST}: {e}"))?
        .next()
        .ok_or_else(|| format!("cannot resolve {PRINTER_HOST}"))
}

struct Document {
    path: PathBuf,
    title: String,
    language: Language,
    header: Vec<u8>,
}

impl Document {
    fn open(path: &Path, settings: &Settings) -> Result<Document, String> {
        let file = File::open(path).map_err(|e| format!("cannot open {}: {e}", path.display()))?;
        let mut head = Vec::new();
        file.take(8)
            .read_to_end(&mut head)
            .map_err(|e| format!("cannot read {}: {e}", path.display()))?;
        let language = Language::detect(&head).ok_or_else(|| {
            format!(
                "{} is not a PDF or PostScript file; export it to PDF first",
                path.display()
            )
        })?;
        let name = path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default();

        Ok(Document {
            path: path.to_path_buf(),
            title: lpd::sanitize(&name),
            language,
            header: pjl::header(language, settings),
        })
    }

    fn job<'a>(&'a self, number: u16, host: &'a str) -> lpd::Job<'a> {
        lpd::Job {
            number,
            host,
            user: STUDENT_ID,
            title: &self.title,
        }
    }

    fn send(&self, server: SocketAddr, job: &lpd::Job) -> Result<(), String> {
        let file = File::open(&self.path)
            .map_err(|e| format!("cannot open {}: {e}", self.path.display()))?;
        let file_len = file
            .metadata()
            .map_err(|e| format!("cannot read {}: {e}", self.path.display()))?
            .len();
        let data_len = self.header.len() as u64 + file_len + pjl::JOB_END.len() as u64;
        let mut data = self
            .header
            .as_slice()
            .chain(file.take(file_len))
            .chain(pjl::JOB_END);

        lpd::send(server, PRINTER_QUEUE, job, &mut data, data_len)
            .map_err(|e| format!("{}: {e}", self.path.display()))
    }

    fn describe(&self, job: &lpd::Job, copies: u32) {
        println!(
            "{}: {}, {copies} upload(s) to lpd://{PRINTER_HOST}/{PRINTER_QUEUE}",
            self.path.display(),
            self.language.name()
        );
        println!("  LPD control file:");
        for line in job.control_file().lines() {
            println!("    {line}");
        }
        println!("  PJL header:");
        for line in String::from_utf8_lossy(&self.header).lines() {
            println!("    {}", line.replace('\x1b', "<ESC>"));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(args: &[&str]) -> Result<Request, String> {
        parse_args(args.iter().map(OsString::from))
    }

    #[test]
    fn parse_args_defaults_to_one_copy_and_printer_settings() {
        assert_eq!(
            parse(&["a.pdf", "b.ps"]),
            Ok(Request::Print(Options {
                copies: 1,
                settings: Settings::default(),
                dry_run: false,
                files: vec![PathBuf::from("a.pdf"), PathBuf::from("b.ps")],
            }))
        );
    }

    #[test]
    fn parse_args_reads_all_options() {
        assert_eq!(
            parse(&[
                "-n",
                "3",
                "--duplex",
                "short",
                "--mono",
                "--dry-run",
                "a.pdf"
            ]),
            Ok(Request::Print(Options {
                copies: 3,
                settings: Settings {
                    duplex: Some(Duplex::ShortEdge),
                    color: Some(ColorMode::Mono),
                },
                dry_run: true,
                files: vec![PathBuf::from("a.pdf")],
            }))
        );
    }

    #[test]
    fn parse_args_treats_everything_after_double_dash_as_files() {
        let Ok(Request::Print(options)) = parse(&["--", "--mono", "-n"]) else {
            panic!("expected a print request");
        };
        assert_eq!(
            options.files,
            vec![PathBuf::from("--mono"), PathBuf::from("-n")]
        );
        assert_eq!(options.settings, Settings::default());
    }

    #[test]
    fn parse_args_rejects_bad_input() {
        assert_eq!(parse(&[]), Err("no files given".to_string()));
        assert!(parse(&["-n", "0", "a.pdf"]).is_err());
        assert!(parse(&["-d", "both", "a.pdf"]).is_err());
        assert!(parse(&["--mono", "--color", "a.pdf"]).is_err());
        assert!(parse(&["--duplex"]).is_err());
        assert!(parse(&["-x", "a.pdf"]).is_err());
        assert_eq!(parse(&["a.pdf", "--help"]), Ok(Request::Help));
    }

    #[test]
    fn document_open_rejects_other_file_types() {
        let path = env::temp_dir().join(format!("institute-print-test-{}.txt", std::process::id()));
        std::fs::write(&path, "plain text").unwrap();
        let result = Document::open(&path, &Settings::default());
        let _ = std::fs::remove_file(&path);
        let err = result.err().expect("a text file must be rejected");
        assert!(err.contains("is not a PDF or PostScript file"));
    }
}
