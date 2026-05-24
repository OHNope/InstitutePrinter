use printer_installer::config::STUDENT_ID;
use std::ffi::OsStr;
use std::fs::{self, File};
use std::io::{self, Write};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::time::{SystemTime, UNIX_EPOCH};

const PRINTER_NAME: &str = "InstitutePrinter";
const DEVICE_URI: &str = "titech-lpd://172.20.83.212/kyocera";
const BACKEND_NAME: &str = "titech-lpd";
const BACKEND_MODE: u32 = 0o700;
const PPD_MODEL_MARKER: &str = "Kyocera ECOSYS P8060cdn";
const PPD_DRIVER_MARKER: &str = "KPDL";
const OLD_EMBEDDED_PPD_NAME: &str = "Kyocera_ECOSYS_P8060cdn.ppd";
const REQUIRED_FILTERS: &[&str] = &["kyofilter_F", "kyoprefilter_F"];
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
    let ppd_model = find_kyocera_ppd_model()?;

    require_kyocera_filters(&filter_dir)?;

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
        "Create printer queue {PRINTER_NAME} with URI {DEVICE_URI} and model {ppd_model}"
    ))?;
    let mut create_args = vec![
        "-p".to_string(),
        PRINTER_NAME.to_string(),
        "-E".to_string(),
        "-v".to_string(),
        DEVICE_URI.to_string(),
        "-m".to_string(),
        ppd_model.clone(),
    ];
    for (name, value) in INSTALLABLE_OPTIONS {
        create_args.push("-o".to_string());
        create_args.push(format!("{name}={value}"));
    }
    run_command_owned(&lpadmin, &create_args)?;

    debug_confirm(&format!("Accept jobs for {PRINTER_NAME}"))?;
    run_command(&cupsaccept, &[PRINTER_NAME])?;

    debug_confirm(&format!("Enable printer {PRINTER_NAME}"))?;
    run_command(&cupsenable, &[PRINTER_NAME])?;

    println!("Installation completed.");
    println!("Printer name: {PRINTER_NAME}");
    println!("Backend: {BACKEND_NAME}");
    println!("Device URI: {DEVICE_URI}");
    println!("Fixed LPD user: {STUDENT_ID}");
    println!("PPD model: {ppd_model}");

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

fn require_kyocera_filters(filter_dir: &Path) -> Result<(), String> {
    let missing: Vec<String> = REQUIRED_FILTERS
        .iter()
        .map(|name| filter_dir.join(name))
        .filter(|path| !path.exists())
        .map(|path| path.display().to_string())
        .collect();

    if missing.is_empty() {
        return Ok(());
    }

    Err(format!(
        "Kyocera driver filters are not installed. Missing: {}. Install Kyocera driver software for ECOSYS P8060cdn, then run this installer again.",
        missing.join(", ")
    ))
}

fn find_kyocera_ppd_model() -> Result<String, String> {
    let lpinfo = command_path("/usr/sbin/lpinfo", "lpinfo");
    let output = run_command(&lpinfo, &["-m"])?;
    let stdout = String::from_utf8_lossy(&output.stdout);

    parse_ppd_model(&stdout).ok_or_else(|| {
        format!(
            "Kyocera ECOSYS P8060cdn KPDL PPD is not installed. Install Kyocera driver software for ECOSYS P8060cdn, then run this installer again."
        )
    })
}

fn parse_ppd_model(lpinfo_output: &str) -> Option<String> {
    let model_marker = PPD_MODEL_MARKER.to_ascii_lowercase();
    let driver_marker = PPD_DRIVER_MARKER.to_ascii_lowercase();

    lpinfo_output.lines().find_map(|line| {
        let lower = line.to_ascii_lowercase();
        if !lower.contains(&model_marker) || !lower.contains(&driver_marker) {
            return None;
        }
        if lower.contains(&OLD_EMBEDDED_PPD_NAME.to_ascii_lowercase()) {
            return None;
        }

        ppd_model_name(line)
    })
}

fn ppd_model_name(line: &str) -> Option<String> {
    for suffix in [".PPD", ".ppd"] {
        if let Some(index) = line.find(suffix) {
            return Some(line[..index + suffix.len()].to_string());
        }
    }

    line.split_whitespace().next().map(str::to_string)
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
    fn parse_ppd_model_finds_kyocera_kpdl_entry() {
        let lpinfo_output = "\
drv:///sample.drv/generic.ppd Generic PostScript Printer
Library/Printers/PPDs/Contents/Resources/Kyocera ECOSYS P8060cdn.PPD Kyocera ECOSYS P8060cdn (KPDL)
";

        assert_eq!(
            parse_ppd_model(lpinfo_output),
            Some(
                "Library/Printers/PPDs/Contents/Resources/Kyocera ECOSYS P8060cdn.PPD".to_string()
            )
        );
    }

    #[test]
    fn parse_ppd_model_ignores_non_kpdl_entries() {
        let lpinfo_output = "\
drv:///sample.drv/generpcl.ppd Generic PCL Laser Printer
Library/Printers/PPDs/Contents/Resources/Kyocera ECOSYS P8060cdn.PPD Kyocera ECOSYS P8060cdn
";

        assert_eq!(parse_ppd_model(lpinfo_output), None);
    }

    #[test]
    fn parse_ppd_model_ignores_old_embedded_ppd() {
        let lpinfo_output = "\
Library/Printers/PPDs/Contents/Resources/Kyocera_ECOSYS_P8060cdn.ppd Kyocera ECOSYS P8060cdn KPDL
";

        assert_eq!(parse_ppd_model(lpinfo_output), None);
    }
}
