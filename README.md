# institute-print

`institute-print` sends PDF and PostScript files straight to the institute Kyocera printer (ECOSYS P8060cdn) over LPD, with your student ID as the LPD user.

There is no printer driver, no print queue, no installer and no administrator step. The printer renders PDF (PDF Direct Print) and PostScript (KPDL3) itself, so the tool only wraps the file in a short PJL header and uploads it. The same code runs on Windows, macOS and Linux.

For how the printing works and why no driver is needed, see [docs/printing-principles.md](docs/printing-principles.md).

## Requirements

- Rust toolchain with Cargo.
- Access to the school/internal network that can reach `172.20.83.212`.

## Configure your student ID

Edit [src/config.rs](src/config.rs):

```rust
pub const STUDENT_ID: &str = "00B00000";
```

Replace `00B00000` with your own student ID before building. The required format is `NNBNNNNN`; a wrong format makes `cargo check`, `cargo build` and `cargo test` fail before a binary is produced.

Do not commit a real student ID to a public repository. Keep the committed placeholder as `00B00000` and change it only in your local working tree.

The printer address and LPD queue (`lpd://172.20.83.212/kyocera`) are in the same file.

## Build

```sh
cargo build --release
```

The binary is `target/release/institute-print` (`institute-print.exe` on Windows). Copy it anywhere on your `PATH`; it needs no other files.

## Print

```sh
institute-print report.pdf
institute-print --duplex long --mono -n 2 slides.pdf handout.pdf
```

Options:

| Option | Meaning |
|---|---|
| `-n`, `--copies N` | Print N copies (default 1). Each copy is a separate upload. |
| `-d`, `--duplex MODE` | `long`, `short` or `off`. |
| `--mono` | Black and white. |
| `--color` | Color. |
| `--dry-run` | Show the LPD control file and PJL header that would be sent, without connecting. |
| `-h`, `--help` | Show help. |

Options you leave out are not sent, so the printer's own defaults apply.

Only PDF and PostScript files are accepted. To print from another application, save or export the document as PDF first (on Windows, "Microsoft Print to PDF" works for any application).

All files are checked before anything is sent; the tool then uploads them in order and stops at the first failure.

## Verify on the real printer

Two parts can only be confirmed by printing on the institute printer:

- `--duplex`, `--mono` and `--color` use the standard HP PJL variables `DUPLEX`, `BINDING` and `RENDERMODE`. Kyocera's PPD for this printer does not list them (it sets duplex and color with PostScript code), so check one test page with each option. If one has no effect, leave it out and the printer default applies.
- PDF files are sent with `@PJL ENTER LANGUAGE=PDF`. If a PDF prints as garbage or an error page, report it together with the `--dry-run` output.

## Source port

LPD servers may only accept jobs sent from a port below 1024, as RFC 1179 asks. The tool always tries such a port first. Windows and macOS allow this for normal users. On Linux it needs root (or `CAP_NET_BIND_SERVICE`); otherwise the tool prints a warning and sends from an ordinary port, which the printer may refuse.

## Run tests

```sh
cargo test
```

The tests cover argument parsing, file type detection, the PJL header, the LPD control file, and a full LPD upload against a local fake server.

## Security note

The fixed student ID is a convenience, not authentication. Anyone who can reach the printer can send any LPD user name, and anyone with the binary can read the student ID embedded in it. Do not treat it as a secret.
