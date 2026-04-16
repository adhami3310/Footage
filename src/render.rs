use std::{
    path::PathBuf,
    sync::{Arc, atomic::AtomicBool},
};

use ges::prelude::*;
use gst::{ClockTime, PadProbeData, PadProbeType};
use gstreamer_pbutils::Discoverer;
use log::{error, info};

use crate::{
    info::{Dimensions, Framerate},
    orientation::VideoOrientation,
    profiles::{ContainerFormat, ContainerSelection, OutputFormat},
};

pub struct RenderJob {
    pub input_path: PathBuf,
    pub output_path: PathBuf,
    pub output_format: OutputFormat,
    pub framerate: Framerate,
    pub scaled_dimension: Dimensions<u32>,
    pub orientation: VideoOrientation,
    pub full_scaled_width: f64,
    pub full_scaled_height: f64,
    pub crop_left: f64,
    pub crop_top: f64,
    pub mute: bool,
    pub inpoint: ClockTime,
    pub duration: ClockTime,
    pub sender: async_channel::Sender<Result<(u64, u64), ()>>,
    pub running_flag: Arc<AtomicBool>,
}

pub fn run_render(job: RenderJob) {
    let RenderJob {
        input_path,
        output_path,
        output_format,
        framerate,
        scaled_dimension,
        orientation,
        full_scaled_width,
        full_scaled_height,
        crop_left,
        crop_top,
        mute,
        inpoint,
        duration,
        sender,
        running_flag,
    } = job;

    // When output == input, write to a temporary file to avoid truncating
    // the source before the pipeline reads it.
    let same_file = output_path == input_path;
    let render_path = if same_file {
        let mut temp = output_path.clone();
        temp.set_extension(format!(
            "tmp.{}",
            output_path
                .extension()
                .unwrap_or_default()
                .to_string_lossy()
        ));
        info!(
            "Output path matches input path, rendering to temporary file: {}",
            temp.display()
        );
        temp
    } else {
        output_path.clone()
    };

    let clip = ges::UriClip::new(
        url::Url::from_file_path(input_path.clone())
            .unwrap()
            .as_str(),
    )
    .unwrap();

    let timeline = ges::Timeline::new_audio_video();

    let layer = timeline.append_layer();
    layer.add_clip(&clip).unwrap();

    if let Some(track) = timeline.tracks().first() {
        track.set_restriction_caps(
            &gst::Caps::builder("video/x-raw")
                .field(
                    "framerate",
                    gst::Fraction::new(framerate.nominator as i32, framerate.denominator as i32),
                )
                .field("width", scaled_dimension.width as i32)
                .field("height", scaled_dimension.height as i32)
                .build(),
        );
        track.elements().into_iter().for_each(|track_element| {
            ges::prelude::TrackElementExt::set_child_property(
                &track_element,
                "video-direction",
                &orientation.to_gst_video_orientation_method().to_value(),
            )
            .unwrap();

            ges::prelude::TrackElementExt::set_child_property(
                &track_element,
                "width",
                &(full_scaled_width as i32).to_value(),
            )
            .unwrap();
            ges::prelude::TrackElementExt::set_child_property(
                &track_element,
                "height",
                &(full_scaled_height as i32).to_value(),
            )
            .unwrap();
            ges::prelude::TrackElementExt::set_child_property(
                &track_element,
                "posx",
                &((-crop_left * full_scaled_width) as i32).to_value(),
            )
            .unwrap();
            ges::prelude::TrackElementExt::set_child_property(
                &track_element,
                "posy",
                &((-crop_top * full_scaled_height) as i32).to_value(),
            )
            .unwrap();
        });
    }

    clip.add_top_effect(&ges::Effect::new("videorate").unwrap(), 0)
        .ok();

    clip.set_inpoint(inpoint);
    clip.set_duration(Some(duration));

    let pipeline = ges::Pipeline::new();
    pipeline.set_timeline(&timeline).unwrap();

    match output_format.container_selection {
        ContainerSelection::Same => {
            let profile = gstreamer_pbutils::EncodingProfile::from_discoverer(
                &Discoverer::new(gst::ClockTime::SECOND)
                    .unwrap()
                    .discover_uri(
                        url::Url::from_file_path(input_path.clone())
                            .unwrap()
                            .as_str(),
                    )
                    .unwrap(),
            )
            .unwrap();

            let (video_caps, audio_caps): (Vec<_>, Vec<_>) = profile
                .input_caps()
                .iter()
                .map(|discovered_caps| {
                    let discovered_caps = discovered_caps.to_owned();
                    let is_video = discovered_caps.name().starts_with("video");

                    if is_video {
                        let mut discovered_caps = discovered_caps;
                        discovered_caps.remove_fields(["width", "height", "framerate"]);

                        let mut caps = gst::Caps::builder(discovered_caps.name());
                        for (name, value) in discovered_caps.into_iter() {
                            caps = caps.field(name, value.clone());
                        }
                        caps.build()
                    } else {
                        // For audio, only keep the codec name to avoid
                        // over-constraining encoder selection.
                        gst::Caps::builder(discovered_caps.name()).build()
                    }
                })
                .partition(|c| c.to_string().starts_with("video"));

            let profile_format = profile.format();

            let mut container_profile =
                gstreamer_pbutils::EncodingContainerProfile::builder(&profile_format)
                    .name("container");

            if let Some(video_cap) = video_caps.first() {
                let video_profile =
                    gstreamer_pbutils::EncodingVideoProfile::builder(video_cap).build();

                container_profile = container_profile.add_profile(video_profile);
            }

            if !mute && let Some(audio_cap) = audio_caps.first() {
                let audio_profile =
                    gstreamer_pbutils::EncodingAudioProfile::builder(audio_cap).build();

                container_profile = container_profile.add_profile(audio_profile);
            }

            pipeline
                .set_render_settings(
                    url::Url::from_file_path(&render_path).unwrap().as_str(),
                    &container_profile.build(),
                )
                .unwrap();
        }
        ContainerSelection::Format(ContainerFormat::GifContainer) => {
            pipeline
                .set_render_settings(
                    url::Url::from_file_path(&render_path).unwrap().as_str(),
                    &output_format.video_encoding.unwrap().encoding_profile(),
                )
                .unwrap();
        }
        ContainerSelection::Format(container) => {
            let video_profile = output_format.video_encoding.unwrap().encoding_profile();

            let container_caps = gst::Caps::builder(container.format()).build();

            let mut container_profile =
                gstreamer_pbutils::EncodingContainerProfile::builder(&container_caps)
                    .name("container")
                    .add_profile(video_profile);

            if !mute {
                let audio_profile = gstreamer_pbutils::EncodingAudioProfile::builder(
                    &gst::Caps::builder(output_format.audio_encoding.unwrap().get_format()).build(),
                )
                .build();
                container_profile = container_profile.add_profile(audio_profile);
            }

            pipeline
                .set_render_settings(
                    url::Url::from_file_path(&render_path).unwrap().as_str(),
                    &container_profile.build(),
                )
                .unwrap();
        }
    }

    pipeline.set_mode(ges::PipelineFlags::RENDER).unwrap();

    let sender_pad = sender.clone();

    let another_running_flag = running_flag.clone();

    timeline
        .pads()
        .first()
        .unwrap()
        .add_probe(PadProbeType::DATA_DOWNSTREAM, move |_, info| {
            if let Some(PadProbeData::Buffer(data)) = &info.data
                && let Some(pts) = data.pts()
                && sender_pad
                    .send_blocking(Ok((pts.mseconds(), duration.mseconds())))
                    .is_err()
            {
                return gst::PadProbeReturn::Drop;
            }

            if !another_running_flag
                .clone()
                .load(std::sync::atomic::Ordering::SeqCst)
            {
                if let Err(e) = sender_pad.send_blocking(Err(())) {
                    error!("Failed to send cancellation from pad probe: {e}");
                }
                return gst::PadProbeReturn::Drop;
            }

            gst::PadProbeReturn::Ok
        });

    pipeline.set_state(gst::State::Playing).unwrap();

    let bus = pipeline
        .bus()
        .expect("Pipeline without bus. Shouldn't happen!");

    info!("Starting pipeline");

    let mut success = false;

    for msg in bus.iter_timed(gst::ClockTime::NONE) {
        use gst::MessageView;

        match msg.view() {
            MessageView::Eos(..) => {
                success = true;
                if let Err(e) = sender.send_blocking(Ok((1, 1))) {
                    error!("Failed to send EOS: {e}");
                }
                break;
            }
            MessageView::Error(e) => {
                error!(
                    "Error from {:?}: {} ({:?})",
                    e.src().map(|s| s.path_string()),
                    e.error(),
                    e.debug()
                );
                pipeline.set_state(gst::State::Null).unwrap();

                if let Err(e) = sender.send_blocking(Err(())) {
                    error!("Failed to send error: {e}");
                }
                break;
            }
            _ => {
                if !running_flag.load(std::sync::atomic::Ordering::SeqCst) {
                    pipeline.set_state(gst::State::Null).unwrap();

                    if let Err(e) = sender.send_blocking(Err(())) {
                        error!("Failed to send cancellation: {e}");
                    }
                    break;
                }
            }
        }
    }

    pipeline.set_state(gst::State::Null).unwrap();

    if same_file {
        if success {
            info!(
                "Renaming temporary file to output: {}",
                output_path.display()
            );
            if let Err(e) = std::fs::rename(&render_path, &output_path) {
                error!("Failed to rename temporary file to output: {e}");
            }
        } else {
            info!("Removing temporary file: {}", render_path.display());
            if let Err(e) = std::fs::remove_file(&render_path) {
                error!("Failed to remove temporary file: {e}");
            }
        }
    }
}
