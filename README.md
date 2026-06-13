# solite

A new SQLite CLI, with a builtin runtime, REPL, Jupyter kernel, and MCP server. 


> [!WARNING]  
> Solite is ultra-alpha software and under active development! Expect broken features and breaking changes in the future.

## Shell completion

`solite` ships dynamic shell completion for bash, zsh, fish, elvish, and
PowerShell. Beyond subcommands and flags, it completes file paths
(extension-aware: `.sql`/`.ipynb` for scripts, `.db`/`.sqlite`/`.sqlite3` for
databases), procedure names from `-- name:` annotated `.sql` files, and SQL
keywords/table names inside query arguments.

Add the matching line to your shell config and start a new shell:

```sh
# bash — ~/.bashrc
source <(solite completions bash)

# zsh — ~/.zshrc
source <(solite completions zsh)

# fish — write a completions file
solite completions fish > ~/.config/fish/completions/solite.fish
```

Run `solite completions --help` for elvish/PowerShell instructions. Completion
uses the dynamic engine (the binary is re-invoked at TAB time), so it always
reflects the installed version with no script to regenerate.