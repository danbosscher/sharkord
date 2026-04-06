use adw::prelude::*;
use anyhow::Result;
use gtk::glib;
use sharkord_linux_client::config::{SavedSession, load_config, save_config};
use sharkord_linux_client::native_client::{
    NativeBootstrap, NativeClient, NativeEventEnvelope, NativeFile, NativeMessage,
    NativeMessagesResponse, NativeSearchResults, NativeTempFile, text_channels,
};
use std::cell::RefCell;
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::mpsc;
use std::time::{Duration, Instant};

const APP_ID: &str = "org.zooi.SharkordLinuxClient";
const TYPING_SIGNAL_MS: u64 = 300;
const TYPING_EXPIRY_MS: u64 = 800;

#[derive(Clone)]
struct SessionState {
    generation: u64,
    username: String,
    client: NativeClient,
    token: String,
    bootstrap: NativeBootstrap,
    selected_channel_id: u64,
}

#[derive(Clone, Default)]
struct ReplyTarget {
    message_id: u64,
    parent_message_id: Option<u64>,
    preview: String,
}

#[derive(Default)]
struct AppState {
    active_generation: u64,
    editing_message_id: Option<u64>,
    current_thread_parent_message_id: Option<u64>,
    replying_to: Option<ReplyTarget>,
    pending_uploads: Vec<NativeTempFile>,
    unread_channel_counts: BTreeMap<u64, u64>,
    typing_users_by_channel: BTreeMap<u64, Vec<u64>>,
    typing_users_by_thread: BTreeMap<u64, Vec<u64>>,
    typing_timeout_tokens: BTreeMap<String, u64>,
    last_typing_signal_by_context: BTreeMap<String, Instant>,
    upload_in_flight: bool,
    reconnect_in_flight: bool,
    restore_in_flight: bool,
    session: Option<SessionState>,
}

