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

## Development

```sh
cargo test                       # unit and end-to-end tests (needs git)
scripts/check-git-boundary.sh    # git access only in wtm-core/src/git/
cargo deny check                 # dependency licenses (GPL-2.0-only compatible)
```

Packages are built and tested in containers, so only docker is needed:

```sh
packaging/docker-build.sh        # .deb and .rpm into target/packages
packaging/test-packages.sh       # install and smoke test on each distro
```

Documentation uses [ProperDocs](https://properdocs.org):

```sh
pip install -r docs/requirements.txt
cargo run -q -- generate markdown > docs/reference/commands.md
properdocs serve
```

Releases are made by pushing a tag `v<version>` matching the version in
`Cargo.toml`.
