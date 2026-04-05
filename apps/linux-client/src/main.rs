use adw::prelude::*;
use anyhow::Result;
use gtk::glib;
use sharkord_linux_client::config::{StoredConfig, load_config, save_config};
use sharkord_linux_client::native_client::{
    NativeBootstrap, NativeClient, NativeEventEnvelope, NativeMessage, NativeMessagesResponse,
    NativeSearchResults, text_channels,
};
use std::cell::RefCell;
use std::rc::Rc;
use std::sync::mpsc;
use std::time::Duration;

const APP_ID: &str = "org.zooi.SharkordLinuxClient";

#[derive(Clone)]
struct SessionState {
    generation: u64,
    client: NativeClient,
    token: String,
    bootstrap: NativeBootstrap,
    selected_channel_id: u64,
}

#[derive(Default)]
struct AppState {
    active_generation: u64,
    editing_message_id: Option<u64>,
    current_thread_parent_message_id: Option<u64>,
    reconnect_in_flight: bool,
    session: Option<SessionState>,
}

enum UiMessage {
    Connected {
        generation: u64,
        client: NativeClient,
        token: String,
        bootstrap: NativeBootstrap,
        initial_channel_id: u64,
        messages: NativeMessagesResponse,
    },
    MessagesLoaded {
        generation: u64,
        channel_id: u64,
        messages: NativeMessagesResponse,
    },
    SearchLoaded {
        generation: u64,
        query: String,
        results: NativeSearchResults,
    },
    ThreadLoaded {
        generation: u64,
        parent_message: NativeMessage,
        messages: NativeMessagesResponse,
    },
    Event {
        generation: u64,
        event: NativeEventEnvelope,
    },
    StreamStopped {
        generation: u64,
        message: String,
    },
    Error {
        generation: u64,
        message: String,
    },
}

type NavigationUi = (
    gtk::Label,
    gtk::Label,
    gtk::ListBox,
    gtk::Stack,
    gtk::Entry,
    gtk::Button,
    gtk::Button,
);

type ThreadUi = (
    gtk::Label,
    gtk::ListBox,
    gtk::Stack,
    gtk::Entry,
    gtk::Button,
    gtk::Button,
);

fn strip_html(input: &str) -> String {
    let mut output = String::with_capacity(input.len());
    let mut inside_tag = false;

    for character in input.chars() {
        match character {
            '<' => inside_tag = true,
            '>' => inside_tag = false,
            _ if !inside_tag => output.push(character),
            _ => {}
        }
    }

    output
        .replace("&nbsp;", " ")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&amp;", "&")
        .trim()
        .to_string()
}

fn clear_list_box(list_box: &gtk::ListBox) {
    while let Some(child) = list_box.first_child() {
        list_box.remove(&child);
    }
}

fn reset_edit_mode(
    app_state: &Rc<RefCell<AppState>>,
    compose_entry: &gtk::Entry,
    send_button: &gtk::Button,
    cancel_edit_button: &gtk::Button,
) {
    app_state.borrow_mut().editing_message_id = None;
    compose_entry.set_text("");
    send_button.set_label("Send");
    cancel_edit_button.set_sensitive(false);
}

fn reset_thread_context(app_state: &Rc<RefCell<AppState>>) {
    app_state.borrow_mut().current_thread_parent_message_id = None;
}

fn begin_edit_mode(
    app_state: &Rc<RefCell<AppState>>,
    compose_entry: &gtk::Entry,
    send_button: &gtk::Button,
    cancel_edit_button: &gtk::Button,
    message_id: u64,
    content: &str,
) {
    app_state.borrow_mut().editing_message_id = Some(message_id);
    compose_entry.set_text(content);
    send_button.set_label("Save Edit");
    cancel_edit_button.set_sensitive(true);
    compose_entry.grab_focus();
    compose_entry.set_position(-1);
}

fn populate_channel_list(
    channel_list: &gtk::ListBox,
    bootstrap: &NativeBootstrap,
    selected_channel_id: u64,
) {
    while let Some(child) = channel_list.first_child() {
        channel_list.remove(&child);
    }

    for channel in text_channels(bootstrap) {
        let row = gtk::ListBoxRow::new();
        let label = gtk::Label::builder()
            .xalign(0.0)
            .wrap(true)
            .margin_top(8)
            .margin_bottom(8)
            .margin_start(12)
            .margin_end(12)
            .label(
                channel
                    .name
                    .unwrap_or_else(|| format!("channel-{}", channel.id)),
            )
            .build();

        row.set_child(Some(&label));
        channel_list.append(&row);

        if channel.id == selected_channel_id {
            channel_list.select_row(Some(&row));
        }
    }
}

fn select_channel_row(channel_list: &gtk::ListBox, bootstrap: &NativeBootstrap, channel_id: u64) {
    let channels = text_channels(bootstrap);

    if let Some(index) = channels.iter().position(|channel| channel.id == channel_id) {
        if let Some(row) = channel_list.row_at_index(index as i32) {
            channel_list.select_row(Some(&row));
        }
    }
}

fn append_header_row(list_box: &gtk::ListBox, text: &str) {
    let row = gtk::ListBoxRow::new();
    row.set_activatable(false);
    row.set_selectable(false);

    let label = gtk::Label::builder()
        .xalign(0.0)
        .wrap(true)
        .margin_top(8)
        .margin_bottom(4)
        .margin_start(12)
        .margin_end(12)
        .css_classes(["heading"])
        .label(text)
        .build();

    row.set_child(Some(&label));
    list_box.append(&row);
}

