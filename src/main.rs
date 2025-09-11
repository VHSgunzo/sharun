use std::{
    env,
    str::FromStr,
    path::{Path, PathBuf},
    ffi::{CString, OsStr},
    process::{Command, exit},
    fs::{self, File, write, read_to_string},
    os::unix::{fs::{MetadataExt, PermissionsExt, symlink}, process::CommandExt},
    io::{Read, Result, Error, Write, BufRead, BufReader, ErrorKind::{InvalidData, NotFound}}
};

use cfg_if::cfg_if;
use walkdir::WalkDir;
use nix::unistd::{access, AccessFlags};
use goblin::elf::{Elf, program_header::PT_INTERP};


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
    Path::new(path).canonicalize().unwrap_or_default().to_str().unwrap_or_default().to_string()
}

fn basename(path: &str) -> String {
    let pieces: Vec<&str> = path.rsplit('/').collect();
    pieces.first().unwrap_or(&"").to_string()
}

fn dirname(path: &str) -> String {
    let mut pieces: Vec<&str> = path.split('/').collect();
    if pieces.len() == 1 || path.is_empty() {
        // return ".".to_string();
    } else if !path.starts_with('/') &&
        !path.starts_with('.') &&
        !path.starts_with('~') {
            pieces.insert(0, ".");
    } else if pieces.len() == 2 && path.starts_with('/') {
        pieces.insert(0, "");
    };
    pieces.pop();
    pieces.join(&'/'.to_string())
}

fn is_hardlink(path1: &Path, path2: &Path) -> bool {
    if let Ok(metadata1) = path1.metadata() {
        if let Ok(metadata2) = path2.metadata() {
            return metadata1.ino() == metadata2.ino()
        }
    }
    false
}

fn is_same_rootdir(rootdir: &Path, path1: &Path, path2: &Path) -> bool {
    if let Ok(abs_path1) = path1.canonicalize() {
        if let Ok(abs_path2) = path2.canonicalize() {
            if let Ok(abs_rootdir) = &rootdir.canonicalize() {
                return abs_path1.starts_with(abs_rootdir) && abs_path2.starts_with(abs_rootdir)
            }
        }
    }
    false
}

fn is_writable(path: &str) -> bool {
    access(path, AccessFlags::W_OK).is_ok()
}

fn is_dir(path: &str) -> bool {
    Path::new(path).is_dir()
}

fn is_file(path: &Path) -> bool {
    if let Ok(metadata) = path.metadata() {
        return metadata.is_file()
    }
    false
}

fn is_exe(path: &Path) -> bool {
    if let Ok(metadata) = path.metadata() {
        return metadata.is_file() && metadata.permissions().mode() & 0o111 != 0
    }
    false
}

fn which(executable: &str) -> Option<PathBuf> {
    if let Ok(path) = env::var("PATH") {
        for dir in path.split(':') {
            let full_path = Path::new(dir).join(executable);
            if is_exe(&full_path) {
                return Some(full_path)
            }
        }
    }
    None
}

fn is_script(path: &PathBuf) -> Result<bool> {
    let mut file = File::open(path)?;
    let mut buffer = [0; 2];
    file.read_exact(&mut buffer)?;
    Ok(&buffer[0..2] == b"#!")
}

fn read_first_line(path: &PathBuf) -> Result<String> {
    let file = File::open(path)?;
    let mut reader = BufReader::new(file);
    let mut line = String::new();
    reader.read_line(&mut line)?;
    Ok(line)
}

fn exec_script(path: &PathBuf, exec_args: &[String]) -> Result<()> {
    let first_line = read_first_line(path)?;
    if !first_line.starts_with("#!") {
        return Err(Error::new(NotFound, "Script does not have a valid shebang!"))
    }
    let shebang = first_line[2..].trim();
    let parts: Vec<&str> = shebang.split_whitespace().collect();
    if parts.is_empty() {
        return Err(Error::new(NotFound, "Invalid shebang: no interpreter specified!"))
    }
    let interpreter_path = parts[0];
    let mut command = if interpreter_path.ends_with("/env") {
        if parts.len() < 2 {
            return Err(Error::new(NotFound, "No interpreter specified after env!"))
        }
        let interpreter = parts[1];
        let interpreter_path = match which(interpreter) {
            Some(path) => path,
            None => return Err(Error::new(NotFound,
                format!("Interpreter '{interpreter}' not found in PATH"))
            )
        };
        let mut command = Command::new(&interpreter_path);
        for arg in &parts[2..] {
            command.arg(arg);
        }
        command
    } else {
        let interpreter_name = Path::new(interpreter_path)
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .to_string();
        let interpreter_path = match which(&interpreter_name) {
            Some(path) => path,
            None => PathBuf::from(interpreter_path)
        };
        if !interpreter_path.exists() {
            return Err(Error::new(NotFound,
                format!("Interpreter '{}' not found", interpreter_path.display()))
            )
        }
        let mut command = Command::new(&interpreter_path);
        for arg in &parts[1..] {
            command.arg(arg);
        }
        command
    };
    let err = command.arg(path).args(exec_args).exec();
    Err(Error::new(InvalidData, err))
}

