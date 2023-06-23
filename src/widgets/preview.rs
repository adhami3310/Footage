use std::{
    cell::RefCell,
    path::PathBuf,
    sync::{atomic::AtomicBool, Arc},
};

use glib::clone;
use gst::{ClockTime, PadProbeData, PadProbeType, SeekFlags};
use gstreamer_pbutils::Discoverer;
use gtk::{gdk, gio, glib, subclass::prelude::*};

use ges::prelude::*;
use ges::Effect;

use crate::{
    info::get_width_height,
    orientation::VideoOrientation,
    profiles::{AudioEncoding, ContainerFormat, VideoEncoding},
};

mod imp {

    use std::cell::Cell;

    use crate::{orientation::VideoOrientation, widgets::crop::Crop};

    use super::*;

    use adw::subclass::prelude::BinImpl;
    use glib::subclass::Signal;
    use gtk::CompositeTemplate;

    #[derive(Debug, CompositeTemplate, Default)]
    #[template(resource = "/io/gitlab/adhami3310/Footage/blueprints/video-preview.ui")]
    pub struct VideoPreview {
        #[template_child]
        pub paint: TemplateChild<gtk::Picture>,
        #[template_child]
        pub crop_box: TemplateChild<Crop>,

        pub current_dimensions: Cell<Option<(usize, usize)>>,
        pub orientation: Cell<VideoOrientation>,
        pub audio_level: RefCell<Option<Effect>>,
        pub inpoint: Cell<u64>,
        pub mute: Cell<bool>,
        pub outpoint: Cell<u64>,
        pub effects: RefCell<Vec<String>>,
        pub pipeline: RefCell<Option<ges::Pipeline>>,
        pub clip: RefCell<Option<ges::UriClip>>,
        pub path: RefCell<PathBuf>,
        pub bus_watch: RefCell<Option<glib::SourceId>>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for VideoPreview {
        const NAME: &'static str = "VideoPreview";
        type Type = super::VideoPreview;
        type ParentType = adw::Bin;

        fn class_init(klass: &mut Self::Class) {
            Self::bind_template(klass);
        }

        fn instance_init(obj: &glib::subclass::InitializingObject<Self>) {
            obj.init_template();
        }

        fn new() -> Self {
            Self::default()
        }
    }

    impl ObjectImpl for VideoPreview {
        fn constructed(&self) {}

        fn signals() -> &'static [Signal] {
            use once_cell::sync::Lazy;
            static SIGNALS: Lazy<[Signal; 3]> = Lazy::new(|| {
                [
                    Signal::builder("orientation-flipped")
                        .param_types(std::iter::empty::<glib::Type>())
                        .build(),
                    Signal::builder("set-position")
                        .param_types([glib::Type::U64])
                        .build(),
                    Signal::builder("mode-changed")
                        .param_types([glib::Type::BOOL])
                        .build(),
                ]
            });

            SIGNALS.as_ref()
        }
    }

    impl WidgetImpl for VideoPreview {}

    impl BinImpl for VideoPreview {}
}

glib::wrapper! {
    pub struct VideoPreview(ObjectSubclass<imp::VideoPreview>)
        @extends adw::Bin, gtk::Widget,
        @implements gio::ActionMap, gio::ActionGroup, gtk::Root;
}

impl Default for VideoPreview {
    fn default() -> Self {
        Self::new()
    }
}

#[gtk::template_callbacks]
impl VideoPreview {
    pub fn new() -> Self {
        let bin = glib::Object::builder::<VideoPreview>().build();

        bin
    }

    pub fn load_path(&self, path: PathBuf) -> Result<(usize, usize, u64, Option<i32>), ()> {
        gst::init().unwrap();
        ges::init().unwrap();

        let clip = ges::UriClip::new(url::Url::from_file_path(path.clone()).unwrap().as_str())
            .map_err(|_| ())?;
        let duration = clip.duration().mseconds();
        self.imp().clip.replace(Some(clip));

        self.imp().outpoint.set(duration);

        let (width, height, framerate) =
            get_width_height(path.to_str().unwrap().to_owned()).ok_or(())?;
        self.imp().current_dimensions.set(Some((width, height)));

        self.imp().path.replace(path);
        self.imp().effects.replace(vec![]);

        self.imp().crop_box.set_proportions((0., 0., 0., 0.));
        self.imp().orientation.set(Default::default());

        self.refresh_ui();

        Ok((width, height, duration, framerate))
    }

