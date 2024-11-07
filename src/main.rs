use std::{
    env,
    ffi::CString,
    str::FromStr,
    collections::HashSet,
    path::{Path, PathBuf},
    process::{Command, Stdio, exit},
    io::{Read, Result, Error, Write},
    fs::{File, write, read_to_string}
};

use walkdir::WalkDir;


const SHARUN_NAME: &str = env!("CARGO_PKG_NAME");


fn get_interpreter(library_path: &str) -> Result<PathBuf> {
    let mut interpreters = Vec::new();
    if let Ok(ldname) = env::var("SHARUN_LDNAME") {
        if !ldname.is_empty() {
            interpreters.push(ldname)
        }
    } else {
        #[cfg(target_arch = "x86_64")]          // target x86_64-unknown-linux-musl
        interpreters.append(&mut vec![
            "ld-linux-x86-64.so.2".into(),
            "ld-musl-x86_64.so.1".into(),
            "ld-linux.so.2".into()
        ]);
        #[cfg(target_arch = "aarch64")]         // target aarch64-unknown-linux-musl
        interpreters.append(&mut vec![
            "ld-linux-aarch64.so.1".into(),
            "ld-musl-aarch64.so.1".into()
        ]);
    }
    for interpreter in interpreters {
        let interpreter_path = Path::new(library_path).join(interpreter);
        if interpreter_path.exists() {
            return Ok(interpreter_path)
        }
    }
    Err(Error::last_os_error())
}

fn realpath(path: &str) -> String {
    Path::new(path).canonicalize().unwrap().to_str().unwrap().to_string()
}

fn basename(path: &str) -> String {
    let pieces: Vec<&str> = path.rsplit('/').collect();
    pieces.get(0).unwrap().to_string()
}

fn is_file(path: &str) -> bool {
    let path = Path::new(path);
    path.is_file()
}

fn is_elf32(file_path: &str) -> Result<bool> {
    let mut file = File::open(file_path)?;
    let mut buff = [0u8; 5];
    file.read_exact(&mut buff)?;
    if &buff[0..4] != b"\x7fELF" {
        return Ok(false)
    }
    Ok(buff[4] == 1)
}

fn gen_library_path(library_path: &mut String) -> i32 {
    let lib_path_file = format!("{library_path}/lib.path");
    let old_library_path = library_path.clone();
    let mut added_dirs = HashSet::new();
    let mut new_paths = Vec::new();
    WalkDir::new(&mut *library_path)
        .into_iter()
        .filter_map(|entry| entry.ok())
        .for_each(|entry| {
            let name = entry.file_name().to_string_lossy();
            if name.ends_with(".so") || name.contains(".so.") {
                if let Some(parent) = entry.path().parent() {
                    if let Some(parent_str) = parent.to_str() {
                        if parent_str != library_path && parent.is_dir() &&
                            !added_dirs.contains(parent_str) {
                            added_dirs.insert(parent_str.to_string());
                            new_paths.push(parent_str.to_string());
                        }
                    }
                }
            }
        });
    if !new_paths.is_empty() {
        library_path.push(':');
        library_path.push_str(&new_paths.join(":"))
    }
    if let Err(err) = write(&lib_path_file, &library_path
        .replace(":", "\n")
        .replace(&old_library_path, "+")
    ) {
        eprintln!("Failed to write lib path: {lib_path_file}: {err}"); 1
    } else {
        eprintln!("Write lib path: {lib_path_file}"); 0
    }
}

fn strip_str(str: &str) -> String {
    str.lines()
    .map(|line| line.trim_start())
    .collect::<Vec<_>>()
    .join("\n")
}

fn print_usage() {
    println!("{}", strip_str(&format!("[ {} ]
        |
        [ Usage ]: {SHARUN_NAME} [OPTIONS] [EXEC ARGS]...
        |  Use lib4bin for create 'bin' and 'shared' dirs
        |
        [ Arguments ]:
        |  [EXEC ARGS]...          Command line arguments for execution
        |
        [ Options ]:
        |   l,  lib4bin [ARGS]     Launch the built-in lib4bin
        |  -g,  --gen-lib-path     Generate library path file
        |  -v,  --version          Print version
        |  -h,  --help             Print help
        |
        [ Environments ]:
        |  SHARUN_LDNAME=ld.so     Specifies the name of the interpreter",
    env!("CARGO_PKG_DESCRIPTION"))));
}