#[cfg(feature = "elf32")]
fn is_elf32(path: &String) -> Result<bool> {
    let mut file = File::open(path)?;
    let mut elf_bytes = [0; 5];
    file.read_exact(&mut elf_bytes)?;
    if &elf_bytes[0..4] != b"\x7fELF" {
        return Ok(false)
    }
    Ok(elf_bytes[4] == 1)
}

#[cfg(feature = "pyinstaller")]
fn get_elf(path: &String, is_elf32: bool) -> Result<Vec<u8>> {
    let mut file = File::open(path)?;
    if is_elf32 {
        let mut headers_bytes = Vec::new();
        file.read_to_end(&mut headers_bytes)?;
        Ok(headers_bytes)
    } else {
        let mut elf_header_raw = [0; 64];
        file.read_exact(&mut elf_header_raw)?;
        let section_table_offset = u64::from_le_bytes(elf_header_raw[40..48].try_into().unwrap()); // e_shoff
        let section_count = u16::from_le_bytes(elf_header_raw[60..62].try_into().unwrap()); // e_shnum
        let section_table_size = section_count as u64 * 64;
        let required_bytes = section_table_offset + section_table_size;
        let mut headers_bytes = vec![0; required_bytes as usize];
        std::io::Seek::seek(&mut file, std::io::SeekFrom::Start(0))?;
        file.read_exact(&mut headers_bytes)?;
        Ok(headers_bytes)
    }
}

#[cfg(feature = "pyinstaller")]
fn is_elf_section(elf_bytes: &[u8], section_name: &str) -> Result<bool> {
    if let Ok(elf) = Elf::parse(elf_bytes) {
        if let Some(section_headers) = elf.section_headers.as_slice().get(..) {
            for section_header in section_headers {
                if let Some(name) = elf.shdr_strtab.get_at(section_header.sh_name) {
                    if name == section_name {
                        return Ok(true)
                    }
                }
            }
        }
    }
    Ok(false)
}

fn write_file(elf_path: &String, bytes: &[u8]) -> Result<bool> {
    let mut file = File::create(elf_path)?;
    file.write_all(bytes)?;
    Ok(true)
}

fn set_interp(mut elf_bytes: Vec<u8>, elf_path: &String, new_interp: &str) -> Result<bool> {
    let elf = Elf::parse(&elf_bytes)
        .map_err(|err| Error::new(InvalidData, err))?;
    let interp_header = elf.program_headers.iter().find(|header| header.p_type == PT_INTERP);
    match interp_header {
        Some(header) => {
            let start = header.p_offset as usize;
            let end = start + header.p_filesz as usize;
            let interp_slice = &mut elf_bytes[start..end];
            if interp_slice.last() != Some(&0) {
                return Err(Error::new(InvalidData, "Current INTERP not NUL terminated"));
            }
            if new_interp.len() > (header.p_filesz as usize) - 1 {
                return Err(Error::new(InvalidData, "Current INTERP too small"));
            }
            let new_interp_bytes = new_interp.as_bytes();
            interp_slice[..new_interp_bytes.len()].copy_from_slice(new_interp_bytes);
            for byte in interp_slice.iter_mut().take((header.p_filesz as usize) - 1).skip(new_interp_bytes.len()) { *byte = 0 }
            interp_slice[(header.p_filesz as usize) - 1] = 0;
            write_file(elf_path, &elf_bytes)?;
        }
        None => {
            return Err(Error::new(InvalidData, "Failed to find PT_INTERP header"));
        }
    }
    Ok(true)
}

fn get_env_var<K: AsRef<OsStr>>(key: K) -> String {
    env::var(key).unwrap_or("".into())
}

fn add_to_env<K: AsRef<OsStr>, V: AsRef<OsStr>>(key: K, val: V) {
    let (key, val) = (key.as_ref(), val.as_ref().to_str().unwrap());
    let old_val = get_env_var(key);
    if old_val.is_empty() {
        env::set_var(key, val)
    } else if !old_val.contains(val) {
        env::set_var(key, format!("{val}:{old_val}"))
    }
}

