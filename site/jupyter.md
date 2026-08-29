# Jupyter Kernel

Solite ships a Jupyter kernel so you can work against a SQLite database from
a notebook: cells are SQL statements or [dot commands](/reference/dot),
rendered as interactive HTML tables in the cell output.

## Installing the kernelspec

```
solite jupyter install
```

This registers a "Solite" kernelspec that Jupyter (Notebook, JupyterLab, or
VS Code's notebook UI) can find and launch. Useful flags:

```
solite jupyter install --name my-solite --display "My Solite"  -- custom name/label
solite jupyter install --force                                 -- overwrite an existing kernelspec
solite jupyter list                                             -- see what's installed
solite jupyter uninstall                                        -- remove it
```

`solite jupyter up --connection <file>` starts the kernel itself — Jupyter
invokes this for you; you shouldn't need to run it directly.

Once installed, create a new notebook and pick "Solite" as the kernel. Each
cell runs against the same open database connection, so state (temp tables,
`ATTACH`ed databases, parameters set with `.param`) persists across cells
within a notebook.

## Previewing a table

A cell containing **only** a table or view name — nothing else — renders a
rich preview instead of running as SQL:

```
users
```

```
temp.scratch
```

```
"my table"
```

This is sugar for [`.describe`](/reference/dot#describe): the kernel
checks whether the name exists (in the `main` schema, unless the name is
schema-qualified) and, if so, rewrites the cell to `.describe <name>`
before running it. The notebook's cell history still shows what you typed —
only the executed code changes.

The rewrite only fires when the whole cell is a bare identifier, optionally
schema-qualified, optionally quoted (`` "x" ``, `` `x` ``, `[x]` ), with at
most one trailing `;`. Anything else — a `SELECT`, a comment, extra
whitespace-separated tokens — runs as ordinary SQL. If the name doesn't
exist in `main`, the cell also falls through and runs as SQL, so you see
SQLite's real `no such table` error rather than something invented by the
preview.
