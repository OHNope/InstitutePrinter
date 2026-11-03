// Minimal RFC 1179 (LPD) client. It sends one job per connection the way the
// CUPS lpd backend does by default: control file first, then the data file,
// printed as-is ("l"), with the LPD user field ("P") chosen by the caller.

use socket2::{Domain, Protocol, Socket, Type};
use std::io::{self, BufRead, BufReader, Read, Write};
use std::net::{Ipv4Addr, Ipv6Addr, SocketAddr, TcpStream};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

pub const PORT: u16 = 515;
const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
const IO_TIMEOUT: Duration = Duration::from_secs(60);

pub struct Job<'a> {
    pub number: u16,
    pub host: &'a str,
    pub user: &'a str,
    pub title: &'a str,
}

impl Job<'_> {
    // Shared tail of the control and data file names, e.g. "A042myhost".
    fn file_suffix(&self) -> String {
        format!("A{:03}{}", self.number % 1000, truncate(self.host, 15))
    }

    // Field length limits are the ones in RFC 1179 section 7.2.
    pub fn control_file(&self) -> String {
        let suffix = self.file_suffix();
        format!(
            "H{}\nP{}\nJ{}\nldf{suffix}\nUdf{suffix}\nN{}\n",
            truncate(self.host, 31),
            truncate(self.user, 31),
            truncate(self.title, 99),
            truncate(self.title, 131),
        )
    }
}

pub fn send(
    server: SocketAddr,
    queue: &str,
    job: &Job,
    data: &mut dyn Read,
    data_len: u64,
) -> Result<(), String> {
    let stream = connect(server).map_err(|e| format!("cannot connect to {server}: {e}"))?;
    let mut replies = BufReader::new(
        stream
            .try_clone()
            .map_err(|e| format!("cannot use connection to {server}: {e}"))?,
    );
    let mut stream = stream;
    let suffix = job.file_suffix();
    let mut control = job.control_file().into_bytes();
    let control_len = control.len();
    control.push(0);

    let mut step = |bytes: &[u8], name: &str| command(&mut stream, &mut replies, bytes, name);
    step(format!("\x02{queue}\n").as_bytes(), "receive job")?;
    step(
        format!("\x02{control_len} cf{suffix}\n").as_bytes(),
        "control file header",
    )?;
    step(&control, "control file")?;
    step(
        format!("\x03{data_len} df{suffix}\n").as_bytes(),
        "data file header",
    )?;

    let sent =
        io::copy(data, &mut stream).map_err(|e| format!("failed to send print data: {e}"))?;
    if sent != data_len {
        return Err(format!(
            "print data changed while sending: {sent} bytes instead of {data_len}"
        ));
    }
    command(&mut stream, &mut replies, &[0], "print data")
}

fn command(
    stream: &mut TcpStream,
    replies: &mut BufReader<TcpStream>,
    bytes: &[u8],
    name: &str,
) -> Result<(), String> {
    stream
        .write_all(bytes)
        .map_err(|e| format!("failed to send {name}: {e}"))?;
    let reply = replies
        .fill_buf()
        .map_err(|e| format!("no reply to {name}: {e}"))?;
    match reply.first().copied() {
        Some(0) => {
            replies.consume(1);
            Ok(())
        }
        Some(code) => Err(format!("printer rejected {name} (reply code {code})")),
        None => Err(format!("printer closed the connection after {name}")),
    }
}

// RFC 1179 expects clients to send from a port below 1024. The CUPS lpd
// backend runs as root and takes the first free port counting down from
// 1023, so this does the same. Windows and macOS let any user bind these
// ports; Linux does not, so there it falls back to an ordinary port.
fn connect(server: SocketAddr) -> io::Result<TcpStream> {
    for port in (512..=1023).rev() {
        let socket = new_socket(server)?;
        match socket.bind(&local_address(server, port).into()) {
            Ok(()) => {}
            Err(e) if e.kind() == io::ErrorKind::PermissionDenied => break,
            Err(_) => continue,
        }
        match socket.connect_timeout(&server.into(), CONNECT_TIMEOUT) {
            Ok(()) => return into_stream(socket),
            // The port may still be in TIME_WAIT from an earlier job.
            Err(e)
                if matches!(
                    e.kind(),
                    io::ErrorKind::AddrInUse | io::ErrorKind::AddrNotAvailable
                ) =>
            {
                continue
            }
            Err(e) => return Err(e),
        }
    }

    eprintln!("WARN: cannot send from a port below 1024; the printer may refuse the job");
    let socket = new_socket(server)?;
    socket.connect_timeout(&server.into(), CONNECT_TIMEOUT)?;
    into_stream(socket)
}

