use std::{
    cell::RefCell,
    path::PathBuf,
    sync::{Arc, atomic::AtomicBool},
};
use thiserror::Error;

use glib::clone;
use gst::{ClockTime, PadProbeData, PadProbeType, SeekFlags, prelude::*};
use gstreamer_pbutils::Discoverer;
use gtk::{gdk, gio, glib, subclass::prelude::*};

use log::{error, info};

use crate::{
    info::{Dimensions, Framerate, get_info},
    orientation::VideoOrientation,
    profiles::OutputFormat,
    render::{RenderJob, run_render},
};

mod imp {

    use std::cell::Cell;

    use crate::{orientation::VideoOrientation, widgets::crop::Crop};

    use super::*;

    use adw::subclass::prelude::BinImpl;
    use glib::subclass::Signal;
    use gst::bus::BusWatchGuard;
    use gtk::CompositeTemplate;

    #[derive(CompositeTemplate, Default)]
    #[template(resource = "/io/gitlab/adhami3310/Footage/blueprints/video-preview.ui")]
    pub struct VideoPreview {
        #[template_child]
        pub paint: TemplateChild<gtk::Picture>,
        #[template_child]
        pub crop_box: TemplateChild<Crop>,

        pub original_duration: Cell<u64>,
        pub original_dimensions: Cell<Option<Dimensions<u32>>>,

