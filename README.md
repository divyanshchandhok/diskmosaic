<img src="assets/icon.svg" width="64" height="64" alt="">

# wiztree-linux

A cross-platform (Linux, macOS, Windows) Rust reimplementation of
[WizTree](https://diskanalyzer.com), the fast disk space analyser, with a
modern themed interface.

The disk is scanned once, in parallel, into a compact in-memory index; the
tree view, treemap, file list, search, filters and duplicate finder then all
work from that index without touching the disk again.

On NTFS drives (Windows, and Linux `ntfs3`/`ntfs-3g` mounts) the Master File
Table is read directly — WizTree's technique — instead of walking folders:
one mostly sequential read, skipping free MFT regions via `$MFT:$BITMAP`.
On Linux the raw device needs root: the app asks for your password once per
session and streams only the MFT through a small helper
(`wiztree --mft-dump DEV`), or can install a udev rule (`uaccess` on NTFS
partitions) so no prompt is needed. Folders on spinning NTFS disks are walked
for up to 2 s and switch to the MFT if they turn out to be big.

| 700k-entry NTFS USB hard disk, cold cache | time |
|---|---|
| Folder walk (before) | 262 s |
| MFT read | 17 s (disk-bound) |
| Rescan, warm cache | 0.3–1 s |

The headless export was verified row for row against WizTree 4.33's own MFT
scan of the same drive — 700,604 rows — matching sizes, allocated sizes
(including alternate data streams), modification dates and attribute values
exactly.

Created in [T3 Code](https://t3.codes).

## Build

```sh
cargo build --release
./target/release/wiztree [PATH...]
```

`mimalloc` is a default feature; build with `--no-default-features` to use
the system allocator.

## Features

- Tree view with % of parent, size, allocated, item/file/folder counts,
  dates and attributes, plus "[N Files in …]" grouping and a file type panel
- Cushion treemap (WizTree's shading model) with zoom, hover, labels, free
  space and PNG export
- File view: search across every file with WizTree's syntax (wildcards,
  `|` OR, `!` NOT, `>1gb`, dates, `=folder`, `namelen>`, `\regex\`,
  `dupcount>`), plus duplicate detection
- Scan-result filters, CSV/TSV export and import, file-type export
- Live updates from file system monitoring; delete, trash, rename, copy,
  open, reveal and open a terminal
- Nine themes plus Auto, 11 accent colours, ten treemap colour schemes and a
  custom palette
- Translations: drop WizTree's `locale/<lang>/LC_MESSAGES/default.po` folders
  next to the executable

## Command line

WizTree-style options work for headless export, for example:

```sh
wiztree / /export=disk.csv /exportfolders=0 /filter="*.mp4|*.mkv"
wiztree /home /treemapimagefile=home.png /treemapimagewidth=3840 /treemapimageheight=2160
```

Run `wiztree --help` for the full list. `--name=value` works as well.

## Development

```sh
cargo test --release
# Offscreen screenshots of every screen into target/snapshots:
WT_SNAP_PATH=/usr cargo test --release snapshots -- --ignored
# Timing of search / treemap / filter / duplicates:
WT_SNAP_PATH=$HOME cargo test --release bench_views -- --ignored --nocapture
```
