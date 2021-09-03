# gqlmapi-rs

The C++ portion of this crate is based on [Electron-GqlMAPI](https://github.com/microsoft/electron-gqlmapi).
Unlike that project, there is no V8 engine or Node interoperability/threading requirement. I wrote just enough
support code and state management for that API projection though, that I decided to use it as the basis for
an API projection to [Rust](https://www.rust-lang.org/).

The next layer in the [Electron](https://www.electronjs.org/) stack is
[eMAPI](https://github.com/microsoft/eMAPI). The next layer in this stack will probably be a
[Tauri](https://tauri.studio/en) app that does the same thing but in a much more lightweight fashion than
`Electron`.

## Getting Started

This project only builds on Windows, and I've only tested it with x64 builds. It requires that you have
[CMake](https://cmake.org/) installed, the version included with Visual Studio 2019 works fine. It also uses
the [vcpkg](https://github.com/microsoft/vcpkg) package manager for the dependencies. At a minimum, you need
to build/install `cppgraphqlgen` with `vcpkg` for your target triplet, e.g.:

```cmd
> vcpkg install cppgraphqlgen:x64-windows-static
```

You will need to set an environment variable to tell [build.rs](./build.rs) where to find it, or install the
user-wide vcpkg integration before building this crate. In this example, I have `vcpkg` in a subdirectory
called `source\repos\microsoft\vcpkg` under my user profile, and I'm targetting `x64-windows-static`, so I
don't need to copy any DLLs from vcpkg:

```cmd
> set VCPKG_ROOT=%USERPROFILE%\source\repos\microsoft\vcpkg
```

or:

```cmd
> vcpkg integrate install
```

The `build.rs` script determines the target `x64-windows-static` or `x86-windows-static` platform based on
the Rust target.

Make sure you have also cloned the `gqlmapi` sub-module. If you did not clone this repo recursively, you
can still pull down the sub-module with a couple of git commands:

```cmd
> git submodule init
> git submodule update
```

There is a temporary dependency on my fork with [dtolnay/cxx#927](https://github.com/dtolnay/cxx/pull/927),
so it will install `cxx` with `git`. The `cxx` build has a quirk that it depends on the git configuration
if you are building it from a repo instead of the packaged crate:

> ~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~
> When building `cxx` from a git clone, git's symlink support needs
> to be enabled on platforms that have it off by default (Windows).
> Either use:
> 
>    $ git config --global core.symlinks true
> 
> prior to cloning, or else use:
> 
>    $ git clone -c core.symlinks=true https://github.com/dtolnay/cxx
> 
> for the clone.
> 
> Symlinks are only required when compiling locally from a clone of
> the git repository---they are NOT required when building `cxx` as
> a Cargo-managed (possibly transitive) build dependency downloaded
> through crates.io.
> ~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~

Since `cargo` is the one making the clone (and it's from my fork), you need to change the global setting
before you run `cargo build`:

```cmd
> git config --global core.symlinks true
```

After that, you should be ready to build with `cargo build`.

## Dependencies

- [Microsoft Outlook](https://en.wikipedia.org/wiki/Microsoft_Outlook) for runtime MAPI support
- [gqlmapi](https://github.com/microsoft/gqlmapi) for the C++ GraphQL bindings to MAPI
- [vcpkg](https://github.com/microsoft/vcpkg) package manager for the GqlMAPI C++ dependencies
- [cmake](https://docs.rs/crate/cmake/0.1.45) crate to automatically build gqlmapi
- [cxx](https://docs.rs/crate/cxx/1.0.54) crate to generate the C++ bindings
