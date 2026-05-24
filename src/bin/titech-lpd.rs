use printer_installer::config::STUDENT_ID;
use std::env;
use std::fs::{self, File};
use std::io::{self, Write};
use std::path::PathBuf;
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

const BACKEND_SCHEME: &str = "titech-lpd://";
const DELEGATE_SCHEME: &str = "lpd://";

fn main() {
    match run() {
        Ok(code) => std::process::exit(code),
        Err(err) => {
            eprintln!("ERROR: titech-lpd: {err}");
            std::process::exit(1);
        }
    }
}

fn run() -> Result<i32, String> {
    let args: Vec<String> = env::args().collect();

    if args.len() == 1 {
        println!(r#"network titech-lpd "Unknown" "TiTech fixed-user LPD backend""#);
        return Ok(0);
    }

    if args.len() != 6 && args.len() != 7 {
        return Err(format!(
            "expected 5 or 6 CUPS arguments, got {}",
            args.len().saturating_sub(1)
        ));
    }

    let device_uri = env::var("DEVICE_URI").map_err(|_| "DEVICE_URI is not set".to_string())?;
    let delegate_uri = to_lpd_uri(&device_uri)?;
    let delegate = lpd_backend_path();
    let copies = parse_copies(&args[4])?;
    let stdin_file = if args.len() == 6 && copies > 1 {
        Some(spool_stdin_to_temp()?)
    } else {
        None
    };

    eprintln!(
        "INFO: titech-lpd: delegating to {} with fixed user {} and {} upload(s)",
        delegate.display(),
        STUDENT_ID,
        copies
    );

    let result = (|| {
        for copy_index in 0..copies {
            let delegate_args = delegate_args(&args, copy_index, stdin_file.as_ref());
            let status = Command::new(&delegate)
                .args(&delegate_args)
                .env("DEVICE_URI", &delegate_uri)
                .status()
                .map_err(|e| format!("failed to execute {}: {e}", delegate.display()))?;

            let code = status.code().unwrap_or(1);
            if code != 0 {
                return Ok(code);
            }
        }

        Ok(0)
    })();

    if let Some(path) = stdin_file {
        let _ = fs::remove_file(path);
    }

    result
}

fn to_lpd_uri(uri: &str) -> Result<String, String> {
    let rest = uri
        .strip_prefix(BACKEND_SCHEME)
        .ok_or_else(|| format!("unsupported DEVICE_URI scheme: {uri}"))?;
    Ok(format!("{DELEGATE_SCHEME}{rest}"))
}

fn lpd_backend_path() -> PathBuf {
    if let Ok(serverbin) = env::var("CUPS_SERVERBIN") {
        let path = PathBuf::from(serverbin).join("backend/lpd");
        if path.exists() {
            return path;
        }
    }

    for candidate in default_lpd_backend_paths() {
        if candidate.exists() {
            return candidate;
        }
    }

    default_lpd_backend_paths()
        .into_iter()
        .next()
        .unwrap_or_else(|| PathBuf::from("/usr/libexec/cups/backend/lpd"))
}

fn default_lpd_backend_paths() -> Vec<PathBuf> {
    if cfg!(target_os = "linux") {
        vec![
            PathBuf::from("/usr/lib/cups/backend/lpd"),
            PathBuf::from("/usr/libexec/cups/backend/lpd"),
        ]
    } else {
        vec![
            PathBuf::from("/usr/libexec/cups/backend/lpd"),
            PathBuf::from("/usr/lib/cups/backend/lpd"),
        ]
    }
}

fn parse_copies(text: &str) -> Result<u32, String> {
    let copies = text
        .parse::<u32>()
        .map_err(|e| format!("invalid copies value {text}: {e}"))?;
    if copies == 0 {
        return Err("copies value must be at least 1".to_string());
    }
    Ok(copies)
}

fn delegate_args(args: &[String], copy_index: u32, stdin_file: Option<&PathBuf>) -> Vec<String> {
    let mut delegate_args = Vec::with_capacity(args.len() - 1 + usize::from(stdin_file.is_some()));
    delegate_args.push(copy_job_id(&args[1], copy_index));
    delegate_args.push(STUDENT_ID.to_string());
    delegate_args.push(args[3].clone());
    delegate_args.push("1".to_string());
    delegate_args.push(args[5].clone());

    if let Some(path) = stdin_file {
        delegate_args.push(path.to_string_lossy().into_owned());
    } else if args.len() == 7 {
        delegate_args.push(args[6].clone());
    }

    delegate_args
}

fn copy_job_id(job_id: &str, copy_index: u32) -> String {
    if copy_index == 0 {
        return job_id.to_string();
    }

    match job_id.parse::<u32>() {
        Ok(n) => n.saturating_add(copy_index).to_string(),
        Err(_) => job_id.to_string(),
    }
}

fn spool_stdin_to_temp() -> Result<PathBuf, String> {
    let path = temp_path();
    let mut file =
        File::create(&path).map_err(|e| format!("failed to create {}: {e}", path.display()))?;
    io::copy(&mut io::stdin(), &mut file)
        .map_err(|e| format!("failed to spool stdin to {}: {e}", path.display()))?;
    file.flush()
        .map_err(|e| format!("failed to flush {}: {e}", path.display()))?;
    Ok(path)
}

fn temp_path() -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    env::temp_dir().join(format!("titech-lpd-{}-{nanos}.print", std::process::id()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn delegate_args_force_fixed_user_and_single_copy() {
        let args = vec![
            "titech-lpd".to_string(),
            "42".to_string(),
            "cups-user".to_string(),
            "title".to_string(),
            "3".to_string(),
            "Duplex=None".to_string(),
            "/tmp/input.ps".to_string(),
        ];

        assert_eq!(
            delegate_args(&args, 0, None),
            vec![
                "42",
                STUDENT_ID,
                "title",
                "1",
                "Duplex=None",
                "/tmp/input.ps"
            ]
        );
    }

    #[test]
    fn delegate_args_uses_spooled_stdin_file() {
        let args = vec![
            "titech-lpd".to_string(),
            "42".to_string(),
            "cups-user".to_string(),
            "title".to_string(),
            "2".to_string(),
            "ColorModel=Gray".to_string(),
        ];
        let path = PathBuf::from("/tmp/spooled.ps");

        assert_eq!(
            delegate_args(&args, 1, Some(&path)),
            vec![
                "43",
                STUDENT_ID,
                "title",
                "1",
                "ColorModel=Gray",
                "/tmp/spooled.ps"
            ]
        );
    }

    #[test]
    fn parse_copies_rejects_zero() {
        assert_eq!(parse_copies("1"), Ok(1));
        assert!(parse_copies("0").is_err());
    }

    #[test]
    fn default_lpd_backend_paths_include_macos_and_linux_locations() {
        let paths = default_lpd_backend_paths();
        assert!(paths.contains(&PathBuf::from("/usr/libexec/cups/backend/lpd")));
        assert!(paths.contains(&PathBuf::from("/usr/lib/cups/backend/lpd")));
    }
}
