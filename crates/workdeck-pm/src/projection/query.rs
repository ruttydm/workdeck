use super::{
    projector::extract::{key, kind_name},
    schema::sql_error,
    storage::ProjectionReadView,
    types::*,
};
use crate::{Config, ErrorCode, PmError, Priority, Result, SnapshotKind};
use rusqlite::{Connection, OptionalExtension, params};
use std::{
    collections::BTreeMap,
    time::{Duration, Instant},
};
#[path = "select.rs"]
mod select;
#[path = "query_tree.rs"]
mod tree;
type QueryKey = (String, Option<String>, Option<ProjectionTreePosition>);

#[derive(Debug, Default)]
pub(super) struct QueryCache {
    entries: BTreeMap<String, CachedQuery>,
    clock: u64,
    bytes: usize,
}
#[derive(Debug)]
struct CachedQuery {
    keys: Vec<QueryKey>,
    groups: Vec<ProjectionGroup>,
    used: u64,
    bytes: usize,
}
struct Deadline<'a>(&'a Connection);
impl<'a> Deadline<'a> {
    fn start(connection: &'a Connection, milliseconds: u64) -> Result<Self> {
        let deadline = Instant::now() + Duration::from_millis(milliseconds);
        connection
            .progress_handler(1000, Some(move || Instant::now() >= deadline))
            .map_err(sql_error)?;
        Ok(Self(connection))
    }
}
impl Drop for Deadline<'_> {
    fn drop(&mut self) {
        let _ = self.0.progress_handler(0, None::<fn() -> bool>);
    }
}
fn invalid(message: &str) -> PmError {
    PmError::new(ErrorCode::InvalidInput, message)
}
fn stale() -> PmError {
    PmError::new(
        ErrorCode::StaleSource,
        "projection token belongs to a different source generation",
    )
}
fn bound(message: &str) -> PmError {
    PmError::new(ErrorCode::Unsupported, message)
}
fn json_error(error: serde_json::Error) -> PmError {
    PmError::new(ErrorCode::InvalidSchema, error.to_string())
}
fn serialized<T: serde::Serialize>(value: &T) -> Result<usize> {
    Ok(serde_json::to_vec(value).map_err(json_error)?.len())
}
fn decode<T: serde::de::DeserializeOwned>(value: Option<String>) -> Result<Option<T>> {
    value
        .map(|value| serde_json::from_value(serde_json::Value::String(value)).map_err(json_error))
        .transpose()
}