fn populate_search_results(
    list_box: &gtk::ListBox,
    query: &str,
    results: &NativeSearchResults,
    app_state: &Rc<RefCell<AppState>>,
    tx: &mpsc::Sender<UiMessage>,
    navigation_ui: &NavigationUi,
    thread_ui: &ThreadUi,
) {
    clear_list_box(list_box);

    if results.messages.is_empty() && results.files.is_empty() {
        append_header_row(list_box, &format!("No search results for \"{query}\"."));
        return;
    }

    if !results.messages.is_empty() {
        append_header_row(list_box, &format!("Messages matching \"{query}\""));

        for message in &results.messages {
            let row = gtk::ListBoxRow::new();
            row.set_activatable(false);
            row.set_selectable(false);

            let container = gtk::Box::builder()
                .orientation(gtk::Orientation::Vertical)
                .spacing(6)
                .margin_top(8)
                .margin_bottom(8)
                .margin_start(12)
                .margin_end(12)
                .build();

            let meta = gtk::Label::builder()
                .xalign(0.0)
                .wrap(true)
                .css_classes(["dim-label"])
                .label(format!("[{}] {}", message.created_at, message.channel_name))
                .build();

            let content = gtk::Label::builder()
                .xalign(0.0)
                .wrap(true)
                .selectable(true)
                .label(if message.plain_content.is_empty() {
                    "(empty or non-text message)"
                } else {
                    &message.plain_content
                })
                .build();

            let actions = gtk::Box::builder()
                .orientation(gtk::Orientation::Horizontal)
                .spacing(8)
                .build();

            let open_button = gtk::Button::builder().label("Open").build();
            let open_thread_button = gtk::Button::builder().label("Thread").build();

            {
                let app_state = Rc::clone(app_state);
                let tx = tx.clone();
                let message_id = message.id;
                let channel_id = message.channel_id;
                let channel_name = message.channel_name.clone();
                let (
                    status_label,
                    channel_label,
                    channel_list,
                    content_stack,
                    compose_entry,
                    send_button,
                    cancel_edit_button,
                ) = navigation_ui.clone();

                open_button.connect_clicked(move |_| {
                    let (client, token, generation, bootstrap) = {
                        let mut state = app_state.borrow_mut();
                        state.editing_message_id = None;
                        state.current_thread_parent_message_id = None;

                        let Some(session) = state.session.as_mut() else {
                            status_label.set_text("Not connected.");
                            return;
                        };

                        session.selected_channel_id = channel_id;

                        (
                            session.client.clone(),
                            session.token.clone(),
                            session.generation,
                            session.bootstrap.clone(),
                        )
                    };

                    compose_entry.set_text("");
                    send_button.set_label("Send");
                    cancel_edit_button.set_sensitive(false);
                    channel_label.set_text(&format!("Current channel: {}", channel_name));
                    select_channel_row(&channel_list, &bootstrap, channel_id);
                    content_stack.set_visible_child_name("timeline");
                    status_label.set_text(&format!("Opening message {}...", message_id));
                    spawn_fetch_messages_target(
                        tx.clone(),
                        generation,
                        client,
                        token,
                        channel_id,
                        Some(message_id),
                    );
                });
            }

            {
                let app_state = Rc::clone(app_state);
                let tx = tx.clone();
                let channel_id = message.channel_id;
                let thread_parent_message_id = message.parent_message_id.unwrap_or(message.id);
                let channel_name = message.channel_name.clone();
                let (
                    status_label,
                    channel_label,
                    channel_list,
                    _content_stack,
                    compose_entry,
                    send_button,
                    cancel_edit_button,
                ) = navigation_ui.clone();
                let (thread_header_label, _thread_list, thread_stack, _, _, _) = thread_ui.clone();

                open_thread_button.connect_clicked(move |_| {
                    let (client, token, generation, bootstrap) = {
                        let mut state = app_state.borrow_mut();
                        state.editing_message_id = None;
                        state.current_thread_parent_message_id = Some(thread_parent_message_id);

                        let Some(session) = state.session.as_mut() else {
                            status_label.set_text("Not connected.");
                            return;
                        };

                        session.selected_channel_id = channel_id;

                        (
                            session.client.clone(),
                            session.token.clone(),
                            session.generation,
                            session.bootstrap.clone(),
                        )
                    };

                    compose_entry.set_text("");
                    send_button.set_label("Send");
                    cancel_edit_button.set_sensitive(false);
                    channel_label.set_text(&format!("Current channel: {}", channel_name));
                    select_channel_row(&channel_list, &bootstrap, channel_id);
                    thread_header_label
                        .set_text(&format!("Thread for message {}", thread_parent_message_id));
                    thread_stack.set_visible_child_name("thread");
                    status_label
                        .set_text(&format!("Opening thread {}...", thread_parent_message_id));
                    spawn_fetch_thread(
                        tx.clone(),
                        generation,
                        client,
                        token,
                        thread_parent_message_id,
                    );
                });
            }

            container.append(&meta);
            container.append(&content);
            actions.append(&open_button);
            actions.append(&open_thread_button);
            container.append(&actions);
            row.set_child(Some(&container));
            list_box.append(&row);
        }
    }

    if !results.files.is_empty() {
        append_header_row(list_box, &format!("Files matching \"{query}\""));

        for file in &results.files {
            let row = gtk::ListBoxRow::new();
            row.set_activatable(false);
            row.set_selectable(false);

            let container = gtk::Box::builder()
                .orientation(gtk::Orientation::Vertical)
                .spacing(6)
                .margin_top(8)
                .margin_bottom(8)
                .margin_start(12)
                .margin_end(12)
                .build();

            let meta = gtk::Label::builder()
                .xalign(0.0)
                .wrap(true)
                .css_classes(["dim-label"])
                .label(format!(
                    "[{}] {}",
                    file.message_created_at, file.channel_name
                ))
                .build();

            let content = gtk::Label::builder()
                .xalign(0.0)
                .wrap(true)
                .selectable(true)
                .label(format!("Attached file on message {}", file.message_id))
                .build();

            let actions = gtk::Box::builder()
                .orientation(gtk::Orientation::Horizontal)
                .spacing(8)
                .build();

            let open_button = gtk::Button::builder().label("Open").build();

            {
                let app_state = Rc::clone(app_state);
                let tx = tx.clone();
                let channel_id = file.channel_id;
                let message_id = file.message_id;
                let channel_name = file.channel_name.clone();
                let (
                    status_label,
                    channel_label,
                    channel_list,
                    content_stack,
                    compose_entry,
                    send_button,
                    cancel_edit_button,
                ) = navigation_ui.clone();

                open_button.connect_clicked(move |_| {
                    let (client, token, generation, bootstrap) = {
                        let mut state = app_state.borrow_mut();
                        state.editing_message_id = None;
                        state.current_thread_parent_message_id = None;

                        let Some(session) = state.session.as_mut() else {
                            status_label.set_text("Not connected.");
                            return;
                        };

                        session.selected_channel_id = channel_id;

                        (
                            session.client.clone(),
                            session.token.clone(),
                            session.generation,
                            session.bootstrap.clone(),
                        )
                    };

                    compose_entry.set_text("");
                    send_button.set_label("Send");
                    cancel_edit_button.set_sensitive(false);
                    channel_label.set_text(&format!("Current channel: {}", channel_name));
                    select_channel_row(&channel_list, &bootstrap, channel_id);
                    content_stack.set_visible_child_name("timeline");
                    status_label.set_text(&format!("Opening message {}...", message_id));
                    spawn_fetch_messages_target(
                        tx.clone(),
                        generation,
                        client,
                        token,
                        channel_id,
                        Some(message_id),
                    );
                });
            }

            container.append(&meta);
            container.append(&content);
            actions.append(&open_button);
            container.append(&actions);
            row.set_child(Some(&container));
            list_box.append(&row);
        }
    }
}