    pub fn seek(&self, position: u64) {
        self.pause();

        self.quiet_seek(position);
    }

    pub fn quiet_seek(&self, position: u64) {
        let position = position.max(self.imp().inpoint.get()) - self.imp().inpoint.get();

        let op = self.imp().pipeline.borrow();

        if let Some(p) = op.as_ref() {
            p.seek_simple(
                SeekFlags::ACCURATE | SeekFlags::FLUSH,
                ClockTime::from_mseconds(position),
            )
            .ok();
        }
    }

    pub fn refresh_ui(&self) {
        self.kill();

        let timeline = ges::Timeline::new_audio_video();
        if let Some(t) = timeline.tracks().first() {
            t.set_restriction_caps(
                &gst::Caps::builder("video/x-raw")
                    .field("framerate", gst::Fraction::new(30 as i32, 1))
                    .build(),
            );
        }

        let original_clip = self.imp().clip.borrow();
        let clip = original_clip.as_ref().unwrap();

        let layer = timeline.append_layer();
        layer.add_clip(clip).unwrap();

        let pipeline = ges::Pipeline::new();
        pipeline.set_timeline(&timeline).unwrap();

        let gtksink = gst::ElementFactory::make("gtk4paintablesink")
            .build()
            .unwrap();

        let paintable = gtksink.property::<gdk::Paintable>("paintable");

        self.imp().paint.set_paintable(Some(&paintable));

        let sink = gst::Bin::default();
        let convert = gst::ElementFactory::make("videoconvertscale")
            .build()
            .unwrap();

        sink.add(&convert).unwrap();
        sink.add(&gtksink).unwrap();
        convert.link(&gtksink).unwrap();

        let pad = &gst::GhostPad::with_target(None, &convert.static_pad("sink").unwrap()).unwrap();

        let (sender, receiver) = glib::MainContext::channel(glib::PRIORITY_HIGH);

        pad.add_probe(PadProbeType::DATA_DOWNSTREAM, move |_, info| {
            if let Some(PadProbeData::Buffer(data)) = &info.data {
                if let Some(pts) = data.pts() {
                    sender.send(pts.mseconds()).expect("Concurrency Issues");
                }
            }
            gst::PadProbeReturn::Ok
        });

        receiver.attach(
            None,
            clone!(@weak self as this => @default-return Continue(true), move |p| {
                if this.is_playing() {
                    this.emit_by_name::<()>("set-position", &[&(p + this.imp().inpoint.get())]);
                }
                Continue(true)
            }),
        );

        sink.add_pad(pad).unwrap();

        pipeline.set_video_sink(Some(&sink));

        let bus = pipeline.bus().unwrap();

        pipeline
            .set_state(gst::State::Paused)
            .expect("Unable to set the pipeline to the `Playing` state");

        let bus_watch = bus
            .add_watch_local(
                clone!(@weak self as this => @default-return glib::Continue(false), move |_, msg| {
                    use gst::MessageView;

                    match msg.view() {
                        MessageView::Eos(..) => {
                            this.pause();
                            this.emit_by_name::<()>("set-position", &[&this.imp().inpoint.get()]);
                            this.seek(0);
                        }
                        MessageView::Error(err) => {
                            println!(
                                "Error from {:?}: {} ({:?})",
                                err.src().map(|s| s.path_string()),
                                err.error(),
                                err.debug()
                            );
                        }
                        _ => (),
                    };

                    glib::Continue(true)
                }),
            )
            .expect("Failed to add bus watch");

        self.imp().pipeline.replace(Some(pipeline));
        self.imp().bus_watch.replace(Some(bus_watch));
    }

    // fn update_position(&self) {
    //     let position = self.imp().pipeline.borrow().as_ref().unwrap().query_position::<gst::format::ClockTime>().unwrap().mseconds();
    //     self.imp().timeline.set_position(position);
    // }

