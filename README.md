# concord-client

A work-in-progress high-level wrapper for [Concord](https://github.com/walmartlabs/concord/) API.
Uses [tokio](https://tokio.rs/) for async I/O.

Depends on [reqwest](https://github.com/seanmonstar/reqwest) and
[tokio-tungstenite](https://github.com/snapview/tokio-tungstenite/) for HTTP and
WebSocket support, respectively.

## Status

- basic [QueueClient](src/queue_client.rs) implementation:
  - [x] maintains a WebSocket connection to the Concord server
  - [x] provides high-level API like `next_process` and `next_command`
  - [ ] automatic re-connection on errors
- basic [ApiClient](src/api_client.rs) implementation:
  - [x] can update process status
  - [x] download process state
  - [x] create and update log segments
  - [ ] everything else

## Code Conventions

- `warn!` and `error!` messages should include how likely the error is a bug:
  `(possibly a bug)`, `(likely a bug)`, etc. `ApiError` messages, however, should
  not include this information, as the interpretation of the error is up to the
  caller.
