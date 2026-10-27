# Accounts

With an account on GitHub or GitLab, including self-hosted GitLab and
GitHub Enterprise, you can find repositories and register them by name
instead of by URL.

```sh
wtm auth add github github                 # asks for a token
wtm search github                          # your repositories
wtm register github:wtm                    # your own repository "wtm"
wtm register github:someone/project        # anyone's repository
```

## Adding an account

```sh
wtm auth add NAME KIND [--url URL] [--user USER] [--protocol ssh|https]
```

- `NAME` is your name for the account. It's what you write before the `:`
  in `wtm register NAME:PATH`. You can have several accounts per service,
  for example `github` and `work`.
- `KIND` is `github` or `gitlab`.
- `--url` is the web address of a self-hosted service, for example
  `https://gitlab.example.com`. The default is `https://github.com` or
  `https://gitlab.com`.
- `--protocol` chooses which clone URL `register` uses. The default is `ssh`.

wtm asks for an **access token**, with input hidden, and checks it against
the service. Leave it empty to only look up public repositories. Create a
token in the service's settings:

| Service | Token | Needs |
|---|---|---|
| GitHub | personal access token | `repo` scope (classic), or read access to the repositories (fine-grained) |
| GitLab | personal access token | `read_api` scope |

If git already has a token for the service, for example because you use
`gh auth login` or Git Credential Manager, wtm uses that token and doesn't
ask.

## Where tokens are kept

Tokens are kept by **git's credential helper**, the same place git keeps
passwords for HTTPS remotes (see `git help credential`). They never go in
wtm's config file. The token is stored for the account's URL and user, so
git can use it for HTTPS clones too.

`wtm auth remove NAME` removes the account from the config file but keeps
the token, since git may be using it. Add `--forget-token` to remove that
too.

!!! note
    If git has no credential helper configured, it can't remember tokens.
    Set one up first, for example `git config --global credential.helper
    libsecret` (or `store`, which keeps them in a plain file).

## Finding repositories

```sh
wtm search github                 # all repositories you own or are a member of
wtm search github tool            # ... with "tool" in the name or description
wtm search github --all "git tool"    # everything visible to you
```

## Registering

```sh
wtm register github:someone/project    # owner/repo
wtm register work:group/sub/project    # GitLab groups and subgroups
wtm register github:project            # your own: the account's user
```

The repository is registered with its clone URL, ssh or https as set for
the account. After that, it's an ordinary registered repository; the
account isn't needed to work with it.

An `ACCOUNT:` prefix only counts if `ACCOUNT` is one of your accounts.
Otherwise the source is treated as a git URL, so `host:path` style URLs
keep working.

## Network access

wtm uses the `curl` command to talk to the services, so your system's
certificates and proxy settings (`https_proxy`, `~/.curlrc`) apply.
