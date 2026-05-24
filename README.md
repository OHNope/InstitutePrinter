# printer-installer

`printer-installer` builds and installs a CUPS printer queue for the institute Kyocera printer.

The project does not replace the Kyocera printer driver. It uses the Kyocera PPD and filters already installed on the computer, then adds a small custom CUPS backend that forces the LPD user field to your student ID before delegating the actual network upload to the system `lpd` backend.

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
- CUPS command line tools such as `lpadmin`, `lpinfo`, `lpstat`, and `cups-config`.
- Kyocera ECOSYS P8060cdn driver software.
- Access to the school/internal network that can reach the printer IP address.

The installer checks for the Kyocera ECOSYS P8060cdn KPDL PPD through:

```sh
lpinfo -m
```

It also checks that the Kyocera filters exist in the CUPS filter directory:

```text
kyofilter_F
kyoprefilter_F
```

On macOS this directory is usually:

```text
/usr/libexec/cups/filter
```

On Linux it is usually:

```text
/usr/lib/cups/filter
```

The installer asks `cups-config --serverbin` first and only uses platform defaults as a fallback.

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

This checks helper logic such as PPD parsing, CUPS backend path selection, copy handling, and LPD argument rewriting.

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
3. Verifies that the Kyocera driver filters are installed.
4. Finds the Kyocera ECOSYS P8060cdn KPDL PPD from `lpinfo -m`.
5. Installs the embedded `titech-lpd` backend into the CUPS backend directory.
6. Recreates the `InstitutePrinter` queue with `lpadmin`.
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
  -m "<Kyocera ECOSYS P8060cdn KPDL PPD from lpinfo>" \
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

The model name is discovered automatically from `lpinfo -m`; it is not hard-coded.

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
