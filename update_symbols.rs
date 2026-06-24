use std::fs;
use std::path::Path;

fn visit_dirs(dir: &Path, cb: &dyn Fn(&Path)) {
    if dir.is_dir() {
        for entry in fs::read_dir(dir).unwrap() {
            let entry = entry.unwrap();
            let path = entry.path();
            if path.is_dir() {
                if !path.to_string_lossy().contains("target") && !path.to_string_lossy().contains(".vexp") {
                    visit_dirs(&path, cb);
                }
            } else {
                if path.extension().unwrap_or_default() == "rs" {
                    cb(&path);
                }
            }
        }
    }
}

fn main() {
    let process_file = |path: &Path| {
        let content = match fs::read_to_string(path) {
            Ok(c) => c,
            Err(_) => return,
        };
        
        let mut new_content = content.clone();
        
        // This is a bit naive but works for `signature: ...,` or `signature: ... }`
        new_content = new_content.replace("signature: None\n            }", "signature: None,\n                route_path: None,\n                route_method: None\n            }");
        new_content = new_content.replace("signature: None,\n            }", "signature: None,\n                route_path: None,\n                route_method: None\n            }");
        new_content = new_content.replace("signature,\n            }", "signature,\n                route_path: None,\n                route_method: None\n            }");
        new_content = new_content.replace("signature: None, route_path: None, route_method: None }", "signature: None, route_path: None, route_method: None }");
        new_content = new_content.replace("signature, route_path: None, route_method: None }", "signature, route_path: None, route_method: None }");
        
        if new_content != content {
            println!("Updated {:?}", path);
            fs::write(path, new_content).unwrap();
        }
    };
    
    visit_dirs(Path::new("."), &process_file);
}