fn new_socket(server: SocketAddr) -> io::Result<Socket> {
    Socket::new(
        Domain::for_address(server),
        Type::STREAM,
        Some(Protocol::TCP),
    )
}

fn local_address(server: SocketAddr, port: u16) -> SocketAddr {
    match server {
        SocketAddr::V4(_) => (Ipv4Addr::UNSPECIFIED, port).into(),
        SocketAddr::V6(_) => (Ipv6Addr::UNSPECIFIED, port).into(),
    }
}

fn into_stream(socket: Socket) -> io::Result<TcpStream> {
    let stream = TcpStream::from(socket);
    stream.set_read_timeout(Some(IO_TIMEOUT))?;
    stream.set_write_timeout(Some(IO_TIMEOUT))?;
    Ok(stream)
}

// LPD fields are single lines of ASCII, so anything else becomes "_".
pub fn sanitize(text: &str) -> String {
    text.chars()
        .map(|c| {
            if c == ' ' || c.is_ascii_graphic() {
                c
            } else {
                '_'
            }
        })
        .collect()
}

fn truncate(text: &str, max_chars: usize) -> String {
    text.chars().take(max_chars).collect()
}

pub fn local_hostname() -> String {
    let name: String = raw_hostname()
        .unwrap_or_default()
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '.')
        .collect();
    if name.is_empty() {
        "localhost".to_string()
    } else {
        name
    }
}

#[cfg(unix)]
fn raw_hostname() -> Option<String> {
    extern "C" {
        fn gethostname(name: *mut std::ffi::c_char, len: usize) -> std::ffi::c_int;
    }

    let mut buf = [0u8; 256];
    if unsafe { gethostname(buf.as_mut_ptr().cast(), buf.len()) } != 0 {
        return None;
    }
    let end = buf.iter().position(|&b| b == 0).unwrap_or(buf.len());
    Some(String::from_utf8_lossy(&buf[..end]).into_owned())
}

#[cfg(windows)]
fn raw_hostname() -> Option<String> {
    std::env::var("COMPUTERNAME").ok()
}

