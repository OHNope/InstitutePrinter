use printer_installer::config::STUDENT_ID;
use std::ffi::OsStr;
use std::fs::{self, File};
use std::io::{self, Write};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::time::{SystemTime, UNIX_EPOCH};

const PRINTER_NAME: &str = "InstitutePrinter";
const DEVICE_URI: &str = "titech-lpd://172.20.83.212/kyocera";
const BACKEND_NAME: &str = "titech-lpd";
const BACKEND_MODE: u32 = 0o700;
const PPD_DESCRIPTION: &str = "embedded Kyocera ECOSYS P8060cdn (KPDL) PPD";
const PPD_MODE: u32 = 0o644;
// Lines that point CUPS at the Kyocera driver software. They are kept only
// when that software is installed and runnable; otherwise CUPS uses its own
// pdftops/pstops filters, which already emit every PJL and PostScript option
// command defined in the PPD.
const KYOCERA_ONLY_PPD_KEYWORDS: &[&str] = &["*cupsPreFilter", "*cupsFilter", "*APDialogExtension"];
const INSTALLABLE_OPTIONS: &[(&str, &str)] = &[
    ("Option19", "False"),    // Paper feeders
    ("Option26", "False"),    // Job separator
    ("Option25", "False"),    // Mailbox
    ("Option21", "False"),    // Punch unit
    ("Option22", "False"),    // Folding unit
    ("Option17", "None"),     // Document finisher
    ("Option18", "HardDisk"), // Optional disk
];

const EMBEDDED_BACKEND: &[u8] = include_bytes!("../../assets/titech-lpd");
// Unmodified copy of the PPD shipped in Kyocera's macOS driver package.
const EMBEDDED_PPD: &[u8] = include_bytes!("../../assets/Kyocera ECOSYS P8060cdn.PPD");

extern "C" {
    fn geteuid() -> u32;
}

