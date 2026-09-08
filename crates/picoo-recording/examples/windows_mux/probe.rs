use picoo_bitstream::{AccessUnit, Codec, CodecConfiguration, NalFormat, NalLengthSize};
use std::{
    path::{Path, PathBuf},
    ptr,
};
use windows::{
    core::HSTRING,
    Win32::{Media::MediaFoundation::*, System::Com::*},
};

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

struct Runtime;
impl Runtime {
    fn new() -> Result<Self> {
        unsafe {
            CoInitializeEx(None, COINIT_MULTITHREADED).ok()?;
            if let Err(error) = MFStartup(MF_VERSION, MFSTARTUP_FULL) {
                CoUninitialize();
                return Err(error.into());
            }
        }
        Ok(Self)
    }
}
impl Drop for Runtime {
    fn drop(&mut self) {
        unsafe {
            let _ = MFShutdown();
            CoUninitialize();
        }
    }
}
struct Sink(IMFMediaSink);
impl Drop for Sink {
    fn drop(&mut self) {
        unsafe {
            let _ = self.0.Shutdown();
        }
    }
}

pub fn run() -> Result<()> {
    let _runtime = Runtime::new()?;
    let parent = PathBuf::from(
        std::env::args_os()
            .nth(1)
            .ok_or("missing output directory")?,
    );
    std::fs::create_dir_all(&parent)?;
    let output = tempfile::Builder::new()
        .prefix("mf-mux-")
        .tempdir_in(&parent)?
        .keep();
    println!("synthetic mux output: {}", output.display());
    let repository = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let fixtures = repository.join("crates/picoo-media-decode/probes/apple-native-formats");
    let descriptions = repository.join("verification/native-media/mp4-sample-descriptions");
    let mut failures = Vec::new();
    for (wire, codec) in [(1, Codec::Avc), (2, Codec::Hevc)] {
        for (width, height) in [(1280, 720), (1920, 1080)] {
            for fps in [30, 60] {
                let stem = format!("{wire}-{height}-{fps}");
                let record = CodecConfiguration::parse(
                    codec,
                    std::fs::read(fixtures.join(format!("{stem}.config")))?.into(),
                )?;
                record.validate_visible_size(width, height)?;
                let data = std::fs::read(fixtures.join(format!("{stem}.au")))?;
                let au = AccessUnit::parse(
                    codec,
                    NalFormat::LengthPrefixed(NalLengthSize::Four),
                    &data,
                )?;
                let stsd = std::fs::read(descriptions.join(format!("{stem}.stsd")))?;
                // Compare the system's description generation with a supplied
                // native reference before adopting any application box builder.
                for (description, supplied) in
                    [("native", None), ("provided", Some(stsd.as_slice()))]
                {
                    for fragmented in [false, true] {
                        let case = format!("{stem}-{description}-fragmented-{fragmented}");
                        let path = output.join(format!("{case}.mp4"));
                        println!("BEGIN {}", path.display());
                        // MF consumes elementary-stream AUs; the MP4 storage NAL
                        // representation is owned by the sink, not by its input.
                        let payload = au.to_annex_b()?;
                        let result = write(
                            &path, &record, width, height, fps, supplied, &payload, fragmented,
                        )
                        .and_then(|()| read(&path, codec, &au, fps));
                        match result {
                            Ok(()) => println!("PASS {case}"),
                            Err(error) => {
                                let failure = format!("{case}: {error}");
                                eprintln!("FAIL {failure}");
                                failures.push(failure);
                            }
                        }
                    }
                }
            }
        }
    }
    if failures.is_empty() {
        Ok(())
    } else {
        // Independent files let every codec/container combination report its
        // own result. A partial matrix remains a failed validation run.
        Err(format!(
            "{} mux checks failed:\n{}",
            failures.len(),
            failures.join("\n")
        )
        .into())
    }
}

