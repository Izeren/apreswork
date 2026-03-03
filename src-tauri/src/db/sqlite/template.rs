// Copyright 2026 Aleksandr Iushmanov (@izeren)
// SPDX-License-Identifier: Apache-2.0

//! [`RecurringTemplateStore`] implementation: mappers, query helpers, and thin
//! trait impls for both [`SqliteStore`] (mutex-guarded) and [`TxStore`]
//! (in-transaction).

use chrono::{DateTime, Utc};
use rusqlite::Connection;

use super::{parse_datetime, priority_from_i64, priority_to_i64, SqliteStore, TxStore};
use crate::domain::cadence::{Cadence, Period, Window};
use crate::domain::enums::Priority;
use crate::domain::models::RecurringTemplate;
use crate::error::AppError;
use crate::traits::storage::RecurringTemplateStore;

pub(super) fn cadence_to_db(cadence: &Cadence) -> Result<(String, String), AppError> {
    let value = serde_json::to_value(cadence)
        .map_err(|e| AppError::Database(format!("failed to serialize cadence: {e}")))?;
    let cadence_type = value
        .get("period")
        .and_then(|v| v.as_str())
        .ok_or_else(|| AppError::Database("cadence missing period field".into()))?
        .to_owned();
    let serde_json::Value::Object(mut data_map) = value else {
        return Err(AppError::Database(
            "cadence serialized to non-object".into(),
        ));
    };
    data_map.remove("period");
    let cadence_data = serde_json::to_string(&data_map)
        .map_err(|e| AppError::Database(format!("failed to serialize cadence data: {e}")))?;
    Ok((cadence_type, cadence_data))
}

pub(super) fn cadence_from_db(cadence_type: &str, cadence_data: &str) -> Result<Cadence, AppError> {
    let mut data_map: serde_json::Map<String, serde_json::Value> =
        serde_json::from_str(cadence_data)
            .map_err(|e| AppError::Database(format!("invalid cadence_data JSON: {e}")))?;
    data_map.insert(
        "period".to_owned(),
        serde_json::Value::String(cadence_type.to_owned()),
    );
    let cadence: Cadence = serde_json::from_value(serde_json::Value::Object(data_map))
        .map_err(|e| AppError::Database(format!("failed to deserialize cadence: {e}")))?;
    Ok(cadence)
}

const TEMPLATE_SELECT_COLS: &str = "id, title, description, duration_minutes, priority, \
     schedule_id, cadence_type, cadence_data, \
     COALESCE(start_date, created_at) AS start_date, is_active, created_at, updated_at";

/// Build a [`RecurringTemplate`] from a row of the `recurring_templates` table
/// (12 columns in SELECT order).
///
/// Fields requiring conversion (priority, cadence, datetimes) use placeholders;
/// the caller must apply [`finalize_template`] after exiting the rusqlite closure.
fn row_to_template(row: &rusqlite::Row<'_>) -> Result<RecurringTemplate, rusqlite::Error> {
    Ok(RecurringTemplate {
        id: row.get(0)?,
        title: row.get(1)?,
        description: row.get(2)?,
        duration_minutes: row.get(3)?,
        priority: Priority::Medium, // placeholder — overwritten by finalize_template
        schedule_id: row.get(5)?,
        cadence: Cadence::new(Period::Monthly, 1, vec![Window { start: 0, end: 0 }])
            .expect("valid placeholder cadence"), // placeholder, overwritten by finalize_template
        labels: Vec::new(), // populated separately
        is_active: row.get(9)?,
        start_date: DateTime::<Utc>::from(std::time::UNIX_EPOCH), // placeholder — overwritten by finalize_template
        created_at: DateTime::<Utc>::from(std::time::UNIX_EPOCH), // placeholder — overwritten by finalize_template
        updated_at: DateTime::<Utc>::from(std::time::UNIX_EPOCH), // placeholder — overwritten by finalize_template
    })
}

