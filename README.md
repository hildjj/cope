# cope

This crate provides a small wrapper around VS Code's command line, called
[code](https://code.visualstudio.com/docs/configure/command-line), allowing
the user to open files and directories that have a
[devcontainer](https://code.visualstudio.com/docs/devcontainers/create-dev-container)
defined.

## Install

    cargo install --git https://github.com/hildjj/cope.git

## Usage

    cope -h # Gets help from code

Most `code` command lines should work by changing `code` to `cope`.  If you want
to see the actual command line that cope is executing, set the COPE_VERBOSE
environment variable:

    COPE_VERBOSE=1 cope .

## Known limitations

- `--goto` does not work with devocontainers yet
- `chat`, `serve-web`, and `tunnel` do not work yet
