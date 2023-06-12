use std::path::PathBuf;

use adw::prelude::*;
use gettextrs::gettext;
use glib::clone;
use gtk::{gio, glib, subclass::prelude::*};
use itertools::Itertools;

use crate::{
    config::{APP_ID, VERSION},
    profiles::{AudioEncoding, ContainerFormat, VideoEncoding},
    spawn, Listable,
};

mod imp {

    use std::{
        cell::{Cell, RefCell},
        sync::{atomic::AtomicBool, Arc},
    };

    use crate::{config::APP_ID, widgets::preview::VideoPreview};

    use super::*;

    use adw::subclass::prelude::AdwApplicationWindowImpl;
    use gtk::CompositeTemplate;

    #[derive(Debug, CompositeTemplate)]
    #[template(resource = "/io/gitlab/adhami3310/Footage/blueprints/window.ui")]
    pub struct AppWindow {
        #[template_child]
        pub video_preview: TemplateChild<VideoPreview>,
        #[template_child]
        pub rotate_left_button: TemplateChild<gtk::Button>,
        #[template_child]
        pub rotate_right_button: TemplateChild<gtk::Button>,
        #[template_child]
        pub horizontal_flip_button: TemplateChild<gtk::Button>,
        #[template_child]
        pub vertical_flip_button: TemplateChild<gtk::Button>,
        #[template_child]
        pub audio_button: TemplateChild<gtk::ToggleButton>,
        #[template_child]
        pub save_button: TemplateChild<gtk::Button>,
        #[template_child]
        pub stack: TemplateChild<gtk::Stack>,
        #[template_child]
        pub spinner: TemplateChild<gtk::Spinner>,
        #[template_child]
        pub progress_bar: TemplateChild<gtk::ProgressBar>,
        #[template_child]
        pub try_again_button: TemplateChild<gtk::Button>,
        #[template_child]
        pub done_button: TemplateChild<gtk::Button>,
        #[template_child]
        pub container_row: TemplateChild<adw::ComboRow>,
        #[template_child]
        pub video_encoding: TemplateChild<adw::ComboRow>,
        #[template_child]
        pub audio_encoding: TemplateChild<adw::ComboRow>,
        #[template_child]
        pub framerate_button: TemplateChild<gtk::SpinButton>,
        // #[template_child]
        // pub link_axis: TemplateChild<gtk::ToggleButton>,
        #[template_child]
        pub resize_type: TemplateChild<gtk::DropDown>,
        #[template_child]
        pub resize_scale_width_value: TemplateChild<gtk::Entry>,
        #[template_child]
        pub resize_scale_height_value: TemplateChild<gtk::Entry>,
        #[template_child]
        pub resize_width_value: TemplateChild<gtk::Entry>,
        #[template_child]
        pub resize_height_value: TemplateChild<gtk::Entry>,
        #[template_child]
        pub cancel_button: TemplateChild<gtk::Button>,

