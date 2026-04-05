# Linux Client Plan

## Goal

Build a **real native Linux client** for this fork that does not embed the existing web app and does not depend on a Chromium runtime.

The product goal is not "web parity on desktop". The product goal is:

- small memory footprint
- good idle behavior
- direct native audio device handling
- enough chat and voice to replace Discord for a small group

## Chosen stack

- Rust
- GTK4
- libadwaita

This is a better fit for the Linux-first goal than shipping Electron/Tauri as a wrapper over the current web UI.

## Current repo constraints

The existing browser client is strongly tied to browser primitives:

- tRPC client setup is built around browser WebSocket lifecycle and browser storage in [`apps/client/src/lib/trpc.ts`](/home/dan/coolify-sharkcord/sharkord/apps/client/src/lib/trpc.ts#L46)
- connect/login flow assumes `window`, URL state, and browser storage in [`apps/client/src/features/server/actions.ts`](/home/dan/coolify-sharkcord/sharkord/apps/client/src/features/server/actions.ts#L25)
- voice capture/publish depends on `navigator.mediaDevices`, DOM media streams, and mediasoup browser transport setup in [`apps/client/src/components/voice-provider/index.tsx`](/home/dan/coolify-sharkcord/sharkord/apps/client/src/components/voice-provider/index.tsx#L450)
- microphone testing and processing depend on `AudioContext`, AudioWorklets, and HTML audio elements in [`apps/client/src/components/server-screens/user-settings/devices/hooks/use-microphone-test.ts`](/home/dan/coolify-sharkcord/sharkord/apps/client/src/components/server-screens/user-settings/devices/hooks/use-microphone-test.ts#L111)

That means the native client should be treated as a separate application, not as a repackaging exercise.

## Architecture

### 1. Native app shell

`apps/linux-client`

Responsibilities:

- native windowing
- local settings
- login forms
- channel and message UI
- desktop notifications
- microphone and speaker selection UI

### 2. Native-safe protocol layer

The server already has a solid typed contract, but today the web client reaches it through TypeScript+tRPC assumptions that are awkward for a native app.

We need a native-safe boundary for:

- auth
- initial server bootstrap
- event subscriptions
- message fetch/send/edit/delete
- presence
- voice signaling

That boundary can remain backed by the same server logic. The point is to expose it in a way a native client can consume cleanly without importing the browser client architecture.

### 3. Voice transport layer

Use mediasoup's native client path rather than inventing a custom media stack.

Implementation direction:

- keep Sharkord's existing mediasoup server
- implement native signaling for voice join/leave/producer/consumer setup
- use `libmediasoupclient` for send/receive transports

Rust remains the application language, but the media edge will likely require an FFI boundary around the native mediasoup client.

## MVP scope

The first usable version should include only:

1. Login to a configured server.
2. Channel list and message timeline.
3. Plain text send/edit/delete.
4. Voice join and leave.
5. Mic and speaker selection.
6. Push-to-talk.
7. Desktop notifications.

Explicitly defer:

- plugins
- server admin
- screen share
- webcam
- full markdown/editor parity
- profile and moderation parity

## Rollout

### Phase 0

- scaffold the native client app
- prove GTK/libadwaita packaging and local execution

### Phase 1

- extract and document the server connection/auth/bootstrap flow
- define the native-safe client contract for non-voice features
- build a headless sync prototype before spending time on polished UI

Status:

- the first thin slice is now in place
- the server exposes a minimal native HTTP+SSE contract for bootstrap, message fetch/send/edit/delete/search, and event streaming
- `apps/linux-client/src/bin/headless.rs` proves that contract from a non-browser process, including one-shot message get/search/edit/delete operations
- the GTK app now uses the same boundary for real text login, channel browsing, message loading, plain text send, explicit reply mode, inline edit/delete actions, a separate search-results view with navigation back into the live timeline, dedicated thread views with inline edit/delete on thread rows, thread replies through the compose box while a thread is open, manual refresh that respects the active root/thread view, search, token-based reconnect when the event stream drops, saved-session resume with remembered last channel, unread channel markers, and desktop notifications for new messages from other users

### Phase 2

- implement text-only native client
- login
- channels
- messages
- search and refresh
- notifications

Status:

- complete enough for a real text-client prototype, including session resume, reconnect, explicit reply targeting, thread navigation, unread channel markers, and desktop notifications
- still missing attachments, richer read-state UX, and live-server usability cleanup before calling the text slice "done"

### Phase 3

- implement headless native voice proof of concept
- join channel
- publish microphone
- play remote audio
- basic device selection

### Phase 4

- merge voice into the GTK client
- add push-to-talk
- stabilize reconnect and settings persistence

## Immediate next tasks

1. Improve timeline interaction beyond the current utilitarian row list so attachments, richer reply previews, and parent-message jumps read better.
2. Harden native text behavior against real-server usage, especially read-state, notification preferences, and awkward reconnect edge cases.
3. Add file download/upload support so the text client can handle normal everyday chat, not just plain-text rooms.
4. Only after that, invest in native voice transport and richer GTK screens.

## Success criteria

This effort is successful when:

- the native client can sit idle with materially lower memory usage than a browser tab
- voice works reliably with direct Linux device selection
- the app is good enough for a small private group without needing browser UI parity