enum UiMessage {
    Connected {
        generation: u64,
        username: String,
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
    TempFileUploaded {
        generation: u64,
        temp_file: NativeTempFile,
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
    gtk::Label,
);

type ThreadUi = (
    gtk::Label,
    gtk::ListBox,
    gtk::Stack,
    gtk::Entry,
    gtk::Button,
    gtk::Button,
    gtk::Label,
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

fn truncate_preview(input: &str) -> String {
    const MAX_LEN: usize = 48;

    if input.chars().count() <= MAX_LEN {
        return input.to_string();
    }

    let truncated = input.chars().take(MAX_LEN).collect::<String>();
    format!("{truncated}…")
}

fn unread_channel_counts_from_bootstrap(bootstrap: &NativeBootstrap) -> BTreeMap<u64, u64> {
    bootstrap
        .read_states
        .iter()
        .filter_map(|(channel_id, count)| (*count > 0).then_some((*channel_id, *count)))
        .collect()
}

fn typing_context_key(channel_id: u64, parent_message_id: Option<u64>, user_id: u64) -> String {
    match parent_message_id {
        Some(parent_message_id) => format!("thread:{channel_id}:{parent_message_id}:{user_id}"),
        None => format!("channel:{channel_id}:{user_id}"),
    }
}

fn typing_signal_key(channel_id: u64, parent_message_id: Option<u64>) -> String {
    match parent_message_id {
        Some(parent_message_id) => format!("thread:{channel_id}:{parent_message_id}"),
        None => format!("channel:{channel_id}"),
    }
}

fn user_display_name(bootstrap: &NativeBootstrap, user_id: u64) -> String {
    bootstrap
        .users
        .iter()
        .find(|user| user.id == user_id)
        .map(|user| user.name.clone())
        .unwrap_or_else(|| format!("user {user_id}"))
}

fn typing_summary_text(bootstrap: &NativeBootstrap, user_ids: &[u64]) -> Option<String> {
    if user_ids.is_empty() {
        return None;
    }

    let names = user_ids
        .iter()
        .map(|user_id| user_display_name(bootstrap, *user_id))
        .collect::<Vec<_>>();

    Some(match names.len() {
        1 => format!("{} is typing...", names[0]),
        2 => format!("{} and {} are typing...", names[0], names[1]),
        _ => format!("{} and {} others are typing...", names[0], names.len() - 1),
    })
}

fn update_timeline_typing_label(label: &gtk::Label, app_state: &Rc<RefCell<AppState>>) {
    let state = app_state.borrow();
    let Some(session) = state.session.as_ref() else {
        label.set_text("");
        label.set_visible(false);
        return;
    };

    let user_ids = state
        .typing_users_by_channel
        .get(&session.selected_channel_id)
        .cloned()
        .unwrap_or_default();

    if let Some(summary) = typing_summary_text(&session.bootstrap, &user_ids) {
        label.set_text(&summary);
        label.set_visible(true);
    } else {
        label.set_text("");
        label.set_visible(false);
    }
}

fn update_thread_typing_label(label: &gtk::Label, app_state: &Rc<RefCell<AppState>>) {
    let state = app_state.borrow();
    let Some(session) = state.session.as_ref() else {
        label.set_text("");
        label.set_visible(false);
        return;
    };

    let Some(parent_message_id) = state.current_thread_parent_message_id else {
        label.set_text("");
        label.set_visible(false);
        return;
    };

    let user_ids = state
        .typing_users_by_thread
        .get(&parent_message_id)
        .cloned()
        .unwrap_or_default();

    if let Some(summary) = typing_summary_text(&session.bootstrap, &user_ids) {
        label.set_text(&summary);
        label.set_visible(true);
    } else {
        label.set_text("");
        label.set_visible(false);
    }
}

fn reply_preview_text(bootstrap: &NativeBootstrap, message: &NativeMessage) -> Option<String> {
    if let Some(reply_to) = message.reply_to.as_ref() {
        let plain_content = strip_html(&reply_to.content);
        let display_content = if plain_content.is_empty() {
            "(empty or non-text message)".to_string()
        } else {
            plain_content
        };

        return Some(format!(
            "Replying to {}: {}",
            user_display_name(bootstrap, reply_to.user_id),
            truncate_preview(&display_content),
        ));
    }

    message
        .reply_to_message_id
        .map(|reply_to_message_id| format!("Replying to message #{reply_to_message_id}"))
}

fn format_file_size(size: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KiB", "MiB", "GiB", "TiB"];

    let mut value = size as f64;
    let mut unit_index = 0;

    while value >= 1024.0 && unit_index < UNITS.len() - 1 {
        value /= 1024.0;
        unit_index += 1;
    }

    if unit_index == 0 {
        format!("{} {}", size, UNITS[unit_index])
    } else {
        format!("{value:.1} {}", UNITS[unit_index])
    }
}

fn native_file_url(base_url: &str, file: &NativeFile) -> String {
    let mut url = format!("{}/public/{}", base_url.trim_end_matches('/'), file.name);

    if let Some(access_token) = file.access_token.as_ref() {
        url.push_str(&format!("?accessToken={access_token}"));

        if let Some(expires_at) = file.access_token_expires_at {
            url.push_str(&format!("&expires={expires_at}"));
        }
    }

    url
}

fn open_native_file(
    app_state: &Rc<RefCell<AppState>>,
    status_label: &gtk::Label,
    file: &NativeFile,
) {
    let base_url = {
        let state = app_state.borrow();
        let Some(session) = state.session.as_ref() else {
            status_label.set_text("Not connected.");
            return;
        };

        session.client.base_url().to_string()
    };

    let file_url = native_file_url(&base_url, file);

    match gtk::gio::AppInfo::launch_default_for_uri(&file_url, None::<&gtk::gio::AppLaunchContext>)
    {
        Ok(_) => status_label.set_text(&format!("Opened {}", file.original_name)),
        Err(error) => status_label.set_text(&format!("Failed to open file: {error}")),
    }
}

fn append_file_attachments(
    container: &gtk::Box,
    files: &[NativeFile],
    app_state: &Rc<RefCell<AppState>>,
    status_label: &gtk::Label,
) {
    if files.is_empty() {
        return;
    }

    let attachments_box = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(6)
        .build();

    for file in files {
        let row = gtk::Box::builder()
            .orientation(gtk::Orientation::Horizontal)
            .spacing(8)
            .build();

        let text_box = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(2)
            .hexpand(true)
            .build();

        let name_label = gtk::Label::builder()
            .xalign(0.0)
            .wrap(true)
            .label(&file.original_name)
            .build();

        let meta_label = gtk::Label::builder()
            .xalign(0.0)
            .wrap(true)
            .css_classes(["caption", "dim-label"])
            .label(format!(
                "{}{}",
                format_file_size(file.size),
                if file.extension.is_empty() {
                    String::new()
                } else {
                    format!(" · .{}", file.extension)
                }
            ))
            .build();

        let open_button = gtk::Button::builder().label("Open File").build();

        {
            let app_state = Rc::clone(app_state);
            let status_label = status_label.clone();
            let file = file.clone();

            open_button.connect_clicked(move |_| {
                open_native_file(&app_state, &status_label, &file);
            });
        }

        text_box.append(&name_label);
        text_box.append(&meta_label);
        row.append(&text_box);
        row.append(&open_button);
        attachments_box.append(&row);
    }

    container.append(&attachments_box);
}

fn render_pending_uploads(
    attachments_box: &gtk::Box,
    app_state: &Rc<RefCell<AppState>>,
    tx: &mpsc::Sender<UiMessage>,
    status_label: &gtk::Label,
) {
    while let Some(child) = attachments_box.first_child() {
        attachments_box.remove(&child);
    }

    let pending_uploads = app_state.borrow().pending_uploads.clone();

    if pending_uploads.is_empty() {
        attachments_box.set_visible(false);
        return;
    }

    attachments_box.set_visible(true);

    for temp_file in pending_uploads {
        let row = gtk::Box::builder()
            .orientation(gtk::Orientation::Horizontal)
            .spacing(8)
            .build();

        let text_box = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(2)
            .hexpand(true)
            .build();

        let name_label = gtk::Label::builder()
            .xalign(0.0)
            .wrap(true)
            .label(&temp_file.original_name)
            .build();

        let meta_label = gtk::Label::builder()
            .xalign(0.0)
            .wrap(true)
            .css_classes(["caption", "dim-label"])
            .label(format!(
                "{}{}",
                format_file_size(temp_file.size),
                if temp_file.extension.is_empty() {
                    String::new()
                } else {
                    format!(" · {}", temp_file.extension)
                }
            ))
            .build();

        let remove_button = gtk::Button::builder().label("Remove").build();

        {
            let app_state = Rc::clone(app_state);
            let tx = tx.clone();
            let status_label = status_label.clone();
            let attachments_box = attachments_box.clone();
            let file_id = temp_file.id.clone();

            remove_button.connect_clicked(move |_| {
                let deletion = {
                    let mut state = app_state.borrow_mut();
                    let index = state
                        .pending_uploads
                        .iter()
                        .position(|file| file.id == file_id);

                    let Some(index) = index else {
                        status_label.set_text("Attachment was already removed.");
                        return;
                    };

                    let removed = state.pending_uploads.remove(index);
                    let session = state.session.as_ref().map(|session| {
                        (
                            session.client.clone(),
                            session.token.clone(),
                            session.generation,
                        )
                    });

                    (removed, session)
                };

                render_pending_uploads(&attachments_box, &app_state, &tx, &status_label);
                status_label.set_text("Removed staged attachment.");

                if let Some((client, token, generation)) = deletion.1 {
                    spawn_delete_temporary_file(
                        tx.clone(),
                        generation,
                        client,
                        token,
                        deletion.0.id,
                    );
                }
            });
        }

        text_box.append(&name_label);
        text_box.append(&meta_label);
        row.append(&text_box);
        row.append(&remove_button);
        attachments_box.append(&row);
    }
}

fn set_compose_mode_ui(
    app_state: &Rc<RefCell<AppState>>,
    send_button: &gtk::Button,
    cancel_button: &gtk::Button,
    compose_context_label: &gtk::Label,
) {
    let state = app_state.borrow();

    if let Some(message_id) = state.editing_message_id {
        send_button.set_label("Save Edit");
        cancel_button.set_label("Cancel Edit");
        cancel_button.set_sensitive(true);
        compose_context_label.set_text(&format!("Editing message {}", message_id));
        compose_context_label.set_visible(true);
        return;
    }

    if let Some(reply_target) = state.replying_to.as_ref() {
        send_button.set_label("Send Reply");
        cancel_button.set_label("Cancel Reply");
        cancel_button.set_sensitive(true);
        compose_context_label.set_text(&format!(
            "Replying to #{}: {}",
            reply_target.message_id, reply_target.preview
        ));
        compose_context_label.set_visible(true);
        return;
    }

    send_button.set_label("Send");
    cancel_button.set_label("Cancel");
    cancel_button.set_sensitive(false);
    compose_context_label.set_text("");
    compose_context_label.set_visible(false);
}

fn reset_edit_mode(
    app_state: &Rc<RefCell<AppState>>,
    compose_entry: &gtk::Entry,
    send_button: &gtk::Button,
    cancel_edit_button: &gtk::Button,
    compose_context_label: &gtk::Label,
) {
    let mut state = app_state.borrow_mut();
    state.editing_message_id = None;
    state.replying_to = None;
    drop(state);
    compose_entry.set_text("");
    set_compose_mode_ui(
        app_state,
        send_button,
        cancel_edit_button,
        compose_context_label,
    );
}

fn reset_thread_context(app_state: &Rc<RefCell<AppState>>) {
    app_state.borrow_mut().current_thread_parent_message_id = None;
}

fn begin_edit_mode(
    app_state: &Rc<RefCell<AppState>>,
    compose_entry: &gtk::Entry,
    send_button: &gtk::Button,
    cancel_edit_button: &gtk::Button,
    compose_context_label: &gtk::Label,
    message_id: u64,
    content: &str,
) {
    let mut state = app_state.borrow_mut();
    state.replying_to = None;
    state.editing_message_id = Some(message_id);
    drop(state);
    compose_entry.set_text(content);
    set_compose_mode_ui(
        app_state,
        send_button,
        cancel_edit_button,
        compose_context_label,
    );
    compose_entry.grab_focus();
    compose_entry.set_position(-1);
}

fn begin_reply_mode(
    app_state: &Rc<RefCell<AppState>>,
    compose_entry: &gtk::Entry,
    send_button: &gtk::Button,
    cancel_edit_button: &gtk::Button,
    compose_context_label: &gtk::Label,
    message_id: u64,
    parent_message_id: Option<u64>,
    preview: &str,
) {
    let mut state = app_state.borrow_mut();
    state.editing_message_id = None;
    state.replying_to = Some(ReplyTarget {
        message_id,
        parent_message_id,
        preview: truncate_preview(preview),
    });
    drop(state);

    set_compose_mode_ui(
        app_state,
        send_button,
        cancel_edit_button,
        compose_context_label,
    );
    compose_entry.grab_focus();
    compose_entry.set_position(-1);
}

fn open_message_target(
    app_state: &Rc<RefCell<AppState>>,
    tx: &mpsc::Sender<UiMessage>,
    status_label: &gtk::Label,
    channel_label: &gtk::Label,
    channel_list: &gtk::ListBox,
    content_stack: &gtk::Stack,
    compose_entry: &gtk::Entry,
    send_button: &gtk::Button,
    cancel_edit_button: &gtk::Button,
    compose_context_label: &gtk::Label,
    target_message_id: u64,
    override_channel_id: Option<u64>,
) {
    let (client, token, generation, channel_id, channel_name, bootstrap) = {
        let mut state = app_state.borrow_mut();
        state.editing_message_id = None;
        state.replying_to = None;
        state.current_thread_parent_message_id = None;

        let Some(session) = state.session.as_mut() else {
            status_label.set_text("Not connected.");
            return;
        };

        let channel_id = override_channel_id.unwrap_or(session.selected_channel_id);
        session.selected_channel_id = channel_id;

        let bootstrap = session.bootstrap.clone();
        let channel_name = active_text_channel_name(&bootstrap, channel_id);

        (
            session.client.clone(),
            session.token.clone(),
            session.generation,
            channel_id,
            channel_name,
            bootstrap,
        )
    };

    compose_entry.set_text("");
    set_compose_mode_ui(
        app_state,
        send_button,
        cancel_edit_button,
        compose_context_label,
    );
    channel_label.set_text(&format!("Current channel: {}", channel_name));
    select_channel_row(channel_list, &bootstrap, channel_id);
    content_stack.set_visible_child_name("timeline");
    status_label.set_text(&format!("Opening message {}...", target_message_id));
    spawn_fetch_messages_target(
        tx.clone(),
        generation,
        client,
        token,
        channel_id,
        Some(target_message_id),
    );
}

fn populate_channel_list(
    channel_list: &gtk::ListBox,
    bootstrap: &NativeBootstrap,
    selected_channel_id: u64,
    unread_channel_counts: &BTreeMap<u64, u64>,
) {
    while let Some(child) = channel_list.first_child() {
        channel_list.remove(&child);
    }

    for channel in text_channels(bootstrap) {
        let row = gtk::ListBoxRow::new();
        let channel_name = channel
            .name
            .unwrap_or_else(|| format!("channel-{}", channel.id));
        let unread_count = unread_channel_counts.get(&channel.id).copied().unwrap_or(0);
        let label_text = if unread_count > 0 && channel.id != selected_channel_id {
            format!("• {channel_name} ({unread_count})")
        } else {
            channel_name
        };
        let label = gtk::Label::builder()
            .xalign(0.0)
            .wrap(true)
            .margin_top(8)
            .margin_bottom(8)
            .margin_start(12)
            .margin_end(12)
            .label(label_text)
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
                    compose_context_label,
                ) = navigation_ui.clone();

                open_button.connect_clicked(move |_| {
                    let (client, token, generation, bootstrap) = {
                        let mut state = app_state.borrow_mut();
                        state.editing_message_id = None;
                        state.replying_to = None;
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
                    set_compose_mode_ui(
                        &app_state,
                        &send_button,
                        &cancel_edit_button,
                        &compose_context_label,
                    );
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
                    compose_context_label,
                ) = navigation_ui.clone();
                let (thread_header_label, _thread_list, thread_stack, _, _, _, _) =
                    thread_ui.clone();

                open_thread_button.connect_clicked(move |_| {
                    let (client, token, generation, bootstrap) = {
                        let mut state = app_state.borrow_mut();
                        state.editing_message_id = None;
                        state.replying_to = None;
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
                    set_compose_mode_ui(
                        &app_state,
                        &send_button,
                        &cancel_edit_button,
                        &compose_context_label,
                    );
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
                .label(format!(
                    "{} · attached on message {}",
                    file.file.original_name, file.message_id
                ))
                .build();

            let actions = gtk::Box::builder()
                .orientation(gtk::Orientation::Horizontal)
                .spacing(8)
                .build();

            let open_button = gtk::Button::builder().label("Open").build();
            let open_file_button = gtk::Button::builder().label("Open File").build();

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
                    compose_context_label,
                ) = navigation_ui.clone();

                open_button.connect_clicked(move |_| {
                    let (client, token, generation, bootstrap) = {
                        let mut state = app_state.borrow_mut();
                        state.editing_message_id = None;
                        state.replying_to = None;
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
                    set_compose_mode_ui(
                        &app_state,
                        &send_button,
                        &cancel_edit_button,
                        &compose_context_label,
                    );
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
                let status_label = navigation_ui.0.clone();
                let file = file.file.clone();

                open_file_button.connect_clicked(move |_| {
                    open_native_file(&app_state, &status_label, &file);
                });
            }

            container.append(&meta);
            container.append(&content);
            actions.append(&open_button);
            actions.append(&open_file_button);
            container.append(&actions);
            row.set_child(Some(&container));
            list_box.append(&row);
        }
    }
}

fn populate_message_list(
    list_box: &gtk::ListBox,
    bootstrap: &NativeBootstrap,
    messages: &NativeMessagesResponse,
    app_state: &Rc<RefCell<AppState>>,
    tx: &mpsc::Sender<UiMessage>,
    navigation_ui: &NavigationUi,
    status_label: &gtk::Label,
    compose_entry: &gtk::Entry,
    send_button: &gtk::Button,
    cancel_edit_button: &gtk::Button,
    compose_context_label: &gtk::Label,
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
            .label(format!(
                "[{}] {}",
                message.created_at,
                user_display_name(bootstrap, message.user_id)
            ))
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
        let reply_button = gtk::Button::builder().label("Reply").build();
        let view_thread_button = gtk::Button::builder().label("Thread").build();
        let open_parent_button = message
            .reply_to_message_id
            .map(|_| gtk::Button::builder().label("Open Parent").build());

        {
            let app_state = Rc::clone(app_state);
            let compose_entry = compose_entry.clone();
            let send_button = send_button.clone();
            let cancel_edit_button = cancel_edit_button.clone();
            let compose_context_label = compose_context_label.clone();
            let status_label = status_label.clone();
            let message_id = message.id;
            let content_to_edit = plain_content;

            edit_button.connect_clicked(move |_| {
                begin_edit_mode(
                    &app_state,
                    &compose_entry,
                    &send_button,
                    &cancel_edit_button,
                    &compose_context_label,
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
                compose_context_label,
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
                    state.replying_to = None;
                    state.current_thread_parent_message_id = Some(message_id);

                    (client, token, generation)
                };

                compose_entry.set_text("");
                set_compose_mode_ui(
                    &app_state,
                    &send_button,
                    &cancel_edit_button,
                    &compose_context_label,
                );
                thread_header_label.set_text(&format!("Thread for message {}", message_id));
                content_stack.set_visible_child_name("thread");
                status_label.set_text(&format!("Loading thread {}...", message_id));
                spawn_fetch_thread(tx.clone(), generation, client, token, message_id);
            });
        }

        {
            let app_state = Rc::clone(app_state);
            let tx = tx.clone();
            let status_label = status_label.clone();
            let compose_entry = compose_entry.clone();
            let send_button = send_button.clone();
            let cancel_edit_button = cancel_edit_button.clone();
            let compose_context_label = compose_context_label.clone();
            let message_id = message.id;
            let preview = display_content.clone();
            let (
                thread_header_label,
                _thread_list,
                content_stack,
                _thread_compose_entry,
                _thread_send_button,
                _thread_cancel_button,
                _thread_context_label,
            ) = thread_ui.clone();

            reply_button.connect_clicked(move |_| {
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

                    state.current_thread_parent_message_id = Some(message_id);

                    (client, token, generation)
                };

                thread_header_label.set_text(&format!("Thread for message {}", message_id));
                content_stack.set_visible_child_name("thread");
                status_label.set_text(&format!("Loading thread {}...", message_id));
                begin_reply_mode(
                    &app_state,
                    &compose_entry,
                    &send_button,
                    &cancel_edit_button,
                    &compose_context_label,
                    message_id,
                    Some(message_id),
                    &preview,
                );
                spawn_fetch_thread(tx.clone(), generation, client, token, message_id);
            });
        }

        if let (Some(reply_to_message_id), Some(open_parent_button)) =
            (message.reply_to_message_id, open_parent_button.as_ref())
        {
            let app_state = Rc::clone(app_state);
            let tx = tx.clone();
            let (
                status_label,
                channel_label,
                channel_list,
                content_stack,
                compose_entry,
                send_button,
                cancel_edit_button,
                compose_context_label,
            ) = navigation_ui.clone();

            open_parent_button.connect_clicked(move |_| {
                open_message_target(
                    &app_state,
                    &tx,
                    &status_label,
                    &channel_label,
                    &channel_list,
                    &content_stack,
                    &compose_entry,
                    &send_button,
                    &cancel_edit_button,
                    &compose_context_label,
                    reply_to_message_id,
                    None,
                );
            });
        }

        actions.append(&edit_button);
        actions.append(&delete_button);
        actions.append(&reply_button);
        actions.append(&view_thread_button);
        if let Some(open_parent_button) = open_parent_button.as_ref() {
            actions.append(open_parent_button);
        }

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
        if let Some(reply_preview) = reply_preview_text(bootstrap, message) {
            let reply_preview_label = gtk::Label::builder()
                .xalign(0.0)
                .wrap(true)
                .css_classes(["caption", "dim-label"])
                .label(reply_preview)
                .build();

            container.append(&reply_preview_label);
        }
        container.append(&content);
        append_file_attachments(&container, &message.files, app_state, status_label);
        container.append(&actions);
        row.set_child(Some(&container));
        list_box.append(&row);
    }
}

fn populate_thread_list(
    list_box: &gtk::ListBox,
    bootstrap: &NativeBootstrap,
    parent_message: &NativeMessage,
    messages: &NativeMessagesResponse,
    app_state: &Rc<RefCell<AppState>>,
    tx: &mpsc::Sender<UiMessage>,
    navigation_ui: &NavigationUi,
    status_label: &gtk::Label,
    compose_entry: &gtk::Entry,
    send_button: &gtk::Button,
    cancel_edit_button: &gtk::Button,
    compose_context_label: &gtk::Label,
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
            "[{}] {}",
            parent_message.created_at,
            user_display_name(bootstrap, parent_message.user_id)
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
    let reply_button = gtk::Button::builder().label("Reply").build();
    let open_parent_button = parent_message
        .reply_to_message_id
        .map(|_| gtk::Button::builder().label("Open Parent").build());

    {
        let app_state = Rc::clone(app_state);
        let compose_entry = compose_entry.clone();
        let send_button = send_button.clone();
        let cancel_edit_button = cancel_edit_button.clone();
        let compose_context_label = compose_context_label.clone();
        let status_label = status_label.clone();
        let message_id = parent_message.id;
        let content_to_edit = plain_content;

        edit_button.connect_clicked(move |_| {
            begin_edit_mode(
                &app_state,
                &compose_entry,
                &send_button,
                &cancel_edit_button,
                &compose_context_label,
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

    {
        let app_state = Rc::clone(app_state);
        let compose_entry = compose_entry.clone();
        let send_button = send_button.clone();
        let cancel_edit_button = cancel_edit_button.clone();
        let compose_context_label = compose_context_label.clone();
        let status_label = status_label.clone();
        let message_id = parent_message.id;
        let preview = display_content.clone();

        reply_button.connect_clicked(move |_| {
            begin_reply_mode(
                &app_state,
                &compose_entry,
                &send_button,
                &cancel_edit_button,
                &compose_context_label,
                message_id,
                Some(message_id),
                &preview,
            );

            status_label.set_text(&format!("Replying in thread {}", message_id));
        });
    }

    if let (Some(reply_to_message_id), Some(open_parent_button)) = (
        parent_message.reply_to_message_id,
        open_parent_button.as_ref(),
    ) {
        let app_state = Rc::clone(app_state);
        let tx = tx.clone();
        let (
            status_label,
            channel_label,
            channel_list,
            content_stack,
            compose_entry,
            send_button,
            cancel_edit_button,
            compose_context_label,
        ) = navigation_ui.clone();

        open_parent_button.connect_clicked(move |_| {
            open_message_target(
                &app_state,
                &tx,
                &status_label,
                &channel_label,
                &channel_list,
                &content_stack,
                &compose_entry,
                &send_button,
                &cancel_edit_button,
                &compose_context_label,
                reply_to_message_id,
                None,
            );
        });
    }

    actions.append(&edit_button);
    actions.append(&delete_button);
    actions.append(&reply_button);
    if let Some(open_parent_button) = open_parent_button.as_ref() {
        actions.append(open_parent_button);
    }
    container.append(&meta);
    if let Some(reply_preview) = reply_preview_text(bootstrap, parent_message) {
        let reply_preview_label = gtk::Label::builder()
            .xalign(0.0)
            .wrap(true)
            .css_classes(["caption", "dim-label"])
            .label(reply_preview)
            .build();

        container.append(&reply_preview_label);
    }
    container.append(&content);
    append_file_attachments(&container, &parent_message.files, app_state, status_label);
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
            .label(format!(
                "[{}] {}",
                message.created_at,
                user_display_name(bootstrap, message.user_id)
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
        let reply_button = gtk::Button::builder().label("Reply").build();
        let open_parent_button = message
            .reply_to_message_id
            .map(|_| gtk::Button::builder().label("Open Parent").build());

        {
            let app_state = Rc::clone(app_state);
            let compose_entry = compose_entry.clone();
            let send_button = send_button.clone();
            let cancel_edit_button = cancel_edit_button.clone();
            let compose_context_label = compose_context_label.clone();
            let status_label = status_label.clone();
            let message_id = message.id;
            let content_to_edit = plain_content;

            edit_button.connect_clicked(move |_| {
                begin_edit_mode(
                    &app_state,
                    &compose_entry,
                    &send_button,
                    &cancel_edit_button,
                    &compose_context_label,
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

        {
            let app_state = Rc::clone(app_state);
            let compose_entry = compose_entry.clone();
            let send_button = send_button.clone();
            let cancel_edit_button = cancel_edit_button.clone();
            let compose_context_label = compose_context_label.clone();
            let status_label = status_label.clone();
            let message_id = message.id;
            let preview = display_content.clone();
            let parent_message_id = parent_message.id;

            reply_button.connect_clicked(move |_| {
                begin_reply_mode(
                    &app_state,
                    &compose_entry,
                    &send_button,
                    &cancel_edit_button,
                    &compose_context_label,
                    message_id,
                    Some(parent_message_id),
                    &preview,
                );

                status_label.set_text(&format!("Replying to message {}", message_id));
            });
        }

        if let (Some(reply_to_message_id), Some(open_parent_button)) =
            (message.reply_to_message_id, open_parent_button.as_ref())
        {
            let app_state = Rc::clone(app_state);
            let tx = tx.clone();
            let (
                status_label,
                channel_label,
                channel_list,
                content_stack,
                compose_entry,
                send_button,
                cancel_edit_button,
                compose_context_label,
            ) = navigation_ui.clone();

            open_parent_button.connect_clicked(move |_| {
                open_message_target(
                    &app_state,
                    &tx,
                    &status_label,
                    &channel_label,
                    &channel_list,
                    &content_stack,
                    &compose_entry,
                    &send_button,
                    &cancel_edit_button,
                    &compose_context_label,
                    reply_to_message_id,
                    None,
                );
            });
        }

        actions.append(&edit_button);
        actions.append(&delete_button);
        actions.append(&reply_button);
        if let Some(open_parent_button) = open_parent_button.as_ref() {
            actions.append(open_parent_button);
        }
        container.append(&meta);
        if let Some(reply_preview) = reply_preview_text(bootstrap, message) {
            let reply_preview_label = gtk::Label::builder()
                .xalign(0.0)
                .wrap(true)
                .css_classes(["caption", "dim-label"])
                .label(reply_preview)
                .build();

            container.append(&reply_preview_label);
        }
        container.append(&content);
        append_file_attachments(&container, &message.files, app_state, status_label);
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

fn persist_session_config(session: &SessionState) {
    let server = session.client.base_url().to_string();
    let username = session.username.clone();
    let mut config = load_config().ok().flatten().unwrap_or_default();

    config.server = server.clone();
    config.username = username.clone();
    config.auth_token = Some(session.token.clone());
    config.last_channel_id = Some(session.selected_channel_id);
    config.upsert_saved_session(SavedSession {
        server,
        username,
        auth_token: Some(session.token.clone()),
        last_channel_id: Some(session.selected_channel_id),
    });

    let _ = save_config(&config);
}

fn clear_saved_session_token(server: &str, username: &str) {
    let mut config = load_config().ok().flatten().unwrap_or_default();

    config.server = server.to_string();
    config.username = username.to_string();
    config.auth_token = None;
    config.last_channel_id = None;
    config.clear_saved_session(server, username);

    let _ = save_config(&config);
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
        let connection_username = username.clone();
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
                    username: connection_username,
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
    username: String,
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
                    username: username.clone(),
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
    files: Vec<String>,
    thread_parent_message_id: Option<u64>,
    reply_to_message_id: Option<u64>,
) {
    std::thread::spawn(move || {
        let result: Result<_> = (|| {
            let runtime = tokio::runtime::Runtime::new()?;

            runtime.block_on(async move {
                client
                    .send_message(
                        &token,
                        channel_id,
                        &content,
                        &files,
                        thread_parent_message_id,
                        reply_to_message_id,
                    )
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

fn spawn_signal_typing(
    client: NativeClient,
    token: String,
    channel_id: u64,
    parent_message_id: Option<u64>,
) {
    std::thread::spawn(move || {
        let result: Result<_> = (|| {
            let runtime = tokio::runtime::Runtime::new()?;

            runtime.block_on(async move {
                client
                    .signal_typing(&token, channel_id, parent_message_id)
                    .await
            })
        })();

        if let Err(_error) = result {
            // ignore typing-signal failures
        }
    });
}

fn spawn_upload_temporary_file(
    tx: mpsc::Sender<UiMessage>,
    generation: u64,
    client: NativeClient,
    token: String,
    file_path: PathBuf,
) {
    std::thread::spawn(move || {
        let result: Result<_> = (|| {
            let runtime = tokio::runtime::Runtime::new()?;

            runtime.block_on(async move { client.upload_temp_file(&token, &file_path).await })
        })();

        match result {
            Ok(temp_file) => {
                let _ = tx.send(UiMessage::TempFileUploaded {
                    generation,
                    temp_file,
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

fn spawn_delete_temporary_file(
    tx: mpsc::Sender<UiMessage>,
    generation: u64,
    client: NativeClient,
    token: String,
    file_id: String,
) {
    std::thread::spawn(move || {
        let result: Result<_> = (|| {
            let runtime = tokio::runtime::Runtime::new()?;

            runtime.block_on(async move { client.delete_temp_file(&token, &file_id).await })
        })();

        if let Err(error) = result {
            let _ = tx.send(UiMessage::Error {
                generation,
                message: error.to_string(),
            });
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
        .text(&saved_config.server)
        .build();

    let username_entry = gtk::Entry::builder()
        .hexpand(true)
        .placeholder_text("Username")
        .text(&saved_config.username)
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

    let timeline_typing_label = gtk::Label::builder()
        .xalign(0.0)
        .wrap(true)
        .css_classes(["dim-label"])
        .label("")
        .visible(false)
        .build();

    let thread_typing_label = gtk::Label::builder()
        .xalign(0.0)
        .wrap(true)
        .css_classes(["dim-label"])
        .label("")
        .visible(false)
        .build();

    let compose_entry = gtk::Entry::builder()
        .hexpand(true)
        .placeholder_text("Type a plain text message")
        .build();
    compose_entry.set_sensitive(false);

    let pending_attachments_box = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(6)
        .visible(false)
        .build();

    let compose_context_label = gtk::Label::builder()
        .xalign(0.0)
        .wrap(true)
        .css_classes(["dim-label"])
        .label("")
        .visible(false)
        .build();

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

    let attach_button = gtk::Button::builder().label("Add File").build();
    attach_button.set_sensitive(false);

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

    let timeline_box = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(8)
        .build();
    timeline_box.append(&messages_scroll);
    timeline_box.append(&timeline_typing_label);

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
    thread_box.append(&thread_typing_label);

    let content_stack = gtk::Stack::builder()
        .vexpand(true)
        .hexpand(true)
        .transition_type(gtk::StackTransitionType::Crossfade)
        .build();
    content_stack.add_titled(&timeline_box, Some("timeline"), "Timeline");
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
    compose_box.append(&attach_button);
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
    right_column.append(&compose_context_label);
    right_column.append(&pending_attachments_box);
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
        let pending_attachments_box = pending_attachments_box.clone();

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
                state.pending_uploads.clear();
                state.upload_in_flight = false;
                state.active_generation
            };

            connect_button.set_sensitive(false);
            render_pending_uploads(&pending_attachments_box, &app_state, &tx, &status_label);
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
            let (client, token, generation, channel_id, username) = {
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
                let username = session.username.clone();

                state.reconnect_in_flight = true;

                (client, token, generation, channel_id, username)
            };

            reconnect_button.set_sensitive(false);
            status_label.set_text("Reconnecting...");
            spawn_restore_session(tx.clone(), generation, username, client, token, channel_id);
        });
    }

    {
        let tx = tx.clone();
        let app_state = Rc::clone(&app_state);
        let channel_label = channel_label.clone();
        let channel_list_for_selection = channel_list.clone();
        let compose_entry = compose_entry.clone();
        let send_button = send_button.clone();
        let cancel_edit_button = cancel_edit_button.clone();
        let compose_context_label = compose_context_label.clone();
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
                    persist_session_config(session);
                }

                (client, token, generation, channel_id, channel_name)
            };

            reset_edit_mode(
                &app_state,
                &compose_entry,
                &send_button,
                &cancel_edit_button,
                &compose_context_label,
            );
            if let Some(session) = app_state.borrow().session.as_ref() {
                populate_channel_list(
                    &channel_list_for_selection,
                    &session.bootstrap,
                    channel_id,
                    &app_state.borrow().unread_channel_counts,
                );
            }
            content_stack.set_visible_child_name("timeline");
            channel_label.set_text(&format!("Current channel: {}", channel_name));
            spawn_fetch_messages(tx.clone(), generation, client, token, channel_id);
        });
    }

    {
        let tx = tx.clone();
        let app_state = Rc::clone(&app_state);
        let window = window.clone();
        let status_label = status_label.clone();
        let attach_button = attach_button.clone();
        let pending_attachments_box = pending_attachments_box.clone();

        attach_button.clone().connect_clicked(move |_| {
            {
                let state = app_state.borrow();

                if state.session.is_none() {
                    status_label.set_text("Not connected.");
                    return;
                }

                if state.upload_in_flight {
                    status_label.set_text("Attachment upload already in progress.");
                    return;
                }

                if state.editing_message_id.is_some() {
                    status_label.set_text("Attachments are disabled while editing a message.");
                    return;
                }
            }

            let dialog = gtk::FileDialog::builder()
                .title("Attach file")
                .accept_label("Attach")
                .modal(true)
                .build();

            let app_state = Rc::clone(&app_state);
            let tx = tx.clone();
            let status_label = status_label.clone();
            let attach_button = attach_button.clone();
            let pending_attachments_box = pending_attachments_box.clone();
            let window = window.clone();

            glib::MainContext::default().spawn_local(async move {
                let Ok(file) = dialog.open_future(Some(&window)).await else {
                    return;
                };

                let Some(file_path) = file.path() else {
                    status_label.set_text("Selected file is not a local path.");
                    return;
                };

                let file_name = file_path
                    .file_name()
                    .and_then(|value| value.to_str())
                    .unwrap_or("selected file")
                    .to_string();

                let (client, token, generation) = {
                    let mut state = app_state.borrow_mut();

                    if state.upload_in_flight {
                        status_label.set_text("Attachment upload already in progress.");
                        return;
                    }

                    let Some(session) = state.session.as_ref() else {
                        status_label.set_text("Not connected.");
                        return;
                    };

                    let client = session.client.clone();
                    let token = session.token.clone();
                    let generation = session.generation;

                    state.upload_in_flight = true;

                    (client, token, generation)
                };

                attach_button.set_sensitive(false);
                render_pending_uploads(&pending_attachments_box, &app_state, &tx, &status_label);
                status_label.set_text(&format!("Uploading {}...", file_name));
                spawn_upload_temporary_file(tx.clone(), generation, client, token, file_path);
            });
        });
    }

    {
        let tx = tx.clone();
        let app_state = Rc::clone(&app_state);
        let compose_entry = compose_entry.clone();
        let send_button = send_button.clone();
        let cancel_edit_button = cancel_edit_button.clone();
        let compose_context_label = compose_context_label.clone();
        let status_label = status_label.clone();
        let pending_attachments_box = pending_attachments_box.clone();

        send_button.clone().connect_clicked(move |_| {
            let content = compose_entry.text().trim().to_string();

            let (
                client,
                token,
                generation,
                channel_id,
                editing_message_id,
                current_thread_parent_message_id,
                reply_target,
                pending_upload_ids,
            ) = {
                let mut state = app_state.borrow_mut();
                let Some(session) = state.session.as_ref() else {
                    status_label.set_text("Not connected.");
                    return;
                };

                if state.upload_in_flight {
                    status_label.set_text("Wait for the attachment upload to finish.");
                    return;
                }

                let client = session.client.clone();
                let token = session.token.clone();
                let generation = session.generation;
                let channel_id = session.selected_channel_id;
                let current_thread_parent_message_id = state.current_thread_parent_message_id;
                let editing_message_id = state.editing_message_id.take();
                let reply_target = state.replying_to.clone();
                let pending_upload_ids = state
                    .pending_uploads
                    .iter()
                    .map(|file| file.id.clone())
                    .collect::<Vec<_>>();

                if content.is_empty() && pending_upload_ids.is_empty() {
                    if editing_message_id.is_some() {
                        state.editing_message_id = editing_message_id;
                    }

                    return;
                }

                if editing_message_id.is_some() && !pending_upload_ids.is_empty() {
                    state.editing_message_id = editing_message_id;
                    status_label.set_text("Remove staged attachments before saving an edit.");
                    return;
                }

                state.replying_to = None;
                if editing_message_id.is_none() {
                    state.pending_uploads.clear();
                }

                (
                    client,
                    token,
                    generation,
                    channel_id,
                    editing_message_id,
                    current_thread_parent_message_id,
                    reply_target,
                    pending_upload_ids,
                )
            };

            compose_entry.set_text("");
            set_compose_mode_ui(
                &app_state,
                &send_button,
                &cancel_edit_button,
                &compose_context_label,
            );
            render_pending_uploads(&pending_attachments_box, &app_state, &tx, &status_label);

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
                    pending_upload_ids,
                    reply_target
                        .as_ref()
                        .and_then(|target| target.parent_message_id)
                        .or(current_thread_parent_message_id),
                    reply_target.as_ref().map(|target| target.message_id),
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

        compose_entry.connect_changed(move |entry| {
            if entry.text().trim().is_empty() {
                return;
            }

            let (client, token, channel_id, parent_message_id, should_signal) = {
                let mut state = app_state.borrow_mut();
                let Some((client, token, channel_id)) = state.session.as_ref().map(|session| {
                    (
                        session.client.clone(),
                        session.token.clone(),
                        session.selected_channel_id,
                    )
                }) else {
                    return;
                };

                if state.editing_message_id.is_some() {
                    return;
                }

                let parent_message_id = state.current_thread_parent_message_id;
                let signal_key = typing_signal_key(channel_id, parent_message_id);
                let now = Instant::now();
                let should_signal = match state.last_typing_signal_by_context.get(&signal_key) {
                    Some(last_signal) => {
                        now.duration_since(*last_signal).as_millis() >= u128::from(TYPING_SIGNAL_MS)
                    }
                    None => true,
                };

                if should_signal {
                    state.last_typing_signal_by_context.insert(signal_key, now);
                }

                (client, token, channel_id, parent_message_id, should_signal)
            };

            if should_signal {
                spawn_signal_typing(client, token, channel_id, parent_message_id);
            }
        });
    }

    {
        let app_state = Rc::clone(&app_state);
        let compose_entry = compose_entry.clone();
        let send_button = send_button.clone();
        let cancel_edit_button = cancel_edit_button.clone();
        let compose_context_label = compose_context_label.clone();
        let status_label = status_label.clone();

        cancel_edit_button.clone().connect_clicked(move |_| {
            reset_edit_mode(
                &app_state,
                &compose_entry,
                &send_button,
                &cancel_edit_button,
                &compose_context_label,
            );
            status_label.set_text("Compose mode cleared");
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
        let thread_typing_label = thread_typing_label.clone();
        timeline_button.connect_clicked(move |_| {
            reset_thread_context(&app_state);
            content_stack.set_visible_child_name("timeline");
            update_thread_typing_label(&thread_typing_label, &app_state);
        });
    }

    {
        let app_state = Rc::clone(&app_state);
        let content_stack = content_stack.clone();
        let status_label = status_label.clone();
        let thread_typing_label = thread_typing_label.clone();

        thread_back_button.connect_clicked(move |_| {
            reset_thread_context(&app_state);
            content_stack.set_visible_child_name("timeline");
            update_thread_typing_label(&thread_typing_label, &app_state);
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
        let app = app.clone();
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
        let attach_button = attach_button.clone();
        let pending_attachments_box = pending_attachments_box.clone();
        let send_button = send_button.clone();
        let cancel_edit_button = cancel_edit_button.clone();
        let compose_context_label = compose_context_label.clone();
        let server_entry = server_entry.clone();
        let username_entry = username_entry.clone();
        let thread_header_label = thread_header_label.clone();
        let thread_list = thread_list.clone();
        let timeline_typing_label = timeline_typing_label.clone();
        let thread_typing_label = thread_typing_label.clone();
        let window = window.clone();
        let navigation_ui = (
            status_label.clone(),
            channel_label.clone(),
            channel_list.clone(),
            content_stack.clone(),
            compose_entry.clone(),
            send_button.clone(),
            cancel_edit_button.clone(),
            compose_context_label.clone(),
        );
        let thread_ui = (
            thread_header_label.clone(),
            thread_list.clone(),
            content_stack.clone(),
            compose_entry.clone(),
            send_button.clone(),
            cancel_edit_button.clone(),
            compose_context_label.clone(),
        );

        let tx_for_events = tx.clone();
        glib::timeout_add_local(Duration::from_millis(50), move || {
            while let Ok(message) = rx.try_recv() {
                match message {
                    UiMessage::Connected {
                        generation,
                        username,
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
                            state.replying_to = None;
                            state.current_thread_parent_message_id = None;
                            state.upload_in_flight = false;
                            state.reconnect_in_flight = false;
                            state.restore_in_flight = false;
                            state.unread_channel_counts =
                                unread_channel_counts_from_bootstrap(&bootstrap);
                            state.typing_users_by_channel.clear();
                            state.typing_users_by_thread.clear();
                            state.typing_timeout_tokens.clear();
                            state.last_typing_signal_by_context.clear();
                            state.session = Some(SessionState {
                                generation,
                                username,
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
                        attach_button.set_sensitive(true);
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
                        set_compose_mode_ui(
                            &app_state,
                            &send_button,
                            &cancel_edit_button,
                            &compose_context_label,
                        );
                        content_stack.set_visible_child_name("timeline");
                        populate_message_list(
                            &message_list,
                            &bootstrap,
                            &messages,
                            &app_state,
                            &tx_for_events,
                            &navigation_ui,
                            &status_label,
                            &compose_entry,
                            &send_button,
                            &cancel_edit_button,
                            &compose_context_label,
                            &thread_ui,
                        );
                        populate_channel_list(
                            &channel_list,
                            &bootstrap,
                            initial_channel_id,
                            &app_state.borrow().unread_channel_counts,
                        );
                        render_pending_uploads(
                            &pending_attachments_box,
                            &app_state,
                            &tx_for_events,
                            &status_label,
                        );
                        update_timeline_typing_label(&timeline_typing_label, &app_state);
                        update_thread_typing_label(&thread_typing_label, &app_state);

                        if let Some(session) = app_state.borrow().session.as_ref() {
                            persist_session_config(session);
                        }
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
                                state.replying_to = None;
                                state.current_thread_parent_message_id = None;
                                state.unread_channel_counts.remove(&channel_id);
                            }

                            is_current
                        };

                        if !is_current {
                            continue;
                        }

                        compose_entry.set_text("");
                        set_compose_mode_ui(
                            &app_state,
                            &send_button,
                            &cancel_edit_button,
                            &compose_context_label,
                        );
                        content_stack.set_visible_child_name("timeline");
                        let bootstrap = {
                            let state = app_state.borrow();
                            state
                                .session
                                .as_ref()
                                .map(|session| session.bootstrap.clone())
                        };
                        let Some(bootstrap) = bootstrap else {
                            continue;
                        };
                        populate_message_list(
                            &message_list,
                            &bootstrap,
                            &messages,
                            &app_state,
                            &tx_for_events,
                            &navigation_ui,
                            &status_label,
                            &compose_entry,
                            &send_button,
                            &cancel_edit_button,
                            &compose_context_label,
                            &thread_ui,
                        );
                        populate_channel_list(
                            &channel_list,
                            &bootstrap,
                            channel_id,
                            &app_state.borrow().unread_channel_counts,
                        );
                        update_timeline_typing_label(&timeline_typing_label, &app_state);
                        update_thread_typing_label(&thread_typing_label, &app_state);
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
                            &compose_context_label,
                        );
                        reset_thread_context(&app_state);
                        update_thread_typing_label(&thread_typing_label, &app_state);
                        populate_search_results(
                            &search_results_list,
                            &query,
                            &results,
                            &app_state,
                            &tx_for_events,
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
                                state.replying_to = None;
                                state.current_thread_parent_message_id = Some(parent_message.id);
                            }

                            is_current
                        };

                        if !is_current {
                            continue;
                        }

                        compose_entry.set_text("");
                        set_compose_mode_ui(
                            &app_state,
                            &send_button,
                            &cancel_edit_button,
                            &compose_context_label,
                        );
                        let bootstrap = {
                            let state = app_state.borrow();
                            state
                                .session
                                .as_ref()
                                .map(|session| session.bootstrap.clone())
                        };
                        let Some(bootstrap) = bootstrap else {
                            continue;
                        };
                        thread_header_label
                            .set_text(&format!("Thread for message {}", parent_message.id));
                        populate_thread_list(
                            &thread_list,
                            &bootstrap,
                            &parent_message,
                            &messages,
                            &app_state,
                            &tx_for_events,
                            &navigation_ui,
                            &status_label,
                            &compose_entry,
                            &send_button,
                            &cancel_edit_button,
                            &compose_context_label,
                        );
                        update_thread_typing_label(&thread_typing_label, &app_state);
                        content_stack.set_visible_child_name("thread");
                        status_label.set_text("Thread synced");
                    }
                    UiMessage::TempFileUploaded {
                        generation,
                        temp_file,
                    } => {
                        let is_current = {
                            let mut state = app_state.borrow_mut();
                            let Some(session) = state.session.as_ref() else {
                                continue;
                            };

                            let is_current = generation == state.active_generation
                                && generation == session.generation;

                            if is_current {
                                state.upload_in_flight = false;
                                state.pending_uploads.push(temp_file.clone());
                            }

                            is_current
                        };

                        if !is_current {
                            continue;
                        }

                        attach_button.set_sensitive(true);
                        render_pending_uploads(
                            &pending_attachments_box,
                            &app_state,
                            &tx_for_events,
                            &status_label,
                        );
                        status_label.set_text(&format!("Attached {}", temp_file.original_name));
                    }
                    UiMessage::Event { generation, event } => {
                        let window_active = window.is_active();
                        let event_channel_id = event
                            .payload
                            .get("channelId")
                            .and_then(|value| value.as_u64());
                        let new_message = if event.event_type == "newMessage" {
                            serde_json::from_value::<NativeMessage>(event.payload.clone()).ok()
                        } else {
                            None
                        };
                        let mut unread_channel_update = None;
                        let mut notification = None;
                        let mut typing_timeout = None;
                        let mut typing_labels_dirty = false;
                        let refresh_targets = {
                            let mut state = app_state.borrow_mut();
                            let current_thread_parent_message_id =
                                state.current_thread_parent_message_id;

                            match event.event_type.as_str() {
                                "channelReadStatesUpdate" => {
                                    if let (Some(channel_id), Some(count)) = (
                                        event_channel_id,
                                        event.payload.get("count").and_then(|value| value.as_u64()),
                                    ) {
                                        if count == 0 {
                                            state.unread_channel_counts.remove(&channel_id);
                                        } else {
                                            state.unread_channel_counts.insert(channel_id, count);
                                        }

                                        if let Some(session) = state.session.as_ref() {
                                            unread_channel_update = Some((
                                                session.bootstrap.clone(),
                                                session.selected_channel_id,
                                            ));
                                        }
                                    }

                                    None
                                }
                                "channelReadStatesDelta" => {
                                    if let (Some(channel_id), Some(delta)) = (
                                        event_channel_id,
                                        event.payload.get("delta").and_then(|value| value.as_i64()),
                                    ) {
                                        let next_count = state
                                            .unread_channel_counts
                                            .get(&channel_id)
                                            .copied()
                                            .unwrap_or(0)
                                            as i64
                                            + delta;

                                        if next_count <= 0 {
                                            state.unread_channel_counts.remove(&channel_id);
                                        } else {
                                            state
                                                .unread_channel_counts
                                                .insert(channel_id, next_count as u64);
                                        }

                                        if let Some(session) = state.session.as_ref() {
                                            unread_channel_update = Some((
                                                session.bootstrap.clone(),
                                                session.selected_channel_id,
                                            ));
                                        }
                                    }

                                    None
                                }
                                "messageTyping" => {
                                    if let (Some(channel_id), Some(user_id)) = (
                                        event_channel_id,
                                        event
                                            .payload
                                            .get("userId")
                                            .and_then(|value| value.as_u64()),
                                    ) {
                                        let parent_message_id = event
                                            .payload
                                            .get("parentMessageId")
                                            .and_then(|value| value.as_u64());

                                        if let Some(session) = state.session.as_ref() {
                                            if user_id != session.bootstrap.own_user_id {
                                                let typing_users = match parent_message_id {
                                                    Some(parent_message_id) => state
                                                        .typing_users_by_thread
                                                        .entry(parent_message_id)
                                                        .or_default(),
                                                    None => state
                                                        .typing_users_by_channel
                                                        .entry(channel_id)
                                                        .or_default(),
                                                };

                                                if !typing_users.contains(&user_id) {
                                                    typing_users.push(user_id);
                                                }

                                                let timeout_key = typing_context_key(
                                                    channel_id,
                                                    parent_message_id,
                                                    user_id,
                                                );
                                                let timeout_token = state
                                                    .typing_timeout_tokens
                                                    .get(&timeout_key)
                                                    .copied()
                                                    .unwrap_or(0)
                                                    + 1;
                                                state
                                                    .typing_timeout_tokens
                                                    .insert(timeout_key.clone(), timeout_token);
                                                typing_timeout = Some((
                                                    timeout_key,
                                                    timeout_token,
                                                    channel_id,
                                                    parent_message_id,
                                                    user_id,
                                                ));
                                                typing_labels_dirty = true;
                                            }
                                        }
                                    }

                                    None
                                }
                                _ => match state.session.as_ref().map(|session| {
                                    (
                                        session.client.clone(),
                                        session.token.clone(),
                                        session.selected_channel_id,
                                        session.generation,
                                        session.bootstrap.clone(),
                                    )
                                }) {
                                    Some((
                                        client,
                                        token,
                                        selected_channel_id,
                                        session_generation,
                                        bootstrap,
                                    )) if generation == state.active_generation
                                        && generation == session_generation
                                        && matches!(
                                            event.event_type.as_str(),
                                            "newMessage" | "messageUpdate" | "messageDelete"
                                        ) =>
                                    {
                                        if let (Some(channel_id), Some(message)) =
                                            (event_channel_id, new_message.as_ref())
                                        {
                                            if message.user_id != bootstrap.own_user_id
                                                && (channel_id != selected_channel_id
                                                    || !window_active)
                                            {
                                                notification = Some((
                                                    message.id,
                                                    format!(
                                                        "{} in {}",
                                                        user_display_name(
                                                            &bootstrap,
                                                            message.user_id
                                                        ),
                                                        active_text_channel_name(
                                                            &bootstrap, channel_id
                                                        )
                                                    ),
                                                    truncate_preview(&strip_html(&message.content)),
                                                ));
                                            }
                                        }

                                        if event_channel_id != Some(selected_channel_id) {
                                            None
                                        } else {
                                            Some((
                                                client,
                                                token,
                                                selected_channel_id,
                                                current_thread_parent_message_id,
                                            ))
                                        }
                                    }
                                    _ => None,
                                },
                            }
                        };

                        if let Some((bootstrap, selected_channel_id)) = unread_channel_update {
                            populate_channel_list(
                                &channel_list,
                                &bootstrap,
                                selected_channel_id,
                                &app_state.borrow().unread_channel_counts,
                            );
                        }

                        if typing_labels_dirty {
                            update_timeline_typing_label(&timeline_typing_label, &app_state);
                            update_thread_typing_label(&thread_typing_label, &app_state);
                        }

                        if let Some((
                            timeout_key,
                            timeout_token,
                            channel_id,
                            parent_message_id,
                            user_id,
                        )) = typing_timeout
                        {
                            let app_state = Rc::clone(&app_state);
                            let timeline_typing_label = timeline_typing_label.clone();
                            let thread_typing_label = thread_typing_label.clone();

                            glib::timeout_add_local(
                                Duration::from_millis(TYPING_EXPIRY_MS),
                                move || {
                                    let mut state = app_state.borrow_mut();
                                    let should_remove = matches!(
                                        state.typing_timeout_tokens.get(&timeout_key),
                                        Some(current_token) if *current_token == timeout_token
                                    );

                                    if should_remove {
                                        state.typing_timeout_tokens.remove(&timeout_key);

                                        let typing_users = match parent_message_id {
                                            Some(parent_message_id) => state
                                                .typing_users_by_thread
                                                .get_mut(&parent_message_id),
                                            None => {
                                                state.typing_users_by_channel.get_mut(&channel_id)
                                            }
                                        };

                                        if let Some(typing_users) = typing_users {
                                            typing_users.retain(|existing_user_id| {
                                                *existing_user_id != user_id
                                            });
                                        }
                                    }

                                    drop(state);
                                    update_timeline_typing_label(
                                        &timeline_typing_label,
                                        &app_state,
                                    );
                                    update_thread_typing_label(&thread_typing_label, &app_state);

                                    glib::ControlFlow::Break
                                },
                            );
                        }

                        if let Some((message_id, title, body)) = notification {
                            let desktop_notification = gtk::gio::Notification::new(&title);
                            desktop_notification.set_body(Some(&body));
                            app.send_notification(
                                Some(&format!("sharkord-message-{message_id}")),
                                &desktop_notification,
                            );
                        }

                        if let Some((client, token, channel_id, thread_parent_message_id)) =
                            refresh_targets
                        {
                            if let Some(parent_message_id) = thread_parent_message_id {
                                spawn_fetch_thread(
                                    tx_for_events.clone(),
                                    generation,
                                    client,
                                    token,
                                    parent_message_id,
                                );
                            } else {
                                spawn_fetch_messages(
                                    tx_for_events.clone(),
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
                            let username = session.username.clone();

                            state.reconnect_in_flight = true;

                            Some((client, token, channel_id, username))
                        };

                        if let Some((client, token, channel_id, username)) = reconnect {
                            reconnect_button.set_sensitive(false);
                            status_label.set_text("Connection dropped. Reconnecting...");
                            spawn_restore_session(
                                tx_for_events.clone(),
                                generation,
                                username,
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
                            let restore_failed = {
                                let mut state = app_state.borrow_mut();
                                let restore_failed = state.restore_in_flight;
                                let upload_in_flight = state.upload_in_flight;
                                state.reconnect_in_flight = false;
                                state.restore_in_flight = false;
                                state.upload_in_flight = false;
                                (restore_failed, upload_in_flight)
                            };

                            connect_button.set_sensitive(true);
                            reconnect_button.set_sensitive(true);
                            attach_button.set_sensitive(app_state.borrow().session.is_some());
                            render_pending_uploads(
                                &pending_attachments_box,
                                &app_state,
                                &tx_for_events,
                                &status_label,
                            );
                            if restore_failed.0 {
                                clear_saved_session_token(
                                    &server_entry.text(),
                                    &username_entry.text(),
                                );
                            }
                            if restore_failed.1 {
                                status_label.set_text(&format!("Upload failed: {message}"));
                                continue;
                            }
                            status_label.set_text(&message);
                        }
                    }
                }
            }

            glib::ControlFlow::Continue
        });
    }

    if let Some(saved_session) = saved_config.selected_saved_session() {
        if let Some(saved_token) = saved_session.auth_token.clone() {
            let preferred_channel_id = saved_session.last_channel_id.unwrap_or_default();

            if let Ok(client) = NativeClient::new(&saved_session.server) {
                let generation = {
                    let mut state = app_state.borrow_mut();
                    state.active_generation += 1;
                    state.restore_in_flight = true;
                    state.active_generation
                };

                connect_button.set_sensitive(false);
                reconnect_button.set_sensitive(false);
                status_label.set_text("Restoring saved session...");

                spawn_restore_session(
                    tx.clone(),
                    generation,
                    saved_session.username,
                    client,
                    saved_token,
                    preferred_channel_id,
                );
            }
        }
    }

    window.present();
}

fn main() -> glib::ExitCode {
    adw::init().expect("failed to initialize libadwaita");

    let app = adw::Application::builder().application_id(APP_ID).build();
    app.connect_activate(build_ui);

    app.run()
}