fn read_dotenv(dotenv_dir: &str) -> Vec<String> {
    let mut unset_envs = Vec::new();
    let dotenv_path = PathBuf::from(format!("{dotenv_dir}/.env"));
    if dotenv_path.exists() {
        dotenv::from_path(&dotenv_path).ok();
        let data = read_to_string(&dotenv_path).unwrap_or_else(|err|{
            eprintln!("Failed to read .env file: {}: {err}", dotenv_path.display());
            exit(1)
        });
        for string in data.trim().split("\n") {
            let string = string.trim();
            if string.starts_with("unset ") {
                for var_name in string.split_whitespace().skip(1) {
                    unset_envs.push(var_name.into());
                }
            }
        }
    }
    unset_envs
}

#[cfg(feature = "setenv")]
fn add_to_xdg_data_env(xdg_data_dirs: &str, env: &str, path: &str) {
    for xdg_data_dir in xdg_data_dirs.rsplit(":") {
        let env_data_dir = Path::new(xdg_data_dir).join(path);
        if env_data_dir.exists() {
            add_to_env(env, env_data_dir)
        }
    }
}

fn gen_library_path(library_path: &str, lib_path_file: &String) {
    let mut new_paths: Vec<String> = Vec::new();
    let skip_dirs = ["lib-dynload".to_string()];
    WalkDir::new(library_path)
        .into_iter()
        .filter_map(|entry| entry.ok())
        .for_each(|entry| {
            let name = entry.file_name().to_string_lossy();
            if name.ends_with(".so") || name.contains(".so.") {
                if let Some(parent) = entry.path().parent() {
                    if let Some(parent_str) = parent.to_str() {
                        if parent_str != library_path && parent.is_dir() &&
                            !new_paths.contains(&parent_str.into()) &&
                            !skip_dirs.contains(&basename(parent_str)) {
                            new_paths.push(parent_str.into());
                        }
                    }
                }
            }
        });
    if let Err(err) = write(lib_path_file,
        format!("+:{}", &new_paths.join(":"))
            .replace(":", "\n")
            .replace(library_path, "+")
    ) {
        eprintln!("Failed to write lib.path: {lib_path_file}: {err}");
        exit(1)
    } else {
        eprintln!("Write lib.path: {lib_path_file}")
    }
}

