# A small, simple, secure webhook handler

* ~5MB binary
* ~500µs response times (8kb payload)
* verifies `x-hub-signature-256` header from github
* toml configuration to run multiple hooks per route and per repository
* multi-threaded server ([tokio](https://docs.rs/tokio)) with daemonized script execution ([fork](https://docs.rs/fork))

Supports the github [`push` event](https://docs.github.com/en/developers/webhooks-and-events/webhooks/webhook-events-and-payloads#push) or an arbitrary payload `"rook"` event.  Other github event types (like [issues](https://docs.github.com/en/developers/webhooks-and-events/webhooks/webhook-events-and-payloads#issues) or [deployments](https://docs.github.com/en/developers/webhooks-and-events/webhooks/webhook-events-and-payloads#deployment)) are not supported.

TLS support planned.

# Quick start

1. Create a file that contains only the shared secret
2. Create a script to run when the hook is called
3. Create a config file (see below) that maps a url to these two file paths
4. Get a copy of the server (see [releases](https://github.com/numberoverzero/rook/releases) or clone and `cargo build --release`)
5. Start listening for webhooks with `./rook your-config.toml`

## Configuration

There are two types of hooks: `"github"` and `"rook"`.  The only event that the `"github"` hook type supports is [push](https://docs.github.com/en/developers/webhooks-and-events/webhooks/webhook-events-and-payloads#push).

Multiple hooks can listen on the same path but they must be the same type.  When using multiple `"github"` hooks on the same path, the event's `repository` value is used to filter for matching hooks.  When using multiple `"rook"` hooks on the same path, any whose signature is verified will be invoked.

### Sample config

```toml
port = 9000

[[hooks]]
type = "github"
url = "/hooks/gh"
repo = "numberoverzero/webhook-test"
secret_file = "/home/crossj/my_secret"
command_path = "/home/crossj/my_script.sh"

[[hooks]]
type = "github"
url = "/hooks/gh"
repo = "numberoverzero/bloop"
secret_file = "/tmp/my_shared_secret"
command_path = "/tmp/pull_latest.sh"

[[hooks]]
type = "rook"
url = "/build-hooks/blog"
secret_file = "/home/crossj/blog/secret"
command_path = "/home/crossj/blog/rebuild.sh"
```

## Hook data

When using a `"rook"` hook the post body is passed in a single environment variable `$ROOK_INPUT`.  A `"github"` hook has three variables: `$GITHUB_REPO`, `$GITHUB_COMMIT`, `$GITHUB_REF`.  Why not args?  See [security details](#security) below.

### Sample `"github"` script

```sh
#!/usr/bin/env bash
echo "  time: $(date +%s)"    >> output.log
echo "  repo: $GITHUB_REPO"   >> output.log
echo "commit: $GITHUB_COMMIT" >> output.log
echo "   ref: $GITHUB_REF"    >> output.log
```

### Sample `"rook"` script

```sh
#!/usr/bin/env bash
echo "time: $(date +%s)" >> output.log
echo "body: $ROOK_INPUT" >> output.log
```

## Running the server

```sh
$ ./rook my-config.toml
listening on port 9000
140.82.115.81:50925 - - [06/Nov/2021:02:25:57 +0000] "POST /hooks/gh HTTP/1.1" 200 OK - 291µs
140.82.115.117:28685 - - [06/Nov/2021:03:45:42 +0000] "POST /hooks/gh HTTP/1.1" 400 Bad Request - 5µs
140.82.115.117:24349 - - [06/Nov/2021:03:57:15 +0000] "POST /hooks/gh HTTP/1.1" 200 OK - 236µs
```

# Sending a `"rook"` hook

Rook uses the same signing mechanism as github's hooks, with a slightly different header name: `x-rook-signature-256`.

1. Construct a request `body`
2. Load a shared `secret`
3. Calculate `hmacSha256(secret, body)`
4. Put the hex-encoded digest value prefixed with `sha256=` in a request header.  In pseudocode:

```
# some command to run
body = b"build --release --target x86_64-pc-windows-gnu"
secret = hex_to_bytes("d33e7cdf2126defc0e88cd3aab9fffd91681b89291f1dfc74e4c3d3a19405fd6")
digest = bytes_to_hex(hmacSha256(secret, body).digest())

url = "http://localhost:9000"
verb = "POST"
headers = { "x-rook-signature-256": "sha256=" + digest }
request = new_request(verb, url, headers, body)
```

# Implementation Details

Unless you're auditing the code you can safely skip this section.

## Readability

The server is ~0.6kLOC[0] after `cargo fmt` and can be read completely in an hour or two.  ~1/4 is generic logging and config and there is no shared mutable state to track.  You may want to start reading at `main.rs::main`.

[0] `find src -type f -name "*.rs" -print0 | wc -l --files0-from=-`


## Performance

rook is designed to do one thing: map incoming POST requests with valid signatures to a local script and pass some environment variables or arguments.  If you're looking for more complex setups or verbose logging there are hundreds of other feature-rich implementations to explore.

rook provides minimal output (for debugging builds, see [debugging](#debugging)) and doesn't return detailed errors to callers.  It doesn't capture process output from scripts or failures to run scripts.  For example, if you forget to set the executable bit (`chmod +x my_hook.sh`) then rook will return a `500 Internal Error` with no body.

## Security

rook spawns processes from wherever it is running.  Both `"github"` and `"rook"` hooks pass the hook data through environment variables which is [reasonably secure](https://security.stackexchange.com/a/14009) on modern linuxes.  Note that command args are usually insecure because the default `hidepid=0` option when mounting [`proc(5)`](https://man7.org/linux/man-pages/man5/proc.5.html) allows [other users to view them](https://unix.stackexchange.com/questions/163145/how-to-get-whole-command-line-from-a-process).  If you want to forward sensitve data through a `"rook"` hook, you need to protect `/proc/[pid]/cmdline`:
> Users may not access files and subdirectories inside any /proc/[pid] directories but their own (the /proc/[pid] directories themselves remain visible).  Sensitive files such as /proc/[pid]/cmdline and /proc/[pid]/status are now protected against other users.

## Process spawning

* **Pipes**: `stdin`, `stdout`, `stderr` are all set to [null](https://doc.rust-lang.org/std/process/struct.Stdio.html#method.null)
* **Ordering**: rook simultaneously starts all matching hooks for the given path.
* **Non-blocking**: rook returns an http response without waiting for the processes to exit.
* **Non-graceful shutdown**: Because child processes are detached from the main rook process, killing the server will not terminate any running hook scripts.  This is done by calling [`setsid(2)`](https://man7.org/linux/man-pages/man2/setsid.2.html) in the child process after [`fork(2)`](https://man7.org/linux/man-pages/man2/fork.2.html).  This process is described in the [notes](https://man7.org/linux/man-pages/man2/setsid.2.html#NOTES) of `setsid(2)`, specifically:
  > In order to be sure that setsid() will succeed, call fork(2) and have the parent _exit(2), while the child (which by definition can't be a process group leader) calls setsid().
* **Threading**: The main rook process is multi-threaded with [tokio](https://docs.rs/tokio), so care must be taken when forking, as noted in `fork(2)`:
  > The child process is created with a single thread—the one that called fork().  The entire virtual address space of the parent is replicated in the child [..]; the use of pthread_atfork(3) may be helpful for dealing with problems that this can cause.
  
  However, [pthread_atfork(3)](https://man7.org/linux/man-pages/man3/pthread_atfork.3.html) has this to say on the feasibility of correct implementation:
    > The intent of pthread_atfork() was to provide a mechanism whereby the application (or a library) could ensure that mutexes and other process and thread state would be restored to a consistent state. In practice, this task is generally too difficult to be practicable.
  
  Rather than try to use `pthread_atfork(3)` correctly, rook avoids the issue by not sharing mutable state across threads.  One atomic ref-counted ([Arc](https://doc.rust-lang.org/std/sync/struct.Arc.html)) struct holds the read-only route config which will not deadlock if a child process panics.

## Debugging

Additional debugging output is available in non-release builds.  Clone this repository and compile a debug build:

```sh
$ git clone git@github.com:numberoverzero/rook.git
$ cargo build
$ scp target/debug/rook your-server:~/rook-DEBUG
```

Sample output:
```sh
$ ./rook-DEBUG your-config.toml
DEBUG:loaded config:
DEBUG:port 8080 with 2 routes
DEBUG:  1 github /hooks/gh/push
DEBUG:  1 rook   /hooks/rook/status
INFO:listening on port 8080
DEBUG:incoming request
DEBUG:<<<POST /hooks/gh/push
DEBUG:<<<host: "159.89.149.210:8080"
DEBUG:<<<user-agent: "GitHub-Hookshot/f7bdd04"
DEBUG:<<<content-length: "7714"
DEBUG:<<<accept: "*/*"
DEBUG:<<<x-github-delivery: "d219d74c-40ee-11ec-88b9-916812175124"
DEBUG:<<<x-github-event: "push"
DEBUG:<<<x-github-hook-id: "327497270"
DEBUG:<<<x-github-hook-installation-target-id: "423089939"
DEBUG:<<<x-github-hook-installation-target-type: "repository"
DEBUG:<<<x-hub-signature: "sha1=a0fb93ad0aa28a9afc0a7f3f2f00726ae18927e7"
DEBUG:<<<x-hub-signature-256: "sha256=8fd95ba5c47675f73c046ee24ae06de0593468823d32f55985e55ab619be259c"
DEBUG:<<<content-type: "application/json"
DEBUG:<<<connection: "close"
DEBUG:dispatch '/hooks/gh/push' as github
DEBUG:github payload: (numberoverzero/webhook-test, 2a536c03b2ee2e28d946cc3ee5a507751a267c6f, refs/heads/main)
DEBUG:path dispatch failed: HttpResponse<bad route>
INFO:140.82.115.145:59913 - - [08/Nov/2021:23:51:41 +0000] "POST /hooks/gh/push HTTP/1.1" 400 Bad Request - 570µs
DEBUG:incoming request
DEBUG:<<<POST /hooks/rook/status
DEBUG:<<<x-rook-signature-256: "sha256=39c08e2550981e8100a768f4626beee89f9ed1b2dc17797810630be97ee24b01"
DEBUG:<<<content-length: "42"
DEBUG:<<<accept: "*/*"
DEBUG:<<<host: "159.89.149.210:8080"
DEBUG:dispatch '/hooks/rook/status' as rook
DEBUG:rook payload (42b): "{\"some\": \"literal\", \"json\": [{}, 0, null]}"
DEBUG:hmac check success
DEBUG:hook forked
DEBUG:path dispatched successfully
INFO:52.173.143.145:1984 - - [08/Nov/2021:23:51:53 +0000] "POST /hooks/rook/status HTTP/1.1" 200 OK - 603µs
```