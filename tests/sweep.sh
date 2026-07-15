#!/usr/bin/env bash
# Full function sweep for XenovraStream against a mock Telegram API.
# Every check prints PASS/FAIL; exits non-zero if anything failed.
set -uo pipefail

API=http://127.0.0.1:8090/api
BASE=http://127.0.0.1:8090
PASS=0; FAIL=0

ok()   { echo "  PASS  $1"; PASS=$((PASS+1)); }
bad()  { echo "  FAIL  $1  --- $2"; FAIL=$((FAIL+1)); }
chk()  { # chk <desc> <actual> <expected>
  if [ "$2" = "$3" ]; then ok "$1"; else bad "$1" "got '$2' want '$3'"; fi
}
code() { curl -s -o /dev/null -w '%{http_code}' "$@"; }
jq_()  { python3 -c "import sys,json;d=json.load(sys.stdin);print($1)"; }

echo "== auth =="
chk "login with wrong password -> 401" \
  "$(code -X POST $API/auth/login -H 'Content-Type: application/json' -d '{"email":"admin@test.io","password":"nope"}')" 401

TOKEN=$(curl -s -X POST $API/auth/login -H 'Content-Type: application/json' \
  -d '{"email":"admin@test.io","password":"admin12345"}' | jq_ 'd["access_token"]')
[ -n "$TOKEN" ] && ok "login returns a token" || bad "login returns a token" "empty"

AUTH="Authorization: Bearer $TOKEN"
chk "protected route without token -> 401" "$(code $API/videos)" 401
chk "protected route with garbage token -> 401" "$(code $API/videos -H 'Authorization: Bearer garbage')" 401
chk "protected route with token -> 200" "$(code $API/videos -H "$AUTH")" 200

echo "== storages & workers =="
STORAGE=$(curl -s -X POST $API/storages -H "$AUTH" -H 'Content-Type: application/json' \
  -d '{"name":"sweep chan","chat_id":-1005550001}' | jq_ 'd["id"]')
[ -n "$STORAGE" ] && ok "create storage" || bad "create storage" "no id"
chk "duplicate chat_id -> 409" \
  "$(code -X POST $API/storages -H "$AUTH" -H 'Content-Type: application/json' -d '{"name":"dupe","chat_id":-1005550001}')" 409
chk "list storages -> 200" "$(code $API/storages -H "$AUTH")" 200

echo "== upload guards =="
chk "upload with no file -> 400" \
  "$(code -X POST "$API/videos/upload/$STORAGE" -H "$AUTH" -F 'title=nothing')" 400
chk "upload before any bot exists -> 409 (NoStorageWorkers)" \
  "$(code -X POST "$API/videos/upload/$STORAGE" -H "$AUTH" -F 'title=x' -F 'file=@sample.mp4')" 409

curl -s -X POST $API/storage_workers -H "$AUTH" -H 'Content-Type: application/json' \
  -d "{\"name\":\"sweepbot\",\"token\":\"999:SWEEP\",\"storage_id\":\"$STORAGE\"}" -o /dev/null
ok "add bot token"

chk "upload a non-video (this script) -> transcode should later fail, upload accepted" \
  "$(code -X POST "$API/videos/upload/$STORAGE" -H "$AUTH" -F 'title=not a video' -F 'file=@sweep.sh')" 202

