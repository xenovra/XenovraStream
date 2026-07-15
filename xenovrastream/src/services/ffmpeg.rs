use std::path::{Path, PathBuf};
use std::process::Stdio;

use serde::Deserialize;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;

use crate::errors::{XenovraStreamError, XenovraStreamResult};

/// What ffprobe tells us about the uploaded file, reduced to what we need in
/// order to pick a ladder.
#[derive(Debug, Clone)]
pub struct SourceInfo {
    pub duration_secs: f64,
    pub width: i32,
    pub height: i32,
    pub has_audio: bool,
}

/// One rung of the ABR ladder.
#[derive(Debug, Clone, Copy)]
pub struct Rung {
    pub name: &'static str,
    pub height: i32,
    /// Video bitrate in kbit/s.
    pub video_kbps: i32,
    /// Audio bitrate in kbit/s.
    pub audio_kbps: i32,
    /// RFC 6381 codec string matching the x264 profile/level we ask for below.
    pub codecs: &'static str,
}

/// The ladder, smallest first. We never transcode a rung taller than the source
/// — upscaling costs CPU and Telegram storage to deliver a blurrier picture than
/// the original.
pub const LADDER: [Rung; 3] = [
    Rung {
        name: "360p",
        height: 360,
        video_kbps: 800,
        audio_kbps: 96,
        // H.264 main@3.0 + AAC-LC
        codecs: "avc1.4d401e,mp4a.40.2",
    },
    Rung {
        name: "720p",
        height: 720,
        video_kbps: 2500,
        audio_kbps: 128,
        // main@3.1
        codecs: "avc1.4d401f,mp4a.40.2",
    },
    Rung {
        name: "1080p",
        height: 1080,
        video_kbps: 5000,
        audio_kbps: 192,
        // high@4.0
        codecs: "avc1.640028,mp4a.40.2",
    },
];

#[derive(Deserialize)]
struct ProbeOutput {
    format: ProbeFormat,
    streams: Vec<ProbeStream>,
}

#[derive(Deserialize)]
struct ProbeFormat {
    duration: Option<String>,
}

#[derive(Deserialize)]
struct ProbeStream {
    codec_type: Option<String>,
    width: Option<i32>,
    height: Option<i32>,
}

pub struct Ffmpeg;

impl Ffmpeg {
    pub async fn probe(source: &Path) -> XenovraStreamResult<SourceInfo> {
        let output = Command::new("ffprobe")
            .args([
                "-v",
                "error",
                "-print_format",
                "json",
                "-show_format",
                "-show_streams",
            ])
            .arg(source)
            .output()
            .await
            .map_err(|e| XenovraStreamError::FfmpegError(format!("cannot run ffprobe: {e}")))?;

        if !output.status.success() {
            let err = String::from_utf8_lossy(&output.stderr);
            return Err(XenovraStreamError::FfmpegError(format!("probe failed: {err}")));
        }

        let probe: ProbeOutput = serde_json::from_slice(&output.stdout)
            .map_err(|e| XenovraStreamError::FfmpegError(format!("cannot parse probe: {e}")))?;

        let video = probe
            .streams
            .iter()
            .find(|s| s.codec_type.as_deref() == Some("video"))
            .ok_or(XenovraStreamError::NotAVideo)?;

        // A file can carry a "video" stream that is really cover art; without
        // dimensions there is nothing to scale, so treat it as not-a-video.
        let (width, height) = match (video.width, video.height) {
            (Some(w), Some(h)) if w > 0 && h > 0 => (w, h),
            _ => return Err(XenovraStreamError::NotAVideo),
        };

        let duration_secs = probe
            .format
            .duration
            .as_deref()
            .and_then(|d| d.parse::<f64>().ok())
            .filter(|d| *d > 0.0)
            .ok_or_else(|| XenovraStreamError::FfmpegError("source has no duration".to_owned()))?;

        let has_audio = probe
            .streams
            .iter()
            .any(|s| s.codec_type.as_deref() == Some("audio"));

        Ok(SourceInfo {
            duration_secs,
            width,
            height,
            has_audio,
        })
    }

    /// The rungs worth producing for this source. Always at least one, so a
    /// 240p source still yields a playable `360p` entry (scaled to its own
    /// height by the `-2:h` expression, not blown up).
    pub fn ladder_for(info: &SourceInfo) -> Vec<Rung> {
        let fitting: Vec<Rung> = LADDER
            .iter()
            .filter(|r| r.height <= info.height)
            .copied()
            .collect();

        if fitting.is_empty() {
            vec![LADDER[0]]
        } else {
            fitting
        }
    }

