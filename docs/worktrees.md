# Worktrees

A worktree is a normal [git worktree](https://git-scm.com/docs/git-worktree)
of a registered repository, in the checkouts directory:

```
~/wt/
├── project/      worktree of project.git, branch main
├── fix-thing/    worktree of project.git, branch fix-thing
└── other/        worktree of other.git
```

The directory name is the worktree's name. Names are unique in the checkouts
directory, so commands like `wtm drop` and `wtm locate` don't need the
repository name.

## What's in a worktree

The checked out files, and a `.git` *file* (not directory) pointing to the
repository:

```
gitdir: /home/you/.local/share/wtm/repos/project.git/worktrees/fix-thing
```

wtm adds nothing else. Inside a worktree, git works as in any clone: `git
status`, `git commit`, `git push`, `git pull`.

Worktrees created with plain `git worktree add` from a registered repository
also show up in `wtm list`, wherever they are.

## Dropping

`wtm drop` deletes the worktree directory, including ignored files, and the
local branch checked out in it. It refuses if that would lose anything:

| Refused when | Overridden by |
|---|---|
| uncommitted changes, staged or not | `--force` |
| untracked files that are not ignored | `--force` |
| a merge, rebase, cherry-pick, revert or bisect in progress | `--force` |
| the worktree is locked (`git worktree lock`) | `--force` |
| the branch has commits that are on no remote | `--keep-branch` or `--force` |
| a detached HEAD has commits on no branch | `--force` |

"On a remote" means reachable from any remote-tracking branch, as of the last
fetch. Use `--fetch` to fetch first, so commits pushed from elsewhere count.

If a worktree directory was deleted by hand, `wtm list` shows it as
`missing`, and `wtm drop` (or `wtm repair`) makes the repository forget it.

## Moving

Git stores absolute paths between a worktree and its repository. If you
move the checkouts directory or the repos directory, update the
[configuration](configuration.md) and run `wtm repair`.