impl ProjectionReadView {
    pub fn query(&self, query: &ProjectionQuery) -> Result<ProjectionQueryHandle> {
        let hash =
            crate::transactions::canonical_hash(&serde_json::to_value(query).map_err(json_error)?)?;
        let hash_text = hash.as_str().to_owned();
        {
            let mut cache = self.query_cache.borrow_mut();
            cache.clock = cache.clock.saturating_add(1);
            let used = cache.clock;
            if let Some(entry) = cache.entries.get_mut(&hash_text) {
                entry.used = used;
                return Ok(ProjectionQueryHandle {
                    view: self.id().clone(),
                    query: hash,
                    total: entry.keys.len(),
                });
            }
        }
        let keys = self.with_connection(|connection| {
            let _deadline = Deadline::start(connection, self.limits().query_timeout_ms)?;
            let config: String = connection
                .query_row(
                    "SELECT document FROM projection_configuration WHERE id=1",
                    [],
                    |row| row.get(0),
                )
                .map_err(sql_error)?;
            let config: Config = serde_json::from_str(&config).map_err(json_error)?;
            let selection = select::select(query, &config, self.limits().max_query_keys)?;
            let mut statement = connection.prepare(&selection.sql).map_err(sql_error)?;
            let mut rows = statement
                .query(rusqlite::params_from_iter(selection.values))
                .map_err(sql_error)?;
            let mut keys = Vec::new();
            let mut bytes = 0usize;
            while let Some(row) = rows.next().map_err(sql_error)? {
                let key: String = row.get(0).map_err(sql_error)?;
                let group: Option<String> = row.get(1).map_err(sql_error)?;
                bytes = bytes.saturating_add(key.len() + group.as_ref().map_or(0, String::len));
                if keys.len() >= self.limits().max_query_keys
                    || bytes > self.limits().max_query_bytes
                {
                    return Err(bound(
                        "projection query exceeds its key or byte bound; narrow the query",
                    ));
                }
                keys.push((key, group, None));
            }
            if let ProjectionQuery::Features { query } = query {
                if query.tree {
                    return tree::order(
                        connection,
                        keys,
                        &query.collapsed,
                        self.limits().query_timeout_ms,
                    );
                }
                if !query.collapsed.is_empty() {
                    return Err(invalid("collapsed features require a tree query"));
                }
            }
            Ok(keys)
        })?;
        let mut grouped = BTreeMap::<Option<String>, usize>::new();
        for (_, group, _) in &keys {
            *grouped.entry(group.clone()).or_default() += 1;
        }
        let groups = grouped
            .into_iter()
            .map(|(value, count)| ProjectionGroup { value, count })
            .collect::<Vec<_>>();
        let bytes = serialized(&keys)?.saturating_add(serialized(&groups)?);
        if bytes > self.limits().max_query_bytes {
            return Err(bound(
                "projection query exceeds its encoded byte bound; narrow the query",
            ));
        }
        let total = keys.len();
        let mut cache = self.query_cache.borrow_mut();
        while cache.entries.len() >= self.limits().max_query_handles
            || cache.bytes.saturating_add(bytes) > self.limits().max_query_bytes
        {
            let oldest = cache
                .entries
                .iter()
                .min_by_key(|(_, entry)| entry.used)
                .map(|(key, _)| key.clone());
            let Some(oldest) = oldest else { break };
            if let Some(removed) = cache.entries.remove(&oldest) {
                cache.bytes = cache.bytes.saturating_sub(removed.bytes);
            }
        }
        cache.clock = cache.clock.saturating_add(1);
        let used = cache.clock;
        cache.bytes += bytes;
        cache.entries.insert(
            hash_text,
            CachedQuery {
                keys,
                groups,
                used,
                bytes,
            },
        );
        Ok(ProjectionQueryHandle {
            view: self.id().clone(),
            query: hash,
            total,
        })
    }

    pub fn page(
        &self,
        handle: &ProjectionQueryHandle,
        offset: usize,
        limit: usize,
    ) -> Result<ProjectionPage> {
        if limit == 0 || limit > self.limits().max_page_rows {
            return Err(invalid("projection page size is outside its row bound"));
        }
        let keys = {
            let cache = self.query_cache.borrow();
            let entry = self.entry(&cache, handle)?;
            if offset > entry.keys.len() {
                return Err(invalid("projection page offset exceeds the query count"));
            }
            entry
                .keys
                .iter()
                .skip(offset)
                .take(limit)
                .cloned()
                .collect::<Vec<_>>()
        };
        let rows = self.with_connection(|connection| {
            let _deadline = Deadline::start(connection, self.limits().query_timeout_ms)?;
            keys.into_iter()
                .map(|(key, group, tree)| self.row(connection, &key, group, tree))
                .collect::<Result<Vec<_>>>()
        })?;
        let next = offset + rows.len();
        let page = ProjectionPage {
            handle: handle.clone(),
            offset,
            rows,
            next_offset: (next < handle.total).then_some(next),
        };
        if serialized(&page)? > self.limits().max_query_bytes {
            return Err(bound(
                "projection page exceeds its byte bound; request fewer rows",
            ));
        }
        Ok(page)
    }

    /// Read only visible cards. All columns use one immutable handle, and the
    /// combined window respects the same row/byte limits as an ordinary page.
    pub fn board(
        &self,
        handle: &ProjectionQueryHandle,
        request: &ProjectionBoardRequest,
    ) -> Result<Vec<ProjectionBoardColumn>> {
        if request.columns == 0
            || request.columns > 8
            || request.rows == 0
            || request
                .columns
                .checked_mul(request.rows)
                .is_none_or(|rows| rows > self.limits().max_page_rows)
            || request
                .selected
                .is_some_and(|selected| selected >= handle.total)
        {
            return Err(invalid(
                "board window exceeds its column, row or selection bounds",
            ));
        }
        let groups = self.groups(handle)?;
        if request.first_group > groups.len() {
            return Err(invalid("board column offset exceeds the query's groups"));
        }
        let mut start = groups
            .iter()
            .take(request.first_group)
            .map(|group| group.count)
            .sum::<usize>();
        let mut columns = Vec::new();
        for group in groups
            .into_iter()
            .skip(request.first_group)
            .take(request.columns)
        {
            let local = request
                .selected
                .filter(|selected| *selected >= start && *selected < start + group.count)
                .map_or(0, |selected| selected - start);
            let offset = local.saturating_add(1).saturating_sub(request.rows);
            let page = self.page(
                handle,
                start + offset,
                request.rows.min(group.count - offset),
            )?;
            start += group.count;
            columns.push(ProjectionBoardColumn { group, page });
        }
        if serialized(&columns)? > self.limits().max_query_bytes {
            return Err(bound(
                "board window exceeds its byte bound; request fewer cards",
            ));
        }
        Ok(columns)
    }

