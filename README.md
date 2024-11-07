# sharun
Run dynamically linked ELF binaries everywhere (musl and glibc are supported).

![sharun](img/sharun.gif)

* Its works with [userland-execve](https://github.com/io12/userland-execve-rust) by mapping the interpreter (such as ld-linux-x86-64.so.2) into memory, creating a stack for it (containing the auxiliary vector, arguments, and environment variables), and then jumping to the entry point with the new stack.
* [lib4bin](https://github.com/VHSgunzo/sharun/blob/main/lib4bin) pulls out the binary file and all the libraries on which it depends, strip it and forms the `bin`, `shared/{bin,lib,lib32}` directories (see [screenshots](https://github.com/VHSgunzo/sharun?tab=readme-ov-file#screenshots)) and generate a file `shared/{lib,lib32}/lib.path` with a list of all directories that contain libraries for pass it to interpreter `--library-path`. The paths in this file are specified on a new line with a `+` at the beginning and relative to the directory in which it is located.

## Supported architectures:
* aarch64
* x86_64

## To get started:
* **Download the latest revision**
```
git clone https://github.com/VHSgunzo/sharun.git && cd sharun
```

* **Compile a binary**
```
rustup default nightly
rustup target add $(uname -m)-unknown-linux-musl
rustup component add rust-src --toolchain nightly
cargo build --release
cp ./target/$(uname -m)-unknown-linux-musl/release/sharun .
./sharun --help
./sharun lib4bin --help
```
* Or take an already precompiled binary file from the [releases](https://github.com/VHSgunzo/sharun/releases)

## Usage sharun:
```
[ Usage ]: sharun [OPTIONS] [EXEC ARGS]...
    Use lib4bin for create 'bin' and 'shared' dirs

[ Arguments ]:
    [EXEC ARGS]...          Command line arguments for execution

[ Options ]:
     l,  lib4bin [ARGS]     Launch the built-in lib4bin
    -g,  --gen-lib-path     Generate library path file
    -v,  --version          Print version
    -h,  --help             Print help

[ Environments ]:
    SHARUN_LDNAME=ld.so     Specifies the name of the interpreter
```

## Usage lib4bin:
```
[ Usage ]: lib4bin [options] /path/executable

[ Options ]:
  -s, --strip              Strip binaries and libraries (env: STRIP)
  -v, --verbose            Verbose mode (env: VERBOSE)
  -d, --dst-dir '/path'    Destination directory (env: DST_DIR)
  -n, --not-one-dir        Separate directories for each executable (env: ONE_DIR=0)
  -l, --libs-only          Pack only libraries (env: LIBS_ONLY)
  -w, --with-sharun        Pack sharun from PATH or env or download (env: WITH_SHARUN, SHARUN, SHARUN_URL)
  -p, --hard-links         Create hard links to sharun (env: HARD_LINKS)
  -r, --patch-rpath        Patch RPATH to a relative path (env: PATCH_RPATH)
  -g, --gen-lib-path       Generate a lib.path file (env: GEN_LIB_PATH)
  -a, --any-executable     Pack any executable (env: ANY_EXECUTABLE)
  -i, --patch-interpreter  Patch INTERPRETER to a relative path (env: PATCH_INTERPRETER)
  -q, --quiet-mode         Show only errors (env: QUIET_MODE)
  -h, --help               Show this message
```

## Examples:
```
# run lib4bin with the paths to the binary files that you want to make portable
./sharun lib4bin --with-sharun --dst-dir test /bin/bash

# or for correct /proc/self/exe you can use --hard-links flag
./sharun lib4bin --hard-links --with-sharun --dst-dir test /bin/bash
# this will create hard links from 'test/sharun' in the 'test/bin' directory

# now you can move 'test' dir to other linux system and run binaries from the 'bin' dir
./test/bin/bash --version

# or specify them as an argument to 'sharun'
./test/sharun bash --version
```

# Screenshots:
![tree](img/tree.png)

# Projects that use sharun:
* [pelfCreator](https://github.com/xplshn/pelf/blob/pelf-ng/pelfCreator)
* [AppBundleHUB](https://github.com/xplshn/AppBundleHUB)
* [android-tools-AppImage](https://github.com/Samueru-sama/android-tools-AppImage)
* [pavucontrol-qt-AppImage](https://github.com/Samueru-sama/pavucontrol-qt-AppImage)
* [rofi-AppImage](https://github.com/Samueru-sama/rofi-AppImage)

## References
* [userland-execve](https://crates.io/crates/userland-execve)
* https://brioche.dev/blog/portable-dynamically-linked-packages-on-linux
