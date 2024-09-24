use std::{env, fs};
use std::ffi::CString;
use std::path::Path;


const SHARUN_NAME: &str = "sharun";
const LINKER_NAME: &str = "ld-linux-x86-64.so.2";


fn get_linker_name(shared_lib: &str) -> String {
    #[cfg(target_arch = "x86_64")] // target x86_64-unknown-linux-musl
    let linkers = vec![
        "ld-linux-x86-64.so.2",
        "ld-musl-x86_64.so.1",
    ];
    #[cfg(target_arch = "aarch64")] // target aarch64-unknown-linux-musl
    let linkers = vec![
        "ld-linux-aarch64.so.1",
        "ld-musl-aarch64.so.1",
    ];
    for linker in linkers {
        let linker_path = Path::new(shared_lib).join(linker);
        if linker_path.exists() {
            return linker_path.file_name().unwrap().to_str().unwrap().to_string();
        }
    }
    LINKER_NAME.to_string()
}

fn realpath(path: &str) -> String {
    fs::canonicalize(Path::new(path)).unwrap().to_str().unwrap().to_string()
}

fn basename(path: &str) -> String {
    let pieces: Vec<&str> = path.rsplit('/').collect();
    return pieces.get(0).unwrap().to_string();
}

fn is_file(path: &str) -> bool {
    let path = Path::new(path);
    path.is_file()
}

fn main() {
    let sharun: std::path::PathBuf = env::current_exe().unwrap();
    let mut sharun_dir = sharun.parent().unwrap().to_str().unwrap().to_string();
    let lower_dir = format!("{sharun_dir}/../");
    if basename(&sharun_dir) == "bin" && 
       is_file(&format!("{lower_dir}{SHARUN_NAME}")) {
        sharun_dir = realpath(&lower_dir);
    }

    let mut exec_args: Vec<String> = env::args().collect();
    let arg0 = exec_args.remove(0);

    let shared_bin = format!("{sharun_dir}/shared/bin");
    let shared_lib = format!("{sharun_dir}/shared/lib");

    let mut bin_name = basename(&arg0);
    if bin_name == SHARUN_NAME {
        bin_name = exec_args.remove(0);
    }
    let bin = format!("{shared_bin}/{bin_name}");
    let bin_cstr = CString::new(bin.clone()).unwrap();

    let default_linker_name = get_linker_name(&shared_lib);
    let linker_name = env::var("SHARUN_LDNAME")
        .unwrap_or(default_linker_name);
    let linker = format!("{shared_lib}/{linker_name}");
    let linker_path = Path::new(&linker);
    let linker_cstr = CString::new(linker.clone()).unwrap();

    let envs: Vec<CString> = env::vars()
        .map(|(key, value)| CString::new(
            format!("{}={}", key, value)
        ).unwrap()).collect();

    let mut args_cstrs: Vec<CString> = exec_args.iter()
        .map(|arg| CString::new(arg.clone()).unwrap()).collect();
    args_cstrs.insert(0, linker_cstr);
    args_cstrs.insert(1, CString::new("--library-path").unwrap());
    args_cstrs.insert(2, CString::new(shared_lib).unwrap());
    args_cstrs.insert(3, bin_cstr);

    userland_execve::exec(
        &linker_path,
        &args_cstrs,
        &envs,
    );
}