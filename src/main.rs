use std::{
    fs::File,
    io::Read,
    {env, fs},
    path::Path,
    str::FromStr,
    ffi::CString,
    process::exit,
    collections::HashSet
};

use walkdir::WalkDir;


const SHARUN_NAME: &str = env!("CARGO_PKG_NAME");
const LINKER_NAME: &str = "ld-linux-x86-64.so.2";


fn get_linker_name(library_path: &str) -> String {
    #[cfg(target_arch = "x86_64")] // target x86_64-unknown-linux-musl
    let linkers = vec![
        "ld-linux-x86-64.so.2",
        "ld-musl-x86_64.so.1",
        "ld-linux.so.2",
    ];
    #[cfg(target_arch = "aarch64")] // target aarch64-unknown-linux-musl
    let linkers = vec![
        "ld-linux-aarch64.so.1",
        "ld-musl-aarch64.so.1",
    ];
    for linker in linkers {
        let linker_path = Path::new(library_path).join(linker);
        if linker_path.exists() {
            return linker_path.file_name().unwrap().to_str().unwrap().to_string()
        }
    }
    LINKER_NAME.to_string()
}

fn realpath(path: &str) -> String {
    Path::new(path).canonicalize().unwrap().to_str().unwrap().to_string()
}

fn basename(path: &str) -> String {
    let pieces: Vec<&str> = path.rsplit('/').collect();
    return pieces.get(0).unwrap().to_string();
}

fn is_file(path: &str) -> bool {
    let path = Path::new(path);
    path.is_file()
}

fn is_elf32(file_path: &str) -> std::io::Result<bool> {
    let mut file = File::open(file_path)?;
    let mut buff = [0u8; 5];
    file.read_exact(&mut buff)?;
    if &buff[0..4] != b"\x7fELF" {
        return Ok(false)
    }
    Ok(buff[4] == 1)
}

fn scan_library_path(library_path: &mut String) {
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
        library_path.push_str(&new_paths.join(":"));
    }
}

fn main() {
    let sharun = env::current_exe().unwrap();
    let mut sharun_dir = sharun.parent().unwrap().to_str().unwrap().to_string();
    let lower_dir = format!("{sharun_dir}/../");
    if basename(&sharun_dir) == "bin" && 
       is_file(&format!("{lower_dir}{SHARUN_NAME}")) {
        sharun_dir = realpath(&lower_dir);
    }

    let mut exec_args: Vec<String> = env::args().collect();

    let shared_dir = format!("{sharun_dir}/shared");
    let shared_bin = format!("{shared_dir}/bin");

    let mut bin_name = basename(&exec_args.remove(0));
    if bin_name == SHARUN_NAME {
        if exec_args.len() > 0 {
            match exec_args[0].as_str() {
                "-v" | "--version" => {
                    eprintln!("v{}", env!("CARGO_PKG_VERSION"));
                    return
                }
                "-h" | "--help" => {
                    eprintln!("Use lib4bin for create 'bin' and 'shared' dirs!");
                    return
                }
                _ => { bin_name = exec_args.remove(0) }
            }
        } else {
            eprintln!("Specify the executable from the 'shared/bin' dir!");
            if let Ok(bin_dir) = Path::new(&shared_bin).read_dir() {
                for bin in bin_dir {
                    eprintln!("{}", bin.unwrap().file_name().to_str().unwrap())
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
        library_path = format!("{shared_dir}/lib32")
    } else {
        library_path = format!("{shared_dir}/lib")
    }

    let linker_name = env::var("SHARUN_LDNAME")
        .unwrap_or(get_linker_name(&library_path));
    let linker = &format!("{library_path}/{linker_name}");

    if !Path::new(linker).exists() {
        eprintln!("Linker not found: {linker}");
        exit(1)
    }


    let path_file = format!("{library_path}/lib.path");
    if Path::new(&path_file).exists() {
        library_path = fs::read_to_string(path_file).unwrap().trim()
            .replace("\n", ":")
            .replace("+", &library_path)
    } else {
        let old_library_path = library_path.clone();
        scan_library_path(&mut library_path);
        // if let Ok(entries) = Path::new(&library_path).read_dir() {
        //     for entry in entries {
        //         let item = entry.unwrap();
        //         if item.file_type().unwrap().is_dir() {
        //             library_path.push_str(&format!(":{}", item.path().to_str().unwrap()));
        //         }
        //     }
        // } else {
        //     eprintln!("Failed to read dir: {library_path}");
        //     exit(1)
        // }
        let _ = fs::write(path_file, &library_path
            .replace(":", "\n")
            .replace(&old_library_path, "+")
        );
    }

    let envs: Vec<CString> = env::vars()
        .map(|(key, value)| CString::new(
            format!("{}={}", key, value)
    ).unwrap()).collect();

    let mut linker_args = vec![
        CString::from_str(linker).unwrap(),
        CString::new("--library-path").unwrap(),
        CString::new(library_path).unwrap(),
        CString::new(bin).unwrap()
    ];
    for arg in exec_args {
        linker_args.push(CString::from_str(&arg).unwrap())
    }

    userland_execve::exec(
        Path::new(linker),
        &linker_args,
        &envs,
    )
}