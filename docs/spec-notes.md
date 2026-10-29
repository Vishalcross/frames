# Spec notes

How the repo relates to [design-spec.md](design-spec.md). The spec wins; this
file records what has been resolved, what the code adds, and what is open.

## Resolved (spec section 13)

- **The 750 GB is shared** between S4 and the regular MEGA cloud drive
  (confirmed by the owner, 2026-10-30). 40 GB was already in use, so about
  710 GB is free for a ~550 GB library plus thumbnails.

## Still open

- **Egress fair use.** A forum post from S4's launch period mentions free
  egress up to 5x the average monthly stored data. Not confirmed on MEGA's own
  pages.
- **Region and endpoint** for the bucket.
- **Presigned URLs and range requests.** The player (section 8) needs S4 to
  serve presigned GETs with `Range`. Check with a one-object test before
  building the player.
- **Build machines, and how the repo moves between them.** No hosted CI and the
  code stays local, so the Mac app is built on the Mac and the Windows app on
  Windows. The repo is plain git; the transport between the two machines is
  undecided.
- **Library choices** to verify against the sample set (spec section 13).

## Schema additions (marked [+] in schema.sql) — please review

The spec's data model leaves these out, but its rules need them:

| Addition | Why |
|---|---|
| `assets.date_at` | `asset.setDate` is "latest wins" (section 6); the spec only has `state_at`, which covers trash/purge. |
| `assets.purged_at` | After a purge, a newer `asset.add` must bring the asset back, so the purge needs a tombstone to compare against. |
| `assets.thumb_failed` | Section 7 step 5 says to "flag the asset" when thumbnail decoding fails. |
| `import_queue.album_id` | Importing into an album must add `album.add` to the outbox once the asset is logged. |
| `local_meta` table | Home for "own device id" and "own next seq" from `sync_state`. |
| `outbox` as one row per op | Lets the flush pack ops into batches of about 100 assets. |

**Stamps.** Columns that take part in latest-wins merging (`state_at`,
`date_at`, `name_at`, `album_items.at`) hold `<ISO time, ms, Z>|<device-id>`.
Fixed-width time plus device id makes plain string comparison equal to the
spec's order (batch `at`, then device id).

## Decisions made while writing code — please review

- **Folder month uses local wall-clock time** (`taken_at` plus `tz_offset`). A
  photo taken at 00:30 on 1 May local time goes under `2024/05`, not April.
- **EXIF dates without an offset** are stored as if they were UTC, with
  `tz_offset` empty, so the time shown is the time the camera showed.
- **Dates before 1990 or over a day in the future** are skipped as junk when
  choosing the date taken.
- **Snapshot timestamps** use `YYYYMMDDTHHMMSSZ`, so the newest snapshot is the
  alphabetical maximum.
- **Unknown ops** in a change log are skipped, not fatal, so an older app can
  read logs from a newer one.
- **Not handled yet:** Unicode normalisation of file names. macOS stores them
  decomposed; the same name typed on Windows is a different key.
