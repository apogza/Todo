/* window.rs
 *
 * Copyright 2023 Apostol Bakalov
 *
 * This program is free software: you can redistribute it and/or modify
 * it under the terms of the GNU General Public License as published by
 * the Free Software Foundation, either version 3 of the License, or
 * (at your option) any later version.
 *
 * This program is distributed in the hope that it will be useful,
 * but WITHOUT ANY WARRANTY; without even the implied warranty of
 * MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
 * GNU General Public License for more details.
 *
 * You should have received a copy of the GNU General Public License
 * along with this program.  If not, see <http://www.gnu.org/licenses/>.
 *
 * SPDX-License-Identifier: GPLcollection_objects-3.0-or-later
 */

use std::cell::RefCell;
use std::fs::File;

use adw::subclass::prelude::*;
use adw::{prelude::*, ActionRow, NavigationSplitView, ResponseAppearance, MessageDialog};
use gio::Settings;
use gtk::{
    gio, glib, pango, Entry, CustomFilter, CheckButton, NoSelection, FilterListModel, Align,
    ListBox, Stack, ListBoxRow, Label
};
use gtk::glib::SignalHandlerId;
use glib::clone;
use std::cell::OnceCell;
use crate::task_object::TaskObject;
use crate::collection_object::{CollectionData, CollectionObject};
use crate::APP_ID;
use crate::utils::data_path;

mod imp {
    use super::*;
    #[derive(Debug, Default, gtk::CompositeTemplate)]
    #[template(resource = "/org/gnome/ToDo/window.ui")]
    pub struct TodoWindow {
        pub settings: OnceCell<Settings>,
        #[template_child]
        pub entry: TemplateChild<Entry>,
        #[template_child]
        pub tasks_list: TemplateChild<gtk::ListBox>,
        pub tasks: RefCell<Option<gio::ListStore>>,
        #[template_child]
        pub collections_list: TemplateChild<ListBox>,
        #[template_child]
        pub split_view: TemplateChild<NavigationSplitView>,
        #[template_child]
        pub stack: TemplateChild<Stack>,
        pub collections:OnceCell<gio::ListStore>,
        pub current_collection: RefCell<Option<CollectionObject>>,
        pub current_filter_model: RefCell<Option<FilterListModel>>,
        pub tasks_changed_handler_id: RefCell<Option<SignalHandlerId>>
    }

    #[glib::object_subclass]
    impl ObjectSubclass for TodoWindow {
        const NAME: &'static str = "TodoWindow";
        type Type = super::TodoWindow;
        type ParentType = adw::ApplicationWindow;

        fn class_init(klass: &mut Self::Class) {
            klass.bind_template();

            klass.install_action("win.remove-done-tasks", None, |window, _, _| {
                window.remove_done_tasks();
            });

            klass.install_action_async("win.new-collection", None,
                |window, _, _| async move {
                    window.new_collection().await;
                }
            );
        }

        fn instance_init(obj: &glib::subclass::InitializingObject<Self>) {
            obj.init_template();
        }
    }

    impl ObjectImpl for TodoWindow {
        fn constructed(&self) {
            self.parent_constructed();
            let obj = self.obj();

            obj.setup_settings();
            obj.setup_collections();
            obj.restore_data();
            obj.setup_callbacks();
            obj.setup_actions();
        }
    }

    impl WidgetImpl for TodoWindow {}
    impl WindowImpl for TodoWindow {
        fn close_request(&self) -> glib::Propagation {
            let backup_data: Vec<CollectionData> = self
                .obj()
                .collections()
                .iter::<CollectionObject>()
                .filter_map(|collection_object| collection_object.ok())
                .map(|collection_object| collection_object.to_collection_data())
                .collect();

            let file = File::create(data_path()).expect("Could not create json file.");
            serde_json::to_writer(file, &backup_data).expect("Could not write data to json file");

            self.parent_close_request()
        }
    }
    impl ApplicationWindowImpl for TodoWindow {}
    impl AdwApplicationWindowImpl for TodoWindow {}
}

glib::wrapper! {
    pub struct TodoWindow(ObjectSubclass<imp::TodoWindow>)
        @extends gtk::Widget, gtk::Window, gtk::ApplicationWindow, adw::ApplicationWindow,
        @implements gio::ActionGroup, gio::ActionMap;
}

impl TodoWindow {
    pub fn new<P: glib::IsA<gtk::Application>>(application: &P) -> Self {
        glib::Object::builder()
            .property("application", application)
            .build()
    }

