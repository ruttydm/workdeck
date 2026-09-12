//! Iterative preorder over the captured query's parent forest. No native reads.
use super::{QueryKey, bound, invalid, sql_error};
use crate::{FeatureId, Result, projection::ProjectionTreePosition};
use rusqlite::Connection;
use std::{
    collections::{HashMap, HashSet},
    time::{Duration, Instant},
};

pub(super) fn order(
    connection: &Connection,
    keys: Vec<QueryKey>,
    collapsed: &[FeatureId],
    timeout_ms: u64,
) -> Result<Vec<QueryKey>> {
    let deadline = Instant::now() + Duration::from_millis(timeout_ms);
    let indices = keys
        .iter()
        .enumerate()
        .map(|(index, (key, _, _))| (key.as_str(), index))
        .collect::<HashMap<_, _>>();
    let collapsed = collapsed
        .iter()
        .map(|id| id.as_str())
        .collect::<HashSet<_>>();
    let mut parents = vec![None; keys.len()];
    let mut outside = vec![false; keys.len()];
    let mut closed = vec![false; keys.len()];
    let mut statement = connection
        .prepare("SELECT key,id,parent FROM projection_records WHERE kind='feature'")
        .map_err(sql_error)?;
    let mut rows = statement.query([]).map_err(sql_error)?;
    while let Some(row) = rows.next().map_err(sql_error)? {
        if Instant::now() >= deadline {
            return Err(bound("feature tree query exceeded its time bound"));
        }
        let key: String = row.get(0).map_err(sql_error)?;
        let Some(&index) = indices.get(key.as_str()) else {
            continue;
        };
        let id: String = row.get(1).map_err(sql_error)?;
        let parent: Option<String> = row.get(2).map_err(sql_error)?;
        parents[index] = parent
            .as_ref()
            .and_then(|id| indices.get(super::key("feature", id).as_str()).copied());
        outside[index] = parent.is_some() && parents[index].is_none();
        closed[index] = collapsed.contains(id.as_str());
    }
    let mut children = vec![Vec::new(); keys.len()];
    let mut roots = Vec::new();
    // Input order already contains stable name/ID sorting. Preserve it among siblings.
    for (index, parent) in parents.iter().enumerate() {
        if let Some(parent) = parent {
            children[*parent].push(index);
        } else {
            roots.push(index);
        }
    }
    let mut stack = roots
        .into_iter()
        .rev()
        .map(|index| (index, 0, false))
        .collect::<Vec<_>>();
    let mut result = Vec::new();
    let mut visited = 0;
    while let Some((index, depth, hidden)) = stack.pop() {
        if Instant::now() >= deadline {
            return Err(bound("feature tree query exceeded its time bound"));
        }
        visited += 1;
        if !hidden {
            let (key, group, _) = &keys[index];
            result.push((
                key.clone(),
                group.clone(),
                Some(ProjectionTreePosition {
                    depth,
                    children: children[index].len(),
                    parent_outside_view: outside[index],
                }),
            ));
        }
        stack.extend(
            children[index]
                .iter()
                .rev()
                .map(|child| (*child, depth + 1, hidden || closed[index])),
        );
    }
    // Every node has at most one parent. Nodes unreachable from roots imply a cycle;
    // even a collapsed branch must not conceal that malformed structure.
    if visited != keys.len() {
        return Err(invalid("feature tree contains a parent cycle"));
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn forty_thousand_levels_are_iterative_and_collapsing_cannot_hide_a_cycle() {
        let mut connection = Connection::open_in_memory().unwrap();
        connection
            .execute_batch(
                "CREATE TABLE projection_records (key TEXT, kind TEXT, id TEXT, parent TEXT)",
            )
            .unwrap();
        let mut keys = Vec::new();
        {
            let transaction = connection.transaction().unwrap();
            let mut insert = transaction
                .prepare("INSERT INTO projection_records VALUES (?1,'feature',?2,?3)")
                .unwrap();
            for index in 0..40_000 {
                let id = format!("FEAT-{index:026}");
                let parent = (index > 0).then(|| format!("FEAT-{:026}", index - 1));
                let key = super::super::key("feature", &id);
                insert.execute(rusqlite::params![key, id, parent]).unwrap();
                keys.push((key, None, None));
            }
            drop(insert);
            transaction.commit().unwrap();
        }
        keys.reverse();
        let rows = order(&connection, keys.clone(), &[], 20_000).unwrap();
        assert_eq!(rows.len(), 40_000);
        assert_eq!(rows.last().unwrap().2.as_ref().unwrap().depth, 39_999);
        let root: FeatureId = format!("FEAT-{:026}", 0).parse().unwrap();
        let collapsed = order(
            &connection,
            keys.clone(),
            std::slice::from_ref(&root),
            20_000,
        )
        .unwrap();
        assert_eq!(collapsed.len(), 1);
        assert_eq!(collapsed[0].2.as_ref().unwrap().children, 1);
        connection
            .execute(
                "UPDATE projection_records SET parent=?1 WHERE id=?2",
                rusqlite::params![format!("FEAT-{:026}", 39_999), root.as_str()],
            )
            .unwrap();
        let error = order(&connection, keys, &[root], 20_000).unwrap_err();
        assert!(error.message.contains("parent cycle"));
    }
}