        pub current_dimensions: Cell<Option<Dimensions<u32>>>,
        pub orientation: Cell<VideoOrientation>,
        pub inpoint: Cell<u64>,
        pub mute: Cell<bool>,
        pub outpoint: Cell<u64>,
        pub pipeline: RefCell<Option<gst::Element>>,
        pub videoflip: RefCell<Option<gst::Element>>,
        pub path: RefCell<PathBuf>,
        pub ended: Cell<bool>,
        pub bus_watch: RefCell<Option<BusWatchGuard>>,
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
            static SIGNALS: Lazy<[Signal; 4]> = Lazy::new(|| {
                [
                    Signal::builder("orientation-flipped")
                        .param_types(std::iter::empty::<glib::Type>())
                        .build(),
                    Signal::builder("set-position")
                        .param_types([glib::Type::U64])
                        .build(),
                    Signal::builder("preview-ready")
                        .param_types(std::iter::empty::<glib::Type>())
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
    @implements gio::ActionMap, gio::ActionGroup, gtk::Root, gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

impl Default for VideoPreview {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Error)]
pub enum VideoPreviewError {
    #[error("GStreamer error: {0}")]
    Glib(#[from] glib::Error),
    #[error("invalid file path")]
    InvalidPath,
    #[error("failed to get media info")]
    NoInfo,
}

#[gtk::template_callbacks]
impl VideoPreview {
    pub fn new() -> Self {
        glib::Object::builder::<VideoPreview>().build()
    }

    pub fn reset(&self) {
        self.imp().crop_box.reset();
        self.imp().orientation.set(VideoOrientation::Identity);
        self.imp().current_dimensions.set(None);
        self.kill();
        self.imp().videoflip.replace(None);
        self.imp().path.replace(PathBuf::new());
        self.imp().ended.replace(false);
        self.imp().bus_watch.replace(None);
        self.imp().paint.set_paintable(None::<&gdk::Paintable>);
        self.imp().mute.set(false);
        self.imp().original_duration.set(0);
        self.imp().original_dimensions.set(None);
        self.emit_by_name::<()>("mode-changed", &[&false]);
    }

    pub async fn load_path(
        &self,
        path: PathBuf,
    ) -> Result<(Dimensions<u32>, u64, Option<Framerate>, bool), VideoPreviewError> {
        let url =
            url::Url::from_file_path(path.clone()).map_err(|_| VideoPreviewError::InvalidPath)?;

        info!("Loading path: {}", url.as_str());

        let discoverer = Discoverer::new(ClockTime::from_seconds(10))?;
        let info = discoverer.discover_uri(url.as_str())?;
        let duration = info.duration().map(|d| d.mseconds()).unwrap_or(0);

        self.imp().inpoint.set(0);
        self.imp().outpoint.set(duration);
        self.imp().original_duration.set(duration);

        let (dimensions, framerate, has_audio) =
            get_info(path.to_str().unwrap().to_owned()).ok_or(VideoPreviewError::NoInfo)?;

        self.imp().original_dimensions.set(Some(dimensions));
        self.imp().current_dimensions.set(Some(dimensions));

        self.imp().path.replace(path);

        self.imp().crop_box.set_proportions((0., 0., 0., 0.));
        self.imp().orientation.set(Default::default());
        self.emit_by_name::<()>("mode-changed", &[&false]);

        if has_audio {
            self.imp().mute.set(false);
        } else {
            self.imp().mute.set(true);
        }

        self.refresh_ui();

        Ok((dimensions, duration, framerate, has_audio))
    }

    pub fn seek(&self, position: u64) {
        self.pause();

        self.quiet_seek(position);
    }

    pub fn quiet_seek(&self, position: u64) {
        if position == self.imp().outpoint.get() {
            self.imp().ended.set(true);
        }

        let op = self.imp().pipeline.borrow();

        if let Some(p) = op.as_ref() {
            p.seek_simple(SeekFlags::FLUSH, ClockTime::from_mseconds(position))
                .ok();
        }
    }

    pub fn refresh_ui(&self) {
        self.kill();

        let uri = {
            let path = self.imp().path.borrow();
            url::Url::from_file_path(path.as_path())
                .expect("Invalid path for playbin3")
                .to_string()
        };

        let playbin = gst::ElementFactory::make("playbin3")
            .property("uri", &uri)
            .build()
            .unwrap();

        // Video sink: videoconvertscale -> videoflip -> gtk4paintablesink
        let gtksink = gst::ElementFactory::make("gtk4paintablesink")
            .build()
            .unwrap();

        let paintable = gtksink.property::<gdk::Paintable>("paintable");

        self.imp().paint.set_paintable(Some(&paintable));

        let video_sink = gst::Bin::default();
        let convert = gst::ElementFactory::make("videoconvertscale")
            .build()
            .unwrap();
        let flip = gst::ElementFactory::make("videoflip")
            .property(
                "video-direction",
                self.imp()
                    .orientation
                    .get()
                    .to_gst_video_orientation_method(),
            )
            .build()
            .unwrap();

        video_sink.add_many([&convert, &flip, &gtksink]).unwrap();
        gst::Element::link_many([&convert, &flip, &gtksink]).unwrap();

        let pad = gst::GhostPad::with_target(&convert.static_pad("sink").unwrap()).unwrap();

        let (sender, receiver) = async_channel::bounded(1);

        pad.add_probe(PadProbeType::DATA_DOWNSTREAM, move |_, info| {
            if let Some(PadProbeData::Buffer(data)) = &info.data
                && let Some(pts) = data.pts()
            {
                sender
                    .send_blocking(pts.mseconds())
                    .expect("Concurrency Issues");
            }

            gst::PadProbeReturn::Ok
        });

        video_sink.add_pad(&pad).unwrap();

        playbin.set_property("video-sink", &video_sink);

        // Apply mute state
        if self.imp().mute.get() {
            playbin.set_property("mute", true);
        }

        let bus = playbin.bus().unwrap();

        playbin
            .set_state(gst::State::Paused)
            .expect("Unable to set the pipeline to the `Paused` state");

        let bus_watch = bus
            .add_watch_local(clone!(
                #[weak(rename_to=this)]
                self,
                #[upgrade_or]
                glib::ControlFlow::Break,
                move |_, msg| {
                    use gst::MessageView;

                    match msg.view() {
                        MessageView::Eos(..) => {
                            this.pause();
                            this.imp().ended.set(true);
                        }
                        MessageView::Error(err) => {
                            error!(
                                "Error from {:?}: {} ({:?})",
                                err.src().map(|s| s.path_string()),
                                err.error(),
                                err.debug()
                            );
                        }
                        _ => (),
                    };

                    glib::ControlFlow::Continue
                }
            ))
            .expect("Failed to add bus watch");

        self.imp().videoflip.replace(Some(flip));
        self.imp().pipeline.replace(Some(playbin));
        self.imp().bus_watch.replace(Some(bus_watch));

        glib::spawn_future_local(clone!(
            #[weak(rename_to = this)]
            self,
            async move {
                let mut sent_ready = false;

                while let Ok(p) = receiver.recv().await {
                    if !sent_ready {
                        sent_ready = true;
                        this.emit_by_name::<()>("preview-ready", &[]);
                    }
                    if this.is_playing() {
                        this.emit_by_name::<()>("set-position", &[&p]);
                    }
                }
            }
        ));
    }

    pub fn pause(&self) {
        let orig_p = self.imp().pipeline.borrow();
        let p = orig_p.as_ref().unwrap();

        p.set_state(gst::State::Paused).unwrap();
        self.emit_by_name::<()>("mode-changed", &[&false]);
    }

    fn is_playing(&self) -> bool {
        let orig_p = self.imp().pipeline.borrow();
        let p = orig_p.as_ref().unwrap();

        matches!(p.current_state(), gst::State::Playing)
    }

    pub fn play(&self) {
        let orig_p = self.imp().pipeline.borrow();
        let p = orig_p.as_ref().unwrap();

        if self.imp().ended.get() {
            self.imp().ended.set(false);
            self.quiet_seek(self.imp().inpoint.get());
        }

        p.set_state(gst::State::Playing).unwrap();
        self.emit_by_name::<()>("mode-changed", &[&true]);
    }

    pub fn set_range(&self, start: u64, end: u64) {
        self.imp().inpoint.set(start);
        self.imp().outpoint.set(end);
    }

    fn update_videoflip(&self) {
        if let Some(flip) = self.imp().videoflip.borrow().as_ref() {
            flip.set_property(
                "video-direction",
                self.imp()
                    .orientation
                    .get()
                    .to_gst_video_orientation_method(),
            );
        }
        // Force a frame refresh so the change is visible while paused.
        if let Some(p) = self.imp().pipeline.borrow().as_ref()
            && let Some(position) = p.query_position::<ClockTime>()
        {
            p.seek_simple(SeekFlags::FLUSH, position).ok();
        }
    }

    pub fn rotate_right(&self) {
        self.imp()
            .orientation
            .set(self.imp().orientation.get().rotate_right());
        self.update_videoflip();
        self.emit_by_name::<()>("orientation-flipped", &[]);
        self.imp()
            .crop_box
            .set_proportions(self.imp().crop_box.rotate_right_proportions());
        let dimensions = self.imp().current_dimensions.get().unwrap();
        self.imp().current_dimensions.set(Some(dimensions.swap()));
    }

    pub fn rotate_left(&self) {
        self.imp()
            .orientation
            .set(self.imp().orientation.get().rotate_left());
        self.update_videoflip();
        self.emit_by_name::<()>("orientation-flipped", &[]);
        self.imp()
            .crop_box
            .set_proportions(self.imp().crop_box.rotate_left_proportions());

        let dimensions = self.imp().current_dimensions.get().unwrap();
        self.imp().current_dimensions.set(Some(dimensions.swap()));
    }

    pub fn horizontal_flip(&self) {
        self.imp()
            .orientation
            .set(self.imp().orientation.get().horizontal_flip());
        self.update_videoflip();

        self.imp()
            .crop_box
            .set_proportions(self.imp().crop_box.horizontal_flip_proportions());
    }

    pub fn vertical_flip(&self) {
        self.imp()
            .orientation
            .set(self.imp().orientation.get().vertical_flip());
        self.update_videoflip();

        self.imp()
            .crop_box
            .set_proportions(self.imp().crop_box.vertical_flip_proportions());
    }

    pub fn mute(&self) {
        self.imp().mute.set(true);
        if let Some(p) = self.imp().pipeline.borrow().as_ref() {
            p.set_property("mute", true);
        }
    }

    pub fn unmute(&self) {
        self.imp().mute.set(false);
        if let Some(p) = self.imp().pipeline.borrow().as_ref() {
            p.set_property("mute", false);
        }
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
        sender: async_channel::Sender<Result<(u64, u64), ()>>,
        output_format: OutputFormat,
        framerate: Framerate,
        scaled_dimension: Dimensions<u32>,
        running_flag: Arc<AtomicBool>,
    ) {
        self.kill();

        info!(
            "Converting with output path: {:?}, output format: {:?}, framerate: {:?}, scaled dimension: {:?}",
            output_path, output_format, framerate, scaled_dimension
        );

        let input_path = self.imp().path.borrow().to_owned();
        let orientation = self.imp().orientation.get();

        let dimensions = self.imp().original_dimensions.get().unwrap_or_else(|| {
            panic!(
                "Original dimensions should be set before saving. This is a bug. Path: {:?}",
                input_path
            )
        });

        let dimensions: Dimensions<f64> = dimensions.into();

        let input_dimensions = match orientation.is_width_height_swapped() {
            false => dimensions,
            true => dimensions.swap(),
        };

        let (top, right, bottom, left) = self.imp().crop_box.proportions();

        let full_scaled_width = input_dimensions.width
            * (scaled_dimension.width_f64() / (input_dimensions.width * (1. - right - left)));
        let full_scaled_height = input_dimensions.height
            * (scaled_dimension.height_f64() / (input_dimensions.height * (1. - top - bottom)));

        let job = RenderJob {
            input_path,
            output_path,
            output_format,
            framerate,
            scaled_dimension,
            orientation,
            full_scaled_width,
            full_scaled_height,
            crop_left: left,
            crop_top: top,
            mute: self.imp().mute.get(),
            inpoint: ClockTime::from_mseconds(self.imp().inpoint.get()),
            duration: ClockTime::from_mseconds(
                self.imp().outpoint.get() - self.imp().inpoint.get(),
            ),
            sender,
            running_flag,
        };

        std::thread::spawn(move || run_render(job));
    }
}
