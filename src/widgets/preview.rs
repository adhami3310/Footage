use std::cell::RefCell;

use glib::clone;
use gst::{ClockTime, PadProbeData, PadProbeType, SeekFlags};
use gtk::{
    gdk::{self, Paintable},
    gio, glib,
    prelude::PaintableExt,
    subclass::prelude::*,
    traits::{ButtonExt, SnapshotExt},
};

use ges::prelude::*;
use ges::Effect;

use crate::info::get_width_height;

mod imp {

    // use crate::widgets::timeline::Timeline;

    use std::cell::Cell;

    use crate::widgets::{crop::Crop, timeline::Timeline};

    use super::*;

    use adw::subclass::prelude::BinImpl;
    use gtk::CompositeTemplate;

    #[derive(Debug, CompositeTemplate, Default)]
    #[template(resource = "/io/gitlab/adhami3310/Footage/blueprints/video-preview.ui")]
    pub struct VideoPreview {
        #[template_child]
        pub paint: TemplateChild<gtk::Picture>,
        #[template_child]
        pub play_pause: TemplateChild<gtk::Button>,
        #[template_child]
        pub timeline: TemplateChild<Timeline>,
        #[template_child]
        pub crop_box: TemplateChild<Crop>,

        pub proportions_flush: Cell<Option<(f64, f64, f64, f64)>>,
        pub current_dimensions: Cell<Option<(usize, usize)>>,
        pub audio_level: RefCell<Option<Effect>>,
        pub inpoint: Cell<u64>,
        pub mute: Cell<bool>,
        pub outpoint: Cell<u64>,
        pub effects: RefCell<Vec<String>>,
        pub pipeline: RefCell<Option<ges::Pipeline>>,
        pub pipeline_paintable: RefCell<Option<Paintable>>,
        pub clip: RefCell<Option<ges::UriClip>>,
        pub path: RefCell<String>,
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
        fn constructed(&self) {
            self.play_pause
                .connect_clicked(clone!(@weak self as this => move |b| {
                    if b.icon_name().unwrap() == "play-symbolic" {
                        this.obj().play();
                    } else {
                        this.obj().pause();
                    }
                }));

            // self.timeline.set_duration(100);
            // self.timeline.set_range(Some((40, 50)));

            self.timeline.connect_local(
                "set-range",
                true,
                clone!(@weak self as this => @default-return None, move |values| {
                    let values = values.to_vec();
                    let start: u64 = values.get(1).unwrap().get().expect("Expected a U64");
                    let end: u64 = values.get(2).unwrap().get().expect("Expected a U64");
                    // if this.inpoint.get() != start || this.outpoint.get() != end {
                        this.obj().set_range(start, end);
                    // }
                    None
                }),
            );

            self.timeline.connect_local(
                "moving",
                true,
                clone!(@weak self as this => @default-return None, move |_| {
                    this.obj().pause();
                    None
                }),
            );

            self.timeline.connect_local(
                "set-position",
                true,
                clone!(@weak self as this => @default-return None, move |values| {
                    let position: u64 = values[1].get().expect("Expected a U64");

                    this.obj().seek(position);

                    None
                }),
            );
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

#[gtk::template_callbacks]
impl VideoPreview {
    pub fn new() -> Self {
        let bin = glib::Object::builder::<VideoPreview>().build();

        bin
    }

    pub fn load_path(&self, path: &str) {
        gst::init().unwrap();
        ges::init().unwrap();

        let clip = ges::UriClip::new(path).unwrap();
        let duration = clip.duration().mseconds();
        self.imp().clip.replace(Some(clip));

        self.imp().timeline.set_duration(duration);
        self.imp().outpoint.set(duration);
        self.imp().timeline.set_range(Some((0, duration)));

        let (width, height) = get_width_height(path.to_owned()).unwrap();
        self.imp().current_dimensions.set(Some((width, height)));
        
        self.imp().path.replace(path.to_owned());
        self.imp().effects.replace(vec![]);

        self.refresh_ui();
    }

    fn seek(&self, position: u64) {
        let position = position.max(self.imp().inpoint.get()) - self.imp().inpoint.get();

        let op = self.imp().pipeline.borrow();

        self.pause();

        if let Some(p) = op.as_ref() {
            p.seek_simple(
                SeekFlags::ACCURATE | SeekFlags::FLUSH,
                ClockTime::from_mseconds(position),
            )
            .ok();
        }
    }

    pub fn refresh_ui(&self) {
        drop(self.imp().bus_watch.borrow_mut().take());
        if let Some(pipeline) = self.imp().pipeline.borrow_mut().take() {
            pipeline
                .set_state(gst::State::Null)
                .expect("Unable to set the pipeline to the `Null` state");
        }

        let timeline = ges::Timeline::new_audio_video();
        for t in timeline.tracks() {
            t.set_restriction_caps(&gst::Caps::new_any());
        }

        let layer = timeline.append_layer();
        layer
            .add_clip(self.imp().clip.borrow().as_ref().unwrap())
            .unwrap();

        let pipeline = ges::Pipeline::new();
        pipeline.set_timeline(&timeline).unwrap();

        let gtksink = gst::ElementFactory::make("gtk4paintablesink")
            .build()
            .unwrap();

        let paintable = gtksink.property::<gdk::Paintable>("paintable");
        // self.imp().paint.set_paintable(Some(&paintable));

        self.imp().pipeline_paintable.replace(Some(paintable));

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
                    this.imp().timeline.set_position(p + this.imp().inpoint.get());
                }
                Continue(true)
            }),
        );

