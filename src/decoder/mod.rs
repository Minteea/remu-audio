//! Decodes audio samples from various audio file formats.
//!
//! This module provides decoders for common audio formats like MP3, WAV, Vorbis and FLAC.
//! It supports both one-shot playback and looped playback of audio files.
//!
//! # Usage
//!
//! The simplest way to decode files (automatically sets up seeking and duration):
//! ```no_run
//! use std::fs::File;
//! use rodio::Decoder;
//!
//! let file = File::open("audio.mp3").unwrap();
//! let decoder = Decoder::try_from(file).unwrap();  // Automatically sets byte_len from metadata
//! ```
//!
//! For more control over decoder settings, use the builder pattern:
//! ```no_run
//! use std::fs::File;
//! use rodio::Decoder;
//!
//! let file = File::open("audio.mp3").unwrap();
//! let len = file.metadata().unwrap().len();
//!
//! let decoder = Decoder::builder()
//!     .with_data(file)
//!     .with_byte_len(len)      // Enable seeking and duration calculation
//!     .with_seekable(true)     // Enable seeking operations
//!     .with_hint("mp3")        // Optional format hint
//!     .with_gapless(true)      // Enable gapless playback
//!     .build()
//!     .unwrap();
//! ```
//!
//! # Features
//!
//! The following audio formats are supported based on enabled features:
//!
//! - `wav` - WAV format support
//! - `flac` - FLAC format support
//! - `vorbis` - Vorbis format support
//! - `mp3` - MP3 format support via minimp3
//! - `symphonia` - Enhanced format support via the Symphonia backend
//!
//! When using `symphonia`, additional formats like AAC and MP4 containers become available
//! if the corresponding features are enabled.

use std::{
    io::{BufReader, Read, Seek},
    marker::PhantomData,
    time::Duration,
};

#[allow(unused_imports)]
use std::io::SeekFrom;

use rodio::{
    decoder::DecoderError,
    source::{SeekError, Source},
    ChannelCount, Sample, SampleRate,
};

pub mod builder;
pub use builder::{DecoderBuilder, Settings};

mod read_seek_source;
/// Symphonia decoders types
pub mod symphonia;

/// Source of audio samples decoded from an input stream.
/// See the [module-level documentation](self) for examples and usage.
pub struct Decoder<R: Read + Seek>(DecoderImpl<R>);

/// Source of audio samples from decoding a file that never ends.
/// When the end of the file is reached, the decoder starts again from the beginning.
///
/// A `LoopedDecoder` will attempt to seek back to the start of the stream when it reaches
/// the end. If seeking fails for any reason (like IO errors), iteration will stop.
///
/// # Examples
///
/// ```no_run
/// use std::fs::File;
/// use rodio::Decoder;
///
/// let file = File::open("audio.mp3").unwrap();
/// let looped_decoder = Decoder::new_looped(file).unwrap();
/// ```
pub struct LoopedDecoder<R: Read + Seek> {
    /// The underlying decoder implementation.
    inner: Option<DecoderImpl<R>>,
    /// Configuration settings for the decoder.
    settings: Settings,
}

// Cannot really reduce the size of the VorbisDecoder. There are not any
// arrays just a lot of struct fields.
#[allow(clippy::large_enum_variant)]
enum DecoderImpl<R: Read + Seek> {
    Symphonia(symphonia::SymphoniaDecoder, PhantomData<R>),
    // This variant is here just to satisfy the compiler when there are no decoders enabled.
    // It is unreachable and should never be constructed.
    #[allow(dead_code)]
    None(Unreachable, PhantomData<R>),
}

enum Unreachable {}

impl<R: Read + Seek> DecoderImpl<R> {
    #[inline]
    fn next(&mut self) -> Option<Sample> {
        match self {
            DecoderImpl::Symphonia(source, PhantomData) => source.next(),
            DecoderImpl::None(_, _) => unreachable!(),
        }
    }