        pub running_flag: Arc<AtomicBool>,
        pub video_width: Cell<Option<usize>>,
        pub video_height: Cell<Option<usize>>,
        pub selected_video_width: Cell<Option<usize>>,
        pub selected_video_height: Cell<Option<usize>>,
        pub selected_video_path: RefCell<Option<PathBuf>>,
        pub provider: gtk::CssProvider,
        pub settings: gio::Settings,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for AppWindow {
        const NAME: &'static str = "AppWindow";
        type Type = super::AppWindow;
        type ParentType = adw::ApplicationWindow;

        fn class_init(klass: &mut Self::Class) {
            Self::bind_template(klass);
        }

        fn instance_init(obj: &glib::subclass::InitializingObject<Self>) {
            obj.init_template();
        }

        fn new() -> Self {
            Self {
                video_preview: TemplateChild::default(),
                rotate_left_button: TemplateChild::default(),
                rotate_right_button: TemplateChild::default(),
                horizontal_flip_button: TemplateChild::default(),
                vertical_flip_button: TemplateChild::default(),
                audio_button: TemplateChild::default(),
                save_button: TemplateChild::default(),
                stack: TemplateChild::default(),
                spinner: TemplateChild::default(),
                progress_bar: TemplateChild::default(),
                try_again_button: TemplateChild::default(),
                done_button: TemplateChild::default(),
                container_row: TemplateChild::default(),
                video_encoding: TemplateChild::default(),
                audio_encoding: TemplateChild::default(),
                framerate_button: TemplateChild::default(),
                // link_axis: TemplateChild::default(),
                resize_type: TemplateChild::default(),
                resize_scale_width_value: TemplateChild::default(),
                resize_scale_height_value: TemplateChild::default(),
                resize_width_value: TemplateChild::default(),
                resize_height_value: TemplateChild::default(),
                cancel_button: TemplateChild::default(),

                running_flag: Arc::new(AtomicBool::new(false)),
                video_width: Default::default(),
                video_height: Default::default(),
                selected_video_width: Default::default(),
                selected_video_height: Default::default(),
                selected_video_path: RefCell::new(None),
                provider: gtk::CssProvider::new(),
                settings: gio::Settings::new(APP_ID),
            }
        }
    }

    impl ObjectImpl for AppWindow {
        fn constructed(&self) {
            self.parent_constructed();
            let obj = self.obj();
            obj.load_window_size();
            obj.setup_gactions();
        }
    }

    impl WidgetImpl for AppWindow {}
    impl WindowImpl for AppWindow {
        fn close_request(&self) -> gtk::Inhibit {
            let obj = self.obj();

            if let Err(err) = obj.save_window_size() {
                dbg!("Failed to save window state, {}", &err);
            }

            if self.running_flag.load(std::sync::atomic::Ordering::SeqCst) {
                self.obj().close_dialog();
                glib::signal::Inhibit(true)
            } else {
                // Pass close request on to the parent
                self.parent_close_request()
            }
        }
    }

    impl ApplicationWindowImpl for AppWindow {}
    impl AdwApplicationWindowImpl for AppWindow {}
}

glib::wrapper! {
    pub struct AppWindow(ObjectSubclass<imp::AppWindow>)
        @extends gtk::Widget, gtk::Window,  gtk::ApplicationWindow,
        @implements gio::ActionMap, gio::ActionGroup, gtk::Root;
}

#[gtk::template_callbacks]
impl AppWindow {
    pub fn new<P: glib::IsA<gtk::Application>>(app: &P) -> Self {
        let win = glib::Object::builder::<AppWindow>()
            .property("application", app)
            .build();

        win.setup_callbacks();

        let container_formats = gtk::StringList::new(&[]);

        for cf in ContainerFormat::get_all() {
            container_formats.append(&cf.for_display());
        }

        win.imp().container_row.set_model(Some(
            &ContainerFormat::get_all()
                .into_iter()
                .map(|m| m.for_display())
                .collect_vec()
                .to_list(),
        ));

        win
    }

    fn setup_gactions(&self) {
        self.add_action_entries([
            gio::ActionEntry::builder("close")
                .activate(clone!(@weak self as window => move |_,_, _| {
                    window.close();
                }))
                .build(),
            gio::ActionEntry::builder("about")
                .activate(clone!(@weak self as window => move |_, _, _| {
                    window.show_about();
                }))
                .build(),
            gio::ActionEntry::builder("open")
                .activate(clone!(@weak self as window => move |_, _, _| {
                    spawn!(async move {
                        window.open_dialog().await.ok();
                    });
                }))
                .build(),
        ]);
    }

