use crate::cli::ProfileCmd as CliProfileCmd;
use crate::*;

pub(crate) fn run(config_dir: &str, cmd: CliProfileCmd) -> Result<i32> {
    match cmd {
        CliProfileCmd::List => {
            println!("command=profile list");
            Ok(0)
        }
        CliProfileCmd::Show { name } => {
            println!("command=profile show");
            println!("name={name}");
            Ok(0)
        }
        CliProfileCmd::Import { file } => {
            println!("command=profile import");
            println!("file={file}");
            Ok(0)
        }
        CliProfileCmd::Export { name, out } => {
            println!("command=profile export");
            println!("name={} out={}", name, out);
            Ok(0)
        }
        CliProfileCmd::Validate { target } => {
            println!("command=profile validate");
            if Path::new(&target).exists() {
                match validate_profile_file(&target) {
                    Ok(errors) if errors.is_empty() => {
                        println!("result=ok");
                        Ok(0)
                    }
                    Ok(errors) => {
                        println!("result=invalid");
                        for err in errors {
                            println!("error\t{}\t{}", err.path, err.message);
                        }
                        Ok(2)
                    }
                    Err(err) => {
                        eprintln!("failed to validate profile file: {err}");
                        Ok(1)
                    }
                }
            } else {
                match load_profile_from_store(config_dir, &target) {
                    Ok(profile) => {
                        let errors = resguard_core::validate_profile(&profile);
                        if errors.is_empty() {
                            println!("result=ok");
                            Ok(0)
                        } else {
                            println!("result=invalid");
                            for err in errors {
                                println!("error\t{}\t{}", err.path, err.message);
                            }
                            Ok(2)
                        }
                    }
                    Err(err) => {
                        eprintln!(
                            "failed to load profile '{target}' from store {}: {err}",
                            config_dir
                        );
                        Ok(1)
                    }
                }
            }
        }
        CliProfileCmd::New { name, from } => {
            println!("command=profile new");
            println!("name={} from={from:?}", name);
            Ok(0)
        }
        CliProfileCmd::Edit { name } => {
            println!("command=profile edit");
            println!("name={name}");
            Ok(0)
        }
    }
}
