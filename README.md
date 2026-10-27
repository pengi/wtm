# wtm — worktree manager

`wtm` keeps all your git checkouts in one pool:

- one bare repository per registered upstream, in a central repos directory
- any number of worktrees from those repositories, in a flat checkouts
  directory

```sh
wtm register git@github.com:someone/project.git project
wtm new project              # worktree ~/wt/project on the default branch
wtm new project fix-thing    # worktree ~/wt/fix-thing on branch fix-thing
cd "$(wtm locate fix-thing)"
wtm drop fix-thing           # only if nothing would be lost
```

See [docs/](docs/index.md) for installation and usage.

## License

GPL-2.0-only. See [LICENSE](LICENSE).