fn populate_message_list(
    list_box: &gtk::ListBox,
    messages: &NativeMessagesResponse,
    app_state: &Rc<RefCell<AppState>>,
    tx: &mpsc::Sender<UiMessage>,
    status_label: &gtk::Label,
    compose_entry: &gtk::Entry,
    send_button: &gtk::Button,
    cancel_edit_button: &gtk::Button,
    thread_ui: &ThreadUi,
) {
    clear_list_box(list_box);

    if messages.messages.is_empty() {
        append_header_row(list_box, "No recent root messages in this channel yet.");
        return;
    }

    for message in messages.messages.iter().rev() {
        let row = gtk::ListBoxRow::new();
        row.set_activatable(false);
        row.set_selectable(false);

        let container = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(8)
            .margin_top(8)
            .margin_bottom(8)
            .margin_start(12)
            .margin_end(12)
            .build();

        let plain_content = strip_html(&message.content);
        let display_content = if plain_content.is_empty() {
            "(empty or non-text message)".to_string()
        } else {
            plain_content.clone()
        };

        let meta = gtk::Label::builder()
            .xalign(0.0)
            .wrap(true)
            .css_classes(["dim-label"])
            .label(format!("[{}] user {}", message.created_at, message.user_id))
            .build();

        let content = gtk::Label::builder()
            .xalign(0.0)
            .wrap(true)
            .selectable(true)
            .label(&display_content)
            .build();

        let mut context_parts = Vec::new();

        if let Some(reply_to_message_id) = message.reply_to_message_id {
            context_parts.push(format!("reply to #{}", reply_to_message_id));
        }

        if message.parent_message_id.is_some() {
            context_parts.push("in thread".to_string());
        }

        if let Some(reply_count) = message.reply_count.filter(|count| *count > 0) {
            context_parts.push(format!(
                "{} repl{}",
                reply_count,
                if reply_count == 1 { "y" } else { "ies" }
            ));
        }

        let actions = gtk::Box::builder()
            .orientation(gtk::Orientation::Horizontal)
            .spacing(8)
            .build();

        let edit_button = gtk::Button::builder().label("Edit").build();
        let delete_button = gtk::Button::builder()
            .label("Delete")
            .css_classes(["destructive-action"])
            .build();
        let view_thread_button = gtk::Button::builder().label("Thread").build();

        {
            let app_state = Rc::clone(app_state);
            let compose_entry = compose_entry.clone();
            let send_button = send_button.clone();
            let cancel_edit_button = cancel_edit_button.clone();
            let status_label = status_label.clone();
            let message_id = message.id;
            let content_to_edit = plain_content;

            edit_button.connect_clicked(move |_| {
                begin_edit_mode(
                    &app_state,
                    &compose_entry,
                    &send_button,
                    &cancel_edit_button,
                    message_id,
                    &content_to_edit,
                );

                status_label.set_text(&format!("Editing message {}", message_id));
            });
        }

        {
            let app_state = Rc::clone(app_state);
            let tx = tx.clone();
            let status_label = status_label.clone();
            let message_id = message.id;

            delete_button.connect_clicked(move |_| {
                let (client, token, generation, channel_id) = {
                    let state = app_state.borrow();
                    let Some(session) = state.session.as_ref() else {
                        status_label.set_text("Not connected.");
                        return;
                    };

                    (
                        session.client.clone(),
                        session.token.clone(),
                        session.generation,
                        session.selected_channel_id,
                    )
                };

                status_label.set_text(&format!("Deleting message {}...", message_id));
                spawn_delete_message(
                    tx.clone(),
                    generation,
                    client,
                    token,
                    channel_id,
                    message_id,
                    None,
                );
            });
        }

        {
            let app_state = Rc::clone(app_state);
            let tx = tx.clone();
            let message_id = message.id;
            let (
                thread_header_label,
                _thread_list,
                content_stack,
                compose_entry,
                send_button,
                cancel_edit_button,
            ) = thread_ui.clone();
            let status_label = status_label.clone();

            view_thread_button.connect_clicked(move |_| {
                let (client, token, generation) = {
                    let mut state = app_state.borrow_mut();
                    let Some((client, token, generation)) = state.session.as_ref().map(|session| {
                        (
                            session.client.clone(),
                            session.token.clone(),
                            session.generation,
                        )
                    }) else {
                        status_label.set_text("Not connected.");
                        return;
                    };

                    state.editing_message_id = None;
                    state.current_thread_parent_message_id = Some(message_id);

                    (client, token, generation)
                };

                compose_entry.set_text("");
                send_button.set_label("Send");
                cancel_edit_button.set_sensitive(false);
                thread_header_label.set_text(&format!("Thread for message {}", message_id));
                content_stack.set_visible_child_name("thread");
                status_label.set_text(&format!("Loading thread {}...", message_id));
                spawn_fetch_thread(tx.clone(), generation, client, token, message_id);
            });
        }

        actions.append(&edit_button);
        actions.append(&delete_button);
        actions.append(&view_thread_button);

        container.append(&meta);
        if !context_parts.is_empty() {
            let context = gtk::Label::builder()
                .xalign(0.0)
                .wrap(true)
                .css_classes(["caption", "dim-label"])
                .label(context_parts.join(" · "))
                .build();

            container.append(&context);
        }
        container.append(&content);
        container.append(&actions);
        row.set_child(Some(&container));
        list_box.append(&row);
    }
}

fn populate_thread_list(
    list_box: &gtk::ListBox,
    parent_message: &NativeMessage,
    messages: &NativeMessagesResponse,
    app_state: &Rc<RefCell<AppState>>,
    tx: &mpsc::Sender<UiMessage>,
    status_label: &gtk::Label,
    compose_entry: &gtk::Entry,
    send_button: &gtk::Button,
    cancel_edit_button: &gtk::Button,
) {
    clear_list_box(list_box);

    append_header_row(list_box, "Thread starter");

    let row = gtk::ListBoxRow::new();
    row.set_activatable(false);
    row.set_selectable(false);

    let container = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(8)
        .margin_top(8)
        .margin_bottom(8)
        .margin_start(12)
        .margin_end(12)
        .build();

    let plain_content = strip_html(&parent_message.content);
    let display_content = if plain_content.is_empty() {
        "(empty or non-text message)".to_string()
    } else {
        plain_content.clone()
    };

    let meta = gtk::Label::builder()
        .xalign(0.0)
        .wrap(true)
        .css_classes(["dim-label"])
        .label(format!(
            "[{}] user {}",
            parent_message.created_at, parent_message.user_id
        ))
        .build();

    let content = gtk::Label::builder()
        .xalign(0.0)
        .wrap(true)
        .selectable(true)
        .label(&display_content)
        .build();

    let actions = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(8)
        .build();

    let edit_button = gtk::Button::builder().label("Edit").build();
    let delete_button = gtk::Button::builder()
        .label("Delete")
        .css_classes(["destructive-action"])
        .build();

    {
        let app_state = Rc::clone(app_state);
        let compose_entry = compose_entry.clone();
        let send_button = send_button.clone();
        let cancel_edit_button = cancel_edit_button.clone();
        let status_label = status_label.clone();
        let message_id = parent_message.id;
        let content_to_edit = plain_content;

        edit_button.connect_clicked(move |_| {
            begin_edit_mode(
                &app_state,
                &compose_entry,
                &send_button,
                &cancel_edit_button,
                message_id,
                &content_to_edit,
            );

            status_label.set_text(&format!("Editing message {}", message_id));
        });
    }

    {
        let app_state = Rc::clone(app_state);
        let tx = tx.clone();
        let status_label = status_label.clone();
        let message_id = parent_message.id;

        delete_button.connect_clicked(move |_| {
            let (client, token, generation, channel_id) = {
                let state = app_state.borrow();
                let Some(session) = state.session.as_ref() else {
                    status_label.set_text("Not connected.");
                    return;
                };

                (
                    session.client.clone(),
                    session.token.clone(),
                    session.generation,
                    session.selected_channel_id,
                )
            };

            status_label.set_text(&format!("Deleting message {}...", message_id));
            spawn_delete_message(
                tx.clone(),
                generation,
                client,
                token,
                channel_id,
                message_id,
                None,
            );
        });
    }

    actions.append(&edit_button);
    actions.append(&delete_button);
    container.append(&meta);
    container.append(&content);
    container.append(&actions);
    row.set_child(Some(&container));
    list_box.append(&row);

    append_header_row(list_box, "Replies");

    if messages.messages.is_empty() {
        append_header_row(list_box, "No replies yet.");
        return;
    }

    for message in &messages.messages {
        let row = gtk::ListBoxRow::new();
        row.set_activatable(false);
        row.set_selectable(false);

        let container = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(8)
            .margin_top(8)
            .margin_bottom(8)
            .margin_start(12)
            .margin_end(12)
            .build();

        let plain_content = strip_html(&message.content);
        let display_content = if plain_content.is_empty() {
            "(empty or non-text message)".to_string()
        } else {
            plain_content.clone()
        };

        let meta = gtk::Label::builder()
            .xalign(0.0)
            .wrap(true)
            .css_classes(["dim-label"])
            .label(format!("[{}] user {}", message.created_at, message.user_id))
            .build();

        let content = gtk::Label::builder()
            .xalign(0.0)
            .wrap(true)
            .selectable(true)
            .label(&display_content)
            .build();

        let actions = gtk::Box::builder()
            .orientation(gtk::Orientation::Horizontal)
            .spacing(8)
            .build();

        let edit_button = gtk::Button::builder().label("Edit").build();
        let delete_button = gtk::Button::builder()
            .label("Delete")
            .css_classes(["destructive-action"])
            .build();

        {
            let app_state = Rc::clone(app_state);
            let compose_entry = compose_entry.clone();
            let send_button = send_button.clone();
            let cancel_edit_button = cancel_edit_button.clone();
            let status_label = status_label.clone();
            let message_id = message.id;
            let content_to_edit = plain_content;

            edit_button.connect_clicked(move |_| {
                begin_edit_mode(
                    &app_state,
                    &compose_entry,
                    &send_button,
                    &cancel_edit_button,
                    message_id,
                    &content_to_edit,
                );

                status_label.set_text(&format!("Editing message {}", message_id));
            });
        }

        {
            let app_state = Rc::clone(app_state);
            let tx = tx.clone();
            let status_label = status_label.clone();
            let message_id = message.id;
            let parent_message_id = parent_message.id;

            delete_button.connect_clicked(move |_| {
                let (client, token, generation, channel_id) = {
                    let state = app_state.borrow();
                    let Some(session) = state.session.as_ref() else {
                        status_label.set_text("Not connected.");
                        return;
                    };

                    (
                        session.client.clone(),
                        session.token.clone(),
                        session.generation,
                        session.selected_channel_id,
                    )
                };

                status_label.set_text(&format!("Deleting message {}...", message_id));
                spawn_delete_message(
                    tx.clone(),
                    generation,
                    client,
                    token,
                    channel_id,
                    message_id,
                    Some(parent_message_id),
                );
            });
        }

        actions.append(&edit_button);
        actions.append(&delete_button);
        container.append(&meta);
        container.append(&content);
        container.append(&actions);
        row.set_child(Some(&container));
        list_box.append(&row);
    }
}

