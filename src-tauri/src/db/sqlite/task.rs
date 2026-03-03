// Copyright 2026 Aleksandr Iushmanov (@izeren)
// SPDX-License-Identifier: Apache-2.0

//! [`TaskStore`] and [`LabelStore`] implementations: mappers, query helpers,
//! and thin trait impls for both [`SqliteStore`] (mutex-guarded) and
//! [`TxStore`] (in-transaction).

use std::collections::{HashMap, HashSet};
use std::fmt::Write as _;

use chrono::{DateTime, Utc};
use rusqlite::Connection;

use super::{
    format_optional_datetime, parse_datetime, parse_optional_datetime, priority_from_i64,
    priority_to_i64, SqliteStore, TxStore,
};
use crate::domain::enums::{Priority, TaskStatus};
use crate::domain::inputs::{LabelCount, TaskFilter};
use crate::domain::models::Task;
use crate::error::AppError;
use crate::traits::storage::{LabelStore, TaskStore};

pub(super) const fn status_to_str(s: TaskStatus) -> &'static str {
    match s {
        TaskStatus::Backlog => "backlog",
        TaskStatus::Pending => "pending",
        TaskStatus::Scheduled => "scheduled",
        TaskStatus::Completed => "completed",
        TaskStatus::Cancelled => "cancelled",
    }
}

/// Convert a lowercase string back to a [`TaskStatus`], returning an error for unknown values.
pub(super) fn status_from_str(s: &str) -> Result<TaskStatus, AppError> {
    match s {
        "backlog" => Ok(TaskStatus::Backlog),
        "pending" => Ok(TaskStatus::Pending),
        "scheduled" => Ok(TaskStatus::Scheduled),
        "completed" => Ok(TaskStatus::Completed),
        "cancelled" => Ok(TaskStatus::Cancelled),
        other => Err(AppError::Database(format!("unknown task status: {other}"))),
    }
}

/// Build a [`Task`] from a row of the `tasks` table (17 columns in SELECT order).
fn row_to_task(row: &rusqlite::Row<'_>) -> Result<Task, rusqlite::Error> {
    Ok(Task {
        id: row.get(0)?,
        title: row.get(1)?,
        description: row.get(2)?,
        duration_minutes: row.get(3)?,
        time_logged_minutes: row.get(4)?,
        // Priority/status will be fixed up by the caller after conversion.
        // Store raw values to be converted outside the rusqlite closure.
        priority: Priority::Medium,
        status: TaskStatus::Pending,
        start_date: None,
        deadline: None,
        schedule_id: row.get(9)?,
        min_chunk_minutes: row.get(10)?,
        no_split: row.get(11)?,
        recurring_template_id: row.get(12)?,
        expire_at: None,
        is_pinned: row.get(16)?,
        labels: Vec::new(), // populated separately
        created_at: DateTime::<Utc>::from(std::time::UNIX_EPOCH),
        updated_at: DateTime::<Utc>::from(std::time::UNIX_EPOCH),
    })
}

/// Raw DB column values for fields that need conversion (priority, status, datetimes).
type RawTaskFields = (
    i64,
    String,
    Option<String>,
    Option<String>,
    Option<String>,
    String,
    String,
);

/// Read raw string/int columns that need conversion outside the rusqlite closure.
fn row_to_raw_fields(row: &rusqlite::Row<'_>) -> Result<RawTaskFields, rusqlite::Error> {
    Ok((
        row.get(5)?,  // priority
        row.get(6)?,  // status
        row.get(7)?,  // start_date
        row.get(8)?,  // deadline
        row.get(13)?, // expire_at
        row.get(14)?, // created_at
        row.get(15)?, // updated_at
    ))
}

fn parse_opt_dt(s: Option<&str>) -> Result<Option<DateTime<Utc>>, AppError> {
    match s {
        Some(s) => parse_optional_datetime(s),
        None => Ok(None),
    }
}