fn main() {
    let sharun = env::current_exe().unwrap();
    let lib4bin = include_bytes!("../lib4bin");
    let mut exec_args: Vec<String> = env::args().collect();

    let mut sharun_dir = sharun.parent().unwrap().to_str().unwrap().to_string();
    let lower_dir = format!("{sharun_dir}/../");
    if basename(&sharun_dir) == "bin" &&
       is_file(&format!("{lower_dir}{SHARUN_NAME}")) {
        sharun_dir = realpath(&lower_dir)
    }

    let shared_dir = format!("{sharun_dir}/shared");
    let shared_bin = format!("{shared_dir}/bin");
    let shared_lib = format!("{shared_dir}/lib");
    let shared_lib32 = format!("{shared_dir}/lib32");

    let mut bin_name = basename(&exec_args.remove(0));
    if bin_name == SHARUN_NAME {
        if exec_args.len() > 0 {
            match exec_args[0].as_str() {
                "-v" | "--version" => {
                    println!("v{}", env!("CARGO_PKG_VERSION"));
                    return
                }
                "-h" | "--help" => {
                    print_usage();
                    return
                }
                "-g" | "--gen-lib-path" => {
                    let mut ret = 1;
                    for mut library_path in [shared_lib, shared_lib32] {
                        if Path::new(&library_path).exists() {
                            ret = gen_library_path(&mut library_path)
                        }
                    }
                    exit(ret)
                }
                "l" | "lib4bin" => {
                    exec_args.remove(0);
                    let cmd = Command::new("bash")
                        .env("SHARUN", sharun)
                        .envs(env::vars())
                        .stdin(Stdio::piped())
                        .arg("-s").arg("--")
                        .args(exec_args)
                        .spawn();
                    match cmd {
                        Ok(mut bash) => {
                            bash.stdin.take().unwrap().write_all(lib4bin).unwrap_or_else(|err|{
                                eprintln!("Failed to write lib4bin to bash stdin: {err}");
                                exit(1)
                            });
                            exit(bash.wait().unwrap().code().unwrap())
                        }
                        Err(err) => {
                            eprintln!("Failed to run bash: {err}");
                            exit(1)
                        }
                    }

                }
                _ => { bin_name = exec_args.remove(0) }
            }
        } else {
            eprintln!("Specify the executable from the 'shared/bin' dir!");
            if let Ok(bin_dir) = Path::new(&shared_bin).read_dir() {
                for bin in bin_dir {
                    println!("{}", bin.unwrap().file_name().to_str().unwrap())
                }
            }
            exit(1)
        }
    }
    let bin = format!("{shared_bin}/{bin_name}");

    let is_elf32_bin = is_elf32(&bin).unwrap_or_else(|err|{
        eprintln!("Failed to check ELF class: {bin}: {err}");
        exit(1)
    });

    let mut library_path: String;
    if is_elf32_bin {
        library_path = shared_lib32
    } else {
        library_path = shared_lib
    }

    let interpreter = get_interpreter(&library_path).unwrap_or_else(|_|{
        eprintln!("The interpreter was not found!");
        exit(1)
    });

    let lib_path_file = format!("{library_path}/lib.path");
    if Path::new(&lib_path_file).exists() {
        library_path = read_to_string(lib_path_file).unwrap().trim()
            .replace("\n", ":")
            .replace("+", &library_path)
    } else {
        gen_library_path(&mut library_path);
    }

    let envs: Vec<CString> = env::vars()
        .map(|(key, value)| CString::new(
            format!("{}={}", key, value)
    ).unwrap()).collect();

    let mut interpreter_args = vec![
        CString::from_str(&interpreter.to_string_lossy()).unwrap(),
        CString::new("--library-path").unwrap(),
        CString::new(library_path).unwrap(),
        CString::new(bin).unwrap()
    ];
    for arg in exec_args {
        interpreter_args.push(CString::from_str(&arg).unwrap())
    }

    userland_execve::exec(
        interpreter.as_path(),
        &interpreter_args,
        &envs,
    )
}