    async fn new_collection(&self) {
        let entry = Entry::builder()
            .placeholder_text("Name")
            .activates_default(true)
            .build();

        let cancel_response = "cancel";
        let create_response = "create";

        let dialog = MessageDialog::builder()
            .heading("New Collection")
            .transient_for(self)
            .modal(true)
            .destroy_with_parent(true)
            .close_response(cancel_response)
            .default_response(create_response)
            .extra_child(&entry)
            .build();

        dialog.add_responses(&[(cancel_response, "Cancel"), (create_response, "Create")]);
        dialog.set_response_enabled(create_response, false);
        dialog.set_response_appearance(create_response, ResponseAppearance::Suggested);
        entry.connect_changed(clone!(@weak dialog => move |entry| {
            let text = entry.text();
            let empty = text.is_empty();

            dialog.set_response_enabled(create_response, !empty);

            if empty {
                entry.add_css_class("error");
            } else {
                entry.remove_css_class("error");
            }

        }));

        let response = dialog.choose_future().await;

        if response == cancel_response {
            return;
        }

        let tasks = gio::ListStore::new::<TaskObject>();
        let title = entry.text().to_string();
        let collection = CollectionObject::new(&title, tasks);

        self.collections().append(&collection);
        self.set_current_collection(collection);

        self.imp().split_view.set_show_content(true);
    }

    fn setup_settings(&self) {
        let settings = Settings::new(APP_ID);
        self.imp()
            .settings
            .set(settings)
            .expect("settings should be set before calling setup_settings");
    }

    fn settings(&self) -> &Settings {
        self.imp()
            .settings
            .get()
            .expect("Settings houd be set ing setup_settings")
    }

    fn tasks(&self) -> gio::ListStore {
        // Get state
        self.current_collection().tasks()
    }

    fn current_collection(&self) -> CollectionObject {
        self.imp()
            .current_collection
            .borrow()
            .clone()
            .expect("current_collection should be set in 'set_current_collection'")
    }

    fn collections(&self) -> gio::ListStore {
        self.imp()
            .collections
            .get()
            .expect("collections should be set in setyp_collections")
            .clone()
    }

    fn set_filter(&self) {
        self.imp()
            .current_filter_model
            .borrow()
            .clone()
            .expect("current_filter_model should be set in set_current_collection")
            .set_filter(self.filter().as_ref());
    }

    fn setup_collections(&self) {
        let collections = gio::ListStore::new::<CollectionObject>();
        self.imp()
            .collections
            .set(collections.clone())
            .expect("Could not set collections");

        self.imp().collections_list.bind_model(
            Some(&collections),
            clone!(@weak self as window => @default-panic, move |obj| {
                let collection_object = obj.downcast_ref().expect("Expection CollectionObject");
                let row = window.create_collection_row(collection_object);
                row.upcast()
            })
        )
    }

    fn create_collection_row(&self, collection_object: &CollectionObject) -> ListBoxRow {
        let label = Label::builder()
            .ellipsize(pango::EllipsizeMode::End)
            .xalign(0.0)
            .build();

        collection_object.bind_property("title", &label, "label")
            .sync_create()
            .build();

        ListBoxRow::builder().child(&label).build()
    }

    fn set_current_collection(&self, collection: CollectionObject) {
        let tasks = collection.tasks();

        let filter_model = FilterListModel::new(Some(tasks.clone()), self.filter());
        let selection_model = NoSelection::new(Some(filter_model.clone()));
        self.imp().tasks_list.bind_model(
            Some(&selection_model),
            clone!(@weak self as window => @default-panic, move |obj| {
                let task_object = obj.downcast_ref().expect("Expecting TaskObject");
                let row = window.create_task_row(task_object);

                row.upcast()
            })
        );

        self.imp().current_filter_model.replace(Some(filter_model));

        if let Some(handler_id) = self.imp().tasks_changed_handler_id.take() {
            self.tasks().disconnect(handler_id);
        }

        self.set_task_list_visible(&tasks);
        let tasks_changed_handler_id = tasks.connect_items_changed(
            clone!(@weak self as window => move |tasks, _, _, _| {
                window.set_task_list_visible(tasks);
            })
        );

        self.imp()
            .tasks_changed_handler_id
            .replace(Some(tasks_changed_handler_id));

        self.imp().current_collection.replace(Some(collection));
        self.select_collection_row();
    }