/// Finalize a [`Task`] by converting raw DB values for priority, status, and datetimes.
#[allow(clippy::too_many_arguments)] // 1:1 with raw DB columns; grouping would add indirection
fn finalize_task(
    mut task: Task,
    priority_i64: i64,
    status_str: &str,
    start_date_str: Option<&str>,
    deadline_str: Option<&str>,
    expire_at_str: Option<&str>,
    created_at_str: &str,
    updated_at_str: &str,
) -> Result<Task, AppError> {
    task.priority = priority_from_i64(priority_i64)?;
    task.status = status_from_str(status_str)?;
    task.start_date = parse_opt_dt(start_date_str)?;
    task.deadline = parse_opt_dt(deadline_str)?;
    task.expire_at = parse_opt_dt(expire_at_str)?;
    task.created_at = parse_datetime(created_at_str, "created_at")?;
    task.updated_at = parse_datetime(updated_at_str, "updated_at")?;
    Ok(task)
}

/// Fetch labels for multiple tasks in a single query, returning a map from `task_id` to labels.
///
/// Tasks with no labels are absent from the returned map; callers use
/// `.get(&id).cloned().unwrap_or_default()` to obtain an empty `Vec` for those tasks.
/// An empty `task_ids` slice returns an empty map without hitting the database.
///
/// SQL injection safety: only the placeholder count is constructed via string formatting;
/// actual `task_id` values are always bound through rusqlite's parameterized API.
pub(super) fn fetch_labels_for_tasks(
    conn: &Connection,
    task_ids: &[&str],
) -> Result<HashMap<String, Vec<String>>, AppError> {
    if task_ids.is_empty() {
        return Ok(HashMap::new());
    }

    let placeholders: Vec<String> = (1..=task_ids.len()).map(|i| format!("?{i}")).collect();
    let sql = format!(
        "SELECT task_id, label FROM task_labels WHERE task_id IN ({})",
        placeholders.join(", ")
    );
    let mut stmt = conn.prepare(&sql)?;
    let param_refs: Vec<&dyn rusqlite::types::ToSql> = task_ids
        .iter()
        .map(|id| id as &dyn rusqlite::types::ToSql)
        .collect();
    let rows = stmt.query_map(param_refs.as_slice(), |row| {
        let task_id: String = row.get(0)?;
        let label: String = row.get(1)?;
        Ok((task_id, label))
    })?;

    let mut map: HashMap<String, Vec<String>> = HashMap::new();
    for row in rows {
        let (task_id, label) = row?;
        map.entry(task_id).or_default().push(label);
    }
    Ok(map)
}

/// Bind a task's 17 columns (SELECT order: `?1`=id … `?17`=`is_pinned`) and run a
/// prepared write. INSERT and UPDATE share the identical positional binding.
fn execute_task_write(stmt: &mut rusqlite::Statement<'_>, task: &Task) -> Result<(), AppError> {
    stmt.execute(rusqlite::params![
        task.id,
        task.title,
        task.description,
        task.duration_minutes,
        task.time_logged_minutes,
        priority_to_i64(task.priority),
        status_to_str(task.status),
        format_optional_datetime(task.start_date.as_ref()),
        format_optional_datetime(task.deadline.as_ref()),
        task.schedule_id,
        task.min_chunk_minutes,
        task.no_split,
        task.recurring_template_id,
        format_optional_datetime(task.expire_at.as_ref()),
        task.created_at.to_rfc3339(),
        task.updated_at.to_rfc3339(),
        task.is_pinned,
    ])?;
    Ok(())
}

/// Insert every label for `task` into `task_labels`. Callers updating an existing
/// row clear `task_labels` first; on create the row is new so no delete is needed.
fn insert_task_labels(conn: &Connection, task: &Task) -> Result<(), AppError> {
    let mut label_stmt =
        conn.prepare("INSERT INTO task_labels (task_id, label) VALUES (?1, ?2)")?;
    for label in &task.labels {
        label_stmt.execute(rusqlite::params![task.id, label])?;
    }
    Ok(())
}

