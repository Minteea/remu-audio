// Symphonia decoder
// Code from rodio

use core::time::Duration;
use symphonia::{
    core::{
        audio::{AudioBufferRef, SampleBuffer, SignalSpec},
        codecs::{Decoder, DecoderOptions, CODEC_TYPE_NULL},
        errors::Error,
        formats::{FormatOptions, FormatReader, SeekMode, SeekTo, SeekedTo},
        io::MediaSourceStream,
        meta::MetadataOptions,
        probe::Hint,
        units,
    },
    default::get_probe,
};

use super::Settings;
use rodio::{decoder::DecoderError, source, ChannelCount, Sample, SampleRate, Source};

pub struct SymphoniaDecoder {
    decoder: Box<dyn Decoder>,
    current_span_offset: usize,
    format: Box<dyn FormatReader>,
    total_duration: Option<Duration>,
    buffer: SampleBuffer<Sample>,
    spec: SignalSpec,
    seek_mode: SeekMode,
}

impl SymphoniaDecoder {
    pub fn new(mss: MediaSourceStream, settings: &Settings) -> Result<Self, DecoderError> {
        match SymphoniaDecoder::init(mss, settings) {
            Err(e) => match e {
                Error::IoError(e) => Err(DecoderError::IoError(e.to_string())),
                Error::DecodeError(e) => Err(DecoderError::DecodeError(e)),
                Error::SeekError(_) => {
                    unreachable!("Seek errors should not occur during initialization")
                }
                Error::Unsupported(_) => Err(DecoderError::UnrecognizedFormat),
                Error::LimitError(e) => Err(DecoderError::LimitError(e)),
                Error::ResetRequired => Err(DecoderError::ResetRequired),
            },
            Ok(Some(decoder)) => Ok(decoder),
            Ok(None) => Err(DecoderError::NoStreams),
        }
    }

    #[inline]
    pub(crate) fn into_inner(self) -> MediaSourceStream {
        self.format.into_inner()
    }

    fn init(
        mss: MediaSourceStream,
        settings: &Settings,
    ) -> symphonia::core::errors::Result<Option<SymphoniaDecoder>> {
        let mut hint = Hint::new();
        if let Some(ext) = settings.hint.as_ref() {
            hint.with_extension(ext);
        }
        if let Some(typ) = settings.mime_type.as_ref() {
            hint.mime_type(typ);
        }
        let format_opts: FormatOptions = FormatOptions {
            enable_gapless: settings.gapless,
            ..Default::default()
        };
        let metadata_opts: MetadataOptions = Default::default();
        let seek_mode = if settings.coarse_seek {
            SeekMode::Coarse
        } else {
            SeekMode::Accurate
        };
        let mut probed = get_probe().format(&hint, mss, &format_opts, &metadata_opts)?;

        // Prefer metadata that's provided in the container format, over other tags found during the
        // probe operation.
        if let Some(metadata_rev) = probed.metadata.get().as_ref().and_then(|m| m.current()) {
            println!("Tags: {:?}", metadata_rev.tags());
        }

        let stream = match probed.format.default_track() {
            Some(stream) => stream,
            None => return Ok(None),
        };

        // Select the first supported track
        let track_id = probed
            .format
            .tracks()
            .iter()
            .find(|t| t.codec_params.codec != CODEC_TYPE_NULL)
            .ok_or(symphonia::core::errors::Error::Unsupported(
                "No track with supported codec",
            ))?
            .id;

        let track = match probed
            .format
            .tracks()
            .iter()
            .find(|track| track.id == track_id)
        {
            Some(track) => track,
            None => return Ok(None),
        };

        let mut decoder = symphonia::default::get_codecs()
            .make(&track.codec_params, &DecoderOptions::default())?;
        let total_duration = stream
            .codec_params
            .time_base
            .zip(stream.codec_params.n_frames)
            .map(|(base, spans)| base.calc_time(spans).into());

        let decoded = loop {
            let current_span = match probed.format.next_packet() {
                Ok(packet) => packet,
                Err(Error::IoError(_)) => break decoder.last_decoded(),
                Err(e) => return Err(e),
            };

            // If the packet does not belong to the selected track, skip over it
            if current_span.track_id() != track_id {
                continue;
            }

            match decoder.decode(&current_span) {
                Ok(decoded) => break decoded,
                Err(e) => match e {
                    Error::DecodeError(_) => {
                        // Decode errors are intentionally ignored with no retry limit.
                        // This behavior ensures that the decoder skips over problematic packets
                        // and continues processing the rest of the stream.
                        continue;
                    }
                    _ => return Err(e),
                },
            }
        };
        let spec = decoded.spec().to_owned();
        let buffer = SymphoniaDecoder::get_buffer(decoded, &spec);
        Ok(Some(SymphoniaDecoder {
            decoder,
            current_span_offset: 0,
            format: probed.format,
            total_duration,
            buffer,
            spec,
            seek_mode,
        }))
    }

