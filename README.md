# XenovraStream

Upload a video, get a streaming link. **Telegram** is the storage backend.

A video-only sibling of [XenovraDrive](https://codeberg.org/xenovra/XenovraDrive):
where the drive stores arbitrary files as 20 MiB chunks, XenovraStream transcodes
video into an adaptive HLS ladder and stores each segment as its own Telegram
message. You get back a single unlisted link — `/s/<id>` — that plays in any
browser, adapts to the viewer's bandwidth, and needs no account to watch.

```
upload ──► disk ──► queue ──► ffmpeg (360p/720p/1080p) ──► segments ──► Telegram
                                                                           │
  viewer ◄── hls.js ◄── m3u8 + segment proxy ◄── disk cache ◄──────────────┘
```

## ✨ What it does

- **Adaptive bitrate** — every upload is transcoded to 360p, 720p and 1080p, never
  above the source resolution. The player switches rendition on the fly as
  bandwidth changes; viewers can also pick a quality by hand.
- **Unlisted public links** — `/s/<20-hex-chars>`, shareable, no login to watch,
  not enumerable. Holding the link is the authorisation.
- **A real job queue** — transcoding is a row in `transcode_jobs`, not an
  in-memory channel. A restart mid-transcode re-queues the job instead of losing
  the upload.
- **Streaming upload** — multipart goes straight to disk, chunk by chunk. A 10 GB
  file costs 10 GB of disk and almost no RAM.
- **Segment cache** — fetched segments are cached on disk and served from there,
  with a read-ahead on the next two. This is what makes playback viable at all;
  see below.
- **Delete syncs to Telegram** — removing a video removes every segment message
  from the chat, not just the database rows.
- **Multiple bots per storage** — each bot token added to a storage lifts the
  rate limit, which is the main scaling knob.

## ⚡ Why the cache is not optional

Reading one segment back costs **two rate-limited Telegram calls** — `getFile` for
the path, then the download itself — and a bot token allows `TELEGRAM_RATE_LIMIT`
(default 18) calls per minute. Uncached, a single viewer on 6-second segments
burns ~20 calls/min and stalls; a second viewer of the same video doubles it.

With the cache, a segment costs Telegram exactly once no matter how many people
watch it. Two levers if you need more headroom:

- **Add more bots to a storage.** The limiter is per bot token, so N bots means N×
  the ceiling.
- **Raise `CACHE_MAX_MB`.** A cache that holds your popular videos end-to-end
  means Telegram is only touched on a cold first play.

## 🚀 Installation

```bash
git clone https://codeberg.org/xenovra/XenovraStream.git
cd XenovraStream
cp .env.example .env    # edit SECRET_KEY, SUPERUSER_PASS, DATABASE_PASSWORD
make up
```

Or straight from the published image:

```bash
docker pull ghcr.io/xenovra/xenovrastream:latest
```

Then open `http://localhost:8000`, sign in as the superuser, and:

1. **Add a storage** — a Telegram channel. Create the channel, add your bot as an
   admin, and paste the chat id (e.g. `-1001234567890`).
2. **Add a bot token** from [@BotFather](https://t.me/botfather). Add several to
   the same storage; each one raises the rate limit.
3. **Upload a video.** The table shows transcode progress; when it flips to
   `ready`, copy the link.

## ⚙️ Configuration

Everything is environment variables — see [.env.example](.env.example). The ones
that matter for streaming:

| Variable | Default | Notes |
|---|---|---|
| `WORK_DIR` | `/var/lib/xenovrastream` | Uploads, ffmpeg scratch, segment cache. Needs room for the largest source plus its renditions. |
| `CACHE_MAX_MB` | `4096` | Segment cache ceiling. Bigger is better. |
| `SEGMENT_SECS` | `6` | Shorter seeks faster, but multiplies Telegram calls per minute of playback. |
| `TELEGRAM_RATE_LIMIT` | `18` | Calls/min **per bot token**. |
| `X264_PRESET` | `veryfast` | `medium` buys ~15% bitrate for ~3x the transcode time. |
| `UPLOAD_CONCURRENCY` | `4` | Segments pushed to Telegram at once. |

## ⏱️ Transcode cost

CPU-only H.264 encoding, three renditions, no GPU. On 6 cores at `veryfast`,
budget roughly **1-2x realtime per rendition** — a 10-minute video lands in about
15-25 minutes total. Jobs run one at a time on purpose: ffmpeg already saturates
every core, so running two would only make both slower.

If that is too slow, `X264_PRESET=superfast` or dropping to a single rendition are
the levers.

## 🔌 API

Authenticated (`Authorization: Bearer <jwt>`):

| Route | |
|---|---|
| `POST /api/auth/login` | `{email, password}` → `{access_token}` |
| `POST /api/videos/upload/:storage_id` | multipart `file` + `title` → `202` + video |
| `GET /api/videos` | your videos, with status and progress |
| `DELETE /api/videos/:id` | drops rows *and* Telegram messages |

Public — no auth, the link is the credential:

| Route | |
|---|---|
| `GET /s/:public_id` | player page |
| `GET /api/public/:public_id` | title + duration |
| `GET /api/stream/:public_id/master.m3u8` | master playlist |
| `GET /api/stream/:public_id/:rendition/index.m3u8` | media playlist |
| `GET /api/stream/:public_id/:rendition/:n.ts` | segment (cache → Telegram) |

## 🧱 Stack

Rust · axum 0.6 · sqlx/Postgres · ffmpeg · hls.js. The UI is plain static HTML —
no bundler, no build step.

## 🧭 Known limits

- **One transcode at a time.** Fine for personal use; a busy instance would want
  the job table drained by several workers (the `SKIP LOCKED` claim already
  supports it).
- **No automatic retry.** A failed job stays failed and its source is dropped;
  re-upload to try again.
- **Telegram's 20 MB download cap** bounds segment size. At 6s segments and a
  5 Mbps 1080p rendition there is plenty of headroom, but very long segments at
  very high bitrates would hit it.
- **VOD only** — no thumbnails, no subtitles, no live.

## 📄 Licence

MIT — see [LICENSE](LICENSE). Derived from
[Pentaract](https://github.com/Dominux/Pentaract) by Dominux, via XenovraDrive.
