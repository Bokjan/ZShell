# Contributing to ZShell

Thank you for taking the time to help improve ZShell. Bug reports, ideas and questions are all welcome, and most of what has changed in ZShell started as someone's issue.

## How ZShell is developed

ZShell is maintained by one person, and it is source-available rather than open source (see [LICENSE.md](LICENSE.md)): its code is written by the author alone, which keeps its ownership clear. So pull requests are generally not accepted. The way to contribute is a [GitHub issue](https://github.com/Bokjan/ZShell/issues): describe the problem or the idea there, and the maintainer takes it from there. If you would like to change something yourself, please discuss it in an issue first.

## Before you open an issue

- Make sure you have the latest version: Settings › About shows yours and can check for a newer one, and every version is on the [releases page](https://github.com/Bokjan/ZShell/releases).
- Search the existing issues, open and closed. If yours is already there, add what you know to it, or a reaction to show it matters to you too.
- If you have found a security problem, please don't describe it in a public issue. Open one that only says you have found one, and the maintainer will arrange a private way to send the details.

## Reporting a bug

A good report lets the maintainer see the problem for themselves. Please include:

- the ZShell version (Settings › About), and how you installed it: the installer, the portable exe, the Microsoft Store or the macOS disk image;
- your operating system and its version;
- the kind of session: SSH, Telnet, serial or a local terminal, and for SSH how you log in (password, key, agent) and whether a jump host or proxy is involved;
- what you did, what you expected, and what happened instead, with any error message, screenshot or terminal output;
- whether it happens every time, and whether the same thing works with OpenSSH's `ssh` (ZShell follows its behavior where it can).

Leave out passwords, private keys, and host names or addresses you'd rather not share; replacing them with placeholders is fine.

## Suggesting a feature

Any idea is welcome, big or small. If you can, say what you'd use it for, or how you do it today in Xshell, another terminal or with `ssh`: it helps find the best way to get there. The features planned already are in [docs/ROADMAP.md](docs/ROADMAP.md).

## What happens next

The maintainer reads every issue, and may ask questions to understand it. Accepted bugs and features are made in an upcoming version; the people whose issues led to a change are thanked in the [changelog](CHANGELOG.md) and in [THANKS.md](THANKS.md). Not every idea fits ZShell, and when one doesn't, the issue says why.

## Code and text in issues

Code snippets, logs and suggested wording are welcome in issues to illustrate a point. Please note the terms that come with them, from "Feedback and contributions" in [LICENSE.md](LICENSE.md):

> By posting anything in ZShell's issues, including ideas, text and code snippets, you grant the author a perpetual, worldwide, irrevocable, royalty-free license to use, copy, modify, distribute and sublicense it as part of ZShell, under any terms, and you agree not to assert any moral rights against that use, as far as the law allows. Post only what you have the right to share this way. Nothing is owed to you for it.
