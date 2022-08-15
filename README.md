# HackerNews CLI

## Description

Rust CLI that queries [HackerNews API](https://github.com/HackerNews/API). Created to learn about Rust.

## Usage

- Use `cargo run` to run directly from the project's root. To provide  options run `cargo run -- [options]`
- Or create the binary with `cargo install --path <your_path>` and then use `hn [OPTIONS]` as described below

```
HN CLI 1.0
A command line interface for Hacker News

USAGE:
    hn [OPTIONS]

OPTIONS:
    -h, --help                       Print help information
    -l, --length <LENGTH>            The number of stories to retrieve. Should be between 1 and 50
                                     inclusive [default: 10]
    -s, --story-type <STORY_TYPE>    The type of stories to retrieve, can be 'top', 'new' or 'best'
                                     [default: best]
    -V, --version                    Print version information
```