//! Raw historical fixture authoring. This module is compiled only by integration
//! tests; production legacy compatibility exposes no mutation API.
#![allow(dead_code)]
use std::{fs, path::Path};
use workdeck_cli::store::Issue;

pub fn init(root: &Path) {
    fs::create_dir_all(root.join("issues")).unwrap();
    fs::create_dir_all(root.join("agents")).unwrap();
    fs::write(
        root.join("config.toml"),
        "[ui]\ntheme='auto'\npreview=true\n[paths]\ndata_dir='.agents/workdeck'\n",
    )
    .unwrap();
}

pub fn issue(root: &Path, key: &str, title: &str) -> Issue {
    let mut issue = Issue::new(key.into(), title.into());
    issue.created_at = "2026-09-01T00:00:00Z".into();
    issue.updated_at = "2026-09-01T00:00:00Z".into();
    write(root, &format!("issues/{key}.toml"), &issue);
    issue
}

pub fn write(root: &Path, relative: &str, value: &impl serde::Serialize) {
    let path = root.join(relative);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, toml::to_string_pretty(value).unwrap()).unwrap();
}