    fn setup_callbacks(&self) {
        let imp = self.imp();

        imp.rotate_left_button
            .connect_clicked(clone!(@weak self as this => move |_| {
                this.imp().video_preview.rotate_left();
            }));
        imp.rotate_right_button
            .connect_clicked(clone!(@weak self as this => move |_| {
                this.imp().video_preview.rotate_right();
            }));
        imp.horizontal_flip_button
            .connect_clicked(clone!(@weak self as this => move |_| {
                this.imp().video_preview.horizontal_flip();
            }));
        imp.vertical_flip_button
            .connect_clicked(clone!(@weak self as this => move |_| {
                this.imp().video_preview.vertical_flip();
            }));
        imp.audio_button
            .connect_toggled(clone!(@weak self as this => move |b| {
                if b.is_active() {
                    b.set_icon_name("audio-volume-muted-symbolic");
                    this.imp().video_preview.mute();
                } else {
                    b.set_icon_name("audio-volume-high-symbolic");
                    this.imp().video_preview.unmute();
                }
            }));
        imp.save_button
            .connect_clicked(clone!(@weak self as this => move |_| {
                spawn!(async move {
                    this.save_dialog().await.ok();
                });
            }));
        imp.try_again_button
            .connect_clicked(clone!(@weak self as this => move |_| {
                this.imp().video_preview.refresh_ui();
                this.imp().stack.set_visible_child_name("editing");
            }));
        imp.done_button
            .connect_clicked(clone!(@weak self as this => move |_| {
                this.imp().stack.set_visible_child_name("welcome");
            }));
        imp.cancel_button
            .connect_clicked(clone!(@weak self as this => move |_| {
                this.convert_cancel();
            }));
        imp.container_row
            .connect_selected_notify(clone!(@weak self as this => move |_| {
                this.update_options();
            }));
        imp.resize_type
            .connect_selected_notify(clone!(@weak self as this => move |rt| {
                match rt.selected() {
                    0 => {
                        this.imp().resize_width_value.set_visible(false);
                        this.imp().resize_height_value.set_visible(false);
                        this.imp().resize_scale_width_value.set_visible(true);
                        this.imp().resize_scale_height_value.set_visible(true);
                    }
                    1 => {
                        this.imp().resize_width_value.set_visible(true);
                        this.imp().resize_height_value.set_visible(true);
                        this.imp().resize_scale_width_value.set_visible(false);
                        this.imp().resize_scale_height_value.set_visible(false);
                    }
                    _ => unreachable!()
                }
            }));
        imp.resize_width_value
            .connect_changed(clone!(@weak self as this => move |_| {
                this.update_height_from_width();
            }));
        imp.resize_height_value
            .connect_changed(clone!(@weak self as this => move |_| {
                this.update_width_from_height();
            }));

        imp.resize_scale_height_value
            .connect_changed(clone!(@weak self as this => move |_| {
                // if this.imp().link_axis.is_active() && this.imp().link_axis.is_visible() {
                    let old_value = this.imp().resize_scale_width_value.text().as_str().to_owned();
                    let new_value = this.imp().resize_scale_height_value.text().as_str().to_owned();
                    if old_value != new_value && !new_value.is_empty() {
                        this.imp().resize_scale_width_value.set_text(&new_value);
                    }
                // }
            }));

        imp.resize_scale_width_value
            .connect_changed(clone!(@weak self as this => move |_| {
                // if this.imp().link_axis.is_active() && this.imp().link_axis.is_visible() {
                    let old_value = this.imp().resize_scale_height_value.text().as_str().to_owned();
                    let new_value = this.imp().resize_scale_width_value.text().as_str().to_owned();
                    if old_value != new_value && !new_value.is_empty() {
                        this.imp().resize_scale_height_value.set_text(&new_value);
                    }
                // }
            }));

        imp.video_preview.imp().crop_box.connect_local("crop-box-changed", true, clone!(@weak self as this => @default-return None, move |v| {
            this.imp().video_height.get()?;

            let t: f64 = v.get(1).unwrap().get().expect("Expected a F64");
            let r: f64 = v.get(2).unwrap().get().expect("Expected a F64");
            let b: f64 = v.get(3).unwrap().get().expect("Expected a F64");
            let l: f64 = v.get(4).unwrap().get().expect("Expected a F64");

            let selected_height = (this.imp().video_height.get().unwrap() as f64 * (1. - t - b)) as usize / 2 * 2;
            let selected_width = (this.imp().video_width.get().unwrap() as f64 * (1. - l - r)) as usize / 2 * 2;

            this.imp().selected_video_height.set(Some(selected_height));
            this.imp().selected_video_width.set(Some(selected_width));

            this.imp().resize_height_value.set_text(&selected_height.to_string());
            this.imp().resize_width_value.set_text(&selected_width.to_string());

            None
        }));
    }