echo "== happy path =="
RESP=$(curl -s -X POST "$API/videos/upload/$STORAGE" -H "$AUTH" -F 'title=Sweep Clip' -F 'file=@sample.mp4')
VID=$(echo "$RESP" | jq_ 'd["id"]')
PID=$(echo "$RESP" | jq_ 'd["public_id"]')
[ ${#PID} -eq 20 ] && ok "public_id is 20 chars (80 bits)" || bad "public_id length" "${#PID}"

chk "not-ready video's playlist -> 409" "$(code $API/stream/$PID/master.m3u8)" 409

for i in $(seq 1 60); do
  S=$(curl -s $API/videos/$VID -H "$AUTH" | jq_ 'd["status"]')
  case "$S" in ready|failed) break;; esac; sleep 2
done
chk "transcode reaches ready" "$S" ready

DUR=$(curl -s $API/videos/$VID -H "$AUTH" | jq_ 'd["duration_secs"]')
chk "duration probed" "$DUR" "20.0"
PROG=$(curl -s $API/videos/$VID -H "$AUTH" | jq_ 'd["progress"]')
chk "progress 100" "$PROG" "100"

echo "== the non-video upload should have failed cleanly =="
BADV=$(curl -s $API/videos -H "$AUTH" | jq_ '[v["status"] for v in d if v["title"]=="not a video"][0]')
chk "non-video transcode -> failed (not stuck)" "$BADV" failed
BADERR=$(curl -s $API/videos -H "$AUTH" | jq_ '[bool(v["error"]) for v in d if v["title"]=="not a video"][0]')
chk "failed video carries an error message" "$BADERR" "True"

echo "== playlists =="
chk "master.m3u8 -> 200" "$(code $API/stream/$PID/master.m3u8)" 200
MASTER=$(curl -s $API/stream/$PID/master.m3u8)
echo "$MASTER" | grep -q '^#EXTM3U' && ok "master starts with #EXTM3U" || bad "master header" "missing"
chk "master lists 2 renditions (720p source)" "$(echo "$MASTER" | grep -c 'EXT-X-STREAM-INF')" 2
echo "$MASTER" | grep -q '1920x1080' && bad "no 1080p upscale" "found 1080p" || ok "no 1080p upscale from a 720p source"

chk "360p index -> 200" "$(code $API/stream/$PID/360p/index.m3u8)" 200
chk "720p index -> 200" "$(code $API/stream/$PID/720p/index.m3u8)" 200
chk "unknown rendition -> 404" "$(code $API/stream/$PID/4320p/index.m3u8)" 404
MEDIA=$(curl -s $API/stream/$PID/360p/index.m3u8)
echo "$MEDIA" | grep -q 'EXT-X-ENDLIST' && ok "media playlist is closed (seekable VOD)" || bad "ENDLIST" "missing"
chk "media playlist declares VOD" "$(echo "$MEDIA" | grep -c 'EXT-X-PLAYLIST-TYPE:VOD')" 1

echo "== the .m3u8 share alias =="
chk "/s/<id>.m3u8 -> 200" "$(code $BASE/s/$PID.m3u8)" 200
ALIAS=$(curl -s $BASE/s/$PID.m3u8)
[ "$ALIAS" = "$MASTER" ] && ok "/s/<id>.m3u8 == master.m3u8" || bad "alias body" "differs"
CT=$(curl -s -o /dev/null -w '%{content_type}' $BASE/s/$PID.m3u8)
chk "alias content-type is HLS" "$CT" "application/vnd.apple.mpegurl"
chk "/s/<id> still serves the player page" "$(code $BASE/s/$PID)" 200
chk "/s/<unknown>.m3u8 -> 404" "$(code $BASE/s/deadbeefdeadbeefdead.m3u8)" 404

echo "== segments =="
chk "segment -> 200" "$(code $API/stream/$PID/360p/0.ts)" 200
SCT=$(curl -s -o /dev/null -w '%{content_type}' $API/stream/$PID/360p/0.ts)
chk "segment content-type" "$SCT" "video/mp2t"
chk "segment is immutable-cached" "$(curl -s -D- -o /dev/null $API/stream/$PID/360p/0.ts | grep -ci 'immutable')" 1
chk "out-of-range segment -> 404" "$(code $API/stream/$PID/360p/9999.ts)" 404
chk "malformed segment name -> 400" "$(code $API/stream/$PID/360p/abc.ts)" 400
chk "segment needs no auth (public)" "$(code $API/stream/$PID/360p/1.ts)" 200

echo "== stream actually plays =="
rm -rf sweepdl && mkdir sweepdl
curl -s $API/stream/$PID/360p/index.m3u8 | grep '^/api' | while read -r u; do
  curl -s "$BASE$u" -o "sweepdl/$(basename "$u")"
done
cat sweepdl/*.ts > sweepdl/full.ts 2>/dev/null
PROBE=$(ffprobe -v error -show_entries format=duration -of csv=p=0 sweepdl/full.ts 2>/dev/null | head -1)
python3 -c "import sys;d=float('$PROBE' or 0);sys.exit(0 if 19.5<d<20.6 else 1)" \
  && ok "reassembled 360p stream is ~20s of valid video ($PROBE)" \
  || bad "reassembled duration" "$PROBE"

echo "== public meta =="
chk "public meta -> 200" "$(code $API/public/$PID)" 200
TITLE=$(curl -s $API/public/$PID | jq_ 'd["title"]')
chk "public meta title" "$TITLE" "Sweep Clip"
curl -s $API/public/$PID | grep -q 'user_id' && bad "public meta leaks owner" "user_id present" || ok "public meta exposes no owner/storage ids"

echo "== delete =="
BEFORE=$(curl -s http://127.0.0.1:8081/__stats | jq_ 'd["files"]')
[ "$BEFORE" -gt 0 ] && ok "telegram holds $BEFORE segment messages before delete" || bad "telegram pre-delete" "$BEFORE"
chk "delete -> 204" "$(code -X DELETE $API/videos/$VID -H "$AUTH")" 204
sleep 1
AFTER=$(curl -s http://127.0.0.1:8081/__stats | jq_ 'd["files"]')
chk "telegram messages removed on delete" "$AFTER" 0
chk "deleted video's playlist -> 404" "$(code $API/stream/$PID/master.m3u8)" 404
chk "deleted video gone from list" \
  "$(curl -s $API/videos -H "$AUTH" | jq_ '[v["title"] for v in d].count("Sweep Clip")')" 0
chk "delete a non-existent video -> 404" "$(code -X DELETE $API/videos/$VID -H "$AUTH")" 404
chk "delete without auth -> 401" "$(code -X DELETE $API/videos/$VID)" 401

echo "== cascade: no orphan rows =="
ORPHAN=$(docker exec xs-test-db psql -U xstest -tAc \
  "select (select count(*) from renditions r left join videos v on r.video_id=v.id where v.id is null)
        + (select count(*) from segments s left join renditions r on s.rendition_id=r.id where r.id is null)
        + (select count(*) from transcode_jobs j left join videos v on j.video_id=v.id where v.id is null);" | tr -d ' ')
chk "no orphaned renditions/segments/jobs" "$ORPHAN" 0

echo
echo "================================"
echo "  PASSED: $PASS   FAILED: $FAIL"
echo "================================"
[ "$FAIL" -eq 0 ]
