# Printing Principles and Project Design

This document explains how the school printer setup works and how this project fits into it.

## High-level model

The printer is reached over the school internal network by IP address. Jobs are sent with the LPD protocol. The printer or print server expects the LPD user field to contain a student ID, and that field is used as the user identity for the print job.

A normal desktop print setup has three major parts:

1. CUPS accepts a print job from applications.
2. The printer driver converts the document into printer-compatible data.
3. A CUPS backend sends the converted data to the printer.

This project changes the backend part and ships Kyocera's PPD so that part 2 can be done by the standard CUPS filters, without installing Kyocera's driver software. When that software is installed, the queue uses it instead.

## Network and protocol

The configured printer endpoint is:

```text
172.20.83.212
```

The queue name used by the LPD endpoint is:

```text
kyocera
```

In LPD URI form, the real destination is:

```text
lpd://172.20.83.212/kyocera
```

The `/kyocera` part is important. In LPD terminology it is the remote queue name. If you omit it or use the wrong value, the printer server may reject the job or route it incorrectly.

This project installs the CUPS queue with a custom scheme:

```text
titech-lpd://172.20.83.212/kyocera
```

CUPS uses the URI scheme to choose a backend executable. Because the scheme is `titech-lpd`, CUPS runs the custom `titech-lpd` backend installed by this project.

The backend then rewrites the URI scheme back to standard LPD:

```text
lpd://172.20.83.212/kyocera
```

and delegates transmission to the operating system's built-in `lpd` CUPS backend.

## Why the student ID matters

LPD jobs contain a control file. One field in that control file is the user name, commonly known as the `P` field.

Many CUPS clients fill this field from the local operating system account name. For example, if the local account is `alice`, the LPD user field may become `alice`.

For this school printer setup, the expected value is the student ID. If the user field is wrong, the job may not be associated with the correct student account.

This project solves that by ignoring the CUPS-provided user argument and always sending the configured `STUDENT_ID` to the system LPD backend.

## How CUPS backends are invoked

CUPS backends are executable files in the CUPS backend directory, usually:

```text
/usr/libexec/cups/backend
```

on macOS, or:

```text
/usr/lib/cups/backend
```

on Linux.

When a queue uses a URI such as:

```text
titech-lpd://172.20.83.212/kyocera
```

CUPS looks for a backend named:

```text
titech-lpd
```

and runs it with the standard backend arguments:

```text
job-id user title copies options [file]
```

This project's backend builds a new argument list for the system `lpd` backend:

```text
job-id STUDENT_ID title 1 options [file]
```

The key changes are:

- `user` is replaced by `STUDENT_ID`.
- `copies` is forced to `1`.
- If multiple copies are requested, the backend sends the job multiple times.

The multiple-copy behavior avoids relying on the downstream LPD/Kyocera path to interpret the copies field in a way that matches the driver output.

## Role of the Kyocera PPD

The Kyocera PPD describes:

- printer model
- supported paper sizes
- duplex options
- color options
- installable hardware options
- the exact PJL and PostScript commands for each option

The printer understands PostScript (Kyocera calls its interpreter KPDL) directly, so the PPD is all CUPS needs. For a PDF job the CUPS pipeline is:

```text
application/pdf -> cgpdftops (pdftops on Linux) -> pstops -> titech-lpd backend
```

`pstops` writes the PJL header from the PPD's `*JCLBegin` / `*JCLToPSInterpreter` / JCL options, then the PostScript, inserting the `setpagedevice` code of every selected option.

Kyocera's macOS driver adds two filters to that pipeline through `*cupsPreFilter` / `*cupsFilter` lines in the PPD: `kyoprefilter_F` (PDF to PDF) and `kyofilter_F` (after `pstops`). Comparing their output with the plain CUPS output for the same job shows they only add a few PJL comments and a watermark/overlay macro library, and implement Kyocera-only features such as watermarks, private print with PIN, and user login. Their settings (PIN, login, watermark) are entered in Kyocera's print dialog plugin (`*APDialogExtension`, `KfSummary.plugin`) and the Kyocera Print Panel app; without the plugin the filters produce the same data as plain CUPS.

The installer therefore decides per machine:

- If the filters and the plugin are installed and the filters start on this machine, the PPD is installed unchanged and Kyocera's software handles the extra features.
- Otherwise it drops those three lines from the queue's copy of the PPD, so none of Kyocera's binaries are needed.

"Start on this machine" matters on Apple Silicon: `kyofilter_F` contains only Intel code and fails with "Bad CPU type" when Rosetta is not installed. The installer runs each filter once without arguments, which makes a CUPS filter exit immediately, to check this.

The bundled PPD itself is kept unmodified in the repository because its license only allows redistribution of the unaltered file.

This project does not implement a print renderer, PostScript converter, PDF converter, or Kyocera protocol encoder. Those are the standard CUPS filters.

## Role of this project

This project provides two binaries:

```text
titech-lpd
installer
```

`titech-lpd` is the CUPS backend. Its responsibilities are intentionally small:

- advertise itself to CUPS when run without job arguments
- accept CUPS backend job arguments
- convert `titech-lpd://...` to `lpd://...`
- replace the user field with `STUDENT_ID`
- delegate the actual upload to the system `lpd` backend

`installer` installs and configures the queue. Its responsibilities are:

- check root privileges
- install the embedded `titech-lpd` backend
- detect whether Kyocera's driver software is installed and runnable
- create the CUPS queue with the right URI and the embedded PPD, minus its Kyocera filter lines when that software is not usable
- set the assumed Kyocera installable options
- accept and enable the queue

## Why the installer embeds the backend

The installer embeds the backend binary as bytes:

```rust
include_bytes!("../../assets/titech-lpd")
```

The build script first builds `titech-lpd`, copies it to `assets/titech-lpd`, and then builds `installer`.

This makes the installer self-contained: when you run the installer, it already contains the backend that it needs to place into the CUPS backend directory.

Because the backend is embedded as a native executable, the backend and installer must be built for the target machine architecture. For example, an Apple Silicon installer should embed an Apple Silicon backend.

## Printer queue details

The installed queue is named:

```text
InstitutePrinter
```

The queue URI is:

```text
titech-lpd://172.20.83.212/kyocera
```

The `kyocera` queue name is part of the protocol address. When configuring the printer manually with normal LPD, the equivalent address is:

```text
lpd://172.20.83.212/kyocera
```

Some printer setup dialogs split this into separate fields:

- protocol: LPD
- address or host: `172.20.83.212`
- queue: `kyocera`

If the dialog has a queue field, enter `kyocera`. If the dialog asks for a URI, use the full `lpd://172.20.83.212/kyocera` form for a normal manual setup, or `titech-lpd://172.20.83.212/kyocera` when using this project's backend.

## Installable options

The installer currently creates the queue with these Kyocera options:

```text
Option19=False
Option26=False
Option25=False
Option21=False
Option22=False
Option17=None
Option18=HardDisk
```

These mean the queue assumes no optional feeders, separator, mailbox, punch unit, folding unit, or finisher, while keeping the hard disk enabled.

If the real printer has different optional hardware, those constants should be updated in:

```text
src/bin/installer.rs
```

## Security boundary

Embedding a student ID in a local backend is not strong authentication.

It is useful for making the normal print path send the expected LPD user field, but anyone with administrative access to the client machine can bypass it by changing the backend, installing another queue, using a different client, or inspecting the binary.

The security model should be understood as client-side configuration, not as a protected identity mechanism.
