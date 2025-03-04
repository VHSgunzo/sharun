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
* You can also use all in one sharun (`sharun-$ARCH-aio`) version which contains all the necessary dependencies for `lib4bin`. See [create_aio_sharun.sh](https://github.com/VHSgunzo/sharun/blob/main/.github/create_aio_sharun.sh)

## Usage sharun:
```
[ Usage ]: sharun [OPTIONS] [EXEC ARGS]...
    Use lib4bin for create 'bin' and 'shared' dirs

[ Arguments ]:
    [EXEC ARGS]...              Command line arguments for execution

[ Options ]:
     l,  lib4bin [ARGS]         Launch the built-in lib4bin
    -g,  --gen-lib-path         Generate a lib.path file
    -v,  --version              Print version
    -h,  --help                 Print help

[ Environments ]:
    SHARUN_WORKING_DIR=/path    Specifies the path to the working directory
    SHARUN_LDNAME=ld.so         Specifies the name of the interpreter
    SHARUN_DIR                  Sharun directory
```

## Usage lib4bin:
```
[ Usage ]: lib4bin [OPTIONS] /path/executable -- [STRACE MODE EXEC ARGS]

[ Options ]:
    -d, --dst-dir '/path'    Destination directory (env: DST_DIR='/path')
    -e, --strace-mode        Use strace for get libs (env: STRACE_MODE=1)
    -t, --strace-time 5      Specify the time in seconds for strace mode (env: STRACE_TIME=5)
    -g, --gen-lib-path       Generate a lib.path file (env: GEN_LIB_PATH=1)
    -h, --help               Show this message
    -i, --patch-interpreter  Patch INTERPRETER to a relative path (env: PATCH_INTERPRETER=1)
    -k, --with-hooks         Pack additional files required for libraries (env: WITH_HOOKS=1)
    -l, --libs-only          Pack only libraries without executables (env: LIBS_ONLY=1)
    -n, --not-one-dir        Separate directories for each executable (env: ONE_DIR=0)
    -p, --hard-links         Pack sharun and create hard links (env: HARD_LINKS=1)
    -q, --quiet-mode         Show only errors (env: QUIET_MODE=1)
    -r, --patch-rpath        Patch RPATH to a relative path (env: PATCH_RPATH=1)
    -s, --strip              Strip binaries and libraries (env: STRIP=1)
    -v, --verbose            Verbose mode (env: VERBOSE=1)
    -w, --with-sharun        Pack sharun from PATH or env or download
                                (env: WITH_SHARUN=1, SHARUN=/path|URL, SHARUN_URL=URL, UPX_SHARUN=1)
    -o, --with-wrappe        Pack with wrappe from PATH or env or download
                                (env: WITH_WRAPPE=1, WRAPPE=/path|URL, WRAPPE_URL=URL)
    -c, --wrappe-clvl 0-22   Specify the compression level for wrappe (env: WRAPPE_CLVL=0-22) (default: 8)
    -x, --wrappe-exec name   Specify the name of the wrappe packaged executable (env: WRAPPE_EXEC=name)
    -m, --wrappe-args 'args' Specify the args for the wrappe packaged executable (env: WRAPPE_ARGS='args')
    -z, --wrappe-dir '/path' Specify path to the sharun dir for packing with wrappe (env: WRAPPE_DIR='/path')
    -u, --wrappe-no-cleanup  Disable cleanup the wrappe unpack directory after exit (env: WRAPPE_CLEANUP=0)
                                It can also be set at runtime (env: STARTPE_CLEANUP=0)
    -y, --with-python        Pack python using uv from PATH or env or download
                                (env: WITH_PYTHON=1, UV=/path|URL, UV_URL=URL)
    -pp, --python-pkg 'pkg'  Specify the python package for packing (env: PYTHON_PKG='pkg')
    -pv, --python-ver 3.12   Specify the python version for packing (env: PYTHON_VER=3.12)
    -pi, --python-pip        Leave pip after install python package (env: PYTHON_LEAVE_PIP=1)
    -pw, --python-wheel      Leave wheel after install python package (env: PYTHON_LEAVE_WHEEL=1)
    -ps, --python-setuptools Leave setuptools after install python package (env: PYTHON_LEAVE_SETUPTOOLS=1)
```

## Examples:
```
# run lib4bin with the paths to the binary files that you want to make portable:
./sharun lib4bin --with-sharun --dst-dir test /bin/bash

# or for correct /proc/self/exe you can use --hard-links flag:
./sharun lib4bin --hard-links --dst-dir test /bin/bash
# this will create hard links from 'test/sharun' in the 'test/bin' directory

# now you can move 'test' dir to other linux system and run binaries from the 'bin' dir:
./test/bin/bash --version

# or specify them as an argument to 'sharun':
./test/sharun bash --version
```
### Packing the `sharun directory` with your applications into a single executable with [wrappe](https://github.com/Systemcluster/wrappe):
```
# packing one executable file /bin/bash to the test/bash executable:
./sharun lib4bin --with-wrappe --dst-dir test /bin/bash

# packing several executable files to the test/sharun multicall executable:
./sharun lib4bin --with-wrappe --dst-dir test /bin/bash /bin/env /bin/ls

# packing several executable files with bash entrypoint to the test/bash executable:
./sharun lib4bin --wrappe-exec bash --dst-dir test /bin/bash /bin/env /bin/ls
```

### Packing the `sharun directory` with your python application into a single executable with [wrappe](https://github.com/Systemcluster/wrappe):
```
# packing python to the test/sharun multicall executable:
./sharun lib4bin --with-python --with-wrappe --strip --dst-dir test

# packing python with python entrypoint to the test/python executable:
./sharun lib4bin --with-python --wrappe-exec python --strip --dst-dir test

# packing python 3.14 and awscli package with aws entrypoint to the test/aws executable in strace mode:
./sharun lib4bin --wrappe-exec aws --strip --with-hooks --python-ver 3.14 --python-pkg awscli --dst-dir test sharun -- aws s3 ls --no-sign-request s3://globalnightlight

# packing python 3.13 and pygame package with examples.aliens entrypoint to the test/python executable in strace mode:
./sharun lib4bin --wrappe-exec python -m '-m pygame.examples.aliens' --strip --with-hooks --python-ver 3.13 --python-pkg pygame --dst-dir test sharun -- python -m pygame.examples.aliens
```

### Packing the [PyInstaller](https://pyinstaller.org) `onedir` app into a single executable with [wrappe](https://github.com/Systemcluster/wrappe):
```
# download python script:
wget https://raw.githubusercontent.com/gdraheim/docker-systemctl-replacement/refs/heads/master/files/docker/systemctl3.py

# Create PyInstaller onedir app:
pyinstaller --name systemctl --onedir systemctl3.py

# download sharun aio:
wget https://github.com/VHSgunzo/sharun/releases/latest/download/sharun-$(uname -m)-aio -O ./sharun
chmod +x ./sharun

# packing PyInstaller onedir app with strace mode into a single portable executable:
./sharun lib4bin --with-wrappe --with-hooks --strip ./dist/systemctl/systemctl -- --help

# test it:
./systemctl --help
```

* You can create a hard link from `sharun` to `AppRun` and write the name of the executable file from the `bin` directory to the `.app` file for compatibility with [AppImage](https://appimage.org) `AppDir`. If the `.app` file does not exist, the `*.desktop` file will be used.

* Additional env var can be specified in the `.env` file (see [dotenv](https://crates.io/crates/dotenv)). Env var can also be deleted using `unset ENV_VAR` in the end of the `.env` file.

* You can preload libraries using `.preload` file. Specify the necessary libraries in it from a new line. You can use the full paths to libraries or only their names if they are located in `shared/{lib,lib32}/`
This can be useful, for example, to use [ld-preload-open](https://github.com/fritzw/ld-preload-open) library to reassign paths.

## Screenshots:
![tree](img/tree.png)

## Environment variables that are set if sharun finds a directory or file:
|||
|---|---|
|`PATH` | `${SHARUN_DIR}/bin` |
|`PYTHONHOME` and `PYTHONDONTWRITEBYTECODE` | `${SHARUN_DIR}/shared/$LIB/python*` |
|`PERLLIB` | `${SHARUN_DIR}/shared/$LIB/perl*` |
|`GCONV_PATH` | `${SHARUN_DIR}/shared/$LIB/gconv` |
|`GIO_MODULE_DIR` | `${SHARUN_DIR}/shared/$LIB/gio/modules`|
|`GTK_PATH`, `GTK_EXE_PREFIX` and `GTK_DATA_PREFIX` | `${SHARUN_DIR}/shared/$LIB/gtk-*`|
|`QT_PLUGIN_PATH` | `${SHARUN_DIR}/shared/$LIB/qt*/plugins`|
|`BABL_PATH` | `${SHARUN_DIR}/shared/$LIB/babl-*`|
|`GEGL_PATH` | `${SHARUN_DIR}/shared/$LIB/gegl-*`|
|`GIMP2_PLUGINDIR` | `${SHARUN_DIR}/shared/$LIB/gimp/2.0`|
|`TCL_LIBRARY` | `${SHARUN_DIR}/shared/$LIB/tcl*`|
|`TK_LIBRARY` | `${SHARUN_DIR}/shared/$LIB/tk*`|
|`GST_PLUGIN_PATH`, `GST_PLUGIN_SYSTEM_PATH`, `GST_PLUGIN_SYSTEM_PATH_1_0`, and `GST_PLUGIN_SCANNER` | `${SHARUN_DIR}/shared/$LIB/gstreamer-*`|
|`GDK_PIXBUF_MODULEDIR` and `GDK_PIXBUF_MODULE_FILE` | `${SHARUN_DIR}/shared/$LIB/gdk-pixbuf-*`|
|`LIBDECOR_PLUGIN_DIR` | `${SHARUN_DIR}/shared/$LIB/libdecor/plugins-1`|
|`GTK_IM_MODULE_FILE` | `${SHARUN_DIR}/shared/$LIB/gtk-*/*/immodules.cache`|
|`LIBGL_DRIVERS_PATH` | `${SHARUN_DIR}/shared/$LIB/dri`|
|`SPA_PLUGIN_DIR` | `${SHARUN_DIR}/shared/$LIB/spa-*`|
|`PIPEWIRE_MODULE_DIR` | `${SHARUN_DIR}/shared/$LIB/pipewire-*`|
|||
|---|---|
|`XDG_DATA_DIRS` | `${SHARUN_DIR}/share`|
|`VK_DRIVER_FILES` | `${SHARUN_DIR}/share/vulkan/icd.d`|
|`__EGL_VENDOR_LIBRARY_DIRS` | `${SHARUN_DIR}/share/glvnd/egl_vendor.d`|
|`XKB_CONFIG_ROOT` | `${SHARUN_DIR}/share/X11/xkb`|
|`GSETTINGS_SCHEMA_DIR` | `${SHARUN_DIR}/share/glib-2.0/schemas`|
|`GIMP2_DATADIR` | `${SHARUN_DIR}/share/gimp/2.0`|
|`TERMINFO` | `${SHARUN_DIR}/share/terminfo`|
|`MAGIC` | `${SHARUN_DIR}/share/file/misc/magic.mgc`|
|||
|---|---|
|`FONTCONFIG_FILE` | `${SHARUN_DIR}/etc/fonts/fonts.conf`|
|`GIMP2_SYSCONFDIR` | `${SHARUN_DIR}/etc/gimp/2.0`|

## Projects that use sharun:
* [SoarPkgs](https://github.com/pkgforge/soarpkgs)
* [pelfCreator](https://github.com/xplshn/pelf/blob/pelf-ng/pelfCreator)
* [AppBundleHUB](https://github.com/xplshn/AppBundleHUB)
* [AnyLinux-AppImages](https://github.com/pkgforge-dev/Anylinux-AppImages)
* [RMG](https://github.com/Rosalie241/RMG)
* [PrusaSlicer.AppImage](https://github.com/probonopd/PrusaSlicer.AppImage)
* [goverlay](https://github.com/benjamimgois/goverlay)
* [ghostty-appimage](https://github.com/psadi/ghostty-appimage)
* [interstellar](https://github.com/jwr1/interstellar)
* [LibreSprite](https://github.com/LibreSprite/LibreSprite)

## References
* [userland-execve](https://crates.io/crates/userland-execve)
* https://brioche.dev/blog/portable-dynamically-linked-packages-on-linux
