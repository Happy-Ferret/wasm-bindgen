#[macro_use]
extern crate failure;
extern crate parity_wasm;
extern crate wasm_bindgen_shared as shared;
extern crate serde_json;

use std::path::{Path, PathBuf};
use std::fs::File;
use std::io::Write;

use failure::{Error, ResultExt};
use parity_wasm::elements::*;

mod js;

pub struct Bindgen {
    path: Option<PathBuf>,
    nodejs: bool,
}

pub struct Object {
    module: Module,
    program: shared::Program,
    nodejs: bool,
}

impl Bindgen {
    pub fn new() -> Bindgen {
        Bindgen {
            path: None,
            nodejs: false,
        }
    }

    pub fn input_path<P: AsRef<Path>>(&mut self, path: P) -> &mut Bindgen {
        self.path = Some(path.as_ref().to_path_buf());
        self
    }

    pub fn nodejs(&mut self, node: bool) -> &mut Bindgen {
        self.nodejs = node;
        self
    }

    pub fn generate(&mut self) -> Result<Object, Error> {
        let input = match self.path {
            Some(ref path) => path,
            None => panic!("must have a path input for now"),
        };
        let mut module = parity_wasm::deserialize_file(input).map_err(|e| {
            format_err!("{:?}", e)
        })?;
        let program = extract_program(&mut module);
        Ok(Object {
            module,
            program,
            nodejs: self.nodejs,
        })
    }
}

impl Object {
    pub fn write_js_to<P: AsRef<Path>>(&self, path: P) -> Result<(), Error> {
        self._write_js_to(path.as_ref())
    }

    fn _write_js_to(&self, path: &Path) -> Result<(), Error> {
        let js = self.generate_js();
        let mut f = File::create(path).with_context(|_| {
            format!("failed to create file at {:?}", path)
        })?;
        f.write_all(js.as_bytes()).with_context(|_| {
            format!("failed to write file at {:?}", path)
        })?;
        Ok(())
    }

    pub fn write_wasm_to<P: AsRef<Path>>(self, path: P) -> Result<(), Error> {
        self._write_wasm_to(path.as_ref())
    }

    fn _write_wasm_to(self, path: &Path) -> Result<(), Error> {
        parity_wasm::serialize_to_file(path, self.module).map_err(|e| {
            format_err!("{:?}", e)
        })?;
        Ok(())
    }

    pub fn generate_js(&self) -> String {
        let mut js = js::Js::default();
        js.nodejs = self.nodejs;
        js.generate_program(&self.program);
        js.to_string()
    }
}

fn extract_program(module: &mut Module) -> shared::Program {
    let data = module.sections_mut()
        .iter_mut()
        .filter_map(|s| {
            match *s {
                Section::Data(ref mut s) => Some(s),
                _ => None,
            }
        })
        .next();

    let mut ret = shared::Program {
        structs: Vec::new(),
        free_functions: Vec::new(),
        imports: Vec::new(),
    };
    let data = match data {
        Some(data) => data,
        None => return ret,
    };

    for i in (0..data.entries().len()).rev() {
        {
            let value = data.entries()[i].value();
            if !value.starts_with(b"wbg:") {
                continue
            }
            let json = &value[4..];
            let p = match serde_json::from_slice(json) {
                Ok(f) => f,
                Err(_) => continue,
            };
            let shared::Program { structs, free_functions, imports } = p;
            ret.structs.extend(structs);
            ret.free_functions.extend(free_functions);
            ret.imports.extend(imports);
        }
        data.entries_mut().remove(i);
    }
    return ret
}