/// Raw DB column values for template fields that need conversion outside the rusqlite closure.
///
/// Fields: (priority, `cadence_type`, `cadence_data`, `start_date`, `created_at`, `updated_at`)
type RawTemplateFields = (i64, String, String, String, String, String);

fn row_to_raw_template_fields(
    row: &rusqlite::Row<'_>,
) -> Result<RawTemplateFields, rusqlite::Error> {
    Ok((
        row.get(4)?,  // priority
        row.get(6)?,  // cadence_type
        row.get(7)?,  // cadence_data
        row.get(8)?,  // start_date (COALESCE'd)
        row.get(10)?, // created_at
        row.get(11)?, // updated_at
    ))
}

fn finalize_template(
    mut template: RecurringTemplate,
    priority_i64: i64,
    cadence_type: &str,
    cadence_data: &str,
    start_date_str: &str,
    created_at_str: &str,
    updated_at_str: &str,
) -> Result<RecurringTemplate, AppError> {
    template.priority = priority_from_i64(priority_i64)?;
    template.cadence = cadence_from_db(cadence_type, cadence_data)?;
    template.start_date = parse_datetime(start_date_str, "start_date")?;
    template.created_at = parse_datetime(created_at_str, "created_at")?;
    template.updated_at = parse_datetime(updated_at_str, "updated_at")?;
    Ok(template)
}

fn fetch_labels_for_template(
    conn: &Connection,
    template_id: &str,
) -> Result<Vec<String>, AppError> {
    let mut stmt = conn.prepare("SELECT label FROM template_labels WHERE template_id = ?1")?;
    let rows = stmt.query_map(rusqlite::params![template_id], |row| row.get(0))?;
    let mut labels = Vec::new();
    for row in rows {
        labels.push(row?);
    }
    Ok(labels)
}

/// Bind a template's 12 columns (SELECT order: `?1`=id … `?12`=`updated_at`) and
/// run a prepared write. INSERT and UPDATE share the identical positional binding;
/// `cadence_type`/`cadence_data` are pre-split by the caller via [`cadence_to_db`].
fn execute_template_write(
    stmt: &mut rusqlite::Statement<'_>,
    template: &RecurringTemplate,
    cadence_type: &str,
    cadence_data: &str,
) -> Result<(), AppError> {
    stmt.execute(rusqlite::params![
        template.id,
        template.title,
        template.description,
        template.duration_minutes,
        priority_to_i64(template.priority),
        template.schedule_id,
        cadence_type,
        cadence_data,
        template.start_date.to_rfc3339(),
        template.is_active,
        template.created_at.to_rfc3339(),
        template.updated_at.to_rfc3339(),
    ])?;
    Ok(())
}

fn insert_template_labels(conn: &Connection, template: &RecurringTemplate) -> Result<(), AppError> {
    let mut label_stmt =
        conn.prepare("INSERT INTO template_labels (template_id, label) VALUES (?1, ?2)")?;
    for label in &template.labels {
        label_stmt.execute(rusqlite::params![template.id, label])?;
    }
    Ok(())
}

fn create_template(conn: &Connection, template: &RecurringTemplate) -> Result<(), AppError> {
    let (cadence_type, cadence_data) = cadence_to_db(&template.cadence)?;

    let mut stmt = conn.prepare(
        "INSERT INTO recurring_templates (\
            id, title, description, duration_minutes, priority, \
            schedule_id, cadence_type, cadence_data, start_date, is_active, \
            created_at, updated_at\
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
    )?;
    execute_template_write(&mut stmt, template, &cadence_type, &cadence_data)?;
    insert_template_labels(conn, template)
}

