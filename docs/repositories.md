# Repositories

A registered repository is a bare git repository named `<name>.git` in the
repos directory:

```
~/.local/share/wtm/repos/
├── project.git/
└── other.git/
```

Any bare repository in that directory is a registered repository, whether
wtm created it or not. There is no other list.

## What wtm sets up

When registering, wtm creates the repository the way a normal clone would
be set up, not the way `git clone --bare` does it:

- **Remote-tracking branches.** Upstream branches are fetched to
  `refs/remotes/<remote>/*` (`origin/main`, …). Local branches are only
  created when a worktree needs them. Fetching never touches your local
  branches.
- **HEAD** points at the upstream's default branch. wtm uses it as the
  default branch when a remote doesn't say.

Everything else is plain git. You can use git commands on the repository
directly, for example `git -C "$(wtm locate --repo project)" log --all`.

## Remotes

A repository can have several remotes, for example your fork and the
original project:

```sh
wtm remote add project upstream https://github.com/original/project.git
wtm remote set-default project upstream
```

The **default remote** is the one `wtm new` fetches, tracks and creates
branches from. It is, in order:

1. the remote set with `wtm remote set-default`;
2. `origin`, if it exists;
3. the first remote.

`wtm remote set-default` stores its choice in the repository's git config,
as `wtm.defaultRemote`. This is the only setting wtm adds to a repository.

## Branches

Branches belong to the repository, not to a worktree. All worktrees of a
repository see the same branches, stashes and history.

## Removing

`wtm unregister project` deletes the bare repository. It refuses while the
repository has worktrees, or if any local branch has commits that are on no
remote (`--force` overrides the latter).