    #[inline]
    fn size_hint(&self) -> (usize, Option<usize>) {
        match self {
            DecoderImpl::Symphonia(source, PhantomData) => source.size_hint(),
            DecoderImpl::None(_, _) => unreachable!(),
        }
    }

    #[inline]
    fn current_span_len(&self) -> Option<usize> {
        match self {
            DecoderImpl::Symphonia(source, PhantomData) => source.current_span_len(),
            DecoderImpl::None(_, _) => unreachable!(),
        }
    }

    #[inline]
    fn channels(&self) -> ChannelCount {
        match self {
            DecoderImpl::Symphonia(source, PhantomData) => source.channels(),
            DecoderImpl::None(_, _) => unreachable!(),
        }
    }

    #[inline]
    fn sample_rate(&self) -> SampleRate {
        match self {
            DecoderImpl::Symphonia(source, PhantomData) => source.sample_rate(),
            DecoderImpl::None(_, _) => unreachable!(),
        }
    }

    /// Returns the total duration of this audio source.
    ///
    /// # Symphonia Notes
    ///
    /// For formats that lack timing information like MP3 and Vorbis, this requires the decoder to
    /// be initialized with the correct byte length via `Decoder::builder().with_byte_len()`.
    #[inline]
    fn total_duration(&self) -> Option<Duration> {
        match self {
            DecoderImpl::Symphonia(source, PhantomData) => source.total_duration(),
            DecoderImpl::None(_, _) => unreachable!(),
        }
    }

    #[inline]
    fn try_seek(&mut self, pos: Duration) -> Result<(), SeekError> {
        match self {
            DecoderImpl::Symphonia(source, PhantomData) => source.try_seek(pos),
            DecoderImpl::None(_, _) => unreachable!(),
        }
    }
}

/// Converts a `File` into a `Decoder` with automatic optimizations.
/// This is the preferred way to decode files as it enables seeking optimizations
/// and accurate duration calculations.
///
/// This implementation:
/// - Wraps the file in a `BufReader` for better performance
/// - Gets the file length from metadata to improve seeking operations and duration accuracy
/// - Enables seeking by default
///
/// # Errors
///
/// Returns an error if:
/// - The file metadata cannot be read
/// - The audio format cannot be recognized or is not supported
///
/// # Examples
/// ```no_run
/// use std::fs::File;
/// use rodio::Decoder;
///
/// let file = File::open("audio.mp3").unwrap();
/// let decoder = Decoder::try_from(file).unwrap();
/// ```
impl TryFrom<std::fs::File> for Decoder<BufReader<std::fs::File>> {
    type Error = DecoderError;

    fn try_from(file: std::fs::File) -> Result<Self, Self::Error> {
        let len = file
            .metadata()
            .map_err(|e| Self::Error::IoError(e.to_string()))?
            .len();

        Self::builder()
            .with_data(BufReader::new(file))
            .with_byte_len(len)
            .with_seekable(true)
            .build()
    }
}

/// Converts a `BufReader` into a `Decoder`.
/// When working with files, prefer `TryFrom<File>` as it will automatically set byte_len
/// for better seeking performance.
///
/// # Errors
///
/// Returns `DecoderError::UnrecognizedFormat` if the audio format could not be determined
/// or is not supported.
///
/// # Examples
/// ```no_run
/// use std::fs::File;
/// use std::io::BufReader;
/// use rodio::Decoder;
///
/// let file = File::open("audio.mp3").unwrap();
/// let reader = BufReader::new(file);
/// let decoder = Decoder::try_from(reader).unwrap();
/// ```
impl<R> TryFrom<BufReader<R>> for Decoder<BufReader<R>>
where
    R: Read + Seek + Send + Sync + 'static,
{
    type Error = DecoderError;

    fn try_from(data: BufReader<R>) -> Result<Self, Self::Error> {
        Self::new(data)
    }
}

