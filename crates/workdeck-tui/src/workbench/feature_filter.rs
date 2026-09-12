//! Query intent is separate from source-bound feature authoring drafts.
use super::*;
use workdeck_pm::projection::ProjectionFeatureQuery;

const FIELDS: [(&str, &str); 8] = [
    ("query", "Text"),
    ("project", "Project ID"),
    ("milestone", "Milestone ID"),
    ("target", "Target ID"),
    ("lead", "Lead"),
    ("decision", "Decision"),
    ("maturity", "Maturity"),
    ("availability", "Availability"),
];

impl FeatureWorkspace {
    pub(super) fn open_filter(&mut self) -> Result<()> {
        let value = serde_json::to_value(&self.filter).map_err(filter_error)?;
        let fields = FIELDS
            .iter()
            .map(|(key, label)| {
                TextField::new(
                    label,
                    value
                        .get(*key)
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .into(),
                    false,
                )
            })
            .collect();
        let mut form = WorkbenchForm::new(FormKind::Filter, "Filter features", fields);
        form.help
            .push("Blank fields match all. State names use snake_case; IDs are exact.".into());
        self.filter_form = Some(form);
        self.error = None;
        Ok(())
    }

    pub(super) fn submit_filter(&mut self) -> Result<()> {
        let Some(form) = &self.filter_form else {
            return Ok(());
        };
        let mut value = serde_json::Map::new();
        for ((key, _), field) in FIELDS.iter().zip(&form.fields) {
            let content = if *key == "query" {
                field.value.clone()
            } else {
                field.value.trim().into()
            };
            value.insert(
                (*key).into(),
                if *key != "query" && content.is_empty() {
                    Value::Null
                } else {
                    Value::String(content)
                },
            );
        }
        let filter: ProjectionFeatureQuery =
            serde_json::from_value(Value::Object(value)).map_err(filter_error)?;
        self.filter = filter;
        self.filter_form = None;
        self.pending_tree = None;
        self.pending_selected = None;
        self.error = None;
        let query = self.tree_query();
        if let Some(index) = &mut self.index {
            index.set_query(query);
        }
        Ok(())
    }

    pub(super) fn filtered(&self) -> bool {
        self.filter != ProjectionFeatureQuery::default()
    }
}

fn filter_error(error: serde_json::Error) -> PmError {
    PmError::new(
        workdeck_pm::ErrorCode::InvalidInput,
        format!("Invalid feature filter: {error}"),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn invalid_feature_state_retains_filter_text_without_changing_the_query() {
        let mut workspace = FeatureWorkspace::new_indexed(None);
        workspace.open_filter().unwrap();
        workspace.filter_form.as_mut().unwrap().fields[0].value = "literal query".into();
        workspace.filter_form.as_mut().unwrap().fields[5].value = "invented_state".into();
        assert_eq!(
            workspace.submit_filter().unwrap_err().code,
            workdeck_pm::ErrorCode::InvalidInput
        );
        assert_eq!(workspace.filter, ProjectionFeatureQuery::default());
        assert_eq!(
            workspace.filter_form.as_ref().unwrap().fields[0].value,
            "literal query"
        );
        workspace.filter_form.as_mut().unwrap().fields[5].value = "proposed".into();
        workspace.submit_filter().unwrap();
        assert!(workspace.filter_form.is_none());
        assert_eq!(
            workspace.filter.decision,
            Some(workdeck_pm::FeatureDecision::Proposed)
        );
        workspace.open_filter().unwrap();
        assert_eq!(
            workspace.filter_form.as_ref().unwrap().fields[5].value,
            "proposed"
        );
    }
}