// LPD job numbers have three digits. Start somewhere unlikely to repeat a
// recent run and count up from there for each upload.
pub fn first_job_number() -> u16 {
    let micros = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.subsec_micros())
        .unwrap_or(0);
    (micros.wrapping_add(std::process::id()) % 1000) as u16
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::TcpListener;
    use std::thread;

    fn job() -> Job<'static> {
        Job {
            number: 42,
            host: "myhost",
            user: "00B12345",
            title: "report.pdf",
        }
    }

    #[test]
    fn control_file_sets_user_and_prints_data_file_as_is() {
        assert_eq!(
            job().control_file(),
            "Hmyhost\nP00B12345\nJreport.pdf\nldfA042myhost\nUdfA042myhost\nNreport.pdf\n"
        );
    }

    #[test]
    fn control_file_truncates_long_fields() {
        let long_host = "h".repeat(40);
        let long_title = "t".repeat(200);
        let job = Job {
            number: 7,
            host: &long_host,
            user: "00B12345",
            title: &long_title,
        };
        let control = job.control_file();
        let lines: Vec<&str> = control.lines().collect();
        assert_eq!(lines[0].len(), 1 + 31);
        assert_eq!(lines[2].len(), 1 + 99);
        assert_eq!(lines[3], format!("ldfA007{}", "h".repeat(15)));
        assert_eq!(lines[5].len(), 1 + 131);
    }

    #[test]
    fn sanitize_replaces_non_ascii_and_control_characters() {
        assert_eq!(sanitize("課題 3.pdf"), "__ 3.pdf");
        assert_eq!(sanitize("a\nb\tc"), "a_b_c");
    }

    #[test]
    fn local_hostname_is_never_empty() {
        let name = local_hostname();
        assert!(!name.is_empty());
        assert!(name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '.'));
    }

    #[test]
    fn first_job_number_has_three_digits() {
        assert!(first_job_number() < 1000);
    }

    fn read_line(reader: &mut BufReader<TcpStream>) -> String {
        let mut line = Vec::new();
        reader.read_until(b'\n', &mut line).unwrap();
        String::from_utf8(line).unwrap()
    }

    // Reads the file announced by a "\x02<len> cf..." or "\x03<len> df..."
    // subcommand, including its trailing NUL.
    fn read_file(reader: &mut BufReader<TcpStream>, header: &str) -> Vec<u8> {
        let len: usize = header[1..].split(' ').next().unwrap().parse().unwrap();
        let mut bytes = vec![0; len + 1];
        reader.read_exact(&mut bytes).unwrap();
        assert_eq!(bytes.pop(), Some(0));
        bytes
    }

    // Plays the LPD server side and returns the subcommand lines, the control
    // file and the data file it received.
    fn serve_one_job(listener: TcpListener) -> (Vec<String>, String, Vec<u8>) {
        let (mut conn, _) = listener.accept().unwrap();
        let mut reader = BufReader::new(conn.try_clone().unwrap());
        let mut lines = Vec::new();

        lines.push(read_line(&mut reader));
        conn.write_all(&[0]).unwrap();
        lines.push(read_line(&mut reader));
        conn.write_all(&[0]).unwrap();
        let control = String::from_utf8(read_file(&mut reader, &lines[1])).unwrap();
        conn.write_all(&[0]).unwrap();
        lines.push(read_line(&mut reader));
        conn.write_all(&[0]).unwrap();
        let data = read_file(&mut reader, &lines[2]);
        conn.write_all(&[0]).unwrap();
        (lines, control, data)
    }

    #[test]
    fn send_transfers_control_file_then_data_file() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let server = listener.local_addr().unwrap();
        let handle = thread::spawn(move || serve_one_job(listener));

        let data = b"%PDF-1.7 test data";
        send(server, "kyocera", &job(), &mut &data[..], data.len() as u64).unwrap();

        let (lines, control, received) = handle.join().unwrap();
        assert_eq!(
            lines,
            vec![
                "\x02kyocera\n".to_string(),
                format!("\x02{} cfA042myhost\n", control.len()),
                format!("\x03{} dfA042myhost\n", data.len()),
            ]
        );
        assert_eq!(control, job().control_file());
        assert_eq!(received, data);
    }

    #[test]
    fn send_reports_rejected_queue() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let server = listener.local_addr().unwrap();
        let handle = thread::spawn(move || {
            let (mut conn, _) = listener.accept().unwrap();
            let mut line = Vec::new();
            BufReader::new(conn.try_clone().unwrap())
                .read_until(b'\n', &mut line)
                .unwrap();
            conn.write_all(&[1]).unwrap();
        });

        let err = send(server, "nosuchqueue", &job(), &mut &b"x"[..], 1).unwrap_err();
        handle.join().unwrap();
        assert_eq!(err, "printer rejected receive job (reply code 1)");
    }

    #[test]
    fn send_rejects_data_shorter_than_announced() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let server = listener.local_addr().unwrap();
        let handle = thread::spawn(move || {
            let (mut conn, _) = listener.accept().unwrap();
            let mut reader = BufReader::new(conn.try_clone().unwrap());
            read_line(&mut reader);
            conn.write_all(&[0]).unwrap();
            let header = read_line(&mut reader);
            conn.write_all(&[0]).unwrap();
            read_file(&mut reader, &header);
            conn.write_all(&[0]).unwrap();
            read_line(&mut reader);
            conn.write_all(&[0]).unwrap();
            // The client gives up after the short data and closes.
            let mut rest = Vec::new();
            let _ = reader.read_to_end(&mut rest);
        });

        let err = send(server, "kyocera", &job(), &mut &b"short"[..], 100).unwrap_err();
        handle.join().unwrap();
        assert_eq!(
            err,
            "print data changed while sending: 5 bytes instead of 100"
        );
    }
}