#[allow(clippy::too_many_arguments)]
fn write(
    path: &Path,
    record: &CodecConfiguration,
    width: u32,
    height: u32,
    fps: u32,
    stsd: Option<&[u8]>,
    payload: &[u8],
    fragmented: bool,
) -> Result<()> {
    unsafe {
        let media_type = MFCreateMediaType()?;
        media_type.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Video)?;
        media_type.SetGUID(
            &MF_MT_SUBTYPE,
            if record.codec() == Codec::Avc {
                &MFVideoFormat_H264
            } else {
                &MFVideoFormat_HEVC
            },
        )?;
        media_type.SetUINT64(
            &MF_MT_FRAME_SIZE,
            (u64::from(width) << 32) | u64::from(height),
        )?;
        media_type.SetUINT64(&MF_MT_FRAME_RATE, (u64::from(fps) << 32) | 1)?;
        media_type.SetUINT64(&MF_MT_PIXEL_ASPECT_RATIO, (1_u64 << 32) | 1)?;
        media_type.SetUINT32(&MF_MT_INTERLACE_MODE, MFVideoInterlace_Progressive.0 as u32)?;
        if let Some(stsd) = stsd {
            media_type.SetBlob(&MF_MT_MPEG4_SAMPLE_DESCRIPTION, stsd)?;
            media_type.SetUINT32(&MF_MT_MPEG4_CURRENT_SAMPLE_ENTRY, 0)?;
        }
        let mut sequence = Vec::new();
        for nal in record.vps().iter().chain(record.sps()).chain(record.pps()) {
            sequence.extend_from_slice(&[0, 0, 0, 1]);
            sequence.extend_from_slice(nal);
        }
        media_type.SetBlob(&MF_MT_MPEG_SEQUENCE_HEADER, &sequence)?;
        let stream = MFCreateFile(
            MF_ACCESSMODE_READWRITE,
            MF_OPENMODE_FAIL_IF_EXIST,
            MF_FILEFLAGS_NONE,
            &HSTRING::from(path.as_os_str()),
        )?;
        println!("create media sink");
        let sink = Sink(if fragmented {
            MFCreateFMPEG4MediaSink(&stream, &media_type, None)
                .map_err(|error| format!("MFCreateFMPEG4MediaSink: {error}"))?
        } else {
            MFCreateMPEG4MediaSink(&stream, &media_type, None)
                .map_err(|error| format!("MFCreateMPEG4MediaSink: {error}"))?
        });
        let sink_id = sink.0.GetStreamSinkByIndex(0)?.GetIdentifier()?;
        // The SinkWriter uses zero-based stream indices. A sink's own stream
        // identifier is a separate namespace and need not match its ordinal.
        let index = 0;
        println!("sink stream ID={sink_id}, writer stream index={index}");
        let writer = MFCreateSinkWriterFromMediaSink(&sink.0, None)?;
        // Identical compressed input/output: no encoder or decoded pixels.
        println!("set compressed input type");
        writer.SetInputMediaType(index, &media_type, None)?;
        println!("begin writing");
        writer.BeginWriting()?;
        for frame in 0..=3 * fps {
            let sample = MFCreateSample()?;
            let buffer = MFCreateMemoryBuffer(u32::try_from(payload.len())?)?;
            let mut destination = ptr::null_mut();
            buffer.Lock(&mut destination, None, None)?;
            ptr::copy_nonoverlapping(payload.as_ptr(), destination, payload.len());
            buffer.Unlock()?;
            buffer.SetCurrentLength(u32::try_from(payload.len())?)?;
            sample.AddBuffer(&buffer)?;
            let pts = i64::from(frame) * 10_000_000 / i64::from(fps);
            let next = i64::from(frame + 1) * 10_000_000 / i64::from(fps);
            sample.SetSampleTime(pts)?;
            sample.SetSampleDuration(next - pts)?;
            sample.SetUINT32(&MFSampleExtension_CleanPoint, 1)?;
            writer.WriteSample(index, &sample)?;
        }
        println!("finalize");
        writer
            .Finalize()
            .map_err(|error| format!("SinkWriter.Finalize: {error}"))?;
        // Finalize owns completion of the output; then release the writer/sink
        // and their stream references. The native matrix confirmed that an
        // additional explicit Close returns E_INVALIDARG even for fully
        // finalized and independently decodable AVC files.
        drop(writer);
        drop(sink);
    }
    Ok(())
}

fn pictures(codec: Codec, au: &AccessUnit<'_>) -> Vec<Vec<u8>> {
    au.nals()
        .iter()
        .filter(|nal| match codec {
            Codec::Avc => matches!(nal[0] & 31, 1..=5),
            Codec::Hevc => (nal[0] >> 1) & 63 <= 31,
        })
        .map(|nal| nal.to_vec())
        .collect()
}

fn read(path: &Path, codec: Codec, expected: &AccessUnit<'_>, fps: u32) -> Result<()> {
    unsafe {
        println!("read compressed output");
        let reader = MFCreateSourceReaderFromURL(&HSTRING::from(path.as_os_str()), None)?;
        let index = MF_SOURCE_READER_FIRST_VIDEO_STREAM.0 as u32;
        let native = reader.GetNativeMediaType(index, 0)?;
        reader.SetCurrentMediaType(index, None, &native)?;
        let expected = pictures(codec, expected);
        let mut frames = 0;
        // A finite read bound also catches pathological marker-only output.
        for _ in 0..(3 * fps + 32) {
            let mut flags = 0;
            let mut pts = 0;
            let mut sample = None;
            reader.ReadSample(
                index,
                0,
                None,
                Some(&mut flags),
                Some(&mut pts),
                Some(&mut sample),
            )?;
            if let Some(sample) = sample {
                let buffer = sample.ConvertToContiguousBuffer()?;
                let mut source = ptr::null_mut();
                let mut length = 0;
                buffer.Lock(&mut source, None, Some(&mut length))?;
                if source.is_null() || length == 0 {
                    buffer.Unlock()?;
                    return Err("empty compressed sample".into());
                }
                let bytes = std::slice::from_raw_parts(source, length as usize).to_vec();
                buffer.Unlock()?;
                // MF's compressed source output is Annex B, independent of
                // the length-prefixed MP4 storage representation.
                let actual = AccessUnit::parse(codec, NalFormat::AnnexB, &bytes)?;
                if pictures(codec, &actual) != expected {
                    return Err("VCL bytes changed".into());
                }
                let expected_pts = i64::from(frames) * 10_000_000 / i64::from(fps);
                if (pts - expected_pts).abs() > 10 {
                    return Err(format!("PTS changed: {pts} vs {expected_pts}").into());
                }
                frames += 1;
            }
            if flags & MF_SOURCE_READERF_ENDOFSTREAM.0 as u32 != 0 {
                if frames != 3 * fps + 1 {
                    return Err(format!("frame count {frames}").into());
                }
                return Ok(());
            }
        }
    }
    Err("source did not reach EOF".into())
}
