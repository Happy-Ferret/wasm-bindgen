extern crate wasm_bindgen_cli_support as cli;

use std::env;
use std::fs;
use std::io::{Write, Read};
use std::path::{PathBuf, Path};
use std::process::Command;
use std::sync::atomic::*;
use std::sync::{Once, ONCE_INIT};
use std::time::Instant;

static CNT: AtomicUsize = ATOMIC_USIZE_INIT;
thread_local!(static IDX: usize = CNT.fetch_add(1, Ordering::SeqCst));

pub struct Project {
    files: Vec<(String, String)>,
}

pub fn project() -> Project {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let dir = dir.parent().unwrap() // chop off `test-support`
        .parent().unwrap(); // chop off `crates`

    let mut lockfile = String::new();
    fs::File::open(&dir.join("Cargo.lock")).unwrap()
        .read_to_string(&mut lockfile).unwrap();
    Project {
        files: vec![
            ("Cargo.toml".to_string(), format!(r#"
                [package]
                name = "test{}"
                version = "0.0.1"
                authors = []

                [workspace]

                [lib]
                crate-type = ["cdylib"]

                [dependencies]
                wasm-bindgen = {{ path = '{}' }}

                [profile.dev]
                opt-level = 2 # TODO: decrease when upstream is not buggy
            "#, IDX.with(|x| *x), dir.display())),

            ("Cargo.lock".to_string(), lockfile),

            ("run.js".to_string(), r#"
                var fs = require("fs");
                var out = require("./out.compat");
                var test = require("./test.compat");
                var wasm = fs.readFileSync("out.wasm");
                var process = require("process");

                out.instantiate(wasm, test.imports).then(m => {
                    test.test(m);
                }).catch(function(error) {
                    console.error(error);
                    process.exit(1);
                });
            "#.to_string()),
        ],
    }
}

pub fn root() -> PathBuf {
    let idx = IDX.with(|x| *x);

    let mut me = env::current_exe().unwrap();
    me.pop(); // chop off exe name
    me.pop(); // chop off `deps`
    me.pop(); // chop off `debug` / `release`
    me.push("generated-tests");
    me.push(&format!("test{}", idx));
    return me
}

fn babel() -> PathBuf {
    static INIT: Once = ONCE_INIT;

    let mut me = env::current_exe().unwrap();
    me.pop(); // chop off exe name
    me.pop(); // chop off `deps`
    me.pop(); // chop off `debug` / `release`
    let install_dir = me.clone();
    me.push("node_modules/babel-cli/bin/babel.js");

    INIT.call_once(|| {
        if !me.exists() {
            let mut npm = if cfg!(windows) {
                let mut n = Command::new("cmd");
                n.arg("/c").arg("npm");
                n
            } else {
                Command::new("npm")
            };
            run(npm
                .arg("install")
                .arg("babel-cli")
                .arg("babel-preset-env")
                .current_dir(&install_dir), "npm");
            assert!(me.exists());
        }
    });

    return me
}

impl Project {
    pub fn file(&mut self, name: &str, contents: &str) -> &mut Project {
        self.files.push((name.to_string(), contents.to_string()));
        self
    }

    pub fn test(&mut self) {
        let root = root();
        drop(fs::remove_dir_all(&root));
        for &(ref file, ref contents) in self.files.iter() {
            let dst = root.join(file);
            fs::create_dir_all(dst.parent().unwrap()).unwrap();
            fs::File::create(&dst).unwrap().write_all(contents.as_ref()).unwrap();
        }

        let target_dir = root.parent().unwrap() // chop off test name
            .parent().unwrap(); // chop off `generated-tests`

        let mut cmd = Command::new("cargo");
        cmd.arg("build")
            .arg("--target")
            .arg("wasm32-unknown-unknown")
            .current_dir(&root)
            .env("CARGO_TARGET_DIR", &target_dir);
        run(&mut cmd, "cargo");

        let idx = IDX.with(|x| *x);
        let mut out = target_dir.join(&format!("wasm32-unknown-unknown/debug/test{}.wasm", idx));
        if Command::new("wasm-gc").output().is_ok() {
            let tmp = out;
            out = tmp.with_extension("gc.wasm");
            let mut cmd = Command::new("wasm-gc");
            cmd.arg(&tmp).arg(&out);
            run(&mut cmd, "wasm-gc");
        }

        let obj = cli::Bindgen::new()
            .input_path(&out)
            .nodejs(true)
            .generate()
            .expect("failed to run bindgen");
        obj.write_js_to(root.join("out.js")).expect("failed to write js");
        obj.write_wasm_to(root.join("out.wasm")).expect("failed to write wasm");

        let mut cmd = Command::new("node");
        cmd.arg(babel())
            .arg(root.join("out.js"))
            .arg("--presets").arg("env")
            .arg("--out-file").arg(root.join("out.compat.js"));
        run(&mut cmd, "node");
        let mut cmd = Command::new("node");
        cmd.arg(babel())
            .arg(root.join("test.js"))
            .arg("--presets").arg("env")
            .arg("--out-file").arg(root.join("test.compat.js"));
        run(&mut cmd, "node");

        let mut cmd = Command::new("node");
        cmd.arg("run.js")
            .current_dir(&root);
        run(&mut cmd, "node");
    }
}

fn run(cmd: &mut Command, program: &str) {
    println!("···················································");
    println!("running {:?}", cmd);
    let start = Instant::now();
    let output = match cmd.output() {
        Ok(output) => output,
        Err(err) => panic!("failed to spawn `{}`: {}", program, err),
    };
    println!("exit: {}", output.status);
    let dur = start.elapsed();
    println!("dur: {}.{:03}ms", dur.as_secs(), dur.subsec_nanos() / 1_000_000);
    if output.stdout.len() > 0 {
        println!("stdout ---\n{}", String::from_utf8_lossy(&output.stdout));
    }
    if output.stderr.len() > 0 {
        println!("stderr ---\n{}", String::from_utf8_lossy(&output.stderr));
    }
    assert!(output.status.success());
}
