# Printing Principles and Project Design

This document explains how the school printer is reached and why this project needs no printer driver.

## High-level model

The printer is reached over the school internal network by IP address, and jobs are sent with the LPD protocol (RFC 1179). The printer or print server uses the LPD user field of each job as the user identity, and expects it to be a student ID.

A normal desktop print setup has three parts:

1. The operating system's print system (CUPS on macOS/Linux, the spooler on Windows) accepts a job from an application.
2. A printer driver converts the document into data the printer understands.
3. A backend or port monitor sends that data to the printer, filling in the LPD user field with the local account name.

This project skips all three. The ECOSYS P8060cdn interprets PDF (Kyocera's PDF Direct Print) and PostScript (KPDL3) itself, so a PDF or PostScript file already is printer data. `institute-print` reads the file, wraps it in a short PJL header, and uploads it with its own LPD client, putting the student ID in the user field.

## Network and protocol

The endpoint is:

```text
lpd://172.20.83.212/kyocera
```

`kyocera` is the LPD remote queue name. If it is omitted or wrong, the server may reject the job or route it incorrectly. Both values are in `src/config.rs`.

For each upload the tool opens one TCP connection to port 515 and sends, in this order (the default order of the CUPS `lpd` backend):

1. `\x02kyocera\n`: receive a job for queue `kyocera`.
2. The control file.
3. The data file.

The server acknowledges each step with a zero byte; anything else is reported as a rejection.

The control file looks like this:

```text
H<host name>
P<student ID>
J<file name>
ldfA<nnn><host>
UdfA<nnn><host>
N<file name>
```

`P` is the user field. `l` prints the data file as-is, without any server-side filtering. Host and file names are reduced to printable ASCII and cut to the RFC 1179 length limits; `<nnn>` is a three-digit job number.

## Why the student ID matters

Many clients fill the LPD user field from the local account name, for example `alice`. The school printer associates the job with the student account named in that field, so a wrong value means the job is not credited to you or is rejected. This project always sends the configured `STUDENT_ID`.

## Print data and PJL

The data file is the original file between a PJL header and footer:

```text
<ESC>%-12345X@PJL JOB
@PJL SET DUPLEX=ON              (only with --duplex long/short)
@PJL SET BINDING=LONGEDGE       (LONGEDGE or SHORTEDGE)
@PJL SET RENDERMODE=GRAYSCALE   (only with --mono / --color: GRAYSCALE or COLOR)
@PJL ENTER LANGUAGE=PDF         (or POSTSCRIPT)
<file contents>
<ESC>%-12345X@PJL EOJ
<ESC>%-12345X
```

The job start, language switch and job end strings come from Kyocera's PPD for this printer (`*JCLBegin`, `*JCLToPSInterpreter`, `*JCLEnd`).

The PPD sets duplex and color mode with PostScript `setpagedevice` code. A PDF file is not PostScript, so that code cannot be used for PDF Direct Print. The tool uses the standard HP PJL variables `DUPLEX`, `BINDING` and `RENDERMODE` instead. They are not listed in the PPD and still need to be confirmed on the real printer. Settings the user does not ask for are not sent, so the printer's own defaults apply.

The file type is detected from its first bytes: `%PDF-` for PDF, `%!` for PostScript. Anything else is rejected with a hint to export it to PDF.

## Copies

Each copy is a separate upload of the file, with its own job number. This keeps the behavior of the earlier CUPS backend: the printer never has to interpret a copy count, and every copy is a normal one-copy job for the server.

## Source port

RFC 1179 asks clients to send from a port below 1024, and some servers enforce it. The CUPS `lpd` backend did this by running as root and taking the first free port counting down from 1023. `institute-print` does the same. Windows and macOS let normal users bind these ports. Linux requires root or `CAP_NET_BIND_SERVICE`, so on Linux the tool warns and falls back to an ordinary port.

A port that was just used stays in TIME_WAIT for a while; the tool then simply moves on to the next port.

## Why there is no driver or print queue

An earlier version of this project installed a CUPS queue with Kyocera's PPD and a custom CUPS backend that rewrote the LPD user field. That only worked on macOS and Linux, needed root, and had no counterpart on Windows: Windows would have needed a signed driver package for the PPD plus either a port monitor DLL inside the spooler or a background relay service, because its built-in LPR port always sends the Windows account name.

Since the printer renders PDF and PostScript itself, the driver part was never doing essential work. Uploading the file directly gives one small program that behaves the same on every operating system and needs no installation.

The trade-off is that applications cannot print to the institute printer from their own print dialog. Documents are exported to PDF first and then sent with `institute-print`.

Kyocera-only features that need Kyocera's driver software (watermarks, private print with PIN, job accounting login in the print dialog) are not available.

## Security boundary

Embedding a student ID in a local binary is not authentication. Anyone on the network can send any LPD user name, and the student ID can be read from the binary. Treat it as client-side configuration, not as a protected identity.
