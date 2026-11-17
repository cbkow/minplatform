# S20 - RAR, cab and iso through the system's own archive tool (opened and closed 2026-11-17)

**Status: VERDICT: PASS on macOS and on Windows 11 (2026-11-17), with one limit: a RAR locked with a password is
not opened by the tool on either system.**

## Why

Canon row 508, point 6: UFB's rebuild unpacks zip, 7z and the tar family by its own code and bundles no tool
(1.4.2 bundled 7-Zip). RAR has no free reader in Rust, and cab / iso are rare. Both systems ship libarchive's
`bsdtar`: `/usr/bin/bsdtar` on macOS, `C:\Windows\System32\tar.exe` on Windows 10 1803 and later.

## The question

Can the system's own tool, run as a child process, list and unpack RAR (versions 4 and 5), cab and iso archives on
both systems, with the same result?

## The pass bar

Seven sample archives without a password - RAR 4 stored, RAR 4 compressed, RAR 5 stored, RAR 5 compressed, RAR 5
first part of a multi-part set, cab, iso (Joliet) - each: the listing names its entries, the unpack ends with exit
code 0, and the bytes unpacked are the same on both systems. 7 of 7 on each system. Time box: one hour.

## The samples

libarchive's own public test archives (its repository, `libarchive/test/*.uu`), fetched and decoded into a scratch
folder; nothing of them is kept in this repository.

## What was seen

| | macOS (bsdtar 3.5.3, libarchive 3.7.4) | Windows 11 build 26200 (bsdtar 3.8.9) |
|---|---|---|
| cab | exit 0, 138 bytes | exit 0, 138 bytes |
| iso (Joliet) | exit 0, 12 bytes | exit 0, 12 bytes |
| RAR 4 | exit 0, 40 bytes | exit 0, 40 bytes |
| RAR 4, compressed | exit 0, 40,242 bytes | exit 0, 40,242 bytes |
| RAR 5, stored | exit 0, 29 bytes | exit 0, 29 bytes |
| RAR 5, compressed | exit 0, 1,200 bytes | exit 0, 1,200 bytes |
| RAR 5, first part of several | exit 0, 144,608 bytes | exit 0, 144,608 bytes |
| RAR 4, locked data | exit 1 | exit 1 (it leaves files of garbage behind) |

7 of 7 on both. The file counts differ by one in three rows: a link inside the archive is made on Windows as a
file and on macOS as a link (not counted there as a file).

- The tool refuses absolute paths and `..` by itself (its default without `-P`).
- `-xvf` writes one line `x <name>` an entry to its error stream: with the count from `-tf` that is the share done.
- A locked RAR ends with exit 1 and no word about a password ("Error exit delayed from previous errors"); what it
  wrote must be thrown away. There is no way to hand it a password for RAR.

Not tried: wim (no sample; libarchive does not list it among what it reads), a large archive, a solid RAR of many
files, Windows 10, an archive on a share.

## VERDICT: PASS

RAR, cab and iso are unpacked through the system's tool into the same partial folder as every other archive, so
the placement rules and "nothing is overwritten" hold for them too. A failure is said with the tool's own first
complaint and the hint that a locked RAR is not opened. wim is not offered.