    fn update_width_from_height(&self) {
        // if self.imp().link_axis.is_active() && self.imp().link_axis.is_visible() {
        if let (Some(video_width), Some(video_height)) = (
            self.imp().selected_video_width.get(),
            self.imp().selected_video_height.get(),
        ) {
            let old_value = self.imp().resize_width_value.text().as_str().to_owned();
            let other_text = self.imp().resize_height_value.text().as_str().to_owned();
            if other_text.is_empty() {
                return;
            }

            let other_way = generate_height_from_width(
                old_value.parse().unwrap_or(0),
                (video_width, video_height),
            )
            .to_string();

            if other_way == other_text {
                return;
            }

            let new_value = generate_width_from_height(
                other_text.parse().unwrap_or(0),
                (video_width, video_height),
            )
            .to_string();

            if old_value != new_value && new_value != "0" {
                self.imp().resize_width_value.set_text(&new_value);
            }
        }
        // }
    }

    fn update_height_from_width(&self) {
        // if self.imp().link_axis.is_active() && self.imp().link_axis.is_visible() {
        if let (Some(video_width), Some(video_height)) = (
            self.imp().selected_video_width.get(),
            self.imp().selected_video_height.get(),
        ) {
            let old_value = self.imp().resize_height_value.text().as_str().to_owned();
            let other_text = self.imp().resize_width_value.text().as_str().to_owned();
            if other_text.is_empty() {
                return;
            }

            let other_way = generate_width_from_height(
                old_value.parse().unwrap_or(0),
                (video_width, video_height),
            )
            .to_string();

            if other_way == other_text {
                return;
            }

            let new_value = generate_height_from_width(
                other_text.parse().unwrap_or(0),
                (video_width, video_height),
            )
            .to_string();

            if old_value != new_value && new_value != "0" {
                self.imp().resize_height_value.set_text(&new_value);
            }
        }
        // }
    }

    fn close_dialog(&self) {
        let stop_converting_dialog = adw::MessageDialog::new(
            Some(self),
            Some(&gettext("Stop rendering?")),
            Some(&gettext("You will lose all progress.")),
        );

        stop_converting_dialog.add_response("cancel", &gettext("_Cancel"));
        stop_converting_dialog.add_response("stop", &gettext("_Stop"));
        stop_converting_dialog
            .set_response_appearance("stop", adw::ResponseAppearance::Destructive);
        stop_converting_dialog.connect_response(
            None,
            clone!(@weak self as this => move |_, response_id| {
                if response_id == "stop" {
                    this.imp()
                        .running_flag
                        .store(false, std::sync::atomic::Ordering::SeqCst);
                    this.close();
                }
            }),
        );
        stop_converting_dialog.present();
    }

    fn convert_cancel(&self) {
        let stop_converting_dialog = adw::MessageDialog::new(
            Some(self),
            Some(&gettext("Stop rendering?")),
            Some(&gettext("You will lose all progress.")),
        );

        stop_converting_dialog
            .add_responses(&[("cancel", &gettext("_Cancel")), ("stop", &gettext("_Stop"))]);
        stop_converting_dialog
            .set_response_appearance("stop", adw::ResponseAppearance::Destructive);

        stop_converting_dialog.connect_response(
            None,
            clone!(@weak self as this => move |_, response_id| {
                if response_id == "stop" {
                    this.imp()
                        .running_flag
                        .store(false, std::sync::atomic::Ordering::SeqCst);
                    this.imp().stack.set_visible_child_name("failure");
                }
            }),
        );

        stop_converting_dialog.present();
    }

