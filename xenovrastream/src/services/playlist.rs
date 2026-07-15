use std::fmt::Write;

use crate::models::videos::{Rendition, Segment};

/// Builds HLS playlists on the fly from the rows we stored at transcode time.
///
/// Nothing is persisted as text: the playlist is a pure function of the
/// rendition and segment rows, so segment URLs can be re-pointed (or the whole
/// route moved) without a migration.
pub struct Playlist;

impl Playlist {
    /// The master playlist — one `#EXT-X-STREAM-INF` per rendition, cheapest
    /// first so a cold player starts low and adapts up.
    pub fn master(public_id: &str, renditions: &[Rendition]) -> String {
        let mut out = String::from("#EXTM3U\n#EXT-X-VERSION:3\n");

        for r in renditions {
            let _ = write!(
                out,
                "#EXT-X-STREAM-INF:BANDWIDTH={},RESOLUTION={}x{},CODECS=\"{}\"\n\
                 /api/stream/{}/{}/index.m3u8\n",
                r.bandwidth, r.width, r.height, r.codecs, public_id, r.name
            );
        }

        out
    }

    /// A media playlist. `EXT-X-PLAYLIST-TYPE:VOD` plus `EXT-X-ENDLIST` tell the
    /// player the whole timeline is known, which is what enables seeking.
    pub fn media(public_id: &str, rendition: &Rendition, segments: &[Segment]) -> String {
        let mut out = String::new();
        let _ = write!(
            out,
            "#EXTM3U\n\
             #EXT-X-VERSION:3\n\
             #EXT-X-TARGETDURATION:{}\n\
             #EXT-X-MEDIA-SEQUENCE:0\n\
             #EXT-X-PLAYLIST-TYPE:VOD\n\
             #EXT-X-INDEPENDENT-SEGMENTS\n",
            rendition.target_duration
        );

        for s in segments {
            let _ = write!(
                out,
                "#EXTINF:{:.6},\n/api/stream/{}/{}/{}.ts\n",
                s.duration, public_id, rendition.name, s.position
            );
        }

        out.push_str("#EXT-X-ENDLIST\n");
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    fn rendition(name: &str, bandwidth: i32) -> Rendition {
        Rendition {
            id: Uuid::nil(),
            video_id: Uuid::nil(),
            name: name.to_owned(),
            width: 640,
            height: 360,
            bandwidth,
            codecs: "avc1.4d401e,mp4a.40.2".to_owned(),
            target_duration: 6,
        }
    }

    fn segment(position: i32, duration: f64) -> Segment {
        Segment {
            id: Uuid::nil(),
            rendition_id: Uuid::nil(),
            position,
            duration,
            size: 1024,
            telegram_file_id: "x".to_owned(),
            telegram_message_id: 1,
        }
    }

    #[test]
    fn master_lists_every_rendition() {
        let out = Playlist::master("abc", &[rendition("360p", 896_000), rendition("720p", 2_628_000)]);

        assert!(out.starts_with("#EXTM3U\n"));
        assert!(out.contains("BANDWIDTH=896000"));
        assert!(out.contains("/api/stream/abc/360p/index.m3u8"));
        assert!(out.contains("/api/stream/abc/720p/index.m3u8"));
    }

    #[test]
    fn media_is_a_closed_vod_playlist() {
        let out = Playlist::media("abc", &rendition("360p", 896_000), &[segment(0, 6.0), segment(1, 3.5)]);

        assert!(out.contains("#EXT-X-PLAYLIST-TYPE:VOD"));
        assert!(out.contains("#EXT-X-TARGETDURATION:6"));
        assert!(out.contains("#EXTINF:6.000000,\n/api/stream/abc/360p/0.ts"));
        // The tail segment is short; the playlist must still declare its real
        // duration or players drift at the end.
        assert!(out.contains("#EXTINF:3.500000,\n/api/stream/abc/360p/1.ts"));
        assert!(out.trim_end().ends_with("#EXT-X-ENDLIST"));
    }
}
