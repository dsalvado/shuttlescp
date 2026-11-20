# Parallel transfers of large files (design notes)

Status: **idea, not implemented.** Written 2026-11-19 from a read of the code at commit
`e878445`. Nothing here has been built or measured.

## Why

A single large file is copied as one stream, in 256 KiB steps (`CHUNK_BYTES` in
`crates/shuttlescp-core/src/transfer/copy.rs`). Several files copy side by side, but one
big file never uses more than one connection. On a high-latency link a single SFTP stream
cannot fill the pipe.

rclone attacks this in two ways:

- **Multi-thread copies.** A file above `--multi-thread-cutoff` (default 256 MiB) is cut
  into parts and up to `--multi-thread-streams` (default 4) streams copy them at once,
  each writing at its own offset of the destination file, so there is no assembly step.
  This needs a destination that can be written at arbitrary offsets.
- **SFTP pipelining.** `--sftp-concurrency` (default 64) keeps many requests in flight on
  one connection.

rclone is Go, so it cannot be added as a crate; embedding it (subprocess or `librclone`)
would duplicate the login, host-key, agent, two-factor and keyring handling and replace the
resume and pause logic. The decision is to take the idea and write it in Rust.

## What already exists

- `russh-sftp` 3.0.1 already pipelines: by default up to 16 reads and 16 writes in flight,
  with writes capped at 32 KiB each (`max_write_packet_len`). `start_sftp` in
  `remote/sftp.rs` uses `SftpSession::new` with the defaults, so nothing is tuned.
- `Sftp::twin()` opens a second session with the same login (a new connection, or a second
  channel when the login needed a one-time code, so the code is not asked again).
- `open_at` and `open_write_at` take an offset. Files are copied through a partial file
  (`.filepart`) and renamed when complete.
- Pause, stop and the shared limit are checked inside `copy_contents`.
- `Site::files_at_once` (default 4, maximum 10) is the per-site limit on files at once;
  `FilePlaces` (`transfer/places.rs`) is the global limit shared by all jobs.
- A comment in `copy_contents` records that an extra read-ahead queue of our own made
  copies about 10% slower (2026-10, 25 ms each way). The `live_relay_speed` test measures
  this. Gains must be measured, not assumed.

## Decisions

- Streams per file are **derived from the site's `files_at_once`**; there is no new
  per-site setting and no change to the login dialog.
- One Preferences option turns the feature on. It starts **off**.
- SFTP first. FTP needs a connection pool and many servers only resume at one offset.

## Design

1. **When to split.** Only for a file above a size threshold (suggest 256 MiB) when one
   side is a server. Local-to-local copies and small files behave as now.
2. **Ranges.** Cut the file into fixed ranges (suggest 32 MiB). Workers take the next
   range from a shared counter, so a slow stream does not hold the others back.
3. **Workers.** Each worker has its own connection (`twin()`), opens the source at its
   range start and the partial file at the same offset, copies to the range end, then takes
   the next range.
4. **Partial file.** Extend it to its full size first, so ranges write into one file.
   Local files on Windows may become sparse.
5. **Finish.** After the last range, the existing steps run unchanged: rename, set the
   time, verify, delete the source if moving.

### Connection budget

The site's `files_at_once` is the most connections used at once, files and streams
together.

    streams for a file = max(1, budget / files running), at most the number of ranges

With a budget of 4: one big file gets 4 streams, two files get 2 each, four files get 1
each (today's behaviour). Because workers pull ranges from a shared counter, a connection
freed by a finished file can join a big file already copying, so the last big file in a
folder speeds up at the end. Site to site uses the lower of the two sites' budgets.

A split file holds **one** place in `FilePlaces`; its streams are not counted as files.

## Resume

Today resume means "the partial file's size is how much is done", checked against the last
64 KiB (`resume_offset`). With parallel ranges the size says nothing about what is
finished. Needed: a small sidecar record next to the partial file (or in the state folder
used for unfinished transfers) listing the completed ranges. Resume redoes every range not
marked complete and spot-checks the end of a few finished ones, as `resume_offset` does.

The retry logic (`PARTWAY_TRIES`, `PARTWAY_PAUSE`) would apply per range, not per file.

## Open problems

- **The budget is per job today**: `copy.rs` creates one semaphore per copy job, so two
  jobs to the same server can each use the whole budget. Connections must be counted per
  server across jobs (keyed by site). `FilePlaces` counts files, not connections.
- **Existing users**: a site set to 10 files at once would allow 10 streams for one file,
  which can hit per-user connection limits. Cap streams per file (suggest 4) in the first
  release.
- **Progress** must be the monotonic sum of the ranges (the UI expects one `done` value).
- **Pause** must make every worker flush before the sidecar is saved.
- Some servers reject large writes or parallel writes to one file; fall back to a single
  stream.

## Plan

1. Tune SFTP pipelining (raise concurrency, larger write packets within the server's
   limits, with a safe fallback) and measure on a high-latency link. Single function:
   `start_sftp`.
2. Count connections per server across jobs.
3. SFTP range workers using `twin()` and the budget rule above.
4. The sidecar for resume.
5. The Preferences option.

Step 1 may remove most of the need for the rest; do it first and decide with numbers.