        sink.add_pad(pad).unwrap();

        pipeline.set_video_sink(Some(&sink));

        let bus = pipeline.bus().unwrap();

        pipeline
            .set_state(gst::State::Playing)
            .expect("Unable to set the pipeline to the `Playing` state");

        let bus_watch = bus
            .add_watch_local(
                clone!(@weak self as this => @default-return glib::Continue(false), move |_, msg| {
                    use gst::MessageView;

                    match msg.view() {
                        MessageView::Eos(..) => {
                            this.pause();
                            this.imp().timeline.set_position(this.imp().inpoint.get());
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
                        MessageView::NewClock(_) => {
                            this.replace_with_pipeline();
                            if let Some(p) = this.imp().proportions_flush.get() {
                                this.imp().crop_box.set_proportions(p);
                                this.imp().proportions_flush.replace(None);
                            }
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

    fn current_frame(&self) -> Paintable {
        let paint = self.imp().paint.paintable().unwrap();

        let snapshot = gtk::Snapshot::new();

        paint.snapshot(
            &snapshot,
            paint.intrinsic_width() as f64,
            paint.intrinsic_height() as f64,
        );

        snapshot.to_paintable(None).unwrap()
    }

    fn pause(&self) {
        let orig_p = self.imp().pipeline.borrow();
        let p = orig_p.as_ref().unwrap();

        p.set_state(gst::State::Paused).unwrap();
        self.imp().play_pause.set_icon_name("play-symbolic");
    }

    fn is_playing(&self) -> bool {
        let orig_p = self.imp().pipeline.borrow();
        let p = orig_p.as_ref().unwrap();

        matches!(p.current_state(), gst::State::Playing)
    }

    fn play(&self) {
        let orig_p = self.imp().pipeline.borrow();
        let p = orig_p.as_ref().unwrap();

        p.set_state(gst::State::Playing).unwrap();
        self.imp().play_pause.set_icon_name("pause-symbolic");
    }

    fn set_range(&self, start: u64, end: u64) {
        self.replace_with_thumbnail();
        let original_clip = self.imp().clip.borrow();
        let clip = original_clip.as_ref();
        if let Some(clip) = clip {
            clip.set_inpoint(ClockTime::from_mseconds(start));
            clip.set_duration(Some(ClockTime::from_mseconds(end - start)));
            self.imp().inpoint.set(start);
            self.imp().outpoint.set(end);
        }
        self.refresh_ui();
    }

    fn replace_with_thumbnail(&self) {
        self.pause();

        let current_p = self.current_frame();

        self.imp().paint.set_paintable(Some(&current_p));
    }

    pub fn replace_with_pipeline(&self) {
        self.imp()
            .paint
            .set_paintable(self.imp().pipeline_paintable.borrow().as_ref());

        self.pause();
    }

    fn add_effect(&self, effect: &ges::Effect) {
        let original_clip = self.imp().clip.borrow();
        let clip = original_clip.as_ref();

        if let Some(clip) = clip {
            clip.add_top_effect(effect, 0).unwrap();
        }
    }

    fn remove_effect(&self, effect: &ges::Effect) {
        let original_clip = self.imp().clip.borrow();
        let clip = original_clip.as_ref();

        if let Some(clip) = clip {
            clip.remove(effect).unwrap();
        }
    }

    pub fn rotate_right(&self) {
        self.replace_with_thumbnail();
        self.add_effect(&ges::Effect::new("videoflip method=clockwise").unwrap());
        self.imp().effects.borrow_mut().push("videoflip method=clockwise".to_owned());
        self.imp()
            .proportions_flush
            .set(Some(self.imp().crop_box.rotate_right_proportions()));
        self.refresh_ui();
        let (width, height) = self.imp().current_dimensions.get().unwrap();
        self.imp().current_dimensions.set(Some((height, width)));
    }

    pub fn rotate_left(&self) {
        self.replace_with_thumbnail();
        self.add_effect(&ges::Effect::new("videoflip method=counterclockwise").unwrap());
        self.imp().effects.borrow_mut().push("videoflip method=counterclockwise".to_owned());

        self.imp()
            .proportions_flush
            .set(Some(self.imp().crop_box.rotate_left_proportions()));
        self.refresh_ui();
        let (width, height) = self.imp().current_dimensions.get().unwrap();
        self.imp().current_dimensions.set(Some((height, width)));
    }

    pub fn horizontal_flip(&self) {
        self.replace_with_thumbnail();
        self.add_effect(&ges::Effect::new("videoflip method=horizontal-flip").unwrap());
        self.imp().effects.borrow_mut().push("videoflip method=horizontal-flip".to_owned());

        self.imp()
            .proportions_flush
            .set(Some(self.imp().crop_box.horizontal_flip_proportions()));
        self.refresh_ui();
    }

    pub fn vertical_flip(&self) {
        self.replace_with_thumbnail();
        self.add_effect(&ges::Effect::new("videoflip method=vertical-flip").unwrap());
        self.imp().effects.borrow_mut().push("videoflip method=vertical-flip".to_owned());

        self.imp()
            .proportions_flush
            .set(Some(self.imp().crop_box.vertical_flip_proportions()));
        self.refresh_ui();
    }

    pub fn mute(&self) {
        self.replace_with_thumbnail();
        let new_av = ges::Effect::new("volume volume=0").unwrap();
        self.imp().mute.set(true);
        let av_orig = self.imp().audio_level.replace(Some(new_av));
        
        if let Some(av_orig) = av_orig {
            self.remove_effect(&av_orig);
        }

        self.add_effect(self.imp().audio_level.borrow().as_ref().unwrap());
        self.refresh_ui();
    }
    
    pub fn unmute(&self) {
        self.replace_with_thumbnail();
        let new_av = ges::Effect::new("volume volume=1").unwrap();
        self.imp().mute.set(false);
        
        let av_orig = self.imp().audio_level.replace(Some(new_av));

        if let Some(av_orig) = av_orig {
            self.remove_effect(&av_orig);
        }

        self.add_effect(self.imp().audio_level.borrow().as_ref().unwrap());
        self.refresh_ui();
    }

    pub fn save(&self, path: String, sender: glib::Sender<Result<f64, ()>>) {
        drop(self.imp().bus_watch.borrow_mut().take());
        if let Some(pipeline) = self.imp().pipeline.borrow_mut().take() {
            pipeline
                .set_state(gst::State::Null)
                .expect("Unable to set the pipeline to the `Null` state");
        }

        let input_path = self.imp().path.borrow().to_owned();

        let (width, height) = get_width_height(input_path.to_owned()).unwrap();
        let (top, right, bottom, left) = self.imp().crop_box.proportions();

        let mut effects = self.imp().effects.borrow().to_owned();

        if self.imp().mute.get() {
            effects.push("volume volume=0".to_owned());
        }

        let inpoint = self.imp().clip.borrow().as_ref().unwrap().inpoint(); 
        let duration = self.imp().clip.borrow().as_ref().unwrap().duration(); 

        std::thread::spawn(move || {
            let clip = ges::UriClip::new(&input_path).unwrap();

            clip.add(
                &ges::Effect::new(&format!(
                    "videocrop top={} right={} bottom={} left={}",
                    (top * height as f64) as u64,
                    (right * width as f64) as u64,
                    (bottom * height as f64) as u64,
                    (left * width as f64) as u64
                ))
                .unwrap(),
            )
            .ok();

            for effect in effects {
                clip.add_top_effect(&ges::Effect::new(&effect).unwrap(), 0).unwrap();
            }
            
            clip.set_inpoint(inpoint);
            clip.set_duration(Some(duration));


            // Every audiostream piped into the encodebin should be encoded using opus.
            let audio_profile = gstreamer_pbutils::EncodingAudioProfile::builder(
                &gst::Caps::builder("audio/x-opus").build(),
            )
            .build();

            // Every videostream piped into the encodebin should be encoded using vp8.
            let video_profile = gstreamer_pbutils::EncodingVideoProfile::builder(
                &gst::Caps::builder("video/x-vp8").build(),
            )
            .build();

            // All streams are then finally combined into a webm container.
            let container_profile = gstreamer_pbutils::EncodingContainerProfile::builder(
                &gst::Caps::builder("video/webm").build(),
            )
            .name("container")
            .add_profile(video_profile)
            .add_profile(audio_profile)
            .build();

            let timeline = ges::Timeline::new_audio_video();
            for t in timeline.tracks() {
                t.set_restriction_caps(&gst::Caps::new_any());
            }

            let sender_pad = sender.clone();
            timeline.pads().last().unwrap().add_probe(PadProbeType::DATA_DOWNSTREAM, move |_, info| {
                if let Some(PadProbeData::Buffer(data)) = &info.data {
                    if let Some(pts) = data.pts() {
                        sender_pad.send(Ok(pts.mseconds() as f64/ duration.mseconds() as f64)).expect("Concurrency Issues");
                    }
                }
                gst::PadProbeReturn::Ok
            });

            let layer = timeline.append_layer();            
            layer.add_clip(&clip).unwrap();

            let pipeline = ges::Pipeline::new();
            pipeline.set_timeline(&timeline).unwrap();
            pipeline
                .set_render_settings(
                    url::Url::from_file_path(path).unwrap().as_str(),
                    &container_profile,
                )
                .unwrap();

            pipeline.set_mode(ges::PipelineFlags::RENDER).unwrap();

            pipeline.set_state(gst::State::Playing).unwrap();

            let bus = pipeline
                .bus()
                .expect("Pipeline without bus. Shouldn't happen!");

            for msg in bus.iter_timed(gst::ClockTime::NONE) {
                use gst::MessageView;

                match msg.view() {
                    MessageView::Eos(..) => {
                        sender.send(Ok(1.)).expect("Concurrency Issues");
                        break
                    },
                    MessageView::Error(_) => {
                        pipeline.set_state(gst::State::Null).unwrap();

                        sender.send(Err(())).expect("Concurrency Issues");
                    }
                    _ => (),
                }
            }

            pipeline.set_state(gst::State::Null).unwrap();
        });
    }
}
