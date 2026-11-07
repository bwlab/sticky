use gtk::glib;
use gtk::prelude::*;
use gtk::subclass::prelude::*;

mod imp {
    use super::*;

    #[derive(Default, gtk::CompositeTemplate)]
    #[template(resource = "/com/vixalien/sticky/ui/theme-selector.ui")]
    pub struct ThemeSelector {
        #[template_child]
        pub follow: TemplateChild<gtk::CheckButton>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for ThemeSelector {
        const NAME: &'static str = "ThemeSelector";
        type Type = super::ThemeSelector;
        type ParentType = gtk::Widget;

        fn class_init(klass: &mut Self::Class) {
            klass.set_layout_manager_type::<gtk::BinLayout>();
            klass.set_css_name("themeselector");
            klass.bind_template();
        }

        fn instance_init(obj: &glib::subclass::InitializingObject<Self>) {
            obj.init_template();
        }
    }

    impl ObjectImpl for ThemeSelector {
        fn constructed(&self) {
            self.parent_constructed();
            let obj = self.obj();
            let style_manager = adw::StyleManager::default();

            let follow = self.follow.clone();
            let update_follow = move |m: &adw::StyleManager| follow.set_visible(m.system_supports_color_schemes());
            style_manager.connect_system_supports_color_schemes_notify(update_follow.clone());
            update_follow(&style_manager);

            let weak = obj.downgrade();
            let update_dark = move |m: &adw::StyleManager| {
                if let Some(o) = weak.upgrade() {
                    if m.is_dark() {
                        o.add_css_class("dark");
                    } else {
                        o.remove_css_class("dark");
                    }
                }
            };
            style_manager.connect_dark_notify(update_dark.clone());
            update_dark(&style_manager);

            if let Some(display) = gtk::gdk::Display::default() {
                let provider = gtk::CssProvider::new();
                provider.load_from_resource("/com/vixalien/sticky/theme-selector.css");
                gtk::style_context_add_provider_for_display(&display, &provider, gtk::STYLE_PROVIDER_PRIORITY_APPLICATION);
            }
        }

        fn dispose(&self) {
            while let Some(child) = self.obj().first_child() {
                child.unparent();
            }
        }
    }

    impl WidgetImpl for ThemeSelector {}
}

glib::wrapper! {
    pub struct ThemeSelector(ObjectSubclass<imp::ThemeSelector>) @extends gtk::Widget,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

impl ThemeSelector {
    pub fn new() -> ThemeSelector {
        glib::Object::new()
    }
}
