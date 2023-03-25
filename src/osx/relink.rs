use goblin::mach::Mach;
use scroll::{Pread, Pwrite};
use std::collections::HashSet;
use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};

fn modify_dylib(dylib_path: &Path, prefix: &Path) -> Result<(), Box<dyn std::error::Error>> {
    println!("Dylib: {:?}", dylib_path);
    let mut data = fs::read(dylib_path)?;

    match goblin::mach::Mach::parse(&data)? {
        Mach::Binary(mach) => {
            let mut modified = false;
            println!("Load commands: {}", mach.load_commands.len());
            for command in &mach.load_commands {
                // println!("Command: {:?}", command.command);
                if let goblin::mach::load_command::CommandVariant::LoadDylib(ref cmd) =
                    command.command
                {
                    // println!("Dylib: {:?}", cmd.dylib.name);
                    let libname_offset = command.offset + cmd.dylib.name as usize;
                    let libname = data
                        .pread::<&str>(libname_offset)
                        .expect("Could not get libname")
                        .to_string();

                    println!("Dylib name: {}", libname);
                    let lib_path = Path::new(&libname);
                    if lib_path.starts_with(prefix) {
                        let new_libname =
                            format!("@rpath/{}", lib_path.file_name().unwrap().to_string_lossy());
                        let mut lvec = new_libname.as_bytes().iter().cloned().collect::<Vec<_>>();
                        lvec.extend(std::iter::repeat(0).take(libname.len() - new_libname.len()));

                        let old_cmdsize = cmd.cmdsize as usize;
                        let new_cmdsize = lvec.len();
                        println!("Old cmdsize: {}, new cmdsize: {}", old_cmdsize, new_cmdsize);

                        data.pwrite_with(lvec.as_slice(), libname_offset, ())?;

                        println!("Dylib Modified: '{}' -> '{}'", libname, new_libname);
                        modified = true;
                    }
                }
                if let goblin::mach::load_command::CommandVariant::Rpath(
                    goblin::mach::load_command::RpathCommand {
                        cmd: _,
                        cmdsize: _,
                        path,
                    },
                ) = command.command
                {
                    let rpath_offset = command.offset + path as usize;
                    let rpath = data
                        .pread::<&str>(rpath_offset)
                        .expect("Could not read rpath");

                    if rpath.starts_with("/") {
                        let rpath_path = Path::new(rpath);
                        if rpath_path.starts_with(prefix) {
                            let new_rpath = "@loader_path/../lib";
                            let old_rpath = rpath.to_string();
                            let mut bytes =
                                new_rpath.as_bytes().iter().cloned().collect::<Vec<_>>();
                            bytes.extend(
                                std::iter::repeat(0).take(old_rpath.len() - new_rpath.len()),
                            );
                            data.pwrite_with(bytes.as_slice(), rpath_offset, ())?;
                            modified = true;
                        }
                    }
                }
            }

            if modified {
                let mut output = File::create(dylib_path)?;
                output.write_all(&data)?;
            }
        }
        _ => {
            eprintln!("Not a valid Mach-O binary.");
            return Err("Not a valid Mach-O binary".into());
        }
    }

    Ok(())
}

pub fn relink_paths(
    paths: &HashSet<PathBuf>,
    prefix: &Path,
) -> Result<(), Box<dyn std::error::Error>> {
    for p in paths {
        if fs::metadata(p).unwrap().is_symlink() {
            println!("Skipping symlink: {}", p.display());
            continue;
        }

        if let Some(ext) = p.extension() {
            if ext.to_string_lossy() == "dylib" {
                println!("Relinking: {}", p.display());
                match modify_dylib(p, prefix) {
                    Ok(_) => {}
                    Err(e) => {
                        eprintln!("Error: {}", e);
                    }
                }
            }
        } else if p.parent().unwrap().ends_with("bin") {
            println!("Relinking: {}", p.display());
            match modify_dylib(p, prefix) {
                Ok(_) => {}
                Err(e) => {
                    eprintln!("Error: {}", e);
                }
            }
        }
    }

    Ok(())
}
