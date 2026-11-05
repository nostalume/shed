//! A real native argv/byte-stream witness, shared by backend conformance tests.
use std::{
    env, fs,
    io::{self, Write},
    process,
};

fn main() {
    if let Ok(path) = env::var("SHED_NATIVE_MARKER") {
        fs::write(path, b"executed").unwrap();
    }
    let args: Vec<String> = env::args().skip(1).collect();
    if args.is_empty() {
        return;
    }
    match args[0].as_str() {
        "argv" => {
            let mut file = fs::File::create(&args[1]).unwrap();
            for arg in &args[2..] {
                file.write_all(&(arg.len() as u64).to_le_bytes()).unwrap();
                file.write_all(arg.as_bytes()).unwrap();
            }
        }
        "env" => {
            let mut file = fs::File::create(&args[1]).unwrap();
            for name in &args[2..] {
                match env::var(name) {
                    Ok(value) => {
                        file.write_all(&[1]).unwrap();
                        file.write_all(&(value.len() as u64).to_le_bytes()).unwrap();
                        file.write_all(value.as_bytes()).unwrap();
                    }
                    Err(env::VarError::NotPresent) => file.write_all(&[0]).unwrap(),
                    Err(error) => panic!("invalid environment value: {error}"),
                }
            }
        }
        "emit" => {
            // Both streams exceed pipe capacity in one case, detecting serial reads.
            io::stderr().write_all(&vec![b'!'; 131_072]).unwrap();
            io::stdout()
                .write_all(&fs::read(&args[1]).unwrap())
                .unwrap();
            process::exit(args[2].parse().unwrap());
        }
        "where" => fs::write(&args[1], env::current_exe().unwrap().to_str().unwrap()).unwrap(),
        "out" => println!("native output"),
        "exit" => process::exit(args[1].parse().unwrap()),
        mode => panic!("unknown fixture mode {mode}"),
    }
}