    pub fn pause(&self) {
        let orig_p = self.imp().pipeline.borrow();
        let p = orig_p.as_ref().unwrap();

        p.set_state(gst::State::Paused).unwrap();
        self.emit_by_name::<()>("mode-changed", &[&false]);
        // self.imp().play_pause.set_icon_name("play-symbolic");
    }

    fn is_playing(&self) -> bool {
        let orig_p = self.imp().pipeline.borrow();
        let p = orig_p.as_ref().unwrap();

        matches!(p.current_state(), gst::State::Playing)
    }

    pub fn play(&self) {
        let orig_p = self.imp().pipeline.borrow();
        let p = orig_p.as_ref().unwrap();
        p.set_state(gst::State::Playing).unwrap();
        self.emit_by_name::<()>("mode-changed", &[&true]);
        // self.imp().play_pause.set_icon_name("pause-symbolic");
    }

    pub fn set_range(&self, start: u64, end: u64) {
        let original_clip = self.imp().clip.borrow();
        let clip = original_clip.as_ref();
        if let Some(clip) = clip {
            clip.set_inpoint(ClockTime::from_mseconds(start));
            clip.set_duration(Some(ClockTime::from_mseconds(end - start)));
            self.imp().inpoint.set(start);
            self.imp().outpoint.set(end);
        }
        self.commit();
    }

    fn add_effect(&self, effect: &ges::Effect) {
        let original_clip = self.imp().clip.borrow();
        let clip = original_clip.as_ref();

        if let Some(clip) = clip {
            clip.add_top_effect(effect, 0).unwrap();
        }
        self.commit();
    }

    fn commit(&self) {
        let orig_p = self.imp().pipeline.borrow();
        let p = orig_p.as_ref().unwrap();

        p.timeline().unwrap().commit_sync();
    }

    fn remove_effect(&self, effect: &ges::Effect) {
        let original_clip = self.imp().clip.borrow();
        let clip = original_clip.as_ref();

        if let Some(clip) = clip {
            clip.remove(effect).unwrap();
        }
    }

    pub fn rotate_right(&self) {
        self.imp()
            .orientation
            .set(self.imp().orientation.get().rotate_right());
        self.add_effect(&ges::Effect::new("videoflip method=clockwise").unwrap());
        self.imp()
            .effects
            .borrow_mut()
            .push("videoflip method=clockwise".to_owned());
        self.emit_by_name::<()>("orientation-flipped", &[]);
        self.imp()
            .crop_box
            .set_proportions(self.imp().crop_box.rotate_right_proportions());
        let (width, height) = self.imp().current_dimensions.get().unwrap();
        self.imp().current_dimensions.set(Some((height, width)));
    }

    pub fn rotate_left(&self) {
        self.imp()
            .orientation
            .set(self.imp().orientation.get().rotate_left());
        self.add_effect(&ges::Effect::new("videoflip method=counterclockwise").unwrap());
        self.imp()
            .effects
            .borrow_mut()
            .push("videoflip method=counterclockwise".to_owned());
        self.emit_by_name::<()>("orientation-flipped", &[]);
        self.imp()
            .crop_box
            .set_proportions(self.imp().crop_box.rotate_left_proportions());

        let (width, height) = self.imp().current_dimensions.get().unwrap();
        self.imp().current_dimensions.set(Some((height, width)));
    }

    pub fn horizontal_flip(&self) {
        self.imp()
            .orientation
            .set(self.imp().orientation.get().horizontal_flip());
        self.add_effect(&ges::Effect::new("videoflip method=horizontal-flip").unwrap());
        self.imp()
            .effects
            .borrow_mut()
            .push("videoflip method=horizontal-flip".to_owned());

        self.imp()
            .crop_box
            .set_proportions(self.imp().crop_box.horizontal_flip_proportions());
    }

    pub fn vertical_flip(&self) {
        // self.replace_with_thumbnail();
        self.imp()
            .orientation
            .set(self.imp().orientation.get().vertical_flip());
        self.add_effect(&ges::Effect::new("videoflip method=vertical-flip").unwrap());
        self.imp()
            .effects
            .borrow_mut()
            .push("videoflip method=vertical-flip".to_owned());

        self.imp()
            .crop_box
            .set_proportions(self.imp().crop_box.vertical_flip_proportions());
    }

