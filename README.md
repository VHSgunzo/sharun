# sharun
Run dynamically linked ELF binaries everywhere (musl and glibc are supported)

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
rustup target add x86_64-unknown-linux-musl
rustup component add rust-src --toolchain nightly
cargo build --release
```
* Or take an already precompiled binary file from the [releases](https://github.com/VHSgunzo/sharun/releases)

## Usage:
```
[ Usage ]: sharun [OPTIONS] [EXEC ARGS]...
|  Use lib4bin for create 'bin' and 'shared' dirs
|
[ Arguments ]:
|  [EXEC ARGS]...          Command line arguments for execution
|
[ Options ]:
|  -g,  --gen-lib-path     Generate library path file
|  -v,  --version          Print version
|  -h,  --help             Print help
|
[ Environments ]:
|  SHARUN_LDNAME=ld.so     Specifies the name of the linker
```

## Examples:
```
# create a directory and cd
mkdir test && cd test

# run lib4bin with the paths to the binary files that you want to make portable
../lib4bin /bin/{curl,bash,ls}

# and copy sharun to this directory
cp ../target/x86_64-unknown-linux-musl/release/sharun .

# now you can move 'test' dir to other linux system and run binaries from the 'bin' dir
./bin/ls -lha

# or specify them as an argument to sharun
./sharun ls -lha
```