fn active_text_channel_name(bootstrap: &NativeBootstrap, channel_id: u64) -> String {
    bootstrap
        .channels
        .iter()
        .find(|channel| channel.id == channel_id)
        .and_then(|channel| channel.name.clone())
        .unwrap_or_else(|| format!("Channel {channel_id}"))
}

fn spawn_connect(
    tx: mpsc::Sender<UiMessage>,
    generation: u64,
    server: String,
    username: String,
    password: String,
    server_password: Option<String>,
) {
    std::thread::spawn(move || {
        let result: Result<_> = (|| {
            let runtime = tokio::runtime::Runtime::new()?;

            runtime.block_on(async move {
                let client = NativeClient::new(&server)?;
                let token = client.login(&username, &password).await?;
                let bootstrap = client.bootstrap(&token, server_password.as_deref()).await?;
                let initial_channel_id =
                    sharkord_linux_client::native_client::first_text_channel_id(&bootstrap)
                        .ok_or_else(|| anyhow::anyhow!("no text channel available"))?;
                let messages = client.fetch_messages(&token, initial_channel_id).await?;

                Ok((client, token, bootstrap, initial_channel_id, messages))
            })
        })();

        match result {
            Ok((client, token, bootstrap, initial_channel_id, messages)) => {
                let _ = tx.send(UiMessage::Connected {
                    generation,
                    client: client.clone(),
                    token: token.clone(),
                    bootstrap: bootstrap.clone(),
                    initial_channel_id,
                    messages,
                });

                spawn_event_stream(tx.clone(), generation, client, token);
            }
            Err(error) => {
                let _ = tx.send(UiMessage::Error {
                    generation,
                    message: error.to_string(),
                });
            }
        }
    });
}

fn spawn_restore_session(
    tx: mpsc::Sender<UiMessage>,
    generation: u64,
    client: NativeClient,
    token: String,
    preferred_channel_id: u64,
) {
    std::thread::spawn(move || {
        let result: Result<_> = (|| {
            let runtime = tokio::runtime::Runtime::new()?;

            runtime.block_on(async move {
                let bootstrap = client.bootstrap(&token, None).await?;

                let restored_channel_id = if text_channels(&bootstrap)
                    .iter()
                    .any(|channel| channel.id == preferred_channel_id)
                {
                    preferred_channel_id
                } else {
                    sharkord_linux_client::native_client::first_text_channel_id(&bootstrap)
                        .ok_or_else(|| anyhow::anyhow!("no text channel available"))?
                };

                let messages = client.fetch_messages(&token, restored_channel_id).await?;

                Ok((client, token, bootstrap, restored_channel_id, messages))
            })
        })();

        match result {
            Ok((client, token, bootstrap, initial_channel_id, messages)) => {
                let _ = tx.send(UiMessage::Connected {
                    generation,
                    client: client.clone(),
                    token: token.clone(),
                    bootstrap,
                    initial_channel_id,
                    messages,
                });

                spawn_event_stream(tx.clone(), generation, client, token);
            }
            Err(error) => {
                let _ = tx.send(UiMessage::Error {
                    generation,
                    message: format!("Reconnect failed: {error}"),
                });
            }
        }
    });
}

fn spawn_event_stream(
    tx: mpsc::Sender<UiMessage>,
    generation: u64,
    client: NativeClient,
    token: String,
) {
    let stream_tx = tx.clone();

    std::thread::spawn(move || {
        let result: Result<()> = (|| {
            let runtime = tokio::runtime::Runtime::new()?;

            runtime.block_on(async move {
                client
                    .stream_events(&token, |event| {
                        stream_tx
                            .send(UiMessage::Event { generation, event })
                            .map_err(|error| anyhow::anyhow!(error.to_string()))
                    })
                    .await
            })
        })();

        if let Err(error) = result {
            let _ = tx.send(UiMessage::StreamStopped {
                generation,
                message: format!("event stream stopped: {error}"),
            });
        }
    });
}

fn spawn_fetch_messages(
    tx: mpsc::Sender<UiMessage>,
    generation: u64,
    client: NativeClient,
    token: String,
    channel_id: u64,
) {
    spawn_fetch_messages_target(tx, generation, client, token, channel_id, None);
}

fn spawn_fetch_messages_target(
    tx: mpsc::Sender<UiMessage>,
    generation: u64,
    client: NativeClient,
    token: String,
    channel_id: u64,
    target_message_id: Option<u64>,
) {
    std::thread::spawn(move || {
        let result: Result<_> = (|| {
            let runtime = tokio::runtime::Runtime::new()?;

            runtime.block_on(async move {
                client
                    .fetch_messages_with_target(&token, channel_id, target_message_id)
                    .await
            })
        })();

        match result {
            Ok(messages) => {
                let _ = tx.send(UiMessage::MessagesLoaded {
                    generation,
                    channel_id,
                    messages,
                });
            }
            Err(error) => {
                let _ = tx.send(UiMessage::Error {
                    generation,
                    message: error.to_string(),
                });
            }
        }
    });
}

