use super::{MigrationNotice, convert::notice, scan::Input};
use crate::{ErrorCode, PmError, Result};

pub(super) fn convert(input: &Input, notices: &mut Vec<MigrationNotice>) -> Result<Vec<u8>> {
    let bytes = input.bytes.as_deref().expect("readable config");
    let text = std::str::from_utf8(bytes).map_err(|_| {
        PmError::new(ErrorCode::InvalidSchema, "app config must be UTF-8").at(&input.path)
    })?;
    let mut document = text.parse::<toml_edit::DocumentMut>().map_err(|error| {
        PmError::new(ErrorCode::InvalidSchema, error.to_string()).at(&input.path)
    })?;
    let Some(paths) = document.get("paths") else {
        return Ok(bytes.to_vec());
    };
    let Some(value) = paths.get("data_dir") else {
        return Ok(bytes.to_vec());
    };
    match value.as_str() {
        Some(".agents/workdeck") => {
            let old = document["paths"]["data_dir"].as_value().expect("known string");
            let mut replacement = toml_edit::Value::from(".workdeck");
            *replacement.decor_mut() = old.decor().clone();
            document["paths"]["data_dir"] = toml_edit::Item::Value(replacement);
            notice(notices,input,"app_data_root_mapping","The known default [paths].data_dir changes from .agents/workdeck to .workdeck; other settings and formatting are retained.");
            let rendered = document.to_string();
            let mut expected = super::convert::parse_table(&input.path,bytes)?;
            expected.get_mut("paths").and_then(toml::Value::as_table_mut).expect("known paths table").insert("data_dir".into(),toml::Value::String(".workdeck".into()));
            if super::convert::parse_table(&input.path,rendered.as_bytes())? != expected {
                return Err(PmError::new(ErrorCode::InvalidSchema,"lossless app config patch changed unrelated settings").at(&input.path));
            }
            Ok(rendered.into_bytes())
        }
        Some(".workdeck") => Ok(bytes.to_vec()),
        Some(_) => Err(PmError::new(ErrorCode::Unsupported,"custom [paths].data_dir requires an explicit app-data/source mapping; preview will not redirect an external or alternate store").at(&input.path)),
        None => Err(PmError::new(ErrorCode::InvalidSchema,"[paths].data_dir must be a string").at(&input.path)),
    }
}
