use std::process::Command;

use glib::translate::IntoGlib;
use gst::prelude::*;
use itertools::Itertools;

#[derive(Debug)]
pub struct Framerate {
    pub nominator: u32,
    pub denominator: u32,
}

impl Framerate {
    pub fn value(&self) -> f64 {
        self.nominator as f64 / self.denominator as f64
    }
}

#[derive(Debug, Clone, Copy)]
pub struct Dimensions<T> {
    pub width: T,
    pub height: T,
}

impl Dimensions<u32> {
    pub fn width_f64(&self) -> f64 {
        self.width as f64
    }

    pub fn height_f64(&self) -> f64 {
        self.height as f64
    }
}
impl<T: Copy> Dimensions<T> {
    pub fn swap(&self) -> Dimensions<T> {
        Dimensions {
            width: self.height,
            height: self.width,
        }
    }
}

impl From<Dimensions<f64>> for Dimensions<u32> {
    fn from(value: Dimensions<f64>) -> Self {
        Dimensions {
            width: value.width as u32,
            height: value.height as u32,
        }
    }
}

impl From<Dimensions<u32>> for Dimensions<f64> {
    fn from(value: Dimensions<u32>) -> Self {
        Dimensions {
            width: value.width as f64,
            height: value.height as f64,
        }
    }
}

pub fn get_info(path: String) -> Option<(Dimensions<u32>, Option<Framerate>, bool)> {
    let video_info = get_video_info(path.clone())?;
    let audio_info = get_audio_info(path)?;
    Some((video_info.0, video_info.1, audio_info))
}

fn get_audio_info(path: String) -> Option<bool> {
    let o = Command::new("ffprobe")
        .args(["-v", "error"])
        .args(["-show_entries", "stream=codec_type"])
        .args(["-of", "csv=p=0"])
        .arg(path)
        .output()
        .ok()?;

    let s = std::str::from_utf8(&o.stdout).ok()?;

    Some(s.trim().split('\n').any(|x| x == "audio"))
}

fn get_video_info(path: String) -> Option<(Dimensions<u32>, Option<Framerate>)> {
    let o = Command::new("ffprobe")
        .args(["-v", "error"])
        .args(["-select_streams", "v:0"])
        .args(["-show_entries", "stream=width,height,r_frame_rate"])
        .args(["-of", "csv=s=x:p=0"])
        .arg(path)
        .output()
        .ok()?;

    let s = std::str::from_utf8(&o.stdout).ok()?;

    match s.trim().split('x').collect_vec()[..] {
        [a, b, c] => Some((
            Dimensions {
                width: a.trim().parse().ok()?,
                height: b.trim().parse().ok()?,
            },
            {
                let (x, y) = c.split('/').collect_tuple()?;
                Some(Framerate {
                    nominator: x.trim().parse().ok()?,
                    denominator: y.trim().parse().ok()?,
                })
            },
        )),
        [a, b] => Some((
            Dimensions {
                width: a.trim().parse().ok()?,
                height: b.trim().parse().ok()?,
            },
            None,
        )),
        _ => None,
    }
}

pub fn get_debug_info() {
    let o = Command::new("gst-inspect-1.0").output().unwrap();

    let s = std::str::from_utf8(&o.stdout).unwrap();

    println!("{}", s);

    println!(
        "\n=== GStreamer version: {}.{}.{}.{} ===",
        gst::version().0,
        gst::version().1,
        gst::version().2,
        gst::version().3,
    );

    println!("\n=== Encoder selection priority ===");
    for encoding in crate::profiles::VideoEncoding::ALL {
        let encoders = encoding.available_encoders();
        println!("{}:", encoding.for_display());
        if encoders.is_empty() {
            println!("  (none)");
        }
        for factory in &encoders {
            println!("  {:>6}  {}", factory.rank().into_glib(), factory.name());
        }
    }
}
