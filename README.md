# concord-client

A work-in-progress wrapper for [Concord](https://github.com/walmartlabs/concord/) API.

## Code Conventions

- `warn!` and `error!` messages should include how likely the error is a bug:
  `(possibly a bug)`, `(likely a bug)`, etc. `ApiError` messages, however, should
  not include this information, as the interpretation of the error is up to the
  caller.