    #[inline]
    fn get_buffer(decoded: AudioBufferRef, spec: &SignalSpec) -> SampleBuffer<Sample> {
        let duration = units::Duration::from(decoded.capacity() as u64);
        let mut buffer = SampleBuffer::<Sample>::new(duration, *spec);
        buffer.copy_interleaved_ref(decoded);
        buffer
    }
}

impl Source for SymphoniaDecoder {
    #[inline]
    fn current_span_len(&self) -> Option<usize> {
        Some(self.buffer.len())
    }

    #[inline]
    fn channels(&self) -> ChannelCount {
        self.spec.channels.count() as ChannelCount
    }

    #[inline]
    fn sample_rate(&self) -> SampleRate {
        self.spec.rate
    }

    #[inline]
    fn total_duration(&self) -> Option<Duration> {
        self.total_duration
    }

    fn try_seek(&mut self, pos: Duration) -> Result<(), source::SeekError> {
        if matches!(self.seek_mode, SeekMode::Accurate)
            && self.decoder.codec_params().time_base.is_none()
        {
            return Err(source::SeekError::SymphoniaDecoder(
                rodio::decoder::symphonia::SeekError::AccurateSeekNotSupported,
            ));
        }

        // Seeking should be "saturating", meaning: target positions beyond the end of the stream
        // are clamped to the end.
        let mut target = pos;
        if let Some(total_duration) = self.total_duration {
            if target > total_duration {
                target = total_duration;
            }
        }

        // Remember the current channel, so we can restore it after seeking.
        let active_channel = self.current_span_offset % self.channels() as usize;

        let seek_res = match self.format.seek(
            self.seek_mode,
            SeekTo::Time {
                time: target.into(),
                track_id: None,
            },
        ) {
            Err(Error::SeekError(symphonia::core::errors::SeekErrorKind::ForwardOnly)) => {
                return Err(source::SeekError::SymphoniaDecoder(
                    rodio::decoder::symphonia::SeekError::RandomAccessNotSupported,
                ));
            }
            other => other.map_err(rodio::decoder::symphonia::SeekError::Demuxer),
        }?;

        // Seeking is a demuxer operation without the decoder knowing about it,
        // so we need to reset the decoder to make sure it's in sync and prevent
        // audio glitches.
        self.decoder.reset();

        // Force the iterator to decode the next packet.
        self.current_span_offset = usize::MAX;

        // Symphonia does not seek to the exact position, it seeks to the closest keyframe.
        // If accurate seeking is required, fast-forward to the exact position.
        if matches!(self.seek_mode, SeekMode::Accurate) {
            self.refine_position(seek_res)?;
        }

        // After seeking, we are at the beginning of an inter-sample frame, i.e. the first
        // channel. We need to advance the iterator to the right channel.
        for _ in 0..active_channel {
            self.next();
        }

        Ok(())
    }
}

impl SymphoniaDecoder {
    /// Note span offset must be set after
    fn refine_position(&mut self, seek_res: SeekedTo) -> Result<(), source::SeekError> {
        // Calculate the number of samples to skip.
        let mut samples_to_skip = (Duration::from(
            self.decoder
                .codec_params()
                .time_base
                .expect("time base availability guaranteed by caller")
                .calc_time(seek_res.required_ts.saturating_sub(seek_res.actual_ts)),
        )
        .as_secs_f32()
            * self.sample_rate() as f32
            * self.channels() as f32)
            .ceil() as usize;

        // Re-align the seek position to the first channel.
        samples_to_skip -= samples_to_skip % self.channels() as usize;

        // Skip ahead to the precise position.
        for _ in 0..samples_to_skip {
            self.next();
        }

        Ok(())
    }
}

impl Iterator for SymphoniaDecoder {
    type Item = Sample;

    fn next(&mut self) -> Option<Self::Item> {
        if self.current_span_offset >= self.buffer.len() {
            let decoded = loop {
                let packet = self.format.next_packet().ok()?;
                let decoded = match self.decoder.decode(&packet) {
                    Ok(decoded) => decoded,
                    Err(Error::DecodeError(_)) => {
                        // Skip over packets that cannot be decoded. This ensures the iterator
                        // continues processing subsequent packets instead of terminating due to
                        // non-critical decode errors.
                        continue;
                    }
                    Err(_) => return None,
                };

                // Loop until we get a packet with audio frames. This is necessary because some
                // formats can have packets with only metadata, particularly when rewinding, in
                // which case the iterator would otherwise end with `None`.
                // Note: checking `decoded.frames()` is more reliable than `packet.dur()`, which
                // can resturn non-zero durations for packets without audio frames.
                if decoded.frames() > 0 {
                    break decoded;
                }
            };

            decoded.spec().clone_into(&mut self.spec);
            self.buffer = SymphoniaDecoder::get_buffer(decoded, &self.spec);
            self.current_span_offset = 0;
        }

        let sample = *self.buffer.samples().get(self.current_span_offset)?;
        self.current_span_offset += 1;

        Some(sample)
    }
}
