# concord-client

A work-in-progress wrapper for [Concord](https://github.com/walmartlabs/concord/) API.
Uses [tokio](https://tokio.rs/) for async I/O.

Depends on [reqwest](https://github.com/seanmonstar/reqwest) and
[tokio-tungstenite](https://github.com/snapview/tokio-tungstenite/) for HTTP and WebSocket support, respectively.

## Status

- basic [QueueClient](src/queue_client.rs) implementation:
  - [x] maintains a WebSocket connection to concord-server
  - [x] provides high-level API like `next_process` and `next_command`
  - [x] graceful shutdown (on drop)
  - [ ] configurable timeouts
- basic [ApiClient](src/api_client.rs) implementation:
  - [x] supports both API token and session token authentication
  - [x] start a process
  - [x] get process details
  - [x] update process status
  - [x] download process state
  - [x] create and update log segments
  - [ ] configurable timeouts
  - [ ] everything else

The current feature set is enough to implement a bare-bones concord-agent in Rust.

## Create Features

All features are enabled by default.

- `api-client` - access to Concord's REST APIs;
- `queue-client` - access to Concord's websocket API.