/// Run a prepared `SELECT {TASK_SELECT_COLS}` statement and collect fully
/// finalized [`Task`]s, batch-fetching all labels in one query. Shared by every
/// task read so the row → task → labels assembly lives in exactly one place.
fn collect_tasks(
    conn: &Connection,
    stmt: &mut rusqlite::Statement<'_>,
    params: impl rusqlite::Params,
) -> Result<Vec<Task>, AppError> {
    let rows = stmt.query_map(params, |row| {
        let task = row_to_task(row)?;
        let raw = row_to_raw_fields(row)?;
        Ok((task, raw))
    })?;

    let mut tasks = Vec::new();
    for row in rows {
        let (task, (pri, status, start, deadline, expire, created, updated)) = row?;
        let task = finalize_task(
            task,
            pri,
            &status,
            start.as_deref(),
            deadline.as_deref(),
            expire.as_deref(),
            &created,
            &updated,
        )?;
        tasks.push(task);
    }
    // One batch query for all task labels instead of N per-row fetches.
    let task_ids: Vec<&str> = tasks.iter().map(|t| t.id.as_str()).collect();
    let labels_map = fetch_labels_for_tasks(conn, &task_ids)?;
    for task in &mut tasks {
        task.labels = labels_map.get(&task.id).cloned().unwrap_or_default();
    }
    Ok(tasks)
}

const TASK_SELECT_COLS: &str = "id, title, description, duration_minutes, time_logged_minutes, \
     priority, status, start_date, deadline, schedule_id, \
     min_chunk_minutes, no_split, recurring_template_id, \
     expire_at, created_at, updated_at, is_pinned";

fn create_task(conn: &Connection, task: &Task) -> Result<(), AppError> {
    let mut stmt = conn.prepare(
        "INSERT INTO tasks (\
            id, title, description, duration_minutes, time_logged_minutes, \
            priority, status, start_date, deadline, schedule_id, \
            min_chunk_minutes, no_split, recurring_template_id, \
            expire_at, created_at, updated_at, is_pinned\
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17)",
    )?;
    execute_task_write(&mut stmt, task)?;
    insert_task_labels(conn, task)
}

fn get_task(conn: &Connection, id: &str) -> Result<Option<Task>, AppError> {
    let sql = format!("SELECT {TASK_SELECT_COLS} FROM tasks WHERE id = ?1");
    let mut stmt = conn.prepare(&sql)?;
    Ok(collect_tasks(conn, &mut stmt, rusqlite::params![id])?
        .into_iter()
        .next())
}

fn update_task(conn: &Connection, task: &Task) -> Result<(), AppError> {
    let mut stmt = conn.prepare(
        "UPDATE tasks SET \
            title = ?2, description = ?3, duration_minutes = ?4, \
            time_logged_minutes = ?5, priority = ?6, status = ?7, \
            start_date = ?8, deadline = ?9, schedule_id = ?10, \
            min_chunk_minutes = ?11, no_split = ?12, recurring_template_id = ?13, \
            expire_at = ?14, created_at = ?15, updated_at = ?16, is_pinned = ?17 \
        WHERE id = ?1",
    )?;
    execute_task_write(&mut stmt, task)?;

    conn.execute(
        "DELETE FROM task_labels WHERE task_id = ?1",
        rusqlite::params![task.id],
    )?;
    insert_task_labels(conn, task)
}

fn delete_task(conn: &Connection, id: &str) -> Result<(), AppError> {
    conn.execute("DELETE FROM tasks WHERE id = ?1", rusqlite::params![id])?;
    Ok(())
}

/// Append SQL params and return the matching `?N` placeholder strings.
fn push_in_params<T: rusqlite::types::ToSql + 'static>(
    param_idx: &mut usize,
    params: &mut Vec<Box<dyn rusqlite::types::ToSql>>,
    values: impl IntoIterator<Item = T>,
) -> Vec<String> {
    values
        .into_iter()
        .map(|v| {
            *param_idx += 1;
            params.push(Box::new(v));
            format!("?{param_idx}")
        })
        .collect()
}

