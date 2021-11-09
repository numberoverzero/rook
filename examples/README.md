# Running the example

1. Copy this directory to your server
2. Open port 8080 (or modify `config.toml`)
3. Update the repository name in `config.toml` from `YOUR-GITHUB-USERNAME/YOUR-REPO` to your test repository
4. Download a [release](https://github.com/numberoverzero/rook/releases) of rook to the same directory on the server
5. Start listening on the server with `./rook config.toml`.  You will see messages show up as you complete the next steps.
6. In another terminal, start watching for output: `touch output.log && tail -F output.log`
7. Set up your repository's [github webhook](https://docs.github.com/en/developers/webhooks-and-events/webhooks/creating-webhooks#setting-up-a-webhook) including the shared secret value.
8. Add a [rook action](https://github.com/numberoverzero/rook-action) to your repository's [github workflow](https://docs.github.com/en/actions/quickstart#creating-your-first-workflow) including appropriate secrets values.

When you complete step 7, github sends a `"ping"` event which will fail.  That is expected, since rook only supports `"push"` events.  To see the github hook succeed at this point, push a trivial change to your repo.  A few lines will show up in `output.log`.

When you complete step 8, github will first send a `"push"` event for the change to `.github/workflows/main.yml` which will trigger the first handler.  A few seconds later, once github has set up and run your workflow, you will see the second hook run with the input body from your [rook action](https://github.com/numberoverzero/rook-action).