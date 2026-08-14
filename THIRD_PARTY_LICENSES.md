# Third-party licences

The simulator itself is licensed under the EUPL v. 1.2, see [LICENSE](LICENSE).
Material from other projects that is checked into this repository keeps its own
licence; this file lists it.

## Agent skills in `.claude/skills/`

The `bevy*` skills and `similarity-rs` are taken from
[chrisgliddon/bevy-skills](https://github.com/chrisgliddon/bevy-skills) and are
used under the MIT licence. They are documentation for AI coding agents and are
not compiled into any binary.

```
MIT License

Copyright (c) 2026 Chris Gliddon

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.
```

The `editor-ui` and `screenshot` skills are project-specific and covered by the
EUPL like the rest of the repository.

## Rust dependencies

Crates pulled in by Cargo are not vendored here; their licences are those
declared in [Cargo.lock](Cargo.lock) and can be listed with
`cargo license` or `cargo about`.
