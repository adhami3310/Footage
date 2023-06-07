use adw::prelude::*;
use gettextrs::gettext;
use glib::clone;
use gtk::{gio, glib, subclass::prelude::*};
use url::Url;

use crate::{
    config::{APP_ID, VERSION},
    spawn,
};

mod imp {

    use std::cell::RefCell;

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

        pub selected_video_path: RefCell<String>,
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

                selected_video_path: RefCell::new("".to_owned()),
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

            // Pass close request on to the parent
            self.parent_close_request()
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
        imp.audio_button.connect_toggled(clone!(@weak self as this => move |b| {
            if b.is_active() {
                b.set_icon_name("audio-volume-muted-symbolic");
                this.imp().video_preview.mute();
            } else {
                b.set_icon_name("audio-volume-high-symbolic");
                this.imp().video_preview.unmute();
            }
        }));
        imp.save_button.connect_clicked(clone!(@weak self as this => move |_| {
            spawn!(async move {
                this.save_dialog().await.ok();
            });
        }));
        imp.try_again_button.connect_clicked(clone!(@weak self as this => move |_| {
            this.imp().video_preview.refresh_ui();
            this.imp().stack.set_visible_child_name("editing");
        }));
        imp.done_button.connect_clicked(clone!(@weak self as this => move |_| {
            this.imp().stack.set_visible_child_name("welcome");
        }));
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

        let path = files.uris().first().unwrap().path().to_owned();

        self.open_file(path);

        Ok(())
    }

    async fn save_dialog(&self) -> ashpd::Result<()> {
        let path_file_name = std::path::Path::new(&self.imp().selected_video_path.borrow().to_owned()).file_stem().unwrap().to_str().unwrap().to_owned();

        let files = ashpd::desktop::file_chooser::SelectedFiles::save_file()
            .modal(true)
            .identifier(ashpd::WindowIdentifier::from_native(&self.native().unwrap()).await)
            .current_name(Some(format!("{}.mp4", path_file_name).as_ref()))
            .send()
            .await?
            .response()?;

        let path = files.uris().first().unwrap().path().to_owned();

        self.save_file(path);

        Ok(())
    }

    fn save_file(&self, path: String) {
        self.imp().stack.set_visible_child_name("exporting");
        glib::MainContext::default().iteration(true);
        let (sender, receiver) = glib::MainContext::channel(glib::PRIORITY_DEFAULT);
        self.imp().video_preview.save(path, sender);
        receiver.attach(None, clone!(@weak self as this => @default-return Continue(false), move |p| {
            match p {
                Ok(p) if p == 1.0 => {
                    this.imp().stack.set_visible_child_name("success");
                    Continue(false)
                }
                Ok(p) => {
                    this.imp().progress_bar.set_fraction(p);
                    Continue(true)
                }
                _ => {
                    this.imp().stack.set_visible_child_name("failure");
                    Continue(false)
                }
            }
        }));
    }

    fn create_ui(&self, path_url: Url) {
        glib::MainContext::default().iteration(true);
        self.imp().video_preview.load_path(path_url.as_str());
        self.imp().stack.set_visible_child_name("editing");
        self.imp().spinner.stop();
    }

    pub fn open_file(&self, path: String) {
        let path_url = url::Url::from_file_path(&path).unwrap();

        self.imp().selected_video_path.replace(path.clone());

        self.imp().stack.set_visible_child_name("loading");
        self.imp().spinner.start();

        self.create_ui(path_url);
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
