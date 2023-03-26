use goblin::mach::load_command::{CommandVariant, RpathCommand};
use goblin::mach::Mach;
use scroll::{Pread, Pwrite};
use std::collections::HashSet;
use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};

fn modify_dylib(
    dylib_path: &Path,
    prefix: &Path,
    encoded_prefix: &Path,
) -> Result<(), Box<dyn std::error::Error>> {
    println!("Dylib: {:?}", dylib_path);
    let mut data = fs::read(dylib_path)?;

    match goblin::mach::Mach::parse(&data)? {
        Mach::Binary(mach) => {
            let mut modified = false;
            for command in &mach.load_commands {
                match command.command {
                    CommandVariant::IdDylib(dylib_cmd)
                    | CommandVariant::LoadDylib(dylib_cmd)
                    | CommandVariant::LoadWeakDylib(dylib_cmd)
                    | CommandVariant::ReexportDylib(dylib_cmd) => {
                        // println!("Dylib: {:?}", cmd.dylib.name);
                        let libname_offset = command.offset + dylib_cmd.dylib.name as usize;
                        let libname = data
                            .pread::<&str>(libname_offset)
                            .expect("Could not get libname")
                            .to_string();

                        println!("Dylib name: {}", libname);
                        let lib_path = Path::new(&libname);
                        if lib_path.starts_with(encoded_prefix) {
                            let new_libname = format!(
                                "@rpath/{}",
                                lib_path.file_name().unwrap().to_string_lossy()
                            );
                            let mut lvec = new_libname.as_bytes().to_vec();
                            lvec.extend(
                                std::iter::repeat(0).take(libname.len() - new_libname.len()),
                            );

                            let old_cmdsize = dylib_cmd.cmdsize as usize;
                            let new_cmdsize = lvec.len();
                            println!(
                                "Old cmdsize: {}, new cmdsize: {} vs {}",
                                old_cmdsize,
                                new_cmdsize,
                                libname.len()
                            );

                            data.pwrite_with::<&[u8]>(lvec.as_slice(), libname_offset, ())?;

                            let written_libname = data
                                .pread::<&str>(libname_offset)
                                .expect("Could not get libname")
                                .to_string();

                            println!("Dylib Modified: '{}' -> '{}'", libname, written_libname);
                            modified = true;
                        }
                    }
                    CommandVariant::Rpath(RpathCommand {
                        cmd: _,
                        cmdsize: _,
                        path,
                    }) => {
                        let rpath_offset = command.offset + path as usize;
                        let rpath = data
                            .pread::<&str>(rpath_offset)
                            .expect("Could not read rpath");

                        if rpath.starts_with('/') {
                            let rpath_path = Path::new(rpath);
                            if rpath_path.starts_with(encoded_prefix) {
                                // get relative path from dylib to rpath

                                let orig_path = encoded_prefix.join(
                                    dylib_path.strip_prefix(prefix).unwrap().parent().unwrap(),
                                );
                                tracing::info!("Original path: {}", orig_path.display());

                                let relpath = pathdiff::diff_paths(rpath_path, orig_path)
                                    .expect("Could not get relative path");

                                let new_rpath =
                                    format!("@loader_path/{}", relpath.to_string_lossy());
                                tracing::info!("New rpath: {}", new_rpath);

                                let old_rpath = rpath.to_string();
                                let mut bytes = new_rpath.as_bytes().to_vec();
                                bytes.extend(
                                    std::iter::repeat(0).take(old_rpath.len() - new_rpath.len()),
                                );
                                data.pwrite_with(bytes.as_slice(), rpath_offset, ())?;
                                modified = true;
                            }
                        }
                    }
                    _ => {}
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
    encoded_prefix: &Path,
) -> Result<(), Box<dyn std::error::Error>> {
    for p in paths {
        if fs::symlink_metadata(p)?.is_symlink() {
            println!("Skipping symlink: {}", p.display());
            continue;
        }

        if let Some(ext) = p.extension() {
            if ext.to_string_lossy() == "dylib" {
                println!("Relinking: {}", p.display());
                match modify_dylib(p, prefix, encoded_prefix) {
                    Ok(_) => {}
                    Err(e) => {
                        eprintln!("Error: {}", e);
                    }
                }
            }
        } else if p.parent().unwrap().ends_with("bin") {
            println!("Relinking: {}", p.display());
            match modify_dylib(p, prefix, encoded_prefix) {
                Ok(_) => {}
                Err(e) => {
                    eprintln!("Error: {}", e);
                }
            }
        }
    }

    Ok(())
}