#[allow(clippy::too_many_lines)] // filter-building logic is sequential, splitting would obscure flow
fn list_tasks(conn: &Connection, filter: &TaskFilter) -> Result<Vec<Task>, AppError> {
    let mut sql = format!("SELECT {TASK_SELECT_COLS} FROM tasks WHERE 1=1");
    let mut params: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();
    let mut param_idx: usize = 0;

    if let Some(ref statuses) = filter.statuses {
        if !statuses.is_empty() {
            let placeholders = push_in_params(
                &mut param_idx,
                &mut params,
                statuses.iter().map(|s| status_to_str(*s)),
            );
            // SAFETY: write! to a String is infallible.
            let _ = write!(sql, " AND status IN ({})", placeholders.join(", "));
        }
    }

    if let Some(ref labels) = filter.labels {
        if !labels.is_empty() {
            // Match-all (AND): the task must carry every listed label. Dedup
            // first so repeated labels cannot inflate the required count.
            let distinct: HashSet<&String> = labels.iter().collect();
            let placeholders = push_in_params(
                &mut param_idx,
                &mut params,
                distinct.iter().map(|l| (*l).clone()),
            );
            param_idx += 1;
            let count_idx = param_idx;
            // Label lists are tiny; saturate instead of failing on overflow.
            params.push(Box::new(i64::try_from(distinct.len()).unwrap_or(i64::MAX)));
            // SAFETY: write! to a String is infallible.
            let _ = write!(
                sql,
                " AND (SELECT COUNT(DISTINCT label) FROM task_labels \
                 WHERE task_labels.task_id = tasks.id AND label IN ({})) = ?{count_idx}",
                placeholders.join(", ")
            );
        }
    }

    if let Some(ref excluded) = filter.excluded_labels {
        if !excluded.is_empty() {
            // Match-none: drop the task if it carries ANY listed label.
            // IN handles duplicates naturally, so no dedup is needed here.
            let placeholders =
                push_in_params(&mut param_idx, &mut params, excluded.iter().cloned());
            // SAFETY: write! to a String is infallible.
            let _ = write!(
                sql,
                " AND NOT EXISTS (SELECT 1 FROM task_labels \
                 WHERE task_labels.task_id = tasks.id AND label IN ({}))",
                placeholders.join(", ")
            );
        }
    }

    match filter.unlabeled {
        Some(true) => sql.push_str(
            " AND NOT EXISTS (SELECT 1 FROM task_labels WHERE task_labels.task_id = tasks.id)",
        ),
        Some(false) => sql.push_str(
            " AND EXISTS (SELECT 1 FROM task_labels WHERE task_labels.task_id = tasks.id)",
        ),
        None => {}
    }

    if let Some(ref text) = filter.search_text {
        let pattern = format!("%{text}%");
        param_idx += 1;
        let pattern_idx = param_idx;
        params.push(Box::new(pattern));
        // SAFETY: write! to a String is infallible.
        // SQLite ?N parameters may appear multiple times; same value bound to both columns.
        let _ = write!(
            sql,
            " AND (title LIKE ?{pattern_idx} OR description LIKE ?{pattern_idx})"
        );
    }

    if let Some(ref priorities) = filter.priorities {
        if !priorities.is_empty() {
            let placeholders = push_in_params(
                &mut param_idx,
                &mut params,
                priorities.iter().map(|p| priority_to_i64(*p)),
            );
            // SAFETY: write! to a String is infallible.
            let _ = write!(sql, " AND priority IN ({})", placeholders.join(", "));
        }
    }

    if let Some(ref deadline_before) = filter.deadline_before {
        param_idx += 1;
        params.push(Box::new(deadline_before.to_rfc3339()));
        // SAFETY: write! to a String is infallible.
        let _ = write!(sql, " AND deadline < ?{param_idx}");
    }

    if let Some(ref deadline_after) = filter.deadline_after {
        param_idx += 1;
        params.push(Box::new(deadline_after.to_rfc3339()));
        // SAFETY: write! to a String is infallible.
        let _ = write!(sql, " AND deadline > ?{param_idx}");
    }

    if let Some(ref schedule_id) = filter.schedule_id {
        param_idx += 1;
        params.push(Box::new(schedule_id.clone()));
        // SAFETY: write! to a String is infallible.
        let _ = write!(sql, " AND schedule_id = ?{param_idx}");
    }

    if let Some(ref recurring_template_id) = filter.recurring_template_id {
        param_idx += 1;
        params.push(Box::new(recurring_template_id.clone()));
        // SAFETY: write! to a String is infallible.
        let _ = write!(sql, " AND recurring_template_id = ?{param_idx}");
    }

    let param_refs: Vec<&dyn rusqlite::types::ToSql> = params.iter().map(AsRef::as_ref).collect();
    let mut stmt = conn.prepare(&sql)?;
    collect_tasks(conn, &mut stmt, param_refs.as_slice())
}