fn spawn_send_message(
    tx: mpsc::Sender<UiMessage>,
    generation: u64,
    client: NativeClient,
    token: String,
    channel_id: u64,
    content: String,
    thread_parent_message_id: Option<u64>,
) {
    std::thread::spawn(move || {
        let result: Result<_> = (|| {
            let runtime = tokio::runtime::Runtime::new()?;

            runtime.block_on(async move {
                client
                    .send_message(&token, channel_id, &content, thread_parent_message_id, None)
                    .await?;

                if let Some(parent_message_id) = thread_parent_message_id {
                    let parent_message = client.get_message(&token, parent_message_id).await?;
                    let messages = client
                        .fetch_thread_messages(&token, parent_message_id)
                        .await?;

                    Ok((Some(parent_message), messages))
                } else {
                    let messages = client.fetch_messages(&token, channel_id).await?;
                    Ok((None, messages))
                }
            })
        })();

        match result {
            Ok((parent_message, messages)) => {
                if let Some(parent_message) = parent_message {
                    let _ = tx.send(UiMessage::ThreadLoaded {
                        generation,
                        parent_message,
                        messages,
                    });
                } else {
                    let _ = tx.send(UiMessage::MessagesLoaded {
                        generation,
                        channel_id,
                        messages,
                    });
                }
            }
            Err(error) => {
                let _ = tx.send(UiMessage::Error {
                    generation,
                    message: error.to_string(),
                });
            }
        }
    });
}

fn spawn_edit_message(
    tx: mpsc::Sender<UiMessage>,
    generation: u64,
    client: NativeClient,
    token: String,
    channel_id: u64,
    message_id: u64,
    content: String,
    thread_parent_message_id: Option<u64>,
) {
    std::thread::spawn(move || {
        let result: Result<_> = (|| {
            let runtime = tokio::runtime::Runtime::new()?;

            runtime.block_on(async move {
                client.edit_message(&token, message_id, &content).await?;

                if let Some(parent_message_id) = thread_parent_message_id {
                    let parent_message = client.get_message(&token, parent_message_id).await?;
                    let messages = client
                        .fetch_thread_messages(&token, parent_message_id)
                        .await?;

                    Ok((Some(parent_message), messages))
                } else {
                    let messages = client.fetch_messages(&token, channel_id).await?;
                    Ok((None, messages))
                }
            })
        })();

        match result {
            Ok((parent_message, messages)) => {
                if let Some(parent_message) = parent_message {
                    let _ = tx.send(UiMessage::ThreadLoaded {
                        generation,
                        parent_message,
                        messages,
                    });
                } else {
                    let _ = tx.send(UiMessage::MessagesLoaded {
                        generation,
                        channel_id,
                        messages,
                    });
                }
            }
            Err(error) => {
                let _ = tx.send(UiMessage::Error {
                    generation,
                    message: error.to_string(),
                });
            }
        }
    });
}

fn spawn_delete_message(
    tx: mpsc::Sender<UiMessage>,
    generation: u64,
    client: NativeClient,
    token: String,
    channel_id: u64,
    message_id: u64,
    thread_parent_message_id: Option<u64>,
) {
    std::thread::spawn(move || {
        let result: Result<_> = (|| {
            let runtime = tokio::runtime::Runtime::new()?;

            runtime.block_on(async move {
                client.delete_message(&token, message_id).await?;

                if let Some(parent_message_id) = thread_parent_message_id {
                    let parent_message = client.get_message(&token, parent_message_id).await?;
                    let messages = client
                        .fetch_thread_messages(&token, parent_message_id)
                        .await?;

                    Ok((Some(parent_message), messages))
                } else {
                    let messages = client.fetch_messages(&token, channel_id).await?;
                    Ok((None, messages))
                }
            })
        })();

        match result {
            Ok((parent_message, messages)) => {
                if let Some(parent_message) = parent_message {
                    let _ = tx.send(UiMessage::ThreadLoaded {
                        generation,
                        parent_message,
                        messages,
                    });
                } else {
                    let _ = tx.send(UiMessage::MessagesLoaded {
                        generation,
                        channel_id,
                        messages,
                    });
                }
            }
            Err(error) => {
                let _ = tx.send(UiMessage::Error {
                    generation,
                    message: error.to_string(),
                });
            }
        }
    });
}

fn spawn_search_messages(
    tx: mpsc::Sender<UiMessage>,
    generation: u64,
    client: NativeClient,
    token: String,
    query: String,
) {
    std::thread::spawn(move || {
        let request_query = query.clone();
        let result: Result<_> = (|| {
            let runtime = tokio::runtime::Runtime::new()?;

            runtime.block_on(async move { client.search_messages(&token, &request_query).await })
        })();

        match result {
            Ok(results) => {
                let _ = tx.send(UiMessage::SearchLoaded {
                    generation,
                    query,
                    results,
                });
            }
            Err(error) => {
                let _ = tx.send(UiMessage::Error {
                    generation,
                    message: error.to_string(),
                });
            }
        }
    });
}

fn spawn_fetch_thread(
    tx: mpsc::Sender<UiMessage>,
    generation: u64,
    client: NativeClient,
    token: String,
    parent_message_id: u64,
) {
    std::thread::spawn(move || {
        let result: Result<_> = (|| {
            let runtime = tokio::runtime::Runtime::new()?;

            runtime.block_on(async move {
                let parent_message = client.get_message(&token, parent_message_id).await?;
                let messages = client
                    .fetch_thread_messages(&token, parent_message_id)
                    .await?;

                Ok((parent_message, messages))
            })
        })();

        match result {
            Ok((parent_message, messages)) => {
                let _ = tx.send(UiMessage::ThreadLoaded {
                    generation,
                    parent_message,
                    messages,
                });
            }
            Err(error) => {
                let _ = tx.send(UiMessage::Error {
                    generation,
                    message: error.to_string(),
                });
            }
        }
    });
}