fn main() {
    if let Err(err) = run() {
        eprintln!("ERROR: {err}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), String> {
    require_root()?;

    let serverbin = cups_serverbin();
    let backend_dir = serverbin.join("backend");
    let backend_path = backend_dir.join(BACKEND_NAME);
    let filter_dir = serverbin.join("filter");
    let kyocera_driver = check_kyocera_driver(&kyocera_driver_files(EMBEDDED_PPD, &filter_dir));
    let rendering = match &kyocera_driver {
        Ok(()) => "Kyocera driver software (filters and print dialog plugin)".to_string(),
        Err(reason) => {
            format!("standard CUPS filters (Kyocera driver software not used: {reason})")
        }
    };

    debug_confirm(&format!("Is your student ID {}?", STUDENT_ID))?;

    debug_confirm(&format!(
        "Install embedded CUPS backend to {}",
        backend_path.display()
    ))?;
    install_file_atomic(&backend_path, EMBEDDED_BACKEND, BACKEND_MODE)?;

    debug_confirm(&format!(
        "Remove existing printer queue {PRINTER_NAME}, ignoring failure"
    ))?;
    let lpadmin = command_path("/usr/sbin/lpadmin", "lpadmin");
    let cupsaccept = command_path("/usr/sbin/cupsaccept", "cupsaccept");
    let cupsenable = command_path("/usr/sbin/cupsenable", "cupsenable");

    let remove = Command::new(&lpadmin).arg("-x").arg(PRINTER_NAME).output();
    if let Ok(output) = remove {
        if !output.status.success() && cfg!(debug_assertions) {
            eprintln!(
                "WARN: ignoring failed removal of existing queue: {}",
                output_summary(&lpadmin, &["-x", PRINTER_NAME], &output)
            );
        }
    }

    debug_confirm(&format!(
        "Create printer queue {PRINTER_NAME} with URI {DEVICE_URI}, the {PPD_DESCRIPTION} and {rendering}"
    ))?;
    let ppd_path = temp_path_next_to(&std::env::temp_dir().join(format!("{PRINTER_NAME}.ppd")));
    let result = (|| {
        write_file(
            &ppd_path,
            &queue_ppd(EMBEDDED_PPD, kyocera_driver.is_ok()),
            PPD_MODE,
        )?;

        let mut create_args = vec![
            "-p".to_string(),
            PRINTER_NAME.to_string(),
            "-E".to_string(),
            "-v".to_string(),
            DEVICE_URI.to_string(),
            "-P".to_string(),
            path_str(&ppd_path)?.to_string(),
        ];
        for (name, value) in INSTALLABLE_OPTIONS {
            create_args.push("-o".to_string());
            create_args.push(format!("{name}={value}"));
        }
        run_command_owned(&lpadmin, &create_args)
    })();
    let _ = fs::remove_file(&ppd_path);
    result?;

    debug_confirm(&format!("Accept jobs for {PRINTER_NAME}"))?;
    run_command(&cupsaccept, &[PRINTER_NAME])?;

    debug_confirm(&format!("Enable printer {PRINTER_NAME}"))?;
    run_command(&cupsenable, &[PRINTER_NAME])?;

    println!("Installation completed.");
    println!("Printer name: {PRINTER_NAME}");
    println!("Backend: {BACKEND_NAME}");
    println!("Device URI: {DEVICE_URI}");
    println!("Fixed LPD user: {STUDENT_ID}");
    println!("PPD: {PPD_DESCRIPTION}");
    println!("Rendering: {rendering}");

    Ok(())
}

fn require_root() -> Result<(), String> {
    let euid = unsafe { geteuid() };
    if euid != 0 {
        return Err("Please run this installer with sudo.".to_string());
    }
    Ok(())
}

fn debug_confirm(action: &str) -> Result<(), String> {
    if !cfg!(debug_assertions) {
        return Ok(());
    }

    println!("About to: {action}");
    print!("Continue? [y/N] ");
    io::stdout()
        .flush()
        .map_err(|e| format!("failed to flush stdout: {e}"))?;

    let mut line = String::new();
    io::stdin()
        .read_line(&mut line)
        .map_err(|e| format!("failed to read confirmation: {e}"))?;

    match line.trim().to_ascii_lowercase().as_str() {
        "y" | "yes" => Ok(()),
        _ => Err("Aborted.".to_string()),
    }
}

fn cups_serverbin() -> PathBuf {
    for candidate in ["/usr/sbin/cups-config", "cups-config"] {
        if let Ok(output) = Command::new(candidate).arg("--serverbin").output() {
            if output.status.success() {
                let text = String::from_utf8_lossy(&output.stdout).trim().to_string();
                if !text.is_empty() {
                    return PathBuf::from(text);
                }
            }
        }
    }

    let fallback = default_cups_serverbin();
    eprintln!(
        "WARN: failed to determine CUPS serverbin with cups-config; using {}",
        fallback.display()
    );
    fallback
}

fn default_cups_serverbin() -> PathBuf {
    if cfg!(target_os = "linux") {
        PathBuf::from("/usr/lib/cups")
    } else {
        PathBuf::from("/usr/libexec/cups")
    }
}

fn is_kyocera_driver_line(line: &[u8]) -> bool {
    KYOCERA_ONLY_PPD_KEYWORDS
        .iter()
        .any(|keyword| line.starts_with(keyword.as_bytes()))
}

// Files the PPD's Kyocera-only lines refer to: filters inside the CUPS filter
// directory and the print dialog plugin bundle at an absolute path.
fn kyocera_driver_files(ppd: &[u8], filter_dir: &Path) -> Vec<PathBuf> {
    String::from_utf8_lossy(ppd)
        .lines()
        .filter(|line| is_kyocera_driver_line(line.as_bytes()))
        .filter_map(|line| {
            let value = line.split('"').nth(1)?;
            if line.starts_with("*APDialogExtension") {
                Some(PathBuf::from(value))
            } else {
                value
                    .split_whitespace()
                    .last()
                    .map(|name| filter_dir.join(name))
            }
        })
        .collect()
}

// Err explains why the Kyocera driver software cannot be used.
fn check_kyocera_driver(files: &[PathBuf]) -> Result<(), String> {
    if files.is_empty() {
        return Err("the PPD references no Kyocera driver files".to_string());
    }

    for path in files {
        if !path.exists() {
            return Err(format!("{} is not installed", path.display()));
        }

        // Started without arguments, a CUPS filter exits immediately. Failing
        // to start means this machine cannot run it, e.g. an Intel-only filter
        // on Apple Silicon without Rosetta. A file the kernel cannot execute at
        // all is retried through /bin/sh, which then exits with 126 or 127.
        if path.is_file() {
            let status = Command::new(path)
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status()
                .map_err(|e| format!("{} cannot run on this machine: {e}", path.display()))?;
            if matches!(status.code(), Some(126 | 127)) {
                return Err(format!(
                    "{} cannot run on this machine: not an executable for this system",
                    path.display()
                ));
            }
        }
    }

    Ok(())
}

fn queue_ppd(ppd: &[u8], use_kyocera_driver: bool) -> Vec<u8> {
    if use_kyocera_driver {
        return ppd.to_vec();
    }

    ppd.split_inclusive(|&byte| byte == b'\n')
        .filter(|line| !is_kyocera_driver_line(line))
        .flatten()
        .copied()
        .collect()
}

fn write_file(path: &Path, bytes: &[u8], mode: u32) -> Result<(), String> {
    fs::write(path, bytes).map_err(|e| format!("failed to write {}: {e}", path.display()))?;
    fs::set_permissions(path, fs::Permissions::from_mode(mode))
        .map_err(|e| format!("failed to chmod {:o} {}: {e}", mode, path.display()))
}

fn install_file_atomic(path: &Path, bytes: &[u8], mode: u32) -> Result<(), String> {
    let parent = path
        .parent()
        .ok_or_else(|| format!("{} has no parent directory", path.display()))?;
    fs::create_dir_all(parent)
        .map_err(|e| format!("failed to create {}: {e}", parent.display()))?;

    let temp_path = temp_path_next_to(path);
    let result = (|| {
        {
            let mut file = File::create(&temp_path)
                .map_err(|e| format!("failed to create {}: {e}", temp_path.display()))?;
            file.write_all(bytes)
                .map_err(|e| format!("failed to write {}: {e}", temp_path.display()))?;
            file.sync_all()
                .map_err(|e| format!("failed to sync {}: {e}", temp_path.display()))?;
        }

        fs::set_permissions(&temp_path, fs::Permissions::from_mode(mode))
            .map_err(|e| format!("failed to chmod {:o} {}: {e}", mode, temp_path.display()))?;

        chown_root(&temp_path)?;

        fs::rename(&temp_path, path).map_err(|e| {
            format!(
                "failed to rename {} to {}: {e}",
                temp_path.display(),
                path.display()
            )
        })?;

        fs::set_permissions(path, fs::Permissions::from_mode(mode))
            .map_err(|e| format!("failed to chmod {:o} {}: {e}", mode, path.display()))?;

        chown_root(path)?;

        Ok(())
    })();

    if temp_path.exists() {
        let _ = fs::remove_file(&temp_path);
    }

    result
}

fn chown_root(path: &Path) -> Result<(), String> {
    let chown = command_path("/usr/sbin/chown", "chown");
    run_command(&chown, &[root_owner_group(), path_str(path)?])?;
    Ok(())
}

fn root_owner_group() -> &'static str {
    if cfg!(target_os = "macos") {
        "root:wheel"
    } else {
        "root:root"
    }
}

fn temp_path_next_to(path: &Path) -> PathBuf {
    let pid = std::process::id();
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let name = path
        .file_name()
        .and_then(OsStr::to_str)
        .unwrap_or("install-file");
    path.with_file_name(format!(".{name}.{pid}.{nanos}.tmp"))
}

fn command_path(abs: &str, fallback: &str) -> PathBuf {
    if Path::new(abs).exists() {
        PathBuf::from(abs)
    } else {
        PathBuf::from(fallback)
    }
}

fn run_command(program: &Path, args: &[&str]) -> Result<Output, String> {
    let output = Command::new(program)
        .args(args)
        .output()
        .map_err(|e| format!("failed to execute {} {:?}: {e}", program.display(), args))?;

    if !output.status.success() {
        return Err(output_summary(program, args, &output));
    }

    Ok(output)
}

fn run_command_owned(program: &Path, args: &[String]) -> Result<Output, String> {
    let output = Command::new(program)
        .args(args)
        .output()
        .map_err(|e| format!("failed to execute {} {:?}: {e}", program.display(), args))?;

    if !output.status.success() {
        return Err(output_summary_owned(program, args, &output));
    }

    Ok(output)
}

fn output_summary(program: &Path, args: &[&str], output: &Output) -> String {
    let code = output
        .status
        .code()
        .map(|c| c.to_string())
        .unwrap_or_else(|| "terminated by signal".to_string());
    let stderr = String::from_utf8_lossy(&output.stderr);
    format!(
        "command failed: {} {:?}; exit code: {}; stderr: {}",
        program.display(),
        args,
        code,
        stderr.trim()
    )
}

fn output_summary_owned(program: &Path, args: &[String], output: &Output) -> String {
    let code = output
        .status
        .code()
        .map(|c| c.to_string())
        .unwrap_or_else(|| "terminated by signal".to_string());
    let stderr = String::from_utf8_lossy(&output.stderr);
    format!(
        "command failed: {} {:?}; exit code: {}; stderr: {}",
        program.display(),
        args,
        code,
        stderr.trim()
    )
}

fn path_str(path: &Path) -> Result<&str, String> {
    path.to_str()
        .ok_or_else(|| format!("path is not valid UTF-8: {}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backend_installs_as_root_only_backend() {
        assert_eq!(BACKEND_MODE, 0o700);
    }

    #[test]
    fn queue_ppd_drops_kyocera_driver_references() {
        let ppd = b"*NickName: \"Test\"\r\n\
*cupsPreFilter: \"application/pdf 0 kyoprefilter_F\"\r\n\
*cupsFilter: \"application/vnd.cups-postscript 0 kyofilter_F\"\r\n\
*APDialogExtension: \"/Library/Printers/Kyocera/PDEs/KfSummary.plugin\"\r\n\
*Duplex None/None: \"statusdict begin false setduplexmode false settumble end\"\r\n";

        assert_eq!(queue_ppd(ppd, true), ppd.to_vec());
        assert_eq!(
            queue_ppd(ppd, false),
            b"*NickName: \"Test\"\r\n\
*Duplex None/None: \"statusdict begin false setduplexmode false settumble end\"\r\n"
                .to_vec()
        );
    }

    #[test]
    fn embedded_ppd_needs_no_kyocera_software() {
        let ppd = String::from_utf8_lossy(EMBEDDED_PPD);
        assert!(ppd.contains("*NickName: \"Kyocera ECOSYS P8060cdn (KPDL)\""));
        assert!(ppd.contains("kyofilter_F"));

        let queue = String::from_utf8_lossy(&queue_ppd(EMBEDDED_PPD, false)).into_owned();
        assert!(!queue.contains("kyofilter_F"));
        assert!(!queue.contains("kyoprefilter_F"));
        assert!(!queue.contains("/Library/Printers/Kyocera"));
    }

    #[test]
    fn kyocera_driver_files_lists_filters_and_plugin() {
        assert_eq!(
            kyocera_driver_files(EMBEDDED_PPD, Path::new("/cups/filter")),
            vec![
                PathBuf::from("/cups/filter/kyoprefilter_F"),
                PathBuf::from("/cups/filter/kyofilter_F"),
                PathBuf::from("/Library/Printers/Kyocera/PDEs/KfSummary.plugin"),
            ]
        );
    }

    #[test]
    fn check_kyocera_driver_requires_installed_runnable_files() {
        let dir = std::env::temp_dir().join(format!("installer-test-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join("Driver.plugin")).unwrap();
        write_file(&dir.join("filter"), b"#!/bin/sh\nexit 255\n", 0o755).unwrap();
        write_file(
            &dir.join("foreign"),
            b"\xcf\xfa\xed\xfe not runnable",
            0o755,
        )
        .unwrap();

        let installed = vec![dir.join("filter"), dir.join("Driver.plugin")];
        let missing = vec![dir.join("filter"), dir.join("Missing.plugin")];
        let foreign = vec![dir.join("foreign"), dir.join("Driver.plugin")];

        assert_eq!(check_kyocera_driver(&installed), Ok(()));
        assert!(check_kyocera_driver(&missing)
            .unwrap_err()
            .contains("is not installed"));
        assert!(check_kyocera_driver(&foreign)
            .unwrap_err()
            .contains("cannot run on this machine"));
        assert!(check_kyocera_driver(&[]).is_err());

        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn installable_options_exist_in_embedded_ppd() {
        let ppd = String::from_utf8_lossy(EMBEDDED_PPD);
        for (name, value) in INSTALLABLE_OPTIONS {
            assert!(
                ppd.contains(&format!("*{name} {value}/")),
                "missing *{name} {value}"
            );
        }
    }
}
