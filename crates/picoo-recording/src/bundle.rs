//! Durable recording results — REQ-PICOO-MEDIA-067.
//! All operations run on the recording worker, including hash, sync and rename.
use crate::{FinalizedSegment, RecordingError};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::{
    fs::{self, File},
    io::{Read, Write},
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

const MAX_SEGMENTS: usize = 4096;
const MAX_GAPS: usize = 4096;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum RecordingState {
    Arming,
    Recording,
    Complete,
    HasGaps,
    Failed,
}
impl RecordingState {
    fn terminal(self) -> bool {
        matches!(self, Self::Complete | Self::HasGaps | Self::Failed)
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct SourceRange {
    pub connection_generation: u64,
    pub stream_epoch: u32,
    pub first_au: u64,
    pub last_au: u64,
    pub first_pts_us: u64,
    pub last_pts_us: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct SegmentMetadata {
    pub source: SourceRange,
    pub codec: String,
    pub width: u32,
    pub height: u32,
    pub fps: u32,
    pub rotation: u32,
    pub mirrored: bool,
    pub configuration_sha256: String,
}

#[derive(Debug, Clone, Copy, Serialize)]
pub enum GapReason {
    NetworkLoss,
    SourceStopped,
    TimeDiscontinuity,
    Overrun,
}

#[derive(Debug, Clone, Serialize)]
struct Gap {
    reason: GapReason,
    source: Option<SourceRange>,
}
#[derive(Debug, Clone, Serialize)]
struct Segment {
    file: String,
    sha256: String,
    bytes: u64,
    metadata: SegmentMetadata,
}
#[derive(Debug, Clone, Serialize)]
struct Manifest {
    recording_id: String,
    mode: &'static str,
    software_version: &'static str,
    state: RecordingState,
    created_at_unix_ms: u64,
    actual_start_pts_us: Option<u64>,
    ended_at_unix_ms: Option<u64>,
    failure: Option<String>,
    segments: Vec<Segment>,
    gaps: Vec<Gap>,
}

pub struct RecordingBundle {
    path: PathBuf,
    manifest: Manifest,
}

impl RecordingBundle {
    /// The chosen parent must already exist. A private, unique child is created;
    /// existing recording directories and state are never reopened or migrated.
    pub fn create(parent: &Path) -> Result<Self, RecordingError> {
        let parent = parent.canonicalize()?;
        let mut builder = tempfile::Builder::new();
        builder.prefix("recording-");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            builder.permissions(fs::Permissions::from_mode(0o700));
        }
        let path = builder.tempdir_in(parent)?.keep();
        fs::create_dir(path.join("segments"))?;
        let recording_id = path.file_name().unwrap().to_string_lossy().into_owned();
        let bundle = Self {
            path,
            manifest: Manifest {
                recording_id,
                mode: "Encoded",
                software_version: env!("CARGO_PKG_VERSION"),
                state: RecordingState::Arming,
                created_at_unix_ms: now_ms(),
                actual_start_pts_us: None,
                ended_at_unix_ms: None,
                failure: None,
                segments: Vec::new(),
                gaps: Vec::new(),
            },
        };
        bundle.persist(&bundle.manifest)?;
        Ok(bundle)
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
    pub fn state(&self) -> RecordingState {
        self.manifest.state
    }
    pub fn next_partial_path(&self) -> Result<PathBuf, RecordingError> {
        self.ensure_open()?;
        if self.manifest.segments.len() >= MAX_SEGMENTS {
            return Err(RecordingError::InvalidInput("segment capacity reached"));
        }
        Ok(self
            .path
            .join("segments")
            .join(format!("{:06}.partial", self.manifest.segments.len() + 1)))
    }

    pub fn mark_started(&mut self, source_pts_us: u64) -> Result<(), RecordingError> {
        self.ensure_open()?;
        if self.manifest.actual_start_pts_us.is_some() {
            return Ok(());
        }
        let mut next = self.manifest.clone();
        next.actual_start_pts_us = Some(source_pts_us);
        next.state = RecordingState::Recording;
        self.commit(next)
    }

    pub fn record_gap(
        &mut self,
        reason: GapReason,
        source: Option<SourceRange>,
    ) -> Result<(), RecordingError> {
        self.ensure_open()?;
        if self.manifest.gaps.len() >= MAX_GAPS {
            self.fail("gap capacity reached")?;
            return Err(RecordingError::InvalidInput("gap capacity reached"));
        }
        let mut next = self.manifest.clone();
        next.gaps.push(Gap { reason, source });
        self.commit(next)
    }

    pub fn commit_segment(
        &mut self,
        finalized: FinalizedSegment,
        metadata: SegmentMetadata,
    ) -> Result<(), RecordingError> {
        self.ensure_open()?;
        let result = self.commit_segment_inner(finalized, metadata);
        if result.is_err() && !self.manifest.state.terminal() {
            // Preserve the original error; failure persistence is best effort
            // when the storage itself caused the segment commit to fail.
            let _ = self.fail("segment commit failed");
        }
        result
    }

    fn commit_segment_inner(
        &mut self,
        finalized: FinalizedSegment,
        metadata: SegmentMetadata,
    ) -> Result<(), RecordingError> {
        let expected = self.next_partial_path()?;
        if finalized.path != expected {
            return Err(RecordingError::InvalidInput(
                "segment belongs to another bundle or index",
            ));
        }
        if metadata.source.first_au > metadata.source.last_au
            || metadata.source.first_pts_us > metadata.source.last_pts_us
            || !matches!(metadata.codec.as_str(), "avc" | "hevc")
            || !matches!(metadata.fps, 30 | 60)
            || metadata.width == 0
            || metadata.height == 0
            || !matches!(metadata.rotation, 0 | 90 | 180 | 270)
            || metadata.configuration_sha256.len() != 64
            || !metadata
                .configuration_sha256
                .bytes()
                .all(|b| b.is_ascii_hexdigit())
        {
            return Err(RecordingError::InvalidInput("invalid segment metadata"));
        }
        let mut file = File::options().read(true).write(true).open(&expected)?;
        let info = file.metadata()?;
        if !info.is_file() || info.len() == 0 {
            return Err(RecordingError::InvalidInput("empty or non-file segment"));
        }
        file.sync_all()?;
        let sha256 = hash_reader(&mut file)?;
        drop(file);
        let complete = expected.with_extension("mp4");
        let mut temporary = tempfile::TempPath::try_from_path(expected)?;
        // An unsuccessful promotion must retain the partial recording artifact.
        temporary.disable_cleanup(true);
        temporary
            .persist_noclobber(&complete)
            .map_err(|error| error.error)?;
        sync_directory(&self.path.join("segments"))?;
        let mut next = self.manifest.clone();
        next.segments.push(Segment {
            file: format!(
                "segments/{}",
                complete.file_name().unwrap().to_string_lossy()
            ),
            sha256,
            bytes: info.len(),
            metadata,
        });
        self.commit(next)
    }

    pub fn finish(&mut self) -> Result<(), RecordingError> {
        if self.manifest.state.terminal() {
            return Ok(());
        }
        if self.manifest.segments.is_empty() {
            return self.fail("no finalized video segment");
        }
        let mut next = self.manifest.clone();
        next.state = if next.gaps.is_empty() {
            RecordingState::Complete
        } else {
            RecordingState::HasGaps
        };
        next.ended_at_unix_ms = Some(now_ms());
        self.commit(next)
    }

    pub fn fail(&mut self, reason: &str) -> Result<(), RecordingError> {
        if self.manifest.state == RecordingState::Failed {
            return Ok(());
        }
        if self.manifest.state.terminal() {
            return Err(RecordingError::InvalidInput("recording already finalized"));
        }
        let mut next = self.manifest.clone();
        next.state = RecordingState::Failed;
        next.failure = Some(reason.chars().take(512).collect());
        next.ended_at_unix_ms = Some(now_ms());
        self.commit(next)
    }

    fn ensure_open(&self) -> Result<(), RecordingError> {
        if self.manifest.state.terminal() {
            Err(RecordingError::InvalidInput("recording already finalized"))
        } else {
            Ok(())
        }
    }
    fn commit(&mut self, next: Manifest) -> Result<(), RecordingError> {
        match self.persist(&next) {
            Ok(()) => {
                self.manifest = next;
                Ok(())
            }
            Err(error) => {
                // Failure is sticky in memory even when storage cannot accept
                // a failure marker. A later finish must never report Complete.
                self.manifest = next;
                self.manifest.state = RecordingState::Failed;
                self.manifest.failure = Some("manifest persistence failed".into());
                self.manifest.ended_at_unix_ms = Some(now_ms());
                Err(error)
            }
        }
    }
    fn persist(&self, manifest: &Manifest) -> Result<(), RecordingError> {
        let mut temporary = tempfile::NamedTempFile::new_in(&self.path)?;
        serde_json::to_writer_pretty(temporary.as_file_mut(), manifest)?;
        temporary.flush()?;
        temporary.as_file().sync_all()?;
        temporary
            .persist(self.path.join("manifest.json"))
            .map_err(|error| error.error)?;
        sync_directory(&self.path)?;
        Ok(())
    }
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .min(u128::from(u64::MAX)) as u64
}
fn hash_reader(reader: &mut impl Read) -> Result<String, RecordingError> {
    let mut hash = Sha256::new();
    let mut buffer = [0; 64 * 1024];
    loop {
        let count = reader.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        hash.update(&buffer[..count]);
    }
    Ok(format!("{:x}", hash.finalize()))
}
fn sync_directory(path: &Path) -> Result<(), RecordingError> {
    #[cfg(unix)]
    File::open(path)?.sync_all()?;
    #[cfg(not(unix))]
    let _ = path;
    Ok(())
}

#[cfg(test)]
mod tests;
