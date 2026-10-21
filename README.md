# printer-installer

`printer-installer` builds and installs a CUPS printer queue for the institute Kyocera printer.

The installer is self-contained: Kyocera's driver software does not need to be installed. It embeds Kyocera's own PPD for the ECOSYS P8060cdn, renders jobs with the standard CUPS filters (or with Kyocera's driver software when it is installed), and adds a small custom CUPS backend that forces the LPD user field to your student ID before delegating the actual network upload to the system `lpd` backend.

For the detailed printing and project design notes, see [docs/printing-principles.md](docs/printing-principles.md).

## What this project installs

The installer creates a CUPS printer queue named:

```text
InstitutePrinter
```

The queue uses this device URI:

```text
titech-lpd://172.20.83.212/kyocera
```

The custom `titech-lpd` backend converts that URI to:

```text
lpd://172.20.83.212/kyocera
```

and passes the job to the operating system's built-in LPD backend.

## Requirements

Install these before building or running the installer:

- Rust toolchain with Cargo.
- CUPS command line tools such as `lpadmin`, `lpstat`, and `cups-config`.
- Access to the school/internal network that can reach the printer IP address.

Kyocera's driver software is optional. See [Bundled Kyocera PPD](#bundled-kyocera-ppd).

## Bundled Kyocera PPD

[assets/Kyocera ECOSYS P8060cdn.PPD](assets/Kyocera%20ECOSYS%20P8060cdn.PPD) is an unmodified copy of the PPD from Kyocera's macOS driver package (`com.kyocera.kyoceraMacintosh.OSX10.pkg` 4.0.7420, PPD `*FileVersion: "8.4506"`). Its header permits redistribution as long as the copyright notice is intact and the file is not altered, so keep it byte-for-byte identical; [.gitattributes](.gitattributes) stops Git from rewriting its CRLF line endings.

The installer embeds it with `include_bytes!`. Three of its lines point CUPS at Kyocera's driver software:

```text
*cupsPreFilter: "application/pdf 0 kyoprefilter_F"
*cupsFilter: "application/vnd.cups-postscript 0 kyofilter_F"
*APDialogExtension: "/Library/Printers/Kyocera/PDEs/KfSummary.plugin"
```

The installer checks whether that software is usable:

- every file those lines name exists: the two filters in the CUPS filter directory and the print dialog plugin, and
- each filter actually starts on this machine. `kyofilter_F` is Intel-only, so on Apple Silicon it needs Rosetta.

If so, the queue gets the PPD unchanged and works exactly like a queue made by Kyocera's own software, including the Kyocera print dialog features below. Otherwise, for example on a Mac without Kyocera's software, on Apple Silicon without Rosetta, or on Linux, the installer leaves those three lines out and prints the reason. Installing or removing Kyocera's software later takes effect the next time you run the installer.

Without those lines CUPS converts jobs with its own `cgpdftops`/`pdftops` and `pstops` filters, which already emit the PJL header and every PostScript option command written in the PPD (duplex, color mode, paper size, tray, collate, staple, quick copy, ...). The resulting print data is the same as with `kyofilter_F`, apart from Kyocera's watermark/overlay macros.

Features that only Kyocera's filters and print dialog plugin implement are then not available:

- watermark / overlay
- private print and job box with PIN
- user login and job accounting via the Kyocera print dialog

To use them, install Kyocera's macOS driver for the ECOSYS P8060cdn and run the installer again. The Kyocera filters and plugin are not bundled: they are Kyocera binaries, macOS-only, and partly Intel-only, and the filters do nothing useful without the plugin.

The Kyocera driver's Linux PPD is not used: its JCL options contain placeholders such as `timestamp=on` that only Kyocera's Linux filter understands, so stock CUPS would send them to the printer as-is.

## Configure your student ID

Edit [src/config.rs](src/config.rs):

```rust
pub const STUDENT_ID: &str = "00B00000";
```

Replace `00B00000` with your own student ID before building.

The required format is:

```text
NNBNNNNN
```

The project performs a compile-time check. If `STUDENT_ID` has the wrong format, `cargo check`, `cargo build`, and `make release` fail before producing a binary.

Do not commit a real student ID to a public repository. Keep the committed placeholder as `00B00000` and change it only in your local working tree before building your own installer.

## Build commands

Use the Makefile for normal builds.

Release build:

```sh
make release
```

Debug build:

```sh
make debug
```

The build script performs three steps:

1. Builds the `titech-lpd` backend.
2. Copies the backend binary to `assets/titech-lpd`.
3. Builds the `installer` binary, embedding `assets/titech-lpd` with `include_bytes!`.

The generated backend binary is ignored by Git because it is a build artifact.

## Build for a specific macOS architecture

Apple Silicon:

```sh
make release TARGET=aarch64-apple-darwin
make debug TARGET=aarch64-apple-darwin
```

Intel:

```sh
make release TARGET=x86_64-apple-darwin
make debug TARGET=x86_64-apple-darwin
```

When `TARGET` is set, the backend and installer are built for the same target. This matters because the installer embeds the backend binary.

The resulting installer is under:

```text
target/<target>/<profile>/installer
```

For example:

```text
target/aarch64-apple-darwin/release/installer
```

## Build script usage

The Makefile calls [scripts/build-installer.sh](scripts/build-installer.sh).

You can also run it directly:

```sh
./scripts/build-installer.sh
```

This is equivalent to a default release build.

Explicit debug build:

```sh
./scripts/build-installer.sh "" debug
```

Explicit target release build:

```sh
./scripts/build-installer.sh aarch64-apple-darwin release
```

Explicit target debug build:

```sh
./scripts/build-installer.sh x86_64-apple-darwin debug
```

The second argument must be either `debug` or `release`.

## Clean commands

Use:

```sh
make clean
```

or:

```sh
./scripts/clean.sh
```

The clean script runs:

```sh
cargo clean
rm -f assets/titech-lpd
```

This removes Cargo build outputs and the generated embedded backend binary.

Cargo's built-in `cargo clean` does not know about `assets/titech-lpd`, so use the project clean script when you want a full project clean.

## Run tests

Run:

```sh
cargo test
```

This checks helper logic such as the embedded PPD, Kyocera driver detection and filter removal, CUPS backend path selection, copy handling, and LPD argument rewriting.

## Install

First build the release installer:

```sh
make release
```

Then run it with root privileges:

```sh
sudo ./target/release/installer
```

If you built for an explicit target, run the target-specific binary:

```sh
sudo ./target/aarch64-apple-darwin/release/installer
```

or:

```sh
sudo ./target/x86_64-apple-darwin/release/installer
```

Debug builds ask for confirmation before each installation step:

```sh
make debug
sudo ./target/debug/installer
```

or, for an explicit target:

```sh
sudo ./target/x86_64-apple-darwin/debug/installer
```

## What the installer does

The installer:

1. Requires root privileges.
2. Locates the CUPS server binary directory with `cups-config --serverbin`.
3. Checks whether Kyocera's driver software is installed and runnable.
4. Installs the embedded `titech-lpd` backend into the CUPS backend directory.
5. Writes the embedded Kyocera PPD to a temporary file, without its Kyocera filter lines unless the Kyocera driver software is usable.
6. Recreates the `InstitutePrinter` queue with `lpadmin` from that PPD.
7. Accepts and enables the queue.

The backend is installed as:

```text
<cups-serverbin>/backend/titech-lpd
```

On macOS this is usually:

```text
/usr/libexec/cups/backend/titech-lpd
```

The backend is installed with mode `0700` and owned by root. This makes CUPS run it as a root backend, matching the system `lpd` backend behavior on macOS.

The queue creation command is equivalent to:

```sh
/usr/sbin/lpadmin -x InstitutePrinter
/usr/sbin/lpadmin -p InstitutePrinter -E \
  -v titech-lpd://172.20.83.212/kyocera \
  -P <temporary copy of the embedded PPD, see above> \
  -o Option19=False \
  -o Option26=False \
  -o Option25=False \
  -o Option21=False \
  -o Option22=False \
  -o Option17=None \
  -o Option18=HardDisk
/usr/sbin/cupsaccept InstitutePrinter
/usr/sbin/cupsenable InstitutePrinter
```

The installable options describe the physical printer hardware assumed by this project:

- no optional paper feeders
- no job separator
- no mailbox
- no punch unit
- no folding unit
- no document finisher
- hard disk enabled

Adjust these constants in [src/bin/installer.rs](src/bin/installer.rs) if your physical printer has different optional hardware.

## Manual checks

Check that CUPS can discover the backend:

```sh
sudo /usr/libexec/cups/backend/titech-lpd
```

Expected output:

```text
network titech-lpd "Unknown" "TiTech fixed-user LPD backend"
```

Check the installed printer:

```sh
lpstat -v InstitutePrinter
lpoptions -p InstitutePrinter -l
```

Send a simple test print:

```sh
printf 'TiTech LPD test\n' | lp -d InstitutePrinter
```

## Security note

The fixed student ID is a convenience guard, not strong authentication.

It prevents normal print dialogs from accidentally sending the local account name as the LPD user field, but a local administrator can still replace the backend, use another print client, capture traffic, or inspect the binary.

Do not treat a client-side embedded student ID as a secret or as a strong security boundary.
