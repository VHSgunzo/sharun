use std::{
    env, fs, io,
    ffi::CString,
    str::FromStr,
    collections::HashSet,
    path::{Path, PathBuf},
    process::{Command, Stdio, exit},
    io::{Read, Result, Error, Write},
    fs::{File, write, read_to_string},
    os::unix::{fs::{MetadataExt, PermissionsExt}, process::CommandExt}
};

use which::which;
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
    pieces.first().unwrap().to_string()
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

fn is_file(path: &str) -> bool {
    let path = Path::new(path);
    path.is_file()
}

fn is_exe(file_path: &PathBuf) -> Result<bool> {
    let metadata = fs::metadata(file_path)?;
    Ok(metadata.is_file() && metadata.permissions().mode() & 0o111 != 0)
}

fn is_elf(file_path: &PathBuf) -> Result<bool> {
    let mut file = File::open(file_path)?;
    let mut buff = [0u8; 4];
    file.read_exact(&mut buff)?;
    Ok(&buff == b"\x7fELF")
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

fn get_env_var(var: &str) -> String {
    env::var(var).unwrap_or("".into())
}

fn add_to_env(var: &str, val: &str) {
    let old_val = get_env_var(var);
    if old_val.is_empty() {
        env::set_var(var, val)
    } else if !old_val.contains(val) {
        env::set_var(var, format!("{val}:{old_val}"))
    }
}

fn read_dotenv(dotenv_dir: &str) {
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
                    env::remove_var(var_name)
                }
            }
        }
    }
}

fn is_hardlink(file1: &Path, file2: &Path) -> io::Result<bool> {
    let metadata1 = fs::metadata(file1)?;
    let metadata2 = fs::metadata(file2)?;
    Ok(metadata1.ino() == metadata2.ino())
}

fn gen_library_path(library_path: &mut String, lib_path_file: &String) {
    let mut new_paths: Vec<String> = Vec::new();
    WalkDir::new(&mut *library_path)
        .into_iter()
        .filter_map(|entry| entry.ok())
        .for_each(|entry| {
            let name = entry.file_name().to_string_lossy();
            if name.ends_with(".so") || name.contains(".so.") {
                if let Some(parent) = entry.path().parent() {
                    if let Some(parent_str) = parent.to_str() {
                        if parent_str != library_path && parent.is_dir() &&
                            !new_paths.contains(&parent_str.into()) {
                            new_paths.push(parent_str.into());
                        }
                    }
                }
            }
        });
    if let Err(err) = write(lib_path_file,
        format!("+:{}", &new_paths.join(":"))
            .replace(":", "\n")
            .replace(&*library_path, "+")
    ) {
        eprintln!("Failed to write lib.path: {lib_path_file}: {err}");
        exit(1)
    } else {
        eprintln!("Write lib.path: {lib_path_file}")
    }
}

fn print_usage() {
    println!("[ {} ]

[ Usage ]: {SHARUN_NAME} [OPTIONS] [EXEC ARGS]...
    Use lib4bin for create 'bin' and 'shared' dirs

[ Arguments ]:
    [EXEC ARGS]...              Command line arguments for execution

[ Options ]:
     l,  lib4bin [ARGS]         Launch the built-in lib4bin
    -g,  --gen-lib-path         Generate library path file
    -v,  --version              Print version
    -h,  --help                 Print help

[ Environments ]:
    SHARUN_WORKING_DIR=/path    Specifies the path to the working directory
    SHARUN_LDNAME=ld.so         Specifies the name of the interpreter
    SHARUN_DIR                  Sharun directory",
    env!("CARGO_PKG_DESCRIPTION"));
}

