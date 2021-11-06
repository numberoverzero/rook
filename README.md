# A small, simple, secure webhook handler

* ~5MB binary
* ~500µs response times (8kb payload)
* verifies `x-hub-signature-256` header from github hooks
* toml configuration to run multiple hooks per route and per repository

# Quick start

1. Create a file that contains only the shared secret
2. Create a script to run when the hook is called
3. Create a config file (see below) that maps a url to these two file paths
4. Get a copy of the server (see [releases](https://github.com/numberoverzero/rook/releases) or clone and `cargo build --release`)
5. Start listening for webhooks with `./rook your-config.toml`

## Configuration

There are two types of hooks, `"github"` and `"rook"`.  For `"github"` hooks, you can specify which event types to listen for.  Supported github event types are `"push"` and `"deploy"`.

Multiple hooks can listen on the same path, but they must be the same type.  When using multiple `"github"` hooks on the same path, the event's `repository` value and `events` values are used to filter for matching hooks.  When using multiple `"rook"` hooks on the same path, any whose signature is verified will be invoked.

### Sample config

```toml
port = 9000

[[hooks]]
type = "github"
events = ["push", "deploy"]
url = "/hooks/gh"
repo = "numberoverzero/webhook-test"
secret_file = "/home/crossj/my_secret"
command = "/home/crossj/my_script.sh"

[[hooks]]
type = "github"
events = ["push"]
url = "/hooks/gh"
repo = "numberoverzero/bloop"
secret_file = "/tmp/my_shared_secret"
command = "/tmp/pull_latest.sh"

[[hooks]]
type = "rook"
url = "/build-hooks/blog"
secret_file = "/home/crossj/blog/secret"
command = "/home/crossj/blog/rebuild.sh"
```

## Hook data

When using a `"rook"` hook, data is passed as command line args.  There is no event type.  Unless you have modified how `proc` is mounted, these will be visible to any other user on the system.  See [security details below](#security).

When using a `"github"` hook, data is passed through environment variables.  For a `"deploy"` event, these are:

```sh
$GITHUB_EVENT="deploy"
$GITHUB_REPO=""        # eg. "numberoverzero/webhook-test"
$GITHUB_COMMIT=""      # eg. "55cdc72ed8eaca541e12279130d2b7fb5c74b38f"
$GITHUB_REF=""         # eg. "refs/heads/main"
```

For a github `"push"` event, these are:

```sh
$GITHUB_EVENT="push"
$GITHUB_REPO=""        # eg. "numberoverzero/webhook-test"
$GITHUB_COMMIT=""      # eg. "55cdc72ed8eaca541e12279130d2b7fb5c74b38f"
$GITHUB_REF=""         # eg. "refs/heads/main"
```

### Sample `"github"` script

```sh
#!/usr/bin/env bash
echo "  type: $GITHUB_EVENT"  >> output.log
echo "  repo: $GITHUB_REPO"   >> output.log
echo "commit: $GITHUB_COMMIT" >> output.log
echo "   ref: $GITHUB_REF"    >> output.log
```

### Sample `"hook"` script

```sh
#!/usr/bin/env bash
echo "received args: $@" >> output.log
```

## Running the server

```sh
$ ./rook my-config.toml
listening on port 9000
140.82.115.81:50925 - - [06/Nov/2021:02:25:57 +0000] "POST /hooks/gh HTTP/1.1" 200 OK - 3300µs
140.82.115.117:28685 - - [06/Nov/2021:03:45:42 +0000] "POST /hooks/gh HTTP/1.1" 400 Bad Request - 5µs
140.82.115.117:24349 - - [06/Nov/2021:03:57:15 +0000] "POST /hooks/gh HTTP/1.1" 200 OK - 3653µs
```

# Implementation Details

Skip this whole section if you just need to run some simple scripts when a webhook is invoked.

## Performance

rook is designed to do one thing: map incoming POST requests with valid signatures to a local script and pass some environment variables or shell arguments.  If you're looking for more complex setups or verbose logging, there are hundreds of other feature-rich implementations to explore.

rook has almost no debug output, and doesn't return detailed errors to callers.  It doesn't capture process output from hook scripts, or failures to run hook scripts.  For example, if you forget to set the executable bit on your hook script (`chmod +x my_hook.sh`) then rook will simply return a `500 Internal Error` with no body.

## Security

rook spawns processes from wherever it is running, and for `"github"` hooks will pass context through environment variables, which is [reasonably secure](https://security.stackexchange.com/a/14009) on modern linuxes.  For `"rook"` hooks, it will split the post body into args through [shlex](https://docs.rs/shlex/) and then pass those args to the script, which is often insecure, since the default `hidepid` option when mounting [`proc(5)`](https://man7.org/linux/man-pages/man5/proc.5.html) is `0`.  If you want to forward sensitve data through a `"rook"` hook, you need to protect `/proc/[pid]/cmdline` by mounting with `hidepid=1` or `hidepid=2`:
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

## Typed paths

From the config file, rook builds a map of `{url path: [array of hooks]}`.  To keep parsing fast, only one type of hook (`"github"` or `"rook"`) can be served at each path.  The following config is **invalid**:
```toml
port = 9000

[[hooks]]
type = "rook"               # <-- INVALID: different types, same path
url = "/same/path"
secret_file = "/tmp/secret"
command = "/tmp/run.sh"

[[hooks]]
type = "github"             # <-- INVALID: different types, same path
events = ["push"]
url = "/same/path"
repo = "numberoverzero/bar"
secret_file = "/tmp/secret"
command = "/tmp/run.sh"
```