/// Converts a `Cursor` into a `Decoder`.
/// When working with files, prefer `TryFrom<File>` as it will automatically set byte_len
/// for better seeking performance.
///
/// This is useful for decoding audio data that's already in memory.
///
/// # Errors
///
/// Returns `DecoderError::UnrecognizedFormat` if the audio format could not be determined
/// or is not supported.
///
/// # Examples
/// ```no_run
/// use std::io::Cursor;
/// use rodio::Decoder;
///
/// let data = std::fs::read("audio.mp3").unwrap();
/// let cursor = Cursor::new(data);
/// let decoder = Decoder::try_from(cursor).unwrap();
/// ```
impl<T> TryFrom<std::io::Cursor<T>> for Decoder<std::io::Cursor<T>>
where
    T: AsRef<[u8]> + Send + Sync + 'static,
{
    type Error = DecoderError;

    fn try_from(data: std::io::Cursor<T>) -> Result<Self, Self::Error> {
        Self::new(data)
    }
}

impl<R: Read + Seek + Send + Sync + 'static> Decoder<R> {
    /// Returns a builder for creating a new decoder with customizable settings.
    ///
    /// # Examples
    /// ```no_run
    /// use std::fs::File;
    /// use rodio::Decoder;
    ///
    /// let file = File::open("audio.mp3").unwrap();
    /// let decoder = Decoder::builder()
    ///     .with_data(file)
    ///     .with_hint("mp3")
    ///     .with_gapless(true)
    ///     .build()
    ///     .unwrap();
    /// ```
    pub fn builder() -> DecoderBuilder<R> {
        DecoderBuilder::new()
    }

    /// Builds a new decoder with default settings.
    ///
    /// Attempts to automatically detect the format of the source of data.
    ///
    /// # Errors
    ///
    /// Returns `DecoderError::UnrecognizedFormat` if the audio format could not be determined
    /// or is not supported.
    pub fn new(data: R) -> Result<Self, DecoderError> {
        DecoderBuilder::new().with_data(data).build()
    }

    /// Builds a new looped decoder with default settings.
    ///
    /// Attempts to automatically detect the format of the source of data.
    /// The decoder will restart from the beginning when it reaches the end.
    ///
    /// # Errors
    ///
    /// Returns `DecoderError::UnrecognizedFormat` if the audio format could not be determined
    /// or is not supported.
    pub fn new_looped(data: R) -> Result<LoopedDecoder<R>, DecoderError> {
        DecoderBuilder::new().with_data(data).build_looped()
    }

    /// Builds a new decoder with WAV format hint.
    ///
    /// This method provides a hint that the data is WAV format, which may help the decoder
    /// identify the format more quickly. However, if WAV decoding fails, other formats
    /// will still be attempted.
    ///
    /// # Errors
    ///
    /// Returns `DecoderError::UnrecognizedFormat` if no suitable decoder was found.
    ///
    /// # Examples
    /// ```no_run
    /// use rodio::Decoder;
    /// use std::fs::File;
    ///
    /// let file = File::open("audio.wav").unwrap();
    /// let decoder = Decoder::new_wav(file).unwrap();
    /// ```
    pub fn new_wav(data: R) -> Result<Self, DecoderError> {
        DecoderBuilder::new()
            .with_data(data)
            .with_hint("wav")
            .build()
    }

    /// Builds a new decoder with FLAC format hint.
    ///
    /// This method provides a hint that the data is FLAC format, which may help the decoder
    /// identify the format more quickly. However, if FLAC decoding fails, other formats
    /// will still be attempted.
    ///
    /// # Errors
    ///
    /// Returns `DecoderError::UnrecognizedFormat` if no suitable decoder was found.
    ///
    /// # Examples
    /// ```no_run
    /// use rodio::Decoder;
    /// use std::fs::File;
    ///
    /// let file = File::open("audio.flac").unwrap();
    /// let decoder = Decoder::new_flac(file).unwrap();
    /// ```
    pub fn new_flac(data: R) -> Result<Self, DecoderError> {
        DecoderBuilder::new()
            .with_data(data)
            .with_hint("flac")
            .build()
    }

    /// Builds a new decoder with Vorbis format hint.
    ///
    /// This method provides a hint that the data is Vorbis format, which may help the decoder
    /// identify the format more quickly. However, if Vorbis decoding fails, other formats
    /// will still be attempted.
    ///
    /// # Errors
    ///
    /// Returns `DecoderError::UnrecognizedFormat` if no suitable decoder was found.
    ///
    /// # Examples
    /// ```no_run
    /// use rodio::Decoder;
    /// use std::fs::File;
    ///
    /// let file = File::open("audio.ogg").unwrap();
    /// let decoder = Decoder::new_vorbis(file).unwrap();
    /// ```
    pub fn new_vorbis(data: R) -> Result<Self, DecoderError> {
        DecoderBuilder::new()
            .with_data(data)
            .with_hint("ogg")
            .build()
    }

    /// Builds a new decoder with MP3 format hint.
    ///
    /// This method provides a hint that the data is MP3 format, which may help the decoder
    /// identify the format more quickly. However, if MP3 decoding fails, other formats
    /// will still be attempted.
    ///
    /// # Errors
    ///
    /// Returns `DecoderError::UnrecognizedFormat` if no suitable decoder was found.
    ///
    /// # Examples
    /// ```no_run
    /// use rodio::Decoder;
    /// use std::fs::File;
    ///
    /// let file = File::open("audio.mp3").unwrap();
    /// let decoder = Decoder::new_mp3(file).unwrap();
    /// ```
    pub fn new_mp3(data: R) -> Result<Self, DecoderError> {
        DecoderBuilder::new()
            .with_data(data)
            .with_hint("mp3")
            .build()
    }

    /// Builds a new decoder with AAC format hint.
    ///
    /// This method provides a hint that the data is AAC format, which may help the decoder
    /// identify the format more quickly. However, if AAC decoding fails, other formats
    /// will still be attempted.
    ///
    /// # Errors
    ///
    /// Returns `DecoderError::UnrecognizedFormat` if no suitable decoder was found.
    ///
    /// # Examples
    /// ```no_run
    /// use rodio::Decoder;
    /// use std::fs::File;
    ///
    /// let file = File::open("audio.aac").unwrap();
    /// let decoder = Decoder::new_aac(file).unwrap();
    /// ```
    pub fn new_aac(data: R) -> Result<Self, DecoderError> {
        DecoderBuilder::new()
            .with_data(data)
            .with_hint("aac")
            .build()
    }

    /// Builds a new decoder with MP4 container format hint.
    ///
    /// This method provides a hint that the data is in MP4 container format by setting
    /// the MIME type to "audio/mp4". This may help the decoder identify the format
    /// more quickly. However, if MP4 decoding fails, other formats will still be attempted.
    ///
    /// # Errors
    ///
    /// Returns `DecoderError::UnrecognizedFormat` if no suitable decoder was found.
    ///
    /// # Examples
    /// ```no_run
    /// use rodio::Decoder;
    /// use std::fs::File;
    ///
    /// let file = File::open("audio.m4a").unwrap();
    /// let decoder = Decoder::new_mp4(file).unwrap();
    /// ```
    pub fn new_mp4(data: R) -> Result<Self, DecoderError> {
        DecoderBuilder::new()
            .with_data(data)
            .with_mime_type("audio/mp4")
            .build()
    }
}