fn main() {
    let lib4bin = include_bytes!("../lib4bin");

    let sharun: PathBuf = env::current_exe().unwrap();
    let mut exec_args: Vec<String> = env::args().collect();

    let mut sharun_dir = sharun.parent().unwrap().to_str().unwrap().to_string();
    let lower_dir = &format!("{sharun_dir}/../");
    if basename(&sharun_dir) == "bin" &&
       is_file(&format!("{lower_dir}{SHARUN_NAME}")) {
        sharun_dir = realpath(lower_dir)
    }

    env::set_var("SHARUN_DIR", &sharun_dir);

    let bin_dir = &format!("{sharun_dir}/bin");
    let shared_dir = &format!("{sharun_dir}/shared");
    let shared_bin = &format!("{shared_dir}/bin");
    let shared_lib = format!("{shared_dir}/lib");
    let shared_lib32 = format!("{shared_dir}/lib32");

    let arg0 = PathBuf::from(exec_args.remove(0));
    let arg0_name = arg0.file_name().unwrap();
    let arg0_dir = PathBuf::from(dirname(arg0.to_str().unwrap())).canonicalize()
        .unwrap_or_else(|_|{
            if let Ok(which_arg0) = which(arg0_name) {
                which_arg0.parent().unwrap().to_path_buf()
            } else {
                eprintln!("Failed to find ARG0 dir!");
                exit(1)
            }
    });
    let arg0_path = arg0_dir.join(arg0_name);

    let mut bin_name = if arg0_path.is_symlink() && arg0_path.canonicalize().unwrap() == sharun {
        arg0_name.to_str().unwrap().into()
    } else {
        basename(sharun.file_name().unwrap().to_str().unwrap())
    };

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
                    for mut library_path in [shared_lib, shared_lib32] {
                        if Path::new(&library_path).exists() {
                            let lib_path_file = &format!("{library_path}/lib.path");
                            gen_library_path(&mut library_path, lib_path_file)
                        }
                    }
                    return
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
                _ => {
                    bin_name = exec_args.remove(0);
                    let bin_path = PathBuf::from(bin_dir).join(&bin_name);
                    let is_exe = is_exe(&bin_path).unwrap_or(false);
                    let is_elf = is_elf(&bin_path).unwrap_or(false);
                    let is_hardlink = is_hardlink(&sharun, &bin_path).unwrap_or(false);
                    if is_exe && (is_hardlink || !is_elf) {
                        let err = Command::new(&bin_path)
                            .envs(env::vars())
                            .args(exec_args)
                            .exec();
                        eprintln!("Failed to run: {}: {err}", bin_path.display());
                        exit(1)
                    }
                }
            }
        } else {
            eprintln!("Specify the executable from: '{bin_dir}'");
            if let Ok(dir) = Path::new(bin_dir).read_dir() {
                for bin in dir.flatten() {
                    if is_exe(&bin.path()).unwrap_or(false) {
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
                    if path.is_file() {
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

        if get_env_var("ARGV0").is_empty() {
            env::set_var("ARGV0", &arg0)
        }
        env::set_var("APPDIR", &sharun_dir);

        let err = Command::new(app)
            .envs(env::vars())
            .args(exec_args)
            .exec();
        eprintln!("Failed to run App: {app}: {err}");
        exit(1)
    }
    let bin = format!("{shared_bin}/{bin_name}");

    let is_elf32_bin = is_elf32(&bin).unwrap_or_else(|err|{
        eprintln!("Failed to check ELF class: {bin}: {err}");
        exit(1)
    });

    let mut library_path = if is_elf32_bin {
        shared_lib32
    } else {
        shared_lib
    };

    read_dotenv(&sharun_dir);

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

    let etc_dir = PathBuf::from(format!("{sharun_dir}/etc"));
    let share_dir = PathBuf::from(format!("{sharun_dir}/share"));

    let lib_path_file = &format!("{library_path}/lib.path");
    if !Path::new(lib_path_file).exists() {
        gen_library_path(&mut library_path, lib_path_file)
    }
    if let Ok(lib_path_data) = read_to_string(lib_path_file) {
        let lib_path_data = lib_path_data.trim();
        let dirs: HashSet<&str> = lib_path_data.split("\n").map(|string|{
            string.split("/").nth(1).unwrap_or("")
        }).collect();
        for dir in dirs {
            let dir_path = &format!("{library_path}/{dir}");
            if dir.starts_with("python") {
                add_to_env("PYTHONHOME", &sharun_dir);
                env::set_var("PYTHONDONTWRITEBYTECODE", "1")
            }
            if dir.starts_with("perl") {
                add_to_env("PERLLIB", dir_path)
            }
            if dir == "gconv" {
                add_to_env("GCONV_PATH", dir_path)
            }
            if dir.starts_with("gtk-") {
                add_to_env("GTK_PATH", dir_path);
                env::set_var("GTK_EXE_PREFIX", &sharun_dir);
                env::set_var("GTK_DATA_PREFIX", &sharun_dir)
            }
            if dir.starts_with("qt") {
                let plugins = &format!("{dir_path}/plugins");
                if Path::new(plugins).exists() {
                    add_to_env("QT_PLUGIN_PATH", plugins)
                }
            }
            if dir.starts_with("babl-") {
                env::set_var("BABL_PATH", dir_path)
            }
            if dir.starts_with("gegl-") {
                env::set_var("GEGL_PATH", dir_path)
            }
            if dir == "gimp" {
                let plugins = &format!("{dir_path}/2.0");
                if Path::new(plugins).exists() {
                    env::set_var("GIMP2_PLUGINDIR", plugins)
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
                    if name == "loaders.cache" && path.is_file() {
                        env::set_var("GDK_PIXBUF_MODULE_FILE", path);
                        is_loaders_cache = true
                    }
                    if is_loaders && is_loaders_cache {
                        break
                    }
                }
            }
        }
        library_path = lib_path_data
            .replace("\n", ":")
            .replace("+", &library_path)
    } else {
        eprintln!("Failed to read lib.path: {lib_path_file}");
        exit(1)
    }

    add_to_env("PATH", bin_dir);

    if share_dir.exists() {
        if let Ok(dir) = share_dir.read_dir() {
            let share = share_dir.to_string_lossy();
            add_to_env("XDG_DATA_DIRS", "/usr/local/share");
            add_to_env("XDG_DATA_DIRS", "/usr/share");
            add_to_env("XDG_DATA_DIRS", &share);
            for entry in dir.flatten() {
                if entry.path().is_dir() {
                    let name = entry.file_name();
                    match name.to_str().unwrap() {
                        "glvnd" =>  {
                            let egl_vendor = &format!("{share}/glvnd/egl_vendor.d");
                            if Path::new(egl_vendor).exists() {
                                add_to_env("__EGL_VENDOR_LIBRARY_DIRS", "/usr/share/glvnd/egl_vendor.d");
                                add_to_env("__EGL_VENDOR_LIBRARY_DIRS", egl_vendor)
                            }
                        }
                        "vulkan" =>  {
                            let icd = &format!("{share}/vulkan/icd.d");
                            if Path::new(icd).exists() {
                                add_to_env("VK_DRIVER_FILES", "/usr/share/vulkan/icd.d");
                                add_to_env("VK_DRIVER_FILES", icd)
                            }
                        }
                        "X11" =>  {
                            let xkb = &format!("{share}/X11/xkb");
                            if Path::new(xkb).exists() {
                                env::set_var("XKB_CONFIG_ROOT", xkb)
                            }
                        }
                        "glib-2.0" =>  {
                            let schemas = &format!("{share}/glib-2.0/schemas");
                            if Path::new(schemas).exists() {
                                add_to_env("GSETTINGS_SCHEMA_DIR", "/usr/share/glib-2.0/schemas");
                                add_to_env("GSETTINGS_SCHEMA_DIR", schemas)
                            }
                        }
                        "gimp" =>  {
                            let gimp = &format!("{share}/gimp/2.0");
                            if Path::new(gimp).exists() {
                                env::set_var("GIMP2_DATADIR",gimp)
                            }
                        }
                        _ => {}
                    }
                }
            }
        }
    }

    if etc_dir.exists() {
        if let Ok(dir) = etc_dir.read_dir() {
            for entry in dir.flatten() {
                if entry.path().is_dir() {
                    let name = entry.file_name();
                    match name.to_str().unwrap() {
                        "fonts" => {
                            let fonts_conf = etc_dir.join("fonts/fonts.conf");
                            if fonts_conf.exists() {
                                env::set_var("FONTCONFIG_FILE", fonts_conf)
                            }
                        }
                        "gimp" => {
                            let conf = etc_dir.join("gimp/2.0");
                            if conf.exists() {
                                env::set_var("GIMP2_SYSCONFDIR", conf)
                            }
                        }
                        _ => {}
                    }
                }
            }
        }
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