    async fn open_dialog(&self) -> ashpd::Result<()> {
        let files = ashpd::desktop::file_chooser::SelectedFiles::open_file()
            .modal(true)
            .identifier(ashpd::WindowIdentifier::from_native(&self.native().unwrap()).await)
            .multiple(Some(false))
            .filter(
                ashpd::desktop::file_chooser::FileFilter::new("Video Files").mimetype("video/*"),
            )
            .send()
            .await?
            .response()?;

        let path = files.uris().first().unwrap().to_file_path().unwrap();

        self.open_file(path);

        Ok(())
    }

    async fn save_dialog(&self) -> ashpd::Result<()> {
        let input_path = self.imp().selected_video_path.borrow().to_owned().unwrap();

        let input_path_stem = input_path.file_stem().unwrap().to_str().unwrap().to_owned();

        let extension = match self.selected_container() {
            ContainerFormat::Same => input_path.extension().unwrap().to_str().unwrap().to_owned(),
            x => x.extension().to_owned(),
        };

        let files = ashpd::desktop::file_chooser::SelectedFiles::save_file()
            .modal(true)
            .identifier(ashpd::WindowIdentifier::from_native(&self.native().unwrap()).await)
            .current_name(Some(format!("{}.{}", input_path_stem, extension).as_ref()))
            .send()
            .await?
            .response()?;

        let path = files.uris().first().unwrap().to_file_path().unwrap();

        self.save_file(path);

        Ok(())
    }

    fn selected_container(&self) -> ContainerFormat {
        ContainerFormat::get_all()[self.imp().container_row.selected() as usize]
    }

    fn selected_video_encoding(&self) -> Option<VideoEncoding> {
        let list = self.selected_container().viable_matchings().0;
        if list.is_empty() {
            None
        } else {
            Some(list[self.imp().video_encoding.selected() as usize])
        }
    }

    fn selected_audio_encoding(&self) -> Option<AudioEncoding> {
        let list = self.selected_container().viable_matchings().1;
        if list.is_empty() {
            None
        } else {
            Some(list[self.imp().audio_encoding.selected() as usize])
        }
    }

    fn update_options(&self) {
        let imp = self.imp();

        let selected_container = self.selected_container();

        let (available_video, available_audio) = selected_container.viable_matchings();

        imp.audio_encoding.set_visible(available_audio.len() > 1);
        imp.audio_encoding.set_model(Some(
            &available_audio
                .into_iter()
                .map(|e| e.for_display().to_owned())
                .collect_vec()
                .to_list(),
        ));

        imp.video_encoding.set_visible(available_video.len() > 1);
        imp.video_encoding.set_model(Some(
            &available_video
                .into_iter()
                .map(|e| e.for_display().to_owned())
                .collect_vec()
                .to_list(),
        ));
    }

    fn save_file(&self, path: PathBuf) {
        self.imp().stack.set_visible_child_name("exporting");
        glib::MainContext::default().iteration(true);

        let (scaled_width, scaled_height) = match self.imp().resize_type.selected() {
            0 => {
                let (sw, sh): (usize, usize) = (
                    self.imp().resize_scale_width_value.text().parse().unwrap(),
                    self.imp().resize_scale_height_value.text().parse().unwrap(),
                );
                let (bw, bh) = (
                    self.imp().selected_video_width.get().unwrap(),
                    self.imp().selected_video_height.get().unwrap(),
                );
                (bw * sw / 100 / 2 * 2, bh * sh / 100 / 2 * 2)
            }
            1 => (
                self.imp()
                    .resize_width_value
                    .text()
                    .parse::<usize>()
                    .unwrap()
                    / 2
                    * 2,
                self.imp()
                    .resize_height_value
                    .text()
                    .parse::<usize>()
                    .unwrap()
                    / 2
                    * 2,
            ),
            _ => unreachable!(),
        };

        let running_flag = self.imp().running_flag.clone();
        running_flag.store(true, std::sync::atomic::Ordering::SeqCst);

        let (sender, receiver) = glib::MainContext::channel(glib::PRIORITY_DEFAULT);
        self.imp().video_preview.save(
            path,
            sender,
            self.selected_container(),
            self.selected_video_encoding(),
            self.selected_audio_encoding(),
            self.imp().framerate_button.value(),
            scaled_width,
            scaled_height,
            running_flag,
        );
        receiver.attach(
            None,
            clone!(@weak self as this => @default-return Continue(false), move |p| {
                match p {
                    Ok(p) if p == 1.0 => {
                        this.imp().stack.set_visible_child_name("success");
                        this.imp().running_flag.store(false, std::sync::atomic::Ordering::SeqCst);
                        Continue(false)
                    }
                    Ok(p) => {
                        this.imp().progress_bar.set_fraction(p);
                        Continue(true)
                    }
                    _ => {
                        this.imp().stack.set_visible_child_name("failure");
                        this.imp().running_flag.store(false, std::sync::atomic::Ordering::SeqCst);
                        Continue(false)
                    }
                }
            }),
        );
    }