    pub fn locate(
        &self,
        handle: &ProjectionQueryHandle,
        record: &ProjectionRecordKey,
    ) -> Result<Option<usize>> {
        if record.repository != self.id().source.repository {
            return Err(stale());
        }
        let cache = self.query_cache.borrow();
        let entry = self.entry(&cache, handle)?;
        let key = key(&kind_name(record.kind), &record.id);
        Ok(entry
            .keys
            .iter()
            .position(|(candidate, _, _)| candidate == &key))
    }

    pub fn groups(&self, handle: &ProjectionQueryHandle) -> Result<Vec<ProjectionGroup>> {
        let cache = self.query_cache.borrow();
        Ok(self.entry(&cache, handle)?.groups.clone())
    }

    pub fn detail(&self, token: &ProjectionRowToken) -> Result<ProjectionDetail> {
        if &token.view != self.id() || token.key.repository != self.id().source.repository {
            return Err(stale());
        }
        self.with_connection(|connection| {
            let _deadline=Deadline::start(connection,self.limits().query_timeout_ms)?;
            let row=self.row(connection,&key(&kind_name(token.key.kind),&token.key.id),None,None)?;
            if &row.token!=token {return Err(stale());}
            let (document_bytes,document):(i64,Option<String>)=connection.query_row("SELECT bytes,document FROM projection_documents WHERE path=?1",[token.path.to_string_lossy().as_ref()],|row|Ok((row.get(0)?,row.get(1)?))).map_err(sql_error)?;
            let document_bytes=usize::try_from(document_bytes).map_err(|_|invalid("projection document byte count is invalid"))?;
            let kind=kind_name(token.key.kind);
            let predicate="(from_kind=?1 AND from_id=?2) OR (to_kind=?1 AND to_id=?2)";
            let total_relations:i64=connection.query_row(&format!("SELECT COUNT(*) FROM projection_edges WHERE {predicate}"),params![kind,token.key.id],|row|row.get(0)).map_err(sql_error)?;
            let total_relations=usize::try_from(total_relations).map_err(|_|invalid("projection relation count is invalid"))?;
            let mut statement=connection.prepare(&format!("SELECT from_kind,from_id,to_kind,to_id,relation,path,content FROM projection_edges WHERE {predicate} ORDER BY relation,from_kind,from_id,to_kind,to_id,path LIMIT ?3")).map_err(sql_error)?;
            let mut rows=statement.query(params![kind,token.key.id,self.limits().max_page_rows as i64]).map_err(sql_error)?;
            let mut relations=Vec::new();
            while let Some(row)=rows.next().map_err(sql_error)? {
                let from_kind:String=row.get(0).map_err(sql_error)?;let to_kind:String=row.get(2).map_err(sql_error)?;
                relations.push(ProjectionRelation{relation:row.get(4).map_err(sql_error)?,from:ProjectionRecordKey{repository:token.key.repository.clone(),kind:decode(Some(from_kind))?.expect("kind"),id:row.get(1).map_err(sql_error)?},to:ProjectionRecordKey{repository:token.key.repository.clone(),kind:decode(Some(to_kind))?.expect("kind"),id:row.get(3).map_err(sql_error)?},path:row.get::<_,String>(5).map_err(sql_error)?.into(),content:row.get::<_,String>(6).map_err(sql_error)?.parse()?});
            }
            let mut detail=ProjectionDetail{row,document:document.as_ref().map(|_|String::new()),document_bytes,omitted_document_bytes:document_bytes,relations,total_relations};
            while serialized(&detail)?>self.limits().max_detail_bytes {
                if detail.relations.pop().is_none(){return Err(bound("projection detail envelope exceeds its byte bound"));}
            }
            if let Some(document)=document {
                let mut low=0usize;let mut high=document.len();
                while low<high {
                    let middle=low+(high-low).div_ceil(2);let mut boundary=middle;while !document.is_char_boundary(boundary){boundary-=1;}
                    detail.document=Some(document[..boundary].to_owned());detail.omitted_document_bytes=document_bytes.saturating_sub(boundary);
                    if serialized(&detail)?<=self.limits().max_detail_bytes {low=middle;}else{high=middle-1;}
                }
                while !document.is_char_boundary(low){low-=1;}
                detail.document=Some(document[..low].to_owned());detail.omitted_document_bytes=document_bytes.saturating_sub(low);
            }
            Ok(detail)
        })
    }