impl<R> Iterator for Decoder<R>
where
    R: Read + Seek,
{
    type Item = Sample;

    #[inline]
    fn next(&mut self) -> Option<Self::Item> {
        self.0.next()
    }

    #[inline]
    fn size_hint(&self) -> (usize, Option<usize>) {
        self.0.size_hint()
    }
}

impl<R> Source for Decoder<R>
where
    R: Read + Seek,
{
    #[inline]
    fn current_span_len(&self) -> Option<usize> {
        self.0.current_span_len()
    }

    #[inline]
    fn channels(&self) -> ChannelCount {
        self.0.channels()
    }

    fn sample_rate(&self) -> SampleRate {
        self.0.sample_rate()
    }

    #[inline]
    fn total_duration(&self) -> Option<Duration> {
        self.0.total_duration()
    }

    #[inline]
    fn try_seek(&mut self, pos: Duration) -> Result<(), SeekError> {
        self.0.try_seek(pos)
    }
}

impl<R> Iterator for LoopedDecoder<R>
where
    R: Read + Seek,
{
    type Item = Sample;

    /// Returns the next sample in the audio stream.
    ///
    /// When the end of the stream is reached, attempts to seek back to the start
    /// and continue playing. If seeking fails, or if no decoder is available,
    /// returns `None`.
    fn next(&mut self) -> Option<Self::Item> {
        if let Some(inner) = &mut self.inner {
            if let Some(sample) = inner.next() {
                return Some(sample);
            }

            // Take ownership of the decoder to reset it
            let decoder = self.inner.take()?;
            let (new_decoder, sample) = match decoder {
                DecoderImpl::Symphonia(source, PhantomData) => {
                    let mut reader = source.into_inner();
                    reader.seek(SeekFrom::Start(0)).ok()?;
                    let mut source =
                        symphonia::SymphoniaDecoder::new(reader, &self.settings).ok()?;
                    let sample = source.next();
                    (DecoderImpl::Symphonia(source, PhantomData), sample)
                }
            };
            self.inner = Some(new_decoder);
            sample
        } else {
            None
        }
    }

    /// Returns the size hint for this iterator.
    ///
    /// The lower bound is:
    /// - The minimum number of samples remaining in the current iteration if there is an active decoder
    /// - 0 if there is no active decoder (inner is None)
    ///
    /// The upper bound is always `None` since the decoder loops indefinitely.
    /// This differs from non-looped decoders which may provide a finite upper bound.
    ///
    /// Note that even with an active decoder, reaching the end of the stream may result
    /// in the decoder becoming inactive if seeking back to the start fails.
    #[inline]
    fn size_hint(&self) -> (usize, Option<usize>) {
        (
            self.inner.as_ref().map_or(0, |inner| inner.size_hint().0),
            None,
        )
    }
}

