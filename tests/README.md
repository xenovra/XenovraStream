# Integration sweep

`sweep.sh` drives a running XenovraStream over HTTP and checks every route,
including the parts unit tests cannot reach: a real ffmpeg transcode, segments
round-tripping through the Telegram API, playback reassembling into valid video,
and delete clearing the chat.

It runs against **`mock_telegram.py`**, a stand-in implementing the only four
calls the app makes (`sendDocument`, `getFile`, the file download, and
`deleteMessage`). That means no bot token, no real channel, and no rate limits —
so the sweep is safe to run on any machine.

## Running it

```bash
# 1. a throwaway postgres
docker run -d --name xs-test-db \
  -e POSTGRES_USER=xstest -e POSTGRES_PASSWORD=xstest \
  -p 55432:5432 postgres:15.0-alpine

# 2. the fake Telegram
python3 tests/mock_telegram.py 8081 &

# 3. a test video the sweep expects to find next to it
ffmpeg -f lavfi -i "testsrc2=size=1280x720:rate=25:duration=20" \
       -f lavfi -i "sine=frequency=440:duration=20" \
       -c:v libx264 -preset ultrafast -pix_fmt yuv420p \
       -c:a aac -shortest tests/sample.mp4

# 4. the app, pointed at both
PORT=8090 WORKERS=4 \
SUPERUSER_EMAIL=admin@test.io SUPERUSER_PASS=admin12345 \
ACCESS_TOKEN_EXPIRE_IN_SECS=86400 REFRESH_TOKEN_EXPIRE_IN_DAYS=14 \
SECRET_KEY=test-secret-key \
TELEGRAM_API_BASE_URL=http://127.0.0.1:8081 TELEGRAM_RATE_LIMIT=200 \
DATABASE_USER=xstest DATABASE_PASSWORD=xstest DATABASE_NAME=xstest \
DATABASE_HOST=127.0.0.1 DATABASE_PORT=55432 \
WORK_DIR=/tmp/xs-work SEGMENT_SECS=4 X264_PRESET=ultrafast \
  cargo run &

# 5. sweep
cd tests && ./sweep.sh
```

Expect `PASSED: 51   FAILED: 0`. Non-zero exit means something regressed.

The sweep assumes a **720p** source: it asserts the ladder produces exactly two
renditions and never upscales to 1080p.
