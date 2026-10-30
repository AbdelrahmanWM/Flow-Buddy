# Security

Flow Buddy reads your keyboard, so a security problem in it matters more than in most tools. Thank you for looking.

## Reporting a vulnerability

Please **don't open a public issue**. Report it privately through GitHub: the repository's **Security** tab → **Report a vulnerability**. You'll get an answer within a week. Once it's fixed, a release goes out and you'll be credited in the advisory, unless you'd rather not be.

## What counts

Anything that breaks the promises in [docs/privacy.md](docs/privacy.md), for example:

- typed text, keys in order, file names or window titles stored, logged or sent anywhere;
- another user or program on the machine reading what Flow Buddy sees: the socket, the database or the logs;
- the daemon making a network connection;
- snippets reaching an AI when `[text]` didn't allow them, or a file that looks secret being read;
- an API key, or the output of `api_key_command`, written to a log, the config or an error message;
- a message to the daemon that crashes it or makes it misbehave.

## Supported versions

The latest release. Before 1.0 there are no backported fixes.