fn print_usage() {
    println!("[ {} ]

[ Usage ]: {SHARUN_NAME} [OPTIONS] [EXEC ARGS]...",
    env!("CARGO_PKG_DESCRIPTION"));
    #[cfg(feature = "lib4bin")]
    println!("     Use lib4bin for create 'bin' and 'shared' dirs");
    println!("
[ Arguments ]:
    [EXEC ARGS]...              Command line arguments for execution

[ Options ]:");
    #[cfg(feature = "lib4bin")]
    println!("     l,  lib4bin [ARGS]         Launch the built-in lib4bin");
    println!("    -g,  --gen-lib-path         Generate a lib.path file
    -v,  --version              Print version
    -h,  --help                 Print help

[ Environments ]:
    SHARUN_WORKING_DIR=/path    Specifies the path to the working directory
    SHARUN_ALLOW_SYS_VKICD=1    Enables breaking system vulkan/icd.d for vulkan loader
    SHARUN_ALLOW_LD_PRELOAD=1   Enables breaking LD_PRELOAD env variable
    SHARUN_PRINTENV=1           Print environment variables to stderr
    SHARUN_LDNAME=ld.so         Specifies the name of the interpreter
    SHARUN_DIR                  Sharun directory");
}

fn main() {
    let sharun = env::current_exe().unwrap();
    let mut exec_args: Vec<String> = env::args().collect();

    let mut sharun_dir = realpath(&get_env_var("SHARUN_DIR"));
    if sharun_dir.is_empty() ||
        !(is_dir(&sharun_dir) && {
            let sharun_dir_path = Path::new(&sharun_dir);
            let sharun_path = sharun_dir_path.join(SHARUN_NAME);
            sharun_dir_path.join("shared").is_dir() && is_exe(&sharun_path) &&
            is_same_rootdir(sharun_dir_path, &sharun, &sharun_path)
        })
    {
        sharun_dir = sharun.parent().unwrap().to_str().unwrap().to_string();
        let lower_dir = &format!("{sharun_dir}/../");
        if basename(&sharun_dir) == "bin" &&
            is_dir(&format!("{lower_dir}shared")) {
            sharun_dir = realpath(lower_dir)
        }
        env::set_var("SHARUN_DIR", &sharun_dir)
    }

    let bin_dir = &format!("{sharun_dir}/bin");
    let shared_dir = &format!("{sharun_dir}/shared");
    let shared_bin = &format!("{shared_dir}/bin");
    let shared_lib = format!("{shared_dir}/lib");
    let shared_lib32 = format!("{shared_dir}/lib32");

    let arg0 = PathBuf::from(exec_args.remove(0));
    let arg0_name = arg0.file_name().unwrap().to_str().unwrap();
    let arg0_dir = PathBuf::from(dirname(arg0.to_str().unwrap())).canonicalize()
        .unwrap_or_else(|_|{
            if let Some(which_arg0) = which(arg0_name) {
                which_arg0.parent().unwrap().to_path_buf()
            } else {
                eprintln!("Failed to find ARG0 dir!");
                exit(1)
            }
    });

    let arg0_path = arg0_dir.join(arg0_name);
    let arg0_full_path = arg0_path.canonicalize().unwrap();
    let arg0_full_path_name = arg0_full_path.file_name().unwrap().to_string_lossy().to_string();
    let mut bin_name = if arg0_path.is_symlink() &&
        arg0_full_path == Path::new(&sharun_dir).join(SHARUN_NAME) {
        arg0_name.into()
    } else if arg0_path.is_symlink() && Path::new(&shared_bin).join(&arg0_full_path_name).exists() {
        arg0_full_path_name
    } else {
        sharun.file_name().unwrap().to_string_lossy().to_string()
    };
    drop(arg0_dir);
    drop(arg0_full_path);

    if bin_name == SHARUN_NAME {
        if !exec_args.is_empty() {
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
                    for library_path in [shared_lib, shared_lib32] {
                        if Path::new(&library_path).exists() {
                            let lib_path_file = &format!("{library_path}/lib.path");
                            gen_library_path(&library_path, lib_path_file)
                        }
                    }
                    return
                }
                #[cfg(feature = "lib4bin")]
                "l" | "lib4bin" => {
                    let lib4bin_compressed = include_file_compress::include_file_compress_deflate!("lib4bin", 9);
                    let mut decoder = flate2::read::DeflateDecoder::new(&lib4bin_compressed[..]);
                    let mut lib4bin = Vec::new();
                    decoder.read_to_end(&mut lib4bin).unwrap();
                    drop(decoder);
                    exec_args.remove(0);
                    add_to_env("PATH", bin_dir);
                    let cmd = Command::new("bash")
                        .env("SHARUN", sharun)
                        .envs(env::vars())
                        .stdin(std::process::Stdio::piped())
                        .arg("-s").arg("--")
                        .args(exec_args)
                        .spawn();
                    match cmd {
                        Ok(mut bash) => {
                            bash.stdin.take().unwrap().write_all(&lib4bin).unwrap_or_else(|err|{
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
                _ => {
                    bin_name = exec_args.remove(0);
                    let bin_path = PathBuf::from(bin_dir).join(&bin_name);
                    if let Ok(bin_full_path) = bin_path.canonicalize() {
                        let bin_full_path_name = bin_full_path.file_name().unwrap().to_string_lossy().to_string();
                        if bin_path.is_symlink() && Path::new(&shared_bin).join(&bin_full_path_name).exists() {
                            bin_name = bin_full_path_name
                        }
                        if is_exe(&bin_full_path) &&
                            (is_hardlink(&sharun, &bin_full_path) ||
                            !Path::new(&shared_bin).join(&bin_name).exists() ||
                            bin_full_path != sharun)
                        {
                            add_to_env("PATH", bin_dir);
                            match is_script(&bin_path) {
                                Ok(true) => {
                                    if let Err(err) = exec_script(&bin_path, &exec_args) {
                                        eprintln!("Error executing script: {err}");
                                        exit(1);
                                    }
                                }
                                Ok(false) => {
                                    let err = Command::new(&bin_path)
                                        .envs(env::vars())
                                        .args(exec_args)
                                        .exec();
                                    eprintln!("Error executing file {:?}: {err}", &bin_path);
                                    exit(1)
                                }
                                Err(err) => {
                                    eprintln!("Error reading file {:?}: {err}", &bin_path);
                                    exit(1)
                                }
                            }
                        }
                    }
                }
            }
        } else {
            eprintln!("Specify the executable from: '{bin_dir}'");
            if let Ok(dir) = Path::new(bin_dir).read_dir() {
                for bin in dir.flatten() {
                    if is_exe(&bin.path()) {
                        println!("{}", bin.file_name().to_str().unwrap())
                    }
                }
            }
            exit(1)
        }
    } else if bin_name == "AppRun" {
        let appname_file = &format!("{sharun_dir}/.app");
        let mut appname: String = "".into();
        if !Path::new(appname_file).exists() {
            if let Ok(dir) = Path::new(&sharun_dir).read_dir() {
                for entry in dir.flatten() {
                    let path = entry.path();
                    if is_file(&path) {
                        let name = entry.file_name();
                        let name = name.to_str().unwrap();
                        if name.ends_with(".desktop") {
                            let data = read_to_string(path).unwrap_or_else(|err|{
                                eprintln!("Failed to read desktop file: {name}: {err}");
                                exit(1)
                            });
                            appname = data.split("\n").filter_map(|string| {
                                if string.starts_with("Exec=") {
                                    Some(string.replace("Exec=", "").split_whitespace().next().unwrap_or("").into())
                                } else {None}
                            }).next().unwrap_or_else(||"".into())
                        }
                    }
                }
            }
        }

        if appname.is_empty() {
            appname = read_to_string(appname_file).unwrap_or_else(|err|{
                eprintln!("Failed to read .app file: {appname_file}: {err}");
                exit(1)
            })
        }

        if let Some(name) = appname.trim().split("\n").next() {
            appname = basename(name)
            .replace("'", "").replace("\"", "")
        } else {
            eprintln!("Failed to get app name: {appname_file}");
            exit(1)
        }
        let app = &format!("{bin_dir}/{appname}");

        add_to_env("PATH", bin_dir);
        if get_env_var("ARGV0").is_empty() {
            env::set_var("ARGV0", &arg0)
        }
        if get_env_var("APPDIR").is_empty() {
            env::set_var("APPDIR", &sharun_dir)
        }

        let err = Command::new(app)
            .envs(env::vars())
            .args(exec_args)
            .exec();
        eprintln!("Failed to run App: {app}: {err}");
        exit(1)
    }
    let bin = format!("{shared_bin}/{bin_name}");

    cfg_if! {
        if #[cfg(feature = "elf32")] {
            let is_elf32_bin = is_elf32(&bin).unwrap_or_else(|err|{
                eprintln!("Failed to check ELF class: {bin}: {err}");
                exit(1)
            });
        } else {
            let is_elf32_bin = false;
        }
    }

    cfg_if! {
        if #[cfg(feature = "pyinstaller")] {
            let elf_bytes = get_elf(&bin, is_elf32_bin).unwrap_or_else(|err|{
                eprintln!("Failed to read ELF: {}: {err}", &bin);
                exit(1)
            });
        } else {
            let elf_bytes = vec![];
        }
    }

    let mut library_path = if is_elf32_bin {
        shared_lib32
    } else {
        shared_lib
    };

    let unset_envs = read_dotenv(&sharun_dir);

    if get_env_var("SHARUN_ALLOW_LD_PRELOAD") != "1" {
        env::remove_var("LD_PRELOAD")
    }
    env::remove_var("SHARUN_ALLOW_LD_PRELOAD");

    fn create_tmp_symlink(var_name: &str, target_path: &str) {
        let link_name = env::var(var_name).unwrap_or_default();
        if !link_name.is_empty() {
            let link_path = PathBuf::from("/tmp").join(&link_name);
            if link_path.exists() {
                if link_path.is_dir() {
                    if let Err(e) = fs::remove_dir_all(&link_path) {
                        eprintln!("Failed to remove existing directory at {}: {}", link_path.display(), e);
                    }
                } else {
                    if let Err(e) = fs::remove_file(&link_path) {
                        eprintln!("Failed to remove existing file at {}: {}", link_path.display(), e);
                    }
                }
            }
            if let Err(e) = symlink(target_path, &link_path) {
                eprintln!("Failed to create symlink from {} to {}: {}", target_path, link_path.display(), e);
            }
        }
        env::remove_var(var_name);
    }
    create_tmp_symlink("SHARUN_TMP_SHARE", &format!("{}/share", sharun_dir));
    create_tmp_symlink("SHARUN_TMP_LIB", &format!("{}/lib", sharun_dir));
    create_tmp_symlink("SHARUN_TMP_BIN", &format!("{}/bin", sharun_dir));

    let interpreter = get_interpreter(&library_path).unwrap_or_else(|_|{
        eprintln!("Interpreter not found!");
        exit(1)
    });

    let working_dir = &get_env_var("SHARUN_WORKING_DIR");
    if !working_dir.is_empty() {
        env::set_current_dir(working_dir).unwrap_or_else(|err|{
            eprintln!("Failed to change working directory: {working_dir}: {err}");
            exit(1)
        });
        env::remove_var("SHARUN_WORKING_DIR")
    }

    #[cfg(feature = "setenv")]
    {
        let gio_launch_desktop = PathBuf::from(&bin_dir).join("gio-launch-desktop");
        if is_exe(&gio_launch_desktop) {
            env::set_var("GIO_LAUNCH_DESKTOP", gio_launch_desktop)
        }
        if let Ok(dir) = PathBuf::from(&library_path).read_dir() {
            for entry in dir.flatten() {
                let entry_path = entry.path();
                if entry_path.is_dir() {
                    let name = entry.file_name();
                    if let Some(name) = name.to_str() {
                        if name.starts_with("girepository-") {
                            env::set_var("GI_TYPELIB_PATH", entry_path)
                        }
                    }
                }
            }
        }
    }

    let lib_path_file = &format!("{library_path}/lib.path");
    if !Path::new(lib_path_file).exists() && is_writable(&library_path) {
        gen_library_path(&library_path, lib_path_file)
    }

    add_to_env("PATH", bin_dir);

    let mut lib_path_data = read_to_string(lib_path_file).unwrap_or_default();

    #[cfg(feature = "setenv")]
    {
        if !lib_path_data.is_empty() {
            let dirs: std::collections::HashSet<&str> = lib_path_data.split("\n").map(|string|{
                string.split("/").nth(1).unwrap_or("")
            }).collect();
            for dir in dirs {
                let dir_path = &format!("{library_path}/{dir}");
                if dir.starts_with("python") && !is_writable(&sharun_dir) {
                    env::set_var("PYTHONDONTWRITEBYTECODE", "1")
                }
                if dir.starts_with("perl") {
                    add_to_env("PERLLIB", dir_path)
                }
                if dir == "gconv" {
                    add_to_env("GCONV_PATH", dir_path)
                }
                if dir == "gio" {
                    let modules = &format!("{dir_path}/modules");
                    if Path::new(modules).exists() {
                        env::set_var("GIO_MODULE_DIR", modules)
                    }
                }
                if dir == "dri" {
                    env::set_var("LIBGL_DRIVERS_PATH", dir_path);
                    add_to_env("LIBVA_DRIVERS_PATH", "/usr/lib/dri");
                    add_to_env("LIBVA_DRIVERS_PATH", "/usr/lib64/dri");
                    #[cfg(target_arch = "x86_64")]
                    add_to_env("LIBVA_DRIVERS_PATH", "/usr/lib/x86_64-linux-gnu/dri");
                    #[cfg(target_arch = "aarch64")]
                    add_to_env("LIBVA_DRIVERS_PATH", "/usr/lib/aarch64-linux-gnu/dri");
                    add_to_env("LIBVA_DRIVERS_PATH", dir_path)
                }
                if dir == "gbm" {
                    env::set_var("GBM_BACKENDS_PATH", dir_path)
                }
                if dir == "xtables" {
                    env::set_var("XTABLES_LIBDIR", dir_path)
                }
                if dir.starts_with("spa-") {
                    env::set_var("SPA_PLUGIN_DIR", dir_path)
                }
                if dir.starts_with("pipewire-") {
                    env::set_var("PIPEWIRE_MODULE_DIR", dir_path)
                }
                if dir.starts_with("gtk-") {
                    add_to_env("GTK_PATH", dir_path);
                    env::set_var("GTK_EXE_PREFIX", &sharun_dir);
                    env::set_var("GTK_DATA_PREFIX", &sharun_dir);
                    for entry in WalkDir::new(dir_path).into_iter().flatten() {
                        let path = entry.path();
                        if is_file(path) && entry.file_name().to_string_lossy() == "immodules.cache" {
                            env::set_var("GTK_IM_MODULE_FILE", path);
                            break
                        }
                    }
                }
                if dir == "folks" {
                    for entry in WalkDir::new(dir_path).into_iter().flatten() {
                        let path = entry.path();
                        if path.is_dir() && entry.file_name().to_string_lossy() == "backends" {
                            env::set_var("FOLKS_BACKEND_PATH", path);
                            break
                        }
                    }
                }
                if dir.starts_with("qt") {
                    let qt_conf = &format!("{bin_dir}/qt.conf");
                    let plugins = &format!("{dir_path}/plugins");
                    if Path::new(plugins).exists() && ! Path::new(qt_conf).exists() {
                        add_to_env("QT_PLUGIN_PATH", plugins)
                    }
                }
                if dir.starts_with("babl-") {
                    env::set_var("BABL_PATH", dir_path)
                }
                if dir.starts_with("gegl-") {
                    env::set_var("GEGL_PATH", dir_path)
                }
                if dir == "libdecor" {
                    let plugins = &format!("{dir_path}/plugins-1");
                    if Path::new(plugins).exists() {
                        env::set_var("LIBDECOR_PLUGIN_DIR", plugins)
                    }
                }
                if dir.starts_with("tcl") && Path::new(&format!("{dir_path}/msgs")).exists() {
                    add_to_env("TCL_LIBRARY", dir_path);
                    let tk = &format!("{library_path}/{}", dir.replace("tcl", "tk"));
                    if Path::new(&tk).exists() {
                        add_to_env("TK_LIBRARY", tk)
                    }
                }
                if dir.starts_with("gstreamer-") {
                    add_to_env("GST_PLUGIN_PATH", dir_path);
                    add_to_env("GST_PLUGIN_SYSTEM_PATH", dir_path);
                    add_to_env("GST_PLUGIN_SYSTEM_PATH_1_0", dir_path);
                    let gst_scanner = &format!("{dir_path}/gst-plugin-scanner");
                    if Path::new(gst_scanner).exists() {
                        env::set_var("GST_PLUGIN_SCANNER", gst_scanner)
                    }
                }
                if dir.starts_with("gdk-pixbuf-") {
                    let mut is_loaders = false;
                    let mut is_loaders_cache = false;
                    for entry in WalkDir::new(dir_path).into_iter().flatten() {
                        let path = entry.path();
                        let name = entry.file_name().to_string_lossy();
                        if name == "loaders" && path.is_dir() {
                            env::set_var("GDK_PIXBUF_MODULEDIR", path);
                            is_loaders = true
                        }
                        if name == "loaders.cache" && is_file(path) {
                            env::set_var("GDK_PIXBUF_MODULE_FILE", path);
                            is_loaders_cache = true
                        }
                        if is_loaders && is_loaders_cache {
                            break
                        }
                    }
                }
            }
        }

        let share_dir = PathBuf::from(format!("{sharun_dir}/share"));
        if share_dir.exists() {
            if let Ok(dir) = share_dir.read_dir() {
                add_to_env("XDG_DATA_DIRS", "/usr/local/share");
                add_to_env("XDG_DATA_DIRS", "/usr/share");
                add_to_env("XDG_DATA_DIRS", "/run/opengl-driver/share");
                add_to_env("XDG_DATA_DIRS", format!("{}/.local/share", get_env_var("HOME")));
                add_to_env("XDG_DATA_DIRS", &share_dir);
                let xdg_data_dirs = &get_env_var("XDG_DATA_DIRS");
                for entry in dir.flatten() {
                    let entry_path = entry.path();
                    if entry_path.is_dir() {
                        let name = entry.file_name();
                        match name.to_str().unwrap() {
                            "glvnd" => {
                                add_to_xdg_data_env(xdg_data_dirs,
                                    "__EGL_VENDOR_LIBRARY_DIRS", "glvnd/egl_vendor.d")
                            }
                            "vulkan" => {
                                let vk_dir = "vulkan/icd.d";
                                let vk_env = "VK_DRIVER_FILES";
                                if get_env_var("SHARUN_ALLOW_SYS_VKICD") == "1" {
                                    env::remove_var("SHARUN_ALLOW_SYS_VKICD");
                                    add_to_xdg_data_env(xdg_data_dirs, vk_env, vk_dir)
                                } else {
                                    for xdg_data_dir in xdg_data_dirs.rsplit(":") {
                                        let vk_icd_dir = Path::new(xdg_data_dir).join(vk_dir);
                                        if vk_icd_dir.exists() {
                                            if xdg_data_dir.starts_with(share_dir.to_str().unwrap()) {
                                                add_to_env(vk_env, vk_icd_dir);
                                            } else if let Ok(dir) = vk_icd_dir.read_dir() {
                                                for entry in dir.flatten() {
                                                    let path = entry.path();
                                                    if is_file(&path) &&
                                                        entry.file_name().to_string_lossy().contains("nvidia") {
                                                        add_to_env(vk_env, path)
                                                    }
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                            "X11" => {
                                let xkb = &entry_path.join("xkb");
                                if !Path::new("/usr/share/X11/xkb").exists() && xkb.exists() {
                                    env::set_var("XKB_CONFIG_ROOT", xkb)
                                }
                            }
                            "libthai" => {
                                if entry_path.join("thbrk.tri").exists() {
                                    env::set_var("LIBTHAI_DICTDIR", entry_path)
                                }
                            }
                            "glib-2.0" => {
                                add_to_xdg_data_env(xdg_data_dirs,
                                    "GSETTINGS_SCHEMA_DIR", "glib-2.0/schemas")
                            }
                            "terminfo" => {
                                env::set_var("TERMINFO", entry_path)
                            }
                            "file" => {
                                let magic_file = &entry_path.join("misc/magic.mgc");
                                if magic_file.exists() {
                                    env::set_var("MAGIC", magic_file)
                                }
                            }
                            _ => {}
                        }
                    }
                }
            }
        }

        let etc_dir = PathBuf::from(format!("{sharun_dir}/etc"));
        if etc_dir.exists() {
            if let Ok(dir) = etc_dir.read_dir() {
                for entry in dir.flatten() {
                    let entry_path = entry.path();
                    if entry_path.is_dir() {
                        let name = entry.file_name();
                        match name.to_str().unwrap() {
                            "fonts" => {
                                let fonts_conf = entry_path.join("fonts.conf");
                                if !Path::new("/etc/fonts/fonts.conf").exists() && fonts_conf.exists() {
                                    env::set_var("FONTCONFIG_FILE", fonts_conf)
                                }
                            }
                            _ => {}
                        }
                    }
                }
            }
        }
    }

    if !lib_path_data.is_empty() {
        lib_path_data = lib_path_data.trim().into();
        library_path = lib_path_data
            .replace("\n", ":")
            .replace("+", &library_path)
    }

    drop(lib_path_data);

    let ld_library_path_env = &get_env_var("LD_LIBRARY_PATH");
    if !ld_library_path_env.is_empty() {
        library_path += &format!(":{ld_library_path_env}")
    }

    for var_name in unset_envs {
        env::remove_var(var_name)
    }

    if get_env_var("SHARUN_PRINTENV") == "1" {
        env::remove_var("SHARUN_PRINTENV");
        for (k, v) in env::vars() {
            eprintln!("{k}={v}")
        }
    }

    cfg_if! {
        if #[cfg(feature = "pyinstaller")] {
            let is_pyinstaller_elf = is_elf_section(&elf_bytes, "pydata").unwrap_or(false);
            let is_pyinstaller_dir = Path::new(&shared_bin).join("_internal").exists();
        } else {
            let is_pyinstaller_elf = false;
            let is_pyinstaller_dir = false;
        }
    }

    let mut interpreter_args: Vec<CString> = Vec::new();
    if !is_pyinstaller_elf || is_pyinstaller_dir || is_elf32_bin {
        interpreter_args.append(&mut vec![
            CString::from_str(&interpreter.to_string_lossy()).unwrap(),
            CString::new("--library-path").unwrap(),
            CString::new(&*library_path).unwrap(),
            CString::new("--argv0").unwrap()
        ]);

        if is_pyinstaller_elf || is_elf32_bin {
            interpreter_args.push(CString::new(&*bin).unwrap())
        } else {
            interpreter_args.push(CString::new(arg0_path.to_str().unwrap()).unwrap())
        }

        let preload_path = PathBuf::from(format!("{sharun_dir}/.preload"));
        if preload_path.exists() {
            let data = read_to_string(&preload_path).unwrap_or_else(|err|{
                eprintln!("Failed to read .preload file: {}: {err}", preload_path.display());
                exit(1)
            });
            let mut preload: Vec<String> = vec![];
            for string in data.trim().split("\n") {
                preload.push(string.trim().into());
            }
            if !preload.is_empty() {
                interpreter_args.append(&mut vec![
                    CString::new("--preload").unwrap(),
                    CString::new(preload.join(" ")).unwrap()
                ])
            }
        }

        interpreter_args.push(CString::new(&*bin).unwrap());
        for arg in &exec_args {
            interpreter_args.push(CString::from_str(arg).unwrap())
        }
    }

    if is_pyinstaller_elf || is_elf32_bin {
        let err = if is_pyinstaller_dir || (!is_pyinstaller_elf && is_elf32_bin) {
            drop(elf_bytes);
            let interpreter_args: Vec<String> = interpreter_args.iter()
                .map(|s| s.clone().into_string().unwrap()).skip(1).collect();
            Command::new(interpreter)
                .args(interpreter_args)
                .envs(env::vars())
                .exec()
        } else {
            set_interp(elf_bytes, &bin, interpreter.to_str().unwrap())
                .unwrap_or_else(|err|{
                    eprintln!("Failed to set ELF interpreter: {}: {err}", &bin);
                    exit(1)
            });
            Command::new(&bin)
                .args(exec_args)
                .envs(env::vars())
                .exec()
        };
        eprint!("Failed to exec: {bin}: {err}");
        exit(1)
    } else {
        drop(elf_bytes);
        let envs: Vec<CString> = env::vars()
            .map(|(key, value)| CString::new(
                format!("{key}={value}")
        ).unwrap()).collect();

        userland_execve::exec(
            interpreter.as_path(),
            &interpreter_args,
            &envs,
        )
    }
}