fn build_ui(app: &adw::Application) {
    let saved_config = load_config().ok().flatten().unwrap_or_default();

    let app_state = Rc::new(RefCell::new(AppState::default()));
    let (tx, rx) = mpsc::channel::<UiMessage>();

    let server_entry = gtk::Entry::builder()
        .hexpand(true)
        .placeholder_text("https://chat.zooi.org")
        .text(saved_config.server)
        .build();

    let username_entry = gtk::Entry::builder()
        .hexpand(true)
        .placeholder_text("Username")
        .text(saved_config.username)
        .build();

    let password_entry = gtk::PasswordEntry::builder()
        .hexpand(true)
        .placeholder_text("Password")
        .show_peek_icon(true)
        .build();

    let server_password_entry = gtk::PasswordEntry::builder()
        .hexpand(true)
        .placeholder_text("Optional server password")
        .show_peek_icon(true)
        .build();

    let connect_button = gtk::Button::builder()
        .label("Connect")
        .css_classes(["suggested-action"])
        .build();

    let status_label = gtk::Label::builder()
        .xalign(0.0)
        .wrap(true)
        .css_classes(["dim-label"])
        .label("Disconnected")
        .build();

    let server_label = gtk::Label::builder()
        .xalign(0.0)
        .css_classes(["title-3"])
        .label("Native Sharkord")
        .build();

    let channel_label = gtk::Label::builder()
        .xalign(0.0)
        .css_classes(["dim-label"])
        .label("No channel selected")
        .build();

    let channel_list = gtk::ListBox::builder()
        .selection_mode(gtk::SelectionMode::Single)
        .css_classes(["boxed-list"])
        .build();

    let message_list = gtk::ListBox::builder()
        .selection_mode(gtk::SelectionMode::None)
        .css_classes(["boxed-list"])
        .build();
    append_header_row(
        &message_list,
        "Connect to a server to load channels and messages.",
    );

    let search_results_list = gtk::ListBox::builder()
        .selection_mode(gtk::SelectionMode::None)
        .css_classes(["boxed-list"])
        .build();
    append_header_row(&search_results_list, "Search results will appear here.");

    let thread_list = gtk::ListBox::builder()
        .selection_mode(gtk::SelectionMode::None)
        .css_classes(["boxed-list"])
        .build();
    append_header_row(&thread_list, "Open a thread to view replies.");

    let thread_header_label = gtk::Label::builder()
        .xalign(0.0)
        .css_classes(["title-4"])
        .label("Thread")
        .build();

    let compose_entry = gtk::Entry::builder()
        .hexpand(true)
        .placeholder_text("Type a plain text message")
        .build();
    compose_entry.set_sensitive(false);

    let search_entry = gtk::Entry::builder()
        .hexpand(true)
        .placeholder_text("Search messages")
        .build();
    search_entry.set_sensitive(false);

    let search_button = gtk::Button::builder().label("Search").build();
    search_button.set_sensitive(false);

    let timeline_button = gtk::Button::builder().label("Timeline").build();
    timeline_button.set_sensitive(false);

    let thread_back_button = gtk::Button::builder().label("Back").build();
    thread_back_button.set_sensitive(false);

    let refresh_button = gtk::Button::builder().label("Refresh").build();
    refresh_button.set_sensitive(false);

    let reconnect_button = gtk::Button::builder().label("Reconnect").build();
    reconnect_button.set_sensitive(false);

    let send_button = gtk::Button::builder().label("Send").build();
    send_button.set_sensitive(false);

    let cancel_edit_button = gtk::Button::builder().label("Cancel Edit").build();
    cancel_edit_button.set_sensitive(false);

    let connection_box = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(12)
        .margin_top(16)
        .margin_bottom(16)
        .margin_start(16)
        .margin_end(16)
        .build();
    connection_box.append(
        &gtk::Label::builder()
            .xalign(0.0)
            .css_classes(["title-4"])
            .label("Connection")
            .build(),
    );
    connection_box.append(&server_entry);
    connection_box.append(&username_entry);
    connection_box.append(&password_entry);
    connection_box.append(&server_password_entry);
    connection_box.append(&connect_button);
    connection_box.append(&reconnect_button);
    connection_box.append(&status_label);

    let channels_frame = gtk::Frame::builder().label("Text Channels").build();
    let channels_scroll = gtk::ScrolledWindow::builder()
        .vexpand(true)
        .child(&channel_list)
        .build();
    channels_frame.set_child(Some(&channels_scroll));

    let left_column = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(16)
        .margin_top(16)
        .margin_bottom(16)
        .margin_start(16)
        .margin_end(16)
        .width_request(320)
        .build();
    left_column.append(&connection_box);
    left_column.append(&channels_frame);

    let messages_scroll = gtk::ScrolledWindow::builder()
        .vexpand(true)
        .hexpand(true)
        .child(&message_list)
        .build();

    let search_results_scroll = gtk::ScrolledWindow::builder()
        .vexpand(true)
        .hexpand(true)
        .child(&search_results_list)
        .build();

    let thread_scroll = gtk::ScrolledWindow::builder()
        .vexpand(true)
        .hexpand(true)
        .child(&thread_list)
        .build();

    let thread_box = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(12)
        .build();
    thread_box.append(&thread_header_label);
    thread_box.append(&thread_scroll);

    let content_stack = gtk::Stack::builder()
        .vexpand(true)
        .hexpand(true)
        .transition_type(gtk::StackTransitionType::Crossfade)
        .build();
    content_stack.add_titled(&messages_scroll, Some("timeline"), "Timeline");
    content_stack.add_titled(&search_results_scroll, Some("search"), "Search");
    content_stack.add_titled(&thread_box, Some("thread"), "Thread");
    content_stack.set_visible_child_name("timeline");

    let search_box = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(12)
        .build();
    search_box.append(&search_entry);
    search_box.append(&search_button);
    search_box.append(&timeline_button);
    search_box.append(&thread_back_button);
    search_box.append(&refresh_button);

    let compose_box = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(12)
        .build();
    compose_box.append(&compose_entry);
    compose_box.append(&send_button);
    compose_box.append(&cancel_edit_button);

    let right_column = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(12)
        .margin_top(16)
        .margin_bottom(16)
        .margin_start(16)
        .margin_end(16)
        .build();
    right_column.append(&server_label);
    right_column.append(&channel_label);
    right_column.append(&search_box);
    right_column.append(&content_stack);
    right_column.append(&compose_box);

    let paned = gtk::Paned::builder()
        .orientation(gtk::Orientation::Horizontal)
        .wide_handle(true)
        .position(360)
        .build();
    paned.set_start_child(Some(&left_column));
    paned.set_end_child(Some(&right_column));

    let header = adw::HeaderBar::new();
    let title = adw::WindowTitle::builder()
        .title("Sharkord Linux Client")
        .subtitle("Native text-sync prototype")
        .build();
    header.set_title_widget(Some(&title));

    let toolbar = adw::ToolbarView::new();
    toolbar.add_top_bar(&header);
    toolbar.set_content(Some(&paned));

    let window = adw::ApplicationWindow::builder()
        .application(app)
        .title("Sharkord Linux Client")
        .default_width(1280)
        .default_height(820)
        .content(&toolbar)
        .build();

    {
        let tx = tx.clone();
        let app_state = Rc::clone(&app_state);
        let server_entry = server_entry.clone();
        let username_entry = username_entry.clone();
        let password_entry = password_entry.clone();
        let server_password_entry = server_password_entry.clone();
        let status_label = status_label.clone();
        let connect_button = connect_button.clone();

        connect_button.clone().connect_clicked(move |_| {
            let server = server_entry.text().trim().to_string();
            let username = username_entry.text().trim().to_string();
            let password = password_entry.text().to_string();
            let server_password = server_password_entry.text().trim().to_string();

            if server.is_empty() || username.is_empty() || password.is_empty() {
                status_label.set_text("Server, username, and password are required.");
                return;
            }

            let generation = {
                let mut state = app_state.borrow_mut();
                state.active_generation += 1;
                state.active_generation
            };

            connect_button.set_sensitive(false);
            status_label.set_text("Connecting...");

            spawn_connect(
                tx.clone(),
                generation,
                server,
                username,
                password,
                if server_password.is_empty() {
                    None
                } else {
                    Some(server_password)
                },
            );
        });
    }

    {
        let tx = tx.clone();
        let app_state = Rc::clone(&app_state);
        let status_label = status_label.clone();
        let reconnect_button = reconnect_button.clone();

        reconnect_button.clone().connect_clicked(move |_| {
            let (client, token, generation, channel_id) = {
                let mut state = app_state.borrow_mut();
                let Some(session) = state.session.as_ref() else {
                    status_label.set_text("Not connected.");
                    return;
                };

                if state.reconnect_in_flight {
                    status_label.set_text("Reconnect already in progress.");
                    return;
                }

                let client = session.client.clone();
                let token = session.token.clone();
                let generation = session.generation;
                let channel_id = session.selected_channel_id;

                state.reconnect_in_flight = true;

                (client, token, generation, channel_id)
            };

            reconnect_button.set_sensitive(false);
            status_label.set_text("Reconnecting...");
            spawn_restore_session(tx.clone(), generation, client, token, channel_id);
        });
    }

    {
        let tx = tx.clone();
        let app_state = Rc::clone(&app_state);
        let channel_label = channel_label.clone();
        let compose_entry = compose_entry.clone();
        let send_button = send_button.clone();
        let cancel_edit_button = cancel_edit_button.clone();
        let content_stack = content_stack.clone();

        channel_list.connect_row_selected(move |_, maybe_row| {
            let Some(row) = maybe_row else {
                return;
            };

            let (client, token, generation, channel_id, channel_name) = {
                let mut state = app_state.borrow_mut();
                let row_index = row.index() as usize;
                let Some((client, token, generation, channel_id, channel_name)) =
                    state.session.as_ref().and_then(|session| {
                        let channels = text_channels(&session.bootstrap);
                        channels.get(row_index).map(|channel| {
                            (
                                session.client.clone(),
                                session.token.clone(),
                                session.generation,
                                channel.id,
                                channel
                                    .name
                                    .clone()
                                    .unwrap_or_else(|| format!("Channel {}", channel.id)),
                            )
                        })
                    })
                else {
                    return;
                };

                state.current_thread_parent_message_id = None;
                if let Some(session) = state.session.as_mut() {
                    session.selected_channel_id = channel_id;
                }

                (client, token, generation, channel_id, channel_name)
            };

            reset_edit_mode(
                &app_state,
                &compose_entry,
                &send_button,
                &cancel_edit_button,
            );
            content_stack.set_visible_child_name("timeline");
            channel_label.set_text(&format!("Current channel: {}", channel_name));
            spawn_fetch_messages(tx.clone(), generation, client, token, channel_id);
        });
    }

    {
        let tx = tx.clone();
        let app_state = Rc::clone(&app_state);
        let compose_entry = compose_entry.clone();
        let send_button = send_button.clone();
        let cancel_edit_button = cancel_edit_button.clone();
        let status_label = status_label.clone();

        send_button.clone().connect_clicked(move |_| {
            let content = compose_entry.text().trim().to_string();

            if content.is_empty() {
                return;
            }

            let (
                client,
                token,
                generation,
                channel_id,
                editing_message_id,
                current_thread_parent_message_id,
            ) = {
                let mut state = app_state.borrow_mut();
                let Some(session) = state.session.as_ref() else {
                    status_label.set_text("Not connected.");
                    return;
                };

                let client = session.client.clone();
                let token = session.token.clone();
                let generation = session.generation;
                let channel_id = session.selected_channel_id;
                let current_thread_parent_message_id = state.current_thread_parent_message_id;
                let editing_message_id = state.editing_message_id.take();

                (
                    client,
                    token,
                    generation,
                    channel_id,
                    editing_message_id,
                    current_thread_parent_message_id,
                )
            };

            compose_entry.set_text("");
            send_button.set_label("Send");
            cancel_edit_button.set_sensitive(false);

            if let Some(message_id) = editing_message_id {
                status_label.set_text(&format!("Saving edit for message {}...", message_id));
                spawn_edit_message(
                    tx.clone(),
                    generation,
                    client,
                    token,
                    channel_id,
                    message_id,
                    content,
                    current_thread_parent_message_id,
                );
            } else {
                if current_thread_parent_message_id.is_some() {
                    status_label.set_text("Sending thread reply...");
                } else {
                    status_label.set_text("Sending message...");
                }
                spawn_send_message(
                    tx.clone(),
                    generation,
                    client,
                    token,
                    channel_id,
                    content,
                    current_thread_parent_message_id,
                );
            }
        });
    }

    {
        let send_button = send_button.clone();
        compose_entry.connect_activate(move |_| {
            send_button.emit_clicked();
        });
    }

    {
        let app_state = Rc::clone(&app_state);
        let compose_entry = compose_entry.clone();
        let send_button = send_button.clone();
        let cancel_edit_button = cancel_edit_button.clone();
        let status_label = status_label.clone();

        cancel_edit_button.clone().connect_clicked(move |_| {
            reset_edit_mode(
                &app_state,
                &compose_entry,
                &send_button,
                &cancel_edit_button,
            );
            status_label.set_text("Edit cancelled");
        });
    }

    {
        let tx = tx.clone();
        let app_state = Rc::clone(&app_state);
        let search_entry = search_entry.clone();
        let status_label = status_label.clone();

        search_button.connect_clicked(move |_| {
            let query = search_entry.text().trim().to_string();

            if query.len() < 2 {
                status_label.set_text("Search query must be at least 2 characters.");
                return;
            }

            let (client, token, generation) = {
                let state = app_state.borrow();
                let Some(session) = state.session.as_ref() else {
                    status_label.set_text("Not connected.");
                    return;
                };

                (
                    session.client.clone(),
                    session.token.clone(),
                    session.generation,
                )
            };

            status_label.set_text("Searching...");
            spawn_search_messages(tx.clone(), generation, client, token, query);
        });
    }

    {
        let search_button = search_button.clone();
        search_entry.connect_activate(move |_| {
            search_button.emit_clicked();
        });
    }

    {
        let app_state = Rc::clone(&app_state);
        let content_stack = content_stack.clone();
        timeline_button.connect_clicked(move |_| {
            reset_thread_context(&app_state);
            content_stack.set_visible_child_name("timeline");
        });
    }

    {
        let app_state = Rc::clone(&app_state);
        let content_stack = content_stack.clone();
        let status_label = status_label.clone();

        thread_back_button.connect_clicked(move |_| {
            reset_thread_context(&app_state);
            content_stack.set_visible_child_name("timeline");
            status_label.set_text("Back to timeline");
        });
    }

    {
        let tx = tx.clone();
        let app_state = Rc::clone(&app_state);
        let status_label = status_label.clone();

        refresh_button.connect_clicked(move |_| {
            let (client, token, generation, channel_id, thread_parent_message_id) = {
                let state = app_state.borrow();
                let Some(session) = state.session.as_ref() else {
                    status_label.set_text("Not connected.");
                    return;
                };

                (
                    session.client.clone(),
                    session.token.clone(),
                    session.generation,
                    session.selected_channel_id,
                    state.current_thread_parent_message_id,
                )
            };

            if let Some(parent_message_id) = thread_parent_message_id {
                status_label.set_text("Refreshing thread...");
                spawn_fetch_thread(tx.clone(), generation, client, token, parent_message_id);
            } else {
                status_label.set_text("Refreshing...");
                spawn_fetch_messages(tx.clone(), generation, client, token, channel_id);
            }
        });
    }

    {
        let app_state = Rc::clone(&app_state);
        let connect_button = connect_button.clone();
        let status_label = status_label.clone();
        let server_label = server_label.clone();
        let channel_label = channel_label.clone();
        let channel_list = channel_list.clone();
        let message_list = message_list.clone();
        let search_results_list = search_results_list.clone();
        let content_stack = content_stack.clone();
        let compose_entry = compose_entry.clone();
        let search_entry = search_entry.clone();
        let search_button = search_button.clone();
        let timeline_button = timeline_button.clone();
        let thread_back_button = thread_back_button.clone();
        let refresh_button = refresh_button.clone();
        let reconnect_button = reconnect_button.clone();
        let send_button = send_button.clone();
        let cancel_edit_button = cancel_edit_button.clone();
        let server_entry = server_entry.clone();
        let username_entry = username_entry.clone();
        let thread_header_label = thread_header_label.clone();
        let thread_list = thread_list.clone();
        let navigation_ui = (
            status_label.clone(),
            channel_label.clone(),
            channel_list.clone(),
            content_stack.clone(),
            compose_entry.clone(),
            send_button.clone(),
            cancel_edit_button.clone(),
        );
        let thread_ui = (
            thread_header_label.clone(),
            thread_list.clone(),
            content_stack.clone(),
            compose_entry.clone(),
            send_button.clone(),
            cancel_edit_button.clone(),
        );

        glib::timeout_add_local(Duration::from_millis(50), move || {
            while let Ok(message) = rx.try_recv() {
                match message {
                    UiMessage::Connected {
                        generation,
                        client,
                        token,
                        bootstrap,
                        initial_channel_id,
                        messages,
                    } => {
                        {
                            let mut state = app_state.borrow_mut();

                            if generation != state.active_generation {
                                continue;
                            }

                            state.editing_message_id = None;
                            state.current_thread_parent_message_id = None;
                            state.reconnect_in_flight = false;
                            state.session = Some(SessionState {
                                generation,
                                client,
                                token,
                                bootstrap: bootstrap.clone(),
                                selected_channel_id: initial_channel_id,
                            });
                        }

                        connect_button.set_sensitive(true);
                        compose_entry.set_sensitive(true);
                        search_entry.set_sensitive(true);
                        search_button.set_sensitive(true);
                        timeline_button.set_sensitive(true);
                        thread_back_button.set_sensitive(true);
                        refresh_button.set_sensitive(true);
                        reconnect_button.set_sensitive(true);
                        send_button.set_sensitive(true);
                        status_label.set_text("Connected");
                        server_label.set_text(&format!(
                            "{} · {} visible users",
                            bootstrap.server_name,
                            bootstrap.users.len()
                        ));
                        channel_label.set_text(&format!(
                            "Current channel: {}",
                            active_text_channel_name(&bootstrap, initial_channel_id)
                        ));
                        send_button.set_label("Send");
                        cancel_edit_button.set_sensitive(false);
                        content_stack.set_visible_child_name("timeline");
                        populate_message_list(
                            &message_list,
                            &messages,
                            &app_state,
                            &tx,
                            &status_label,
                            &compose_entry,
                            &send_button,
                            &cancel_edit_button,
                            &thread_ui,
                        );
                        populate_channel_list(&channel_list, &bootstrap, initial_channel_id);

                        let _ = save_config(&StoredConfig {
                            server: server_entry.text().to_string(),
                            username: username_entry.text().to_string(),
                        });
                    }
                    UiMessage::MessagesLoaded {
                        generation,
                        channel_id,
                        messages,
                    } => {
                        let is_current = {
                            let mut state = app_state.borrow_mut();
                            let Some(session) = state.session.as_ref() else {
                                continue;
                            };

                            let is_current = generation == state.active_generation
                                && generation == session.generation
                                && channel_id == session.selected_channel_id;

                            if is_current {
                                state.editing_message_id = None;
                                state.current_thread_parent_message_id = None;
                            }

                            is_current
                        };

                        if !is_current {
                            continue;
                        }

                        send_button.set_label("Send");
                        cancel_edit_button.set_sensitive(false);
                        compose_entry.set_text("");
                        content_stack.set_visible_child_name("timeline");
                        populate_message_list(
                            &message_list,
                            &messages,
                            &app_state,
                            &tx,
                            &status_label,
                            &compose_entry,
                            &send_button,
                            &cancel_edit_button,
                            &thread_ui,
                        );
                        status_label.set_text("Synced");
                    }
                    UiMessage::SearchLoaded {
                        generation,
                        query,
                        results,
                    } => {
                        let is_current = {
                            let state = app_state.borrow();
                            matches!(
                                state.session.as_ref(),
                                Some(session)
                                    if generation == state.active_generation
                                        && generation == session.generation
                            )
                        };

                        if !is_current {
                            continue;
                        }

                        reset_edit_mode(
                            &app_state,
                            &compose_entry,
                            &send_button,
                            &cancel_edit_button,
                        );
                        reset_thread_context(&app_state);
                        populate_search_results(
                            &search_results_list,
                            &query,
                            &results,
                            &app_state,
                            &tx,
                            &navigation_ui,
                            &thread_ui,
                        );
                        content_stack.set_visible_child_name("search");
                        status_label.set_text("Search complete");
                    }
                    UiMessage::ThreadLoaded {
                        generation,
                        parent_message,
                        messages,
                    } => {
                        let is_current = {
                            let mut state = app_state.borrow_mut();
                            let Some(session) = state.session.as_ref() else {
                                continue;
                            };

                            let is_current = generation == state.active_generation
                                && generation == session.generation;

                            if is_current {
                                state.editing_message_id = None;
                                state.current_thread_parent_message_id = Some(parent_message.id);
                            }

                            is_current
                        };

                        if !is_current {
                            continue;
                        }

                        send_button.set_label("Send");
                        cancel_edit_button.set_sensitive(false);
                        compose_entry.set_text("");
                        thread_header_label
                            .set_text(&format!("Thread for message {}", parent_message.id));
                        populate_thread_list(
                            &thread_list,
                            &parent_message,
                            &messages,
                            &app_state,
                            &tx,
                            &status_label,
                            &compose_entry,
                            &send_button,
                            &cancel_edit_button,
                        );
                        content_stack.set_visible_child_name("thread");
                        status_label.set_text("Thread synced");
                    }
                    UiMessage::Event { generation, event } => {
                        let refresh_targets = {
                            let state = app_state.borrow();
                            match state.session.as_ref() {
                                Some(session)
                                    if generation == state.active_generation
                                        && generation == session.generation
                                        && matches!(
                                            event.event_type.as_str(),
                                            "newMessage" | "messageUpdate" | "messageDelete"
                                        ) =>
                                {
                                    let event_channel_id = event
                                        .payload
                                        .get("channelId")
                                        .and_then(|value| value.as_u64());

                                    if event_channel_id != Some(session.selected_channel_id) {
                                        None
                                    } else {
                                        Some((
                                            session.client.clone(),
                                            session.token.clone(),
                                            session.selected_channel_id,
                                            state.current_thread_parent_message_id,
                                        ))
                                    }
                                }
                                _ => None,
                            }
                        };

                        if let Some((client, token, channel_id, thread_parent_message_id)) =
                            refresh_targets
                        {
                            if let Some(parent_message_id) = thread_parent_message_id {
                                spawn_fetch_thread(
                                    tx.clone(),
                                    generation,
                                    client,
                                    token,
                                    parent_message_id,
                                );
                            } else {
                                spawn_fetch_messages(
                                    tx.clone(),
                                    generation,
                                    client,
                                    token,
                                    channel_id,
                                );
                            }
                        }
                    }
                    UiMessage::StreamStopped {
                        generation,
                        message,
                    } => {
                        let reconnect = {
                            let mut state = app_state.borrow_mut();
                            let Some(session) = state.session.as_ref() else {
                                status_label.set_text(&message);
                                continue;
                            };

                            if generation != state.active_generation
                                || generation != session.generation
                            {
                                continue;
                            }

                            if state.reconnect_in_flight {
                                status_label.set_text(&message);
                                continue;
                            }

                            let client = session.client.clone();
                            let token = session.token.clone();
                            let channel_id = session.selected_channel_id;

                            state.reconnect_in_flight = true;

                            Some((client, token, channel_id))
                        };

                        if let Some((client, token, channel_id)) = reconnect {
                            reconnect_button.set_sensitive(false);
                            status_label.set_text("Connection dropped. Reconnecting...");
                            spawn_restore_session(
                                tx.clone(),
                                generation,
                                client,
                                token,
                                channel_id,
                            );
                        }
                    }
                    UiMessage::Error {
                        generation,
                        message,
                    } => {
                        let is_current = {
                            let state = app_state.borrow();
                            generation == state.active_generation
                        };

                        if is_current {
                            {
                                let mut state = app_state.borrow_mut();
                                state.reconnect_in_flight = false;
                            }

                            connect_button.set_sensitive(true);
                            reconnect_button.set_sensitive(true);
                            status_label.set_text(&message);
                        }
                    }
                }
            }

            glib::ControlFlow::Continue
        });
    }

    window.present();
}

fn main() -> glib::ExitCode {
    adw::init().expect("failed to initialize libadwaita");

    let app = adw::Application::builder().application_id(APP_ID).build();
    app.connect_activate(build_ui);

    app.run()
}