    fn set_task_list_visible(&self, tasks: &gio::ListStore) {
        self.imp().tasks_list.set_visible(tasks.n_items() > 0);
    }

    fn select_collection_row(&self) {
        if let Some(index) = self.collections().find(&self.current_collection()) {
            let row = self.imp().collections_list.row_at_index(index as i32);
            self.imp().collections_list.select_row(row.as_ref());
        }
    }

    fn create_task_row(&self, task_object: &TaskObject) -> ActionRow {
        let check_button = CheckButton::builder()
            .valign(Align::Center)
            .can_focus(false)
            .build();

        let row = ActionRow::builder()
            .activatable_widget(&check_button)
            .build();
        row.add_prefix(&check_button);

        task_object
            .bind_property("completed", &check_button, "active")
            .bidirectional()
            .sync_create()
            .build();

        task_object
            .bind_property("content", &row, "title")
            .sync_create()
            .build();
        row
    }

    fn setup_callbacks(&self) {
        // Setup callback for activation of the entry
        self.imp()
            .entry
            .connect_activate(clone!(@weak self as window => move |_| {
                window.new_task();
            }));

        self.imp().entry.connect_icon_release(
            clone!(@weak self as window => move |_, _| {
                window.new_task();
            })
        );

        self.settings().connect_changed(
            Some("filter"),
            clone!(@weak self as window => move |_, _| {
                window.set_filter();
            })
        );

        self.set_stack();
        self.collections().connect_items_changed(
            clone!(@weak self as window => move |_, _, _, _| {
                window.set_stack();
            })
        );

        self.imp().collections_list.connect_row_activated(
            clone!(@weak self as window => move |_, row| {
                let index = row.index();
                let selected_collection = window.collections()
                    .item(index as u32)
                    .expect("Collection position needs to exist")
                    .downcast::<CollectionObject>()
                    .expect("Expected a CollectionObject");

                window.set_current_collection(selected_collection);
                window.imp().split_view.set_show_content(true);
            })
        );

        // Setup callback for clicking (and the releasing) the icon of the entry
        self.imp().entry.connect_icon_release(
            clone!(@weak self as window => move |_,_| {
                window.new_task();
            }),
        );
    }

    fn set_stack(&self) {
        if self.collections().n_items() > 0 {
            self.imp().stack.set_visible_child_name("main");
        } else {
            self.imp().stack.set_visible_child_name("placeholder");
        }
    }

    fn new_task(&self) {
        // Get content from entry and clear it
        let buffer = self.imp().entry.buffer();
        let content = buffer.text().to_string();
        if content.is_empty() {
            return;
        }
        buffer.set_text("");

        // Add new task to model
        let task = TaskObject::new(false, content);
        self.tasks().append(&task);
    }

    fn setup_actions(&self) {
        let action_filter = self.settings().create_action("filter");
        self.add_action(&action_filter);
    }

    fn remove_done_tasks(&self) {
        let tasks = self.tasks();
        let mut position = 0;

        while let Some(item) = tasks.item(position) {
            let task_object = item.downcast_ref::<TaskObject>().expect("Expecting TaskObject");

            if task_object.is_completed() {
                tasks.remove(position)
            } else {
                position += 1;
            }
        }
    }

    fn filter(&self) -> Option<CustomFilter> {
        let filter_state: String = self.settings().get("filter");

        let filter_open = CustomFilter::new(|obj| {
            let task_object = obj.downcast_ref::<TaskObject>().expect("Expecting TaskObject");
            !task_object.is_completed()
        });

        let filter_done = CustomFilter::new(|obj| {
            let task_object = obj.downcast_ref::<TaskObject>().expect("Expecting TaskObject");
            task_object.is_completed()
        });

        match filter_state.as_str() {
            "All" => None,
            "Open" => Some(filter_open),
            "Done" => Some(filter_done),
            _ => unreachable!()
        }
    }

    fn restore_data(&self) {
        if let Ok(file) = File::open(data_path()) {
            let backup_data: Vec<CollectionData> = serde_json::from_reader(file)
                .expect("Error reading json file");
            let collections: Vec<CollectionObject> = backup_data
                .into_iter()
                .map(CollectionObject::from_collection_data)
                .collect();

            self.collections().extend_from_slice(&collections);

            if let Some(first_collection) = collections.first() {
                self.set_current_collection(first_collection.clone());
            }

        }
    }
}