    pub fn mute(&self) {
        let new_av = ges::Effect::new("volume volume=0").unwrap();
        self.imp().mute.set(true);
        let av_orig = self.imp().audio_level.replace(Some(new_av));

        if let Some(av_orig) = av_orig {
            self.remove_effect(&av_orig);
        }

        self.add_effect(self.imp().audio_level.borrow().as_ref().unwrap());
    }

    pub fn unmute(&self) {
        let new_av = ges::Effect::new("volume volume=1").unwrap();
        self.imp().mute.set(false);

        let av_orig = self.imp().audio_level.replace(Some(new_av));

        if let Some(av_orig) = av_orig {
            self.remove_effect(&av_orig);
        }

        self.add_effect(self.imp().audio_level.borrow().as_ref().unwrap());
    }

    pub fn kill(&self) {
        if let Some(pipeline) = self.imp().pipeline.borrow_mut().take() {
            pipeline
                .set_state(gst::State::Null)
                .expect("Unable to set the pipeline to the `Null` state");
        }
    }

    pub fn save(
        &self,
        output_path: PathBuf,
        sender: glib::Sender<Result<f64, ()>>,
        container: ContainerFormat,
        video_encoding: Option<VideoEncoding>,
        audio_encoding: Option<AudioEncoding>,
        framerate: f64,
        scaled_width: usize,
        scaled_height: usize,
        running_flag: Arc<AtomicBool>,
    ) {
        self.kill();

        let input_path = self.imp().path.borrow().to_owned();
        let orientation = self.imp().orientation.get();

        let (width, height, _) = get_width_height(input_path.to_str().unwrap().to_owned()).unwrap();

        let (width, height) = match orientation.is_width_height_swapped() {
            false => (width, height),
            true => (height, width),
        };

        let (top, right, bottom, left) = self.imp().crop_box.proportions();

        let full_scaled_width =
            width as f64 * (scaled_width as f64 / (width as f64 * (1. - right - left)));
        let full_scaled_height =
            height as f64 * (scaled_height as f64 / (height as f64 * (1. - top - bottom)));

        let mut effects = self.imp().effects.borrow().to_owned();

        if self.imp().mute.get() {
            effects.push("volume volume=0".to_owned());
        }

        let inpoint = self.imp().clip.borrow().as_ref().unwrap().inpoint();
        let duration = self.imp().clip.borrow().as_ref().unwrap().duration();

        std::thread::spawn(move || {
            let clip = ges::UriClip::new(
                url::Url::from_file_path(input_path.clone())
                    .unwrap()
                    .as_str(),
            )
            .unwrap();

            let timeline = ges::Timeline::new_audio_video();

            let layer = timeline.append_layer();
            layer.add_clip(&clip).unwrap();

            if let Some(t) = timeline.tracks().first() {
                t.set_restriction_caps(
                    &gst::Caps::builder("video/x-raw")
                        .field("framerate", gst::Fraction::new(framerate as i32, 1))
                        .field("width", scaled_width as i32)
                        .field("height", scaled_height as i32)
                        .build(),
                );
                t.elements().into_iter().for_each(|te| {
                    ges::prelude::TrackElementExt::set_child_property(
                        &te,
                        "video-direction",
                        &match orientation {
                            VideoOrientation::Identity => {
                                gstreamer_video::VideoOrientationMethod::Identity
                            }
                            VideoOrientation::R90 => gstreamer_video::VideoOrientationMethod::_90r,
                            VideoOrientation::R180 => gstreamer_video::VideoOrientationMethod::_180,
                            VideoOrientation::R270 => gstreamer_video::VideoOrientationMethod::_90l,
                            VideoOrientation::FlippedIdentity => {
                                gstreamer_video::VideoOrientationMethod::Horiz
                            }
                            VideoOrientation::FR180 => {
                                gstreamer_video::VideoOrientationMethod::Vert
                            }
                            VideoOrientation::FR90 => gstreamer_video::VideoOrientationMethod::UrLl,
                            VideoOrientation::FR270 => {
                                gstreamer_video::VideoOrientationMethod::UlLr
                            }
                        }
                        .to_value(),
                    )
                    .unwrap();

                    ges::prelude::TrackElementExt::set_child_property(
                        &te,
                        "width",
                        &(full_scaled_width as i32).to_value(),
                    )
                    .unwrap();
                    ges::prelude::TrackElementExt::set_child_property(
                        &te,
                        "height",
                        &(full_scaled_height as i32).to_value(),
                    )
                    .unwrap();
                    ges::prelude::TrackElementExt::set_child_property(
                        &te,
                        "posx",
                        &((-left * full_scaled_width) as i32).to_value(),
                    )
                    .unwrap();
                    ges::prelude::TrackElementExt::set_child_property(
                        &te,
                        "posy",
                        &((-top * full_scaled_height) as i32).to_value(),
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

            if container == ContainerFormat::GifContainer {
                pipeline
                    .set_render_settings(
                        url::Url::from_file_path(output_path).unwrap().as_str(),
                        &gstreamer_pbutils::EncodingVideoProfile::builder(
                            &gst::Caps::builder(video_encoding.unwrap().get_format()).build(),
                        )
                        .preset_name(video_encoding.unwrap().get_preset_name())
                        .build(),
                    )
                    .unwrap();
            } else if container == ContainerFormat::Same {
                pipeline
                    .set_render_settings(
                        url::Url::from_file_path(output_path.clone())
                            .unwrap()
                            .as_str(),
                        &gstreamer_pbutils::EncodingProfile::from_discoverer(
                            &Discoverer::new(gst::ClockTime::SECOND)
                                .unwrap()
                                .discover_uri(
                                    url::Url::from_file_path(input_path).unwrap().as_str(),
                                )
                                .unwrap(),
                        )
                        .unwrap(),
                    )
                    .unwrap();
            } else {
                let video_profile = gstreamer_pbutils::EncodingVideoProfile::builder(
                    &gst::Caps::builder(video_encoding.unwrap().get_format()).build(),
                )
                .preset_name(video_encoding.unwrap().get_preset_name())
                .build();

                let audio_profile = gstreamer_pbutils::EncodingAudioProfile::builder(
                    &gst::Caps::builder(audio_encoding.unwrap().get_format()).build(),
                )
                .build();

                let container_profile = gstreamer_pbutils::EncodingContainerProfile::builder(
                    &gst::Caps::builder(container.format()).build(),
                )
                .name("container")
                .add_profile(video_profile)
                .add_profile(audio_profile)
                .build();

                pipeline
                    .set_render_settings(
                        url::Url::from_file_path(output_path).unwrap().as_str(),
                        &container_profile,
                    )
                    .unwrap();
            }

            pipeline.set_mode(ges::PipelineFlags::RENDER).unwrap();

            let sender_pad = sender.clone();
            timeline.pads().first().unwrap().add_probe(
                PadProbeType::DATA_DOWNSTREAM,
                move |_, info| {
                    if let Some(PadProbeData::Buffer(data)) = &info.data {
                        if let Some(pts) = data.pts() {
                            sender_pad
                                .send(Ok(pts.mseconds() as f64 / duration.mseconds() as f64))
                                .expect("Concurrency Issues");
                        }
                    }
                    gst::PadProbeReturn::Ok
                },
            );

            pipeline.set_state(gst::State::Playing).unwrap();

            let bus = pipeline
                .bus()
                .expect("Pipeline without bus. Shouldn't happen!");

            for msg in bus.iter_timed(gst::ClockTime::NONE) {
                use gst::MessageView;

                match msg.view() {
                    MessageView::Eos(..) => {
                        sender.send(Ok(1.)).expect("Concurrency Issues");
                        break;
                    }
                    MessageView::Error(_) => {
                        pipeline.set_state(gst::State::Null).unwrap();

                        sender.send(Err(())).expect("Concurrency Issues");
                    }
                    _ => {
                        if !running_flag.load(std::sync::atomic::Ordering::SeqCst) {
                            pipeline.set_state(gst::State::Null).unwrap();

                            sender.send(Err(())).expect("Concurrency Issues");
                        }
                    }
                }
            }

            pipeline.set_state(gst::State::Null).unwrap();
        });
    }
}