    fn entry<'a>(
        &self,
        cache: &'a QueryCache,
        handle: &ProjectionQueryHandle,
    ) -> Result<&'a CachedQuery> {
        if &handle.view != self.id() {
            return Err(stale());
        }
        let entry = cache.entries.get(handle.query.as_str()).ok_or_else(|| {
            PmError::new(
                ErrorCode::NotFound,
                "projection query handle expired; repeat the same query on this view",
            )
        })?;
        if handle.total != entry.keys.len() {
            return Err(stale());
        }
        Ok(entry)
    }

    fn row(
        &self,
        connection: &Connection,
        key: &str,
        group: Option<String>,
        tree: Option<ProjectionTreePosition>,
    ) -> Result<ProjectionRow> {
        let raw=connection.query_row("SELECT kind,id,path,content,title,archived,retired,status,priority,assignee,project,cycle,milestone,parent,created_at,updated_at,decision,maturity,availability FROM projection_records WHERE key=?1",[key],|r|Ok((r.get::<_,String>(0)?,r.get::<_,String>(1)?,r.get::<_,String>(2)?,r.get::<_,String>(3)?,r.get::<_,String>(4)?,r.get::<_,bool>(5)?,r.get::<_,bool>(6)?,r.get::<_,Option<String>>(7)?,r.get::<_,Option<u8>>(8)?,r.get::<_,Option<String>>(9)?,r.get::<_,Option<String>>(10)?,r.get::<_,Option<String>>(11)?,r.get::<_,Option<String>>(12)?,r.get::<_,Option<String>>(13)?,r.get::<_,Option<String>>(14)?,r.get::<_,Option<String>>(15)?,r.get::<_,Option<String>>(16)?,r.get::<_,Option<String>>(17)?,r.get::<_,Option<String>>(18)?))).optional().map_err(sql_error)?.ok_or_else(||PmError::new(ErrorCode::NotFound,"record is absent from this projection generation"))?;
        let kind: SnapshotKind = decode(Some(raw.0))?.expect("record kind");
        let repository = self.id().source.repository.clone();
        let priority = raw
            .8
            .map(|rank| match rank {
                0 => Ok(Priority::None),
                1 => Ok(Priority::Low),
                2 => Ok(Priority::Medium),
                3 => Ok(Priority::High),
                4 => Ok(Priority::Urgent),
                _ => Err(PmError::new(
                    ErrorCode::InvalidSchema,
                    "indexed priority is invalid",
                )),
            })
            .transpose()?;
        Ok(ProjectionRow {
            tree,
            token: ProjectionRowToken {
                view: self.id().clone(),
                key: ProjectionRecordKey {
                    repository: repository.clone(),
                    kind,
                    id: raw.1,
                },
                path: raw.2.into(),
                content: raw.3.parse()?,
            },
            title: raw.4,
            archived: raw.5,
            retired: raw.6,
            status: raw.7,
            priority,
            lead: (kind == SnapshotKind::Feature)
                .then(|| raw.9.clone())
                .flatten(),
            assignee: raw.9,
            project: raw.10,
            cycle: raw.11,
            milestone: raw.12,
            parent: raw.13.map(|id| ProjectionRecordKey {
                repository,
                kind,
                id,
            }),
            group,
            created_at: raw.14,
            updated_at: raw.15,
            decision: decode(raw.16)?,
            maturity: decode(raw.17)?,
            availability: decode(raw.18)?,
        })
    }
}
