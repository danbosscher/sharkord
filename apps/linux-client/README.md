# Linux Client

This directory is the start of a real native Linux client for Sharkord.

It is intentionally **not** a web wrapper. The current scope is:

- prove a native GTK/libadwaita client can live in this repo cleanly
- define the protocol and media boundaries that need to move out of the browser client
- build toward a lightweight Linux-first MVP instead of full web parity

## Run

```bash
cargo run --manifest-path apps/linux-client/Cargo.toml
```

Run the headless native protocol prototype with:

```bash
cargo run --manifest-path apps/linux-client/Cargo.toml --bin headless -- \
  --server https://chat.zooi.org \
  --username your-user \
  --password your-password \
  --search "hello" \
  --no-stream
```

## Current state

The app currently provides:

- a native GTK/libadwaita text client
- saved server URL and remembered username on Linux
- saved auth tokens and last selected text channels per server/username pair for session resume on Linux
- real login, bootstrap, channel listing, message fetch, plain text send, explicit reply mode, inline edit/delete actions, inline reply previews with parent-jump actions, staged attachment upload from the native compose box, attachment rows with `Open File` actions, a separate search-results view with `Open` actions back into the live timeline plus direct file-open actions, dedicated thread views, thread replies through the main compose box while a thread is open, manual refresh that respects the active timeline/thread view, and token-based reconnect after stream loss
- background SSE event tailing for chat refreshes, persisted unread channel counts/read-state updates, and desktop notifications for new messages from other users
- a headless Rust prototype for login, bootstrap, message fetch/send/get/thread/search/edit/delete, and SSE event tailing

It does **not** yet provide:

- voice transport
- mic/speaker handling
- drag/drop or richer multi-file upload UX
- polished notification preferences or full unread/read-state UX parity with the web client

## Why this stack

This client uses:

- Rust
- GTK4
- libadwaita

That gives us:

- a native Linux UI toolkit
- small runtime overhead compared to Chromium wrappers
- good long-term maintainability for a Linux-first app

## Planned MVP

The target first usable version is intentionally narrow:

1. Login to a single Sharkord server.
2. Browse channels and read/send plain text messages.
3. Join voice, select mic/speaker, and push audio both ways.
4. Support push-to-talk and desktop notifications.

The MVP explicitly excludes:

- plugins
- server administration
- screen share
- webcam
- full rich-compose parity

See [`docs/linux-client-plan.md`](/home/dan/coolify-sharkcord/sharkord/docs/linux-client-plan.md) for the architecture and rollout plan.