    fn create_ui(&self, path: PathBuf) {
        glib::MainContext::default().iteration(true);
        let Ok((width, height, framerate)) = self.imp().video_preview.load_path(path) else {
            self.imp().stack.set_visible_child_name("invalid");
            return;
        };
        self.imp().video_width.set(Some(width));
        self.imp().video_height.set(Some(height));
        self.imp().selected_video_width.set(Some(width));
        self.imp().selected_video_height.set(Some(height));
        self.imp().resize_scale_height_value.set_text("100");
        self.imp().resize_scale_width_value.set_text("100");
        self.imp().resize_height_value.set_text(&height.to_string());
        self.imp().resize_width_value.set_text(&width.to_string());
        self.imp()
            .framerate_button
            .set_value(framerate.unwrap_or(30) as f64);

        self.imp().stack.set_visible_child_name("editing");
        self.imp().spinner.stop();
    }

    pub fn open_file(&self, path: PathBuf) {
        self.imp().selected_video_path.replace(Some(path.clone()));

        self.imp().stack.set_visible_child_name("loading");
        self.imp().spinner.start();

        self.create_ui(path);
    }

    fn show_about(&self) {
        let about = adw::AboutWindow::builder()
            .transient_for(self)
            .application_icon(APP_ID)
            .application_name(gettext("Footage"))
            .developer_name("Khaleel Al-Adhami")
            .website("https://gitlab.com/adhami3310/Footage")
            .issue_url("https://gitlab.com/adhami3310/Footage/-/issues")
            .developers(vec!["Khaleel Al-Adhami"])
            .artists(vec!["kramo https://kramo.hu"])
            .license_type(gtk::License::Gpl30)
            .version(VERSION)
            .build();

        about.present();
    }
}

trait SettingsStore {
    fn save_window_size(&self) -> Result<(), glib::BoolError>;
    fn load_window_size(&self);
}

impl SettingsStore for AppWindow {
    fn save_window_size(&self) -> Result<(), glib::BoolError> {
        let imp = self.imp();

        let (width, height) = self.default_size();

        imp.settings.set_int("window-width", width)?;
        imp.settings.set_int("window-height", height)?;

        imp.settings
            .set_boolean("is-maximized", self.is_maximized())?;

        Ok(())
    }

    fn load_window_size(&self) {
        let imp = self.imp();

        let width = imp.settings.int("window-width");
        let height = imp.settings.int("window-height");
        let is_maximized = imp.settings.boolean("is-maximized");

        self.set_default_size(width, height);

        if is_maximized {
            self.maximize();
        }
    }
}

fn generate_width_from_height(height: usize, image_dim: (usize, usize)) -> usize {
    ((height as f64) * (image_dim.0 as f64) / (image_dim.1 as f64)).round() as usize
}

fn generate_height_from_width(width: usize, image_dim: (usize, usize)) -> usize {
    ((width as f64) * (image_dim.1 as f64) / (image_dim.0 as f64)).round() as usize
}