    /// Transcodes one rung to an HLS playlist plus `.ts` segments under
    /// `out_dir`. `on_progress` is called with 0.0-1.0 as ffmpeg reports it.
    ///
    /// Segments are cut on forced keyframes at exactly `segment_secs`, which is
    /// what lets a player switch rungs mid-playback without artifacts.
    pub async fn transcode_rung<F>(
        source: &Path,
        out_dir: &Path,
        rung: &Rung,
        info: &SourceInfo,
        segment_secs: u8,
        preset: &str,
        mut on_progress: F,
    ) -> XenovraStreamResult<PathBuf>
    where
        F: FnMut(f64),
    {
        tokio::fs::create_dir_all(out_dir).await?;

        let playlist = out_dir.join("index.m3u8");
        let seg_pattern = out_dir.join("%05d.ts");
        let maxrate = format!("{}k", rung.video_kbps);
        let bufsize = format!("{}k", rung.video_kbps * 2);
        let keyframe_expr = format!("expr:gte(t,n_forced*{segment_secs})");
        // `-2` keeps the source aspect ratio and rounds width to an even number,
        // which H.264 requires.
        let scale = format!("scale=-2:{}", rung.height.min(info.height));

        let mut cmd = Command::new("ffmpeg");
        cmd.args(["-nostdin", "-y", "-loglevel", "error"])
            .arg("-i")
            .arg(source)
            .args(["-vf", &scale])
            .args(["-c:v", "libx264", "-preset", preset, "-crf", "23"])
            .args(["-maxrate", &maxrate, "-bufsize", &bufsize])
            .args(["-force_key_frames", &keyframe_expr])
            .args(["-sc_threshold", "0"]);

        if info.has_audio {
            let abr = format!("{}k", rung.audio_kbps);
            cmd.args(["-c:a", "aac", "-b:a", &abr, "-ac", "2"]);
        } else {
            cmd.arg("-an");
        }

        cmd.args(["-f", "hls"])
            .args(["-hls_time", &segment_secs.to_string()])
            .args(["-hls_playlist_type", "vod"])
            .args(["-hls_segment_type", "mpegts"])
            .args(["-hls_flags", "independent_segments"])
            .args(["-hls_list_size", "0"])
            .arg("-hls_segment_filename")
            .arg(&seg_pattern)
            .args(["-progress", "pipe:1", "-nostats"])
            .arg(&playlist)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        let mut child = cmd
            .spawn()
            .map_err(|e| XenovraStreamError::FfmpegError(format!("cannot run ffmpeg: {e}")))?;

        // `-progress pipe:1` emits `key=value` lines; out_time_ms against the
        // probed duration is the only reliable progress signal ffmpeg gives.
        if let Some(stdout) = child.stdout.take() {
            let mut lines = BufReader::new(stdout).lines();
            while let Some(line) = lines.next_line().await? {
                if let Some(us) = line.strip_prefix("out_time_us=") {
                    if let Ok(us) = us.trim().parse::<f64>() {
                        let done = (us / 1_000_000.0 / info.duration_secs).clamp(0.0, 1.0);
                        on_progress(done);
                    }
                }
            }
        }

        let output = child
            .wait_with_output()
            .await
            .map_err(|e| XenovraStreamError::FfmpegError(e.to_string()))?;

        if !output.status.success() {
            let err = String::from_utf8_lossy(&output.stderr);
            let err = err.trim();
            let tail = err.chars().rev().take(500).collect::<String>();
            let tail: String = tail.chars().rev().collect();
            return Err(XenovraStreamError::FfmpegError(format!(
                "{} exited with {}: {tail}",
                rung.name, output.status
            )));
        }

        on_progress(1.0);
        Ok(playlist)
    }

    /// Reads an ffmpeg-written VOD playlist back into `(segment file, duration)`
    /// pairs, in playback order. We re-read rather than predict because the last
    /// segment is always short and keyframe placement can nudge the rest.
    pub async fn parse_playlist(playlist: &Path) -> XenovraStreamResult<Vec<(PathBuf, f64)>> {
        let body = tokio::fs::read_to_string(playlist).await?;
        let dir = playlist.parent().unwrap_or(Path::new("."));

        let mut segments = Vec::new();
        let mut pending_duration: Option<f64> = None;

        for line in body.lines() {
            let line = line.trim();

            if let Some(rest) = line.strip_prefix("#EXTINF:") {
                let value = rest.split(',').next().unwrap_or("").trim();
                pending_duration = value.parse::<f64>().ok();
            } else if !line.is_empty() && !line.starts_with('#') {
                if let Some(duration) = pending_duration.take() {
                    segments.push((dir.join(line), duration));
                }
            }
        }

        if segments.is_empty() {
            return Err(XenovraStreamError::FfmpegError(
                "ffmpeg produced no segments".to_owned(),
            ));
        }

        Ok(segments)
    }
}