fn get_schedulable_tasks(conn: &Connection) -> Result<Vec<Task>, AppError> {
    let sql = format!(
        "SELECT {TASK_SELECT_COLS} FROM tasks \
         WHERE status IN ('pending', 'scheduled') \
         AND time_logged_minutes < duration_minutes"
    );
    let mut stmt = conn.prepare(&sql)?;
    collect_tasks(conn, &mut stmt, ())
}

/// List every distinct label with its task usage count, ordered by label.
///
/// The union over `task_labels` and `template_labels` keeps template-only
/// labels visible; the `LEFT JOIN` back onto `task_labels` keeps the count
/// task-centric (`0` for template-only labels).
fn list_labels(conn: &Connection) -> Result<Vec<LabelCount>, AppError> {
    let mut stmt = conn.prepare(
        "SELECT all_labels.label, COUNT(task_labels.task_id) AS task_count \
         FROM (SELECT label FROM task_labels \
               UNION SELECT label FROM template_labels) AS all_labels \
         LEFT JOIN task_labels ON task_labels.label = all_labels.label \
         GROUP BY all_labels.label \
         ORDER BY all_labels.label",
    )?;
    let rows = stmt.query_map([], |row| {
        Ok(LabelCount {
            label: row.get(0)?,
            task_count: row.get(1)?,
        })
    })?;
    let mut labels = Vec::new();
    for row in rows {
        labels.push(row?);
    }
    Ok(labels)
}

impl LabelStore for SqliteStore {
    fn list_labels(&self) -> Result<Vec<LabelCount>, AppError> {
        list_labels(&*self.lock()?)
    }
}

impl LabelStore for TxStore<'_> {
    fn list_labels(&self) -> Result<Vec<LabelCount>, AppError> {
        list_labels(self.conn)
    }
}

impl TaskStore for SqliteStore {
    fn create_task(&self, task: &Task) -> Result<(), AppError> {
        self.in_tx(|conn| create_task(conn, task))
    }

    fn get_task(&self, id: &str) -> Result<Option<Task>, AppError> {
        get_task(&*self.lock()?, id)
    }

    fn update_task(&self, task: &Task) -> Result<(), AppError> {
        self.in_tx(|conn| update_task(conn, task))
    }

    fn delete_task(&self, id: &str) -> Result<(), AppError> {
        delete_task(&*self.lock()?, id)
    }

    fn list_tasks(&self, filter: &TaskFilter) -> Result<Vec<Task>, AppError> {
        list_tasks(&*self.lock()?, filter)
    }

    fn get_schedulable_tasks(&self) -> Result<Vec<Task>, AppError> {
        get_schedulable_tasks(&*self.lock()?)
    }
}

impl TaskStore for TxStore<'_> {
    fn create_task(&self, task: &Task) -> Result<(), AppError> {
        create_task(self.conn, task)
    }

    fn get_task(&self, id: &str) -> Result<Option<Task>, AppError> {
        get_task(self.conn, id)
    }

    fn update_task(&self, task: &Task) -> Result<(), AppError> {
        update_task(self.conn, task)
    }

    fn delete_task(&self, id: &str) -> Result<(), AppError> {
        delete_task(self.conn, id)
    }

    fn list_tasks(&self, filter: &TaskFilter) -> Result<Vec<Task>, AppError> {
        list_tasks(self.conn, filter)
    }

    fn get_schedulable_tasks(&self) -> Result<Vec<Task>, AppError> {
        get_schedulable_tasks(self.conn)
    }
}
