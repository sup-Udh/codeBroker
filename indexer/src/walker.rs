use ignore::WalkBuilder;
use std::path::Path;

pub fn collect_files(root_dir:&str) -> Vec<String> {
    let mut valid_files = Vec::new();

    let walker = WalkBuilder::new(root_dir).build(); // automatically ignores the .gitingore files the walkbuilder

    for result in walker {
        if let Ok(entry) = result {
            let path = entry.path();

            if path.is_file() {
                if is_supported_file(path) {
                    if let Some(path_str) = path.to_str() {
                        valid_files.push(path_str.to_string());
                    }
                }
            }
        }
    }
    valid_files
}

fn is_supported_file(path: &Path) -> bool {
    if let Some(ext) = path.extension() {
        if let Some(ext_str) = ext.to_str() {
            return ext_str=="rs" ;

        }


    }
    false

}