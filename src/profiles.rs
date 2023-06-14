#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum ContainerFormat {
    Best,
    Same,
    Matroska,
    Mpeg,
    WebM,
    GifContainer,
}

#[derive(Debug, Copy, Clone)]
pub enum VideoEncoding {
    Av1,
    Vp8,
    Vp9,
    H264,
    H265,
    Gif,
}

#[derive(Debug, Copy, Clone)]
pub enum AudioEncoding {
    Aac,
    Ac3,
    Opus,
    Vorbis,
    Flac,
}

use gettextrs::gettext;
use AudioEncoding::*;
use ContainerFormat::*;
use VideoEncoding::*;

impl ContainerFormat {
    pub fn get_all() -> Vec<ContainerFormat> {
        vec![Best, Same, Matroska, Mpeg, WebM, GifContainer]
    }

    pub fn viable_matchings(&self) -> (Vec<VideoEncoding>, Vec<AudioEncoding>) {
        match self {
            Best => (vec![Av1], vec![Opus]),
            Same => (vec![], vec![]),
            Matroska => (
                vec![Av1, Vp9, Vp8, H264, H265],
                vec![Vorbis, Opus, Aac, Ac3, Flac],
            ),
            Mpeg => (vec![Av1, Vp9, Vp8, H264], vec![Opus, Aac, Ac3, Flac]),
            WebM => (vec![Av1, Vp8, Vp9], vec![Vorbis, Opus]),
            GifContainer => (vec![VideoEncoding::Gif], vec![]),
        }
    }

    pub fn format(&self) -> &str {
        match self {
            Best => "video/webm",
            Matroska => "video/x-matroska",
            Mpeg => "video/quicktime",
            WebM => "video/webm",
            GifContainer => "image/gif",
            Same => unreachable!(),
        }
    }

    pub fn extension(&self) -> &str {
        match self {
            Best => "webm",
            Matroska => "mkv",
            Mpeg => "mp4",
            WebM => "webm",
            GifContainer => "gif",
            Same => unreachable!(),
        }
    }

    pub fn for_display(&self) -> String {
        match self {
            Best => gettext("Recommended (WEBM, AV1, Opus)"),
            Same => gettext("Keep as-is"),
            Matroska => "MKV".to_owned(),
            Mpeg => "MP4".to_owned(),
            WebM => "WEBM".to_owned(),
            GifContainer => "GIF".to_owned(),
        }
    }
}

impl VideoEncoding {
    pub fn get_format(&self) -> &str {
        match self {
            Av1 => "video/x-av1",
            Vp8 => "video/x-vp8",
            Vp9 => "video/x-vp9",
            H264 => "video/x-h264",
            H265 => "video/x-h265",
            Gif => "image/gif",
        }
    }

    pub fn get_preset_name(&self) -> &str {
        match self {
            Av1 => "svtav1enc",
            Vp8 => "vp8enc",
            Vp9 => "vp9enc",
            H264 => "vaapih264enc",
            H265 => "vaapih265enc",
            Gif => "gifenc",
        }
    }

    pub fn for_display(&self) -> &str {
        match self {
            Av1 => "AV1",
            Vp8 => "VP8",
            Vp9 => "VP9",
            H264 => "H264",
            H265 => "H265",
            Gif => "GIF",
        }
    }
}

impl AudioEncoding {
    pub fn get_format(&self) -> &str {
        match self {
            Aac => "audio/aac",
            Ac3 => "audio/x-ac3",
            Opus => "audio/x-opus",
            Vorbis => "audio/x-vorbis",
            Flac => "audio/flac",
        }
    }

    pub fn for_display(&self) -> &str {
        match self {
            Aac => "AAC",
            Ac3 => "AC3",
            Opus => "Opus",
            Vorbis => "Vorbis",
            Flac => "FLAC",
        }
    }
}
