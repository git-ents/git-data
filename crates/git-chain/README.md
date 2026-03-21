# git-chain

Append-only event chains stored as Git commit history.
Each commit is an event; the commit chain provides ordering.

## Usage

```sh
git chain append refs/events/log -m "something happened" --payload data.json
git chain walk refs/events/log
git chain walk refs/events/log --thread <commit>
```

## License

MIT OR Apache-2.0