impl<R> Source for LoopedDecoder<R>
where
    R: Read + Seek,
{
    /// Returns the current span length of the underlying decoder.
    ///
    /// Returns `None` if there is no active decoder.
    #[inline]
    fn current_span_len(&self) -> Option<usize> {
        self.inner.as_ref()?.current_span_len()
    }

    /// Returns the number of channels in the audio stream.
    ///
    /// Returns the default channel count if there is no active decoder.
    #[inline]
    fn channels(&self) -> ChannelCount {
        self.inner
            .as_ref()
            .map_or(ChannelCount::default(), |inner| inner.channels())
    }

    /// Returns the sample rate of the audio stream.
    ///
    /// Returns the default sample rate if there is no active decoder.
    #[inline]
    fn sample_rate(&self) -> SampleRate {
        self.inner
            .as_ref()
            .map_or(SampleRate::default(), |inner| inner.sample_rate())
    }

    /// Returns the total duration of this audio source.
    ///
    /// Always returns `None` for looped decoders since they have no fixed end point -
    /// they will continue playing indefinitely by seeking back to the start when reaching
    /// the end of the audio data.
    #[inline]
    fn total_duration(&self) -> Option<Duration> {
        None
    }

    /// Attempts to seek to a specific position in the audio stream.
    ///
    /// # Errors
    ///
    /// Returns `SeekError::NotSupported` if:
    /// - There is no active decoder
    /// - The underlying decoder does not support seeking
    ///
    /// May also return other `SeekError` variants if the underlying decoder's seek operation fails.
    ///
    /// # Note
    ///
    /// Even for looped playback, seeking past the end of the stream will not automatically
    /// wrap around to the beginning - it will return an error just like a normal decoder.
    /// Looping only occurs when reaching the end through normal playback.
    fn try_seek(&mut self, pos: Duration) -> Result<(), SeekError> {
        match &mut self.inner {
            Some(inner) => inner.try_seek(pos),
            None => Err(SeekError::Other(Box::new(DecoderError::IoError(
                "Looped source ended when it failed to loop back".to_string(),
            )))),
        }
    }
}
