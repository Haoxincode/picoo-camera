//! Keep access to written bytes across AVAssetWriter's unlink-on-cancel.
use std::{
    fs::File,
    io::{self, Seek, SeekFrom},
    path::{Path, PathBuf},
};

pub(super) struct RetainedPartial {
    path: PathBuf,
    file: File,
}
impl RetainedPartial {
    pub(super) fn open(path: &Path) -> io::Result<Self> {
        Ok(Self {
            path: path.to_owned(),
            file: File::open(path)?,
        })
    }
    /// Only after native cancellation/completion has stopped further writes.
    pub(super) fn restore(&mut self) -> io::Result<()> {
        if self.path.try_exists()? {
            use std::os::unix::fs::MetadataExt;
            let existing = std::fs::metadata(&self.path)?;
            let retained = self.file.metadata()?;
            if (existing.dev(), existing.ino()) == (retained.dev(), retained.ino()) {
                return Ok(());
            }
            return Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                "partial path was replaced",
            ));
        }
        let parent = self
            .path
            .parent()
            .ok_or_else(|| io::Error::other("missing segment parent"))?;
        let mut temporary = tempfile::NamedTempFile::new_in(parent)?;
        self.file.seek(SeekFrom::Start(0))?;
        io::copy(&mut self.file, temporary.as_file_mut())?;
        temporary.as_file().sync_all()?;
        temporary
            .persist_noclobber(&self.path)
            .map_err(|error| error.error)?;
        File::open(parent)?.sync_all()?;
        self.file = File::open(&self.path)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn preservation_never_overwrites_a_replacement_and_reports_storage_failure() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("segment.partial");
        std::fs::write(&path, b"original").unwrap();
        let mut retained = RetainedPartial::open(&path).unwrap();
        std::fs::remove_file(&path).unwrap();
        std::fs::write(&path, b"replacement").unwrap();
        assert!(retained.restore().is_err());
        assert_eq!(std::fs::read(&path).unwrap(), b"replacement");
        std::fs::remove_file(path).unwrap();
        std::fs::remove_dir(dir.path()).unwrap();
        assert!(retained.restore().is_err());
    }
}