fn get_template(conn: &Connection, id: &str) -> Result<Option<RecurringTemplate>, AppError> {
    let sql = format!("SELECT {TEMPLATE_SELECT_COLS} FROM recurring_templates WHERE id = ?1");
    let mut stmt = conn.prepare(&sql)?;

    let result = stmt.query_row(rusqlite::params![id], |row| {
        let template = row_to_template(row)?;
        let raw = row_to_raw_template_fields(row)?;
        Ok((template, raw))
    });

    match result {
        Ok((template, (pri, cad_type, cad_data, start, created, updated))) => {
            let mut template = finalize_template(
                template, pri, &cad_type, &cad_data, &start, &created, &updated,
            )?;
            template.labels = fetch_labels_for_template(conn, &template.id)?;
            Ok(Some(template))
        }
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
        Err(e) => Err(AppError::from(e)),
    }
}

fn update_template(conn: &Connection, template: &RecurringTemplate) -> Result<(), AppError> {
    let (cadence_type, cadence_data) = cadence_to_db(&template.cadence)?;

    let mut stmt = conn.prepare(
        "UPDATE recurring_templates SET \
            title = ?2, description = ?3, duration_minutes = ?4, \
            priority = ?5, schedule_id = ?6, cadence_type = ?7, \
            cadence_data = ?8, start_date = ?9, is_active = ?10, \
            created_at = ?11, updated_at = ?12 \
        WHERE id = ?1",
    )?;
    execute_template_write(&mut stmt, template, &cadence_type, &cadence_data)?;

    conn.execute(
        "DELETE FROM template_labels WHERE template_id = ?1",
        rusqlite::params![template.id],
    )?;

    insert_template_labels(conn, template)
}

fn delete_template(conn: &Connection, id: &str) -> Result<(), AppError> {
    conn.execute(
        "DELETE FROM recurring_templates WHERE id = ?1",
        rusqlite::params![id],
    )?;
    Ok(())
}

fn list_templates(conn: &Connection) -> Result<Vec<RecurringTemplate>, AppError> {
    let sql = format!("SELECT {TEMPLATE_SELECT_COLS} FROM recurring_templates");
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map([], |row| {
        let template = row_to_template(row)?;
        let raw = row_to_raw_template_fields(row)?;
        Ok((template, raw))
    })?;

    let mut templates = Vec::new();
    for row in rows {
        let (template, (pri, cad_type, cad_data, start, created, updated)) = row?;
        let mut template = finalize_template(
            template, pri, &cad_type, &cad_data, &start, &created, &updated,
        )?;
        template.labels = fetch_labels_for_template(conn, &template.id)?;
        templates.push(template);
    }

    Ok(templates)
}

impl RecurringTemplateStore for SqliteStore {
    fn create_template(&self, template: &RecurringTemplate) -> Result<(), AppError> {
        self.in_tx(|conn| create_template(conn, template))
    }

    fn get_template(&self, id: &str) -> Result<Option<RecurringTemplate>, AppError> {
        get_template(&*self.lock()?, id)
    }

    fn update_template(&self, template: &RecurringTemplate) -> Result<(), AppError> {
        self.in_tx(|conn| update_template(conn, template))
    }

    fn delete_template(&self, id: &str) -> Result<(), AppError> {
        delete_template(&*self.lock()?, id)
    }

    fn list_templates(&self) -> Result<Vec<RecurringTemplate>, AppError> {
        list_templates(&*self.lock()?)
    }
}

impl RecurringTemplateStore for TxStore<'_> {
    fn create_template(&self, template: &RecurringTemplate) -> Result<(), AppError> {
        create_template(self.conn, template)
    }

    fn get_template(&self, id: &str) -> Result<Option<RecurringTemplate>, AppError> {
        get_template(self.conn, id)
    }

    fn update_template(&self, template: &RecurringTemplate) -> Result<(), AppError> {
        update_template(self.conn, template)
    }

    fn delete_template(&self, id: &str) -> Result<(), AppError> {
        delete_template(self.conn, id)
    }

    fn list_templates(&self) -> Result<Vec<RecurringTemplate>, AppError> {
        list_templates(self.conn)
    }
}
